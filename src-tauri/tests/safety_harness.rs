//! End-to-end safety harness.
//!
//! Runs the REAL scan and clean code against a synthetic Windows profile built
//! under a `TempDir`. Nothing here touches the real user profile, the real
//! recycle bin or the registry: `%USERPROFILE%`, `%LOCALAPPDATA%`, `%APPDATA%`
//! and `%TEMP%` are injected, the recycle-bin API is injected, the
//! move-to-recycle-bin call is injected, and the Winapp2 registry probe always
//! answers "not installed".
//!
//! What it proves and what it deliberately does not: `docs/safety-harness.md`.

mod support;

use std::collections::{BTreeSet, HashMap};
use std::path::PathBuf;
use std::time::Instant;

use support::fake_profile::Fixture;
use wincleaner_lib::clean::{clean_rule_with_trash, CleanMode, CleanReport};
use wincleaner_lib::commands::{clean_rules_with, scan_rules_with, ScanProgress};
use wincleaner_lib::rules::{load_rules_with, memoized_env, resolved_paths_with, Rule, RuleKind};
use wincleaner_lib::scan::{scan_rule_with_api, ScanResult};
use wincleaner_lib::winapp2::{convert_with, detect_file_exists_with, detected_rules_with};

/// What the injected recycle-bin query reports. Non-zero so that a rule that
/// silently stopped calling the API would show up in the totals.
const RECYCLE_ITEMS: u64 = 3;
const RECYCLE_BYTES: u64 = 4096;

fn recycle_query() -> Result<(u64, u64), String> {
    Ok((RECYCLE_ITEMS, RECYCLE_BYTES))
}

/// The real `SHEmptyRecycleBinW` is never reachable from a test: this no-op
/// stands in for it, and the harness asserts it was reached.
fn recycle_empty() -> Result<(), String> {
    Ok(())
}

fn native_rules(fx: &Fixture) -> Vec<Rule> {
    let lookup = |n: &str| fx.lookup(n);
    load_rules_with(wincleaner_lib::rules::RULES_TOML, &lookup)
        .expect("rules.toml must load against the fake profile")
}

/// Native rules plus the Winapp2 entries this fake profile makes "detected".
/// The registry probe always answers false: detection is decided by the files
/// the fixture created, never by what happens to be installed on the machine
/// running the test.
fn full_catalogue(fx: &Fixture) -> (Vec<Rule>, Vec<Rule>) {
    let native = native_rules(fx);
    let raw = |n: &str| fx.lookup(n);
    let memo = memoized_env(&raw);
    let (converted, _report) = convert_with(wincleaner_lib::winapp2::WINAPP2_INI, &native, &memo);
    let file = |p: &str| detect_file_exists_with(p, &memo);
    let winapp2 = detected_rules_with(converted, &|_| false, &file);
    (native, winapp2)
}

fn scan_all(fx: &Fixture, rules: &[Rule]) -> Vec<ScanResult> {
    let lookup = |n: &str| fx.lookup(n);
    let mut steps: Vec<ScanProgress> = Vec::new();
    let out = scan_rules_with(
        rules,
        |r| scan_rule_with_api(r, &lookup, &recycle_query).map_err(|e| e.to_string()),
        &mut |p| steps.push(p),
    )
    .expect("the scan must not fail on the fake profile");
    assert_eq!(steps.len(), rules.len(), "one progress event per rule");
    out
}

/// The real cleaning path, `Permanent` mode: `commands::clean_rules_with`
/// driving `clean::clean_rule_with_trash`, exactly what the `clean` command
/// runs, with only the environment and the two recycle-bin calls injected.
fn clean_all(fx: &Fixture, rules: &[Rule], mode: CleanMode) -> (CleanReport, Vec<PathBuf>) {
    let lookup = |n: &str| fx.lookup(n);
    let trashed: std::cell::RefCell<Vec<PathBuf>> = std::cell::RefCell::new(Vec::new());
    let report = clean_rules_with(rules.to_vec(), mode, |rule, mode| {
        let trash = |p: &std::path::Path| {
            trashed.borrow_mut().push(p.to_path_buf());
            Ok(())
        };
        clean_rule_with_trash(
            rule,
            mode,
            &lookup,
            &recycle_query,
            &recycle_empty,
            &trash,
        )
        .map_err(|e| e.to_string())
    });
    (report, trashed.into_inner())
}

/// Windows file names are case-insensitive: the path a rule hands back carries
/// whatever case the directory was first created with, which is not always the
/// case the fixture spelled. Every set comparison here goes through this key.
fn key(path: &std::path::Path) -> String {
    Fixture::strip_verbatim(path)
        .to_string_lossy()
        .to_lowercase()
}

/// No scanned path goes through one of the planted junctions. The junction is
/// what makes the textual containment insufficient: a name under the profile
/// designating a directory outside it.
fn assert_no_path_crosses_a_junction(fx: &Fixture, scans: &[ScanResult]) {
    let links: Vec<String> = fx.junctions.iter().map(|j| format!("{}\\", key(j))).collect();
    for scan in scans {
        for path in &scan.paths {
            let lowered = path.to_lowercase();
            for link in &links {
                assert!(
                    !lowered.starts_with(link),
                    "rule \"{}\" walked through a junction: {path}",
                    scan.rule_id
                );
            }
        }
    }
}

fn assert_sentinels_intact(fx: &Fixture, stage: &str) {
    for s in &fx.sentinels {
        assert!(
            s.path.exists(),
            "SENTINEL DELETED ({stage}): {} — {}",
            s.path.display(),
            s.why
        );
        let got = std::fs::read(&s.path).unwrap();
        assert_eq!(
            got,
            Fixture::sentinel_content(&s.path),
            "SENTINEL REWRITTEN ({stage}): {}",
            s.path.display()
        );
    }
}

fn assert_junk_gone(junk: &[PathBuf], stage: &str) {
    let survivors: Vec<&PathBuf> = junk.iter().filter(|p| p.exists()).collect();
    assert!(
        survivors.is_empty(),
        "{} junk file(s) survived ({stage}), first: {}",
        survivors.len(),
        survivors[0].display()
    );
}

/// Nothing outside the deleted set changed: same bytes, same names.
fn assert_only_junk_disappeared(
    before: &HashMap<PathBuf, Vec<u8>>,
    after: &HashMap<PathBuf, Vec<u8>>,
    junk: &[PathBuf],
) {
    let junk: BTreeSet<String> = junk.iter().map(|p| key(p)).collect();
    for (path, bytes) in before {
        match after.get(path) {
            Some(now) => assert_eq!(
                bytes,
                now,
                "content changed under the fixture: {}",
                path.display()
            ),
            None => assert!(
                junk.contains(&key(path)),
                "a file that was not junk disappeared: {}",
                path.display()
            ),
        }
    }
    for path in after.keys() {
        assert!(
            before.contains_key(path),
            "the cleaner created a file: {}",
            path.display()
        );
    }
}

#[test]
fn the_nine_native_rules_delete_their_junk_and_spare_every_sentinel() {
    let started = Instant::now();
    let fx = Fixture::build();
    let rules = native_rules(&fx);
    assert_eq!(rules.len(), 9, "rules.toml declares nine native rules");
    assert!(
        rules.iter().all(|r| r.unavailable_reason.is_none()),
        "every native rule must apply to the fake profile"
    );

    let before = fx.snapshot();
    let scans = scan_all(&fx, &rules);

    // Every rule that walks files must have found some: a rule shipped without
    // a junk fixture would silently stop being covered by this harness.
    for (rule, scan) in rules.iter().zip(&scans) {
        match rule.kind {
            RuleKind::Files => assert!(
                scan.file_count > 0,
                "rule \"{}\" matched no junk: add a fixture in tests/support/fake_profile.rs",
                rule.id
            ),
            RuleKind::RecycleBin => {
                assert_eq!(scan.file_count, RECYCLE_ITEMS, "injected recycle-bin query");
                assert_eq!(scan.total_bytes, RECYCLE_BYTES);
            }
        }
    }
    let scanned: u64 = scans.iter().map(|s| s.file_count).sum();
    assert_eq!(
        scanned,
        fx.junk.len() as u64 + RECYCLE_ITEMS,
        "the scan must see exactly the junk (plus the injected bin items)"
    );

    assert_no_path_crosses_a_junction(&fx, &scans);

    let (report, trashed) = clean_all(&fx, &rules, CleanMode::Permanent);
    assert!(
        trashed.is_empty(),
        "Permanent mode must never call the recycle-bin move"
    );

    assert_junk_gone(&fx.junk, "native permanent");
    assert_sentinels_intact(&fx, "native permanent");
    assert_only_junk_disappeared(&before, &fx.snapshot(), &fx.junk);

    assert_eq!(
        report.deleted,
        fx.junk.len() as u64 + RECYCLE_ITEMS,
        "deleted count must match the junk list plus the emptied bin"
    );
    assert!(
        report.skipped.iter().all(|s| s.reason.contains("reparse")
            || s.reason.contains("junction")
            || s.reason.contains("outside")),
        "the only skipped entries may be indirections: {:?}",
        report.skipped
    );

    // The junctions were never traversed: their targets are untouched.
    assert!(fx.outside.join("secret.txt").exists());
    assert!(fx.outside2.join("also-secret.txt").exists());

    println!(
        "\n=== safety harness / native ===\n\
         rules exercised   : {}\n\
         junk removed      : {}\n\
         sentinels checked : {}\n\
         junctions planted : {}\n\
         bytes freed       : {}\n\
         elapsed           : {:?}\n",
        rules.len(),
        report.deleted,
        fx.sentinels.len(),
        fx.junctions.len(),
        report.freed_bytes,
        started.elapsed()
    );
}

#[test]
fn the_whole_catalogue_spares_user_data_and_never_crosses_a_junction() {
    let started = Instant::now();
    let mut fx = Fixture::build();
    let (native, winapp2) = full_catalogue(&fx);
    let ids: BTreeSet<&str> = winapp2.iter().map(|r| r.id.as_str()).collect();
    assert!(
        winapp2.len() >= 10,
        "the fixture must make at least ten Winapp2 entries detected, got {}",
        winapp2.len()
    );
    // The sample the fixture plants a DetectFile for. The registry probe
    // always answers false, so nothing else can slip in.
    for expected in [
        "winapp2.audacity",
        "winapp2.dropbox",
        "winapp2.github-desktop",
        "winapp2.obs-studio",
        "winapp2.postman",
        "winapp2.slack",
        "winapp2.spotify",
        "winapp2.telegram-desktop",
        "winapp2.vlc-media-player",
    ] {
        assert!(ids.contains(expected), "{expected} was not detected: {ids:?}");
    }

    // One concrete file per converted glob, so the Winapp2 rules actually
    // delete something instead of walking empty trees.
    let lookup = |n: &str| fx.lookup(n);
    let mut w2_patterns: Vec<String> = Vec::new();
    for rule in &winapp2 {
        w2_patterns.extend(resolved_paths_with(rule, &lookup).expect("converted rule resolves"));
    }
    fx.add_winapp2_junk(&w2_patterns);

    let mut rules = native.clone();
    rules.extend(winapp2.clone());

    let before = fx.snapshot();
    let scans = scan_all(&fx, &rules);
    let (report, trashed) = clean_all(&fx, &rules, CleanMode::Permanent);
    assert!(trashed.is_empty());

    assert_junk_gone(&fx.junk, "full catalogue");
    assert_sentinels_intact(&fx, "full catalogue");
    assert_only_junk_disappeared(&before, &fx.snapshot(), &fx.junk);

    assert!(fx.outside.join("secret.txt").exists(), "junction target");
    assert!(fx.outside2.join("also-secret.txt").exists(), "junction target");

    assert_no_path_crosses_a_junction(&fx, &scans);

    println!(
        "\n=== safety harness / native + winapp2 ===\n\
         native rules       : {}\n\
         winapp2 detected   : {}\n\
         winapp2 rule ids   : {}\n\
         junk removed       : {}\n\
         sentinels checked  : {}\n\
         junctions planted  : {} (targets intact, bait untouched)\n\
         elapsed            : {:?}\n",
        native.len(),
        winapp2.len(),
        winapp2
            .iter()
            .map(|r| r.id.as_str())
            .collect::<Vec<_>>()
            .join(", "),
        report.deleted,
        fx.sentinels.len(),
        fx.junctions.len(),
        started.elapsed()
    );
}

#[test]
fn trash_mode_hands_the_recycle_bin_exactly_the_junk() {
    let fx = Fixture::build();
    assert!(!fx.junk.is_empty(), "the fixture must plant junk");
    let rules = native_rules(&fx);
    let (report, trashed) = clean_all(&fx, &rules, CleanMode::Trash);

    // The injected trash function deletes nothing: everything must still be there.
    assert_sentinels_intact(&fx, "trash");
    for p in &fx.junk {
        assert!(p.exists(), "the injected trash must not delete: {}", p.display());
    }

    let handed: BTreeSet<String> = trashed.iter().map(|p| key(p)).collect();
    let expected: BTreeSet<String> = fx.junk.iter().map(|p| key(p)).collect();
    assert_eq!(
        handed, expected,
        "Trash mode must hand the recycle bin exactly the junk list"
    );
    assert_eq!(report.deleted, fx.junk.len() as u64 + RECYCLE_ITEMS);

    for s in &fx.sentinels {
        assert!(
            !handed.contains(&key(&s.path)),
            "SENTINEL handed to the recycle bin: {}",
            s.path.display()
        );
    }
}
