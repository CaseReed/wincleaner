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

use std::collections::{BTreeSet, HashMap};
use std::ops::{Deref, DerefMut};
use std::path::{Path, PathBuf};
use std::time::Instant;

use tempfile::TempDir;
use wincleaner_lib::clean::{clean_rule_with_trash, CleanMode, CleanReport};
use wincleaner_lib::commands::{clean_rules_with, scan_rules_with, ScanProgress};
use wincleaner_lib::rules::{load_rules_with, resolved_paths_with, Rule, RuleError, RuleKind};
use wincleaner_lib::sandbox::Fixture;
use wincleaner_lib::scan::{scan_rule_with_api, ScanResult};

/// The fixture plus the `TempDir` that owns its directory. The builder itself
/// now lives in the library (`wincleaner_lib::sandbox`), because the Sandbox
/// mode of the application builds the very same tree; `tempfile` stays a
/// dev-dependency, so the harness is what pairs it with a temporary directory.
/// Dropping this removes the whole tree, junctions included, even when an
/// assertion has just failed.
struct Fx {
    fixture: Fixture,
    _dir: TempDir,
}

impl Deref for Fx {
    type Target = Fixture;
    fn deref(&self) -> &Fixture {
        &self.fixture
    }
}

impl DerefMut for Fx {
    fn deref_mut(&mut self) -> &mut Fixture {
        &mut self.fixture
    }
}

fn fixture() -> Fx {
    let dir = TempDir::new().unwrap();
    let fixture = Fixture::build_in(dir.path()).expect("the fake profile must build");
    Fx { fixture, _dir: dir }
}

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
    wincleaner_lib::sandbox::native_rules(fx).expect("rules.toml must load against the fake profile")
}

/// Native rules plus the Winapp2 entries this fake profile makes "detected".
/// The registry probe always answers false: detection is decided by the files
/// the fixture created, never by what happens to be installed on the machine
/// running the test.
///
/// This is the builder Sandbox mode uses too (`commands::enter_sandbox`), so
/// what a user watches on their own machine is what this harness proves.
fn full_catalogue(fx: &Fixture) -> (Vec<Rule>, Vec<Rule>) {
    let (native, winapp2, _report) =
        wincleaner_lib::sandbox::full_catalogue(fx).expect("the sandbox catalogue must build");
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

/// The real `trash::delete` is private to `clean.rs` and reaches the shell,
/// hence the real Recycle Bin of the volume. Every test that must not move a
/// single file there passes THIS instead of a recorder: a `Permanent` run that
/// started calling the move would abort the suite on the spot rather than be
/// caught after the fact by a count.
///
/// This is the `forbidden_recycle_*` pattern of `clean.rs`'s unit tests,
/// extended to the third recycle-bin door. The two doors it cannot close from
/// an integration test are `clean::clean_rule` and `clean::clean_rule_with_api`
/// — both public, both wired to the real `trash_delete`, which is private and
/// therefore not shadowable from here. Nothing in this file names them; a test
/// that did would have to type the name.
fn forbidden_trash(path: &Path) -> Result<(), String> {
    panic!("the real recycle-bin move was reached with: {}", path.display());
}

/// The real cleaning path: `commands::clean_rules_with` driving
/// `clean::clean_rule_with_trash`, exactly what the `clean` command runs, with
/// only the environment, the two recycle-bin calls and the move-to-bin call
/// injected.
fn clean_all_with_trash(
    fx: &Fixture,
    rules: &[Rule],
    mode: CleanMode,
    trash: &dyn Fn(&Path) -> Result<(), String>,
) -> CleanReport {
    let lookup = |n: &str| fx.lookup(n);
    clean_rules_with(rules.to_vec(), mode, |rule, mode| {
        clean_rule_with_trash(rule, mode, &lookup, &recycle_query, &recycle_empty, trash)
            .map_err(|e| e.to_string())
    })
}

/// `Permanent` mode, with the move-to-bin call wired to the panic stub.
fn clean_all_permanent(fx: &Fixture, rules: &[Rule]) -> CleanReport {
    clean_all_with_trash(fx, rules, CleanMode::Permanent, &forbidden_trash)
}

/// `Trash` mode with a recording, non-deleting stand-in.
fn clean_all_trash_recording(fx: &Fixture, rules: &[Rule]) -> (CleanReport, Vec<PathBuf>) {
    let trashed: std::cell::RefCell<Vec<PathBuf>> = std::cell::RefCell::new(Vec::new());
    let trash = |p: &Path| {
        trashed.borrow_mut().push(p.to_path_buf());
        Ok(())
    };
    let report = clean_all_with_trash(fx, rules, CleanMode::Trash, &trash);
    (report, trashed.into_inner())
}

/// Windows file names are case-insensitive: the path a rule hands back carries
/// whatever case the directory was first created with, which is not always the
/// case the fixture spelled. Every set comparison here goes through this key.
fn key(path: &Path) -> String {
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
    let fx = fixture();
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
                "rule \"{}\" matched no junk: add a fixture in src/sandbox.rs",
                rule.id
            ),
            RuleKind::RecycleBin => {
                assert_eq!(scan.file_count, RECYCLE_ITEMS, "injected recycle-bin query");
                assert_eq!(scan.total_bytes, RECYCLE_BYTES);
            }
        }
    }
    // Containment first, arithmetic second: a walk that crossed a junction
    // must be reported as a breach of containment, not as a count that no
    // longer adds up.
    assert_no_path_crosses_a_junction(&fx, &scans);

    let scanned: u64 = scans.iter().map(|s| s.file_count).sum();
    assert_eq!(
        scanned,
        fx.junk.len() as u64 + RECYCLE_ITEMS,
        "the scan must see exactly the junk (plus the injected bin items)"
    );

    // `Permanent` mode: the move-to-bin call is the panic stub, so reaching it
    // aborts the test instead of being caught afterwards by a count.
    let report = clean_all_permanent(&fx, &rules);

    assert_junk_gone(&fx.junk_paths(), "native permanent");
    assert_sentinels_intact(&fx, "native permanent");
    assert_only_junk_disappeared(&before, &fx.snapshot(), &fx.junk_paths());

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
    let mut fx = fixture();
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
    let mut w2_patterns: Vec<(String, String)> = Vec::new();
    for rule in &winapp2 {
        for pattern in resolved_paths_with(rule, &lookup).expect("converted rule resolves") {
            w2_patterns.push((rule.id.clone(), pattern));
        }
    }
    // Every `continue` inside `add_winapp2_junk` is silent: without a floor on
    // what it actually created, a change that made them all fire would leave
    // the Winapp2 rules walking empty trees and the suite green.
    let materialised = fx.add_winapp2_junk(&w2_patterns);
    assert!(
        materialised >= 60,
        "the Winapp2 globs must materialise at least 60 junk files, got {materialised} \
         for {} patterns",
        w2_patterns.len()
    );

    let mut rules = native.clone();
    rules.extend(winapp2.clone());

    let before = fx.snapshot();
    let scans = scan_all(&fx, &rules);

    assert_no_path_crosses_a_junction(&fx, &scans);

    let scanned: u64 = scans.iter().map(|s| s.file_count).sum();
    assert_eq!(
        scanned,
        fx.junk.len() as u64 + RECYCLE_ITEMS,
        "the whole catalogue must see exactly the junk (plus the injected bin items)"
    );

    let report = clean_all_permanent(&fx, &rules);

    assert_junk_gone(&fx.junk_paths(), "full catalogue");
    assert_sentinels_intact(&fx, "full catalogue");
    assert_only_junk_disappeared(&before, &fx.snapshot(), &fx.junk_paths());

    assert!(fx.outside.join("secret.txt").exists(), "junction target");
    assert!(fx.outside2.join("also-secret.txt").exists(), "junction target");

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
    let fx = fixture();
    assert!(!fx.junk.is_empty(), "the fixture must plant junk");
    let rules = native_rules(&fx);
    let (report, trashed) = clean_all_trash_recording(&fx, &rules);

    // The injected trash function deletes nothing: everything must still be there.
    assert_sentinels_intact(&fx, "trash");
    for p in &fx.junk_paths() {
        assert!(p.exists(), "the injected trash must not delete: {}", p.display());
    }

    let handed: BTreeSet<String> = trashed.iter().map(|p| key(p)).collect();
    let expected: BTreeSet<String> = fx.junk_paths().iter().map(|p| key(p)).collect();
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

/// A rule set whose first entry climbs out of `%TEMP%` with `..` segments and
/// lands on the user's `Documents`. Loaded through `rules::load_rules_with`,
/// exactly as `rules.toml` is.
const ESCAPE_UP: &str = r#"
[[rule]]
id = "attack.parent-segment"
category = "System"
label = "Escapes upward"
paths = ["%TEMP%\\..\\..\\..\\Documents\\*"]
exclude = []
risk = "low"

[[rule]]
id = "windows.temp"
category = "System"
label = "Temporary files"
paths = ["%TEMP%\\**\\*"]
exclude = []
risk = "low"
"#;

// ---------------------------------------------------------------------------
// Guard-by-guard proofs.
//
// Each test below is built so that removing ONE guard from `src/` makes it
// fail. `docs/safety-harness.md` records the mutation run that checks this and
// the one guard that has no privilege-free construct to exercise it.
// ---------------------------------------------------------------------------

/// `scan::confined_root`. The junction is the walk root itself, which is the
/// only place that guard can act: `walkdir` descends into its own root even
/// when that root is a reparse point, and `follow_links(false)` does not stop
/// it there.
#[test]
fn a_junction_at_a_rule_root_is_refused_never_walked_and_never_deleted() {
    let mut fx = fixture();
    let root = fx.junction_over_crash_dumps();
    let bait = fx.outside3.join("bait.dmp");
    assert!(bait.exists(), "the fixture must plant the bait");

    let rules: Vec<Rule> = native_rules(&fx)
        .into_iter()
        .filter(|r| r.id == "windows.crash-dumps")
        .collect();
    assert_eq!(rules.len(), 1, "windows.crash-dumps must still exist");
    assert!(
        rules[0].unavailable_reason.is_none(),
        "the rule loads: the refusal happens on disk, not at load time"
    );

    let before = fx.snapshot();
    let scans = scan_all(&fx, &rules);

    assert_no_path_crosses_a_junction(&fx, &scans);
    assert!(
        scans[0].paths.is_empty(),
        "nothing may be scanned through a junction planted on the walk root: {:?}",
        scans[0].paths
    );
    assert_eq!(
        scans[0].file_count, 0,
        "the bait matches CrashDumps\\* and would be counted if the root were walked"
    );
    assert_eq!(
        scans[0].skipped, 1,
        "confined_root must refuse the reparse root AND report it as skipped"
    );

    let report = clean_all_permanent(&fx, &rules);
    assert_eq!(report.deleted, 0, "a refused root deletes nothing");
    assert_eq!(report.freed_bytes, 0);

    assert!(
        bait.exists(),
        "BAIT DELETED through a junction planted on the walk root: {}",
        bait.display()
    );
    assert_eq!(
        std::fs::read(&bait).unwrap(),
        Fixture::sentinel_content(&bait),
        "the bait was rewritten"
    );
    assert!(
        root.exists(),
        "the junction itself must survive: {}",
        root.display()
    );
    assert_sentinels_intact(&fx, "junction at a walk root");
    assert_only_junk_disappeared(&before, &fx.snapshot(), &[]);
}

/// `clean::deletable_path`. Scan and clean are split by a swap performed from
/// inside the injected deletion closure — i.e. AFTER the internal re-scan
/// `clean_rule_with_trash` runs, in the one window the re-scan cannot see.
///
/// The stand-in deletes for real (`remove_file`), so the victim survives only
/// if the guard refuses the path. `Trash` mode is used because it is the only
/// mode whose deletion call is injectable, and the guard runs before it.
#[test]
fn a_directory_swapped_for_a_junction_after_the_scan_is_refused_at_deletion() {
    let fx = fixture();
    let victim = fx.outside4.join("victim.txt");
    let scanned_victim = fx.toctou_victim_path();
    assert!(victim.exists() && scanned_victim.exists(), "fixture");

    let rules = native_rules(&fx);
    let scans = scan_all(&fx, &rules);
    assert_no_path_crosses_a_junction(&fx, &scans);
    let scanned: u64 = scans.iter().map(|s| s.file_count).sum();
    assert_eq!(
        scanned,
        fx.junk.len() as u64 + RECYCLE_ITEMS,
        "the scan must see exactly the junk (plus the injected bin items)"
    );
    let victim_key = key(&scanned_victim);
    assert!(
        scans
            .iter()
            .any(|s| s.paths.iter().any(|p| key(Path::new(p)) == victim_key)),
        "the scan must have recorded the path the swap will hijack"
    );

    // The swap fires on the first deletion of the run and never again. The
    // hijacked path sorts last inside %TEMP% (`zz_toctou`), so it is still
    // ahead of the cursor when the junction appears under it.
    let swapped = std::cell::Cell::new(false);
    let deleted: std::cell::RefCell<Vec<PathBuf>> = std::cell::RefCell::new(Vec::new());
    let trash = |p: &Path| {
        if !swapped.replace(true) {
            fx.swap_toctou_dir_for_junction();
        }
        deleted.borrow_mut().push(p.to_path_buf());
        std::fs::remove_file(p).map_err(|e| e.to_string())
    };
    let report = clean_all_with_trash(&fx, &rules, CleanMode::Trash, &trash);
    assert!(swapped.get(), "the swap must have been performed");

    assert!(
        victim.exists(),
        "VICTIM DELETED through a junction swapped in after the scan: {}",
        victim.display()
    );
    assert_eq!(
        std::fs::read(&victim).unwrap(),
        Fixture::sentinel_content(&victim),
        "the victim was rewritten"
    );
    let handed: BTreeSet<String> = deleted.borrow().iter().map(|p| key(p)).collect();
    assert!(
        !handed.contains(&victim_key),
        "the hijacked path reached the deletion call: deletable_path let it through"
    );
    assert!(
        report
            .skipped
            .iter()
            .any(|s| key(Path::new(&s.path)) == victim_key
                && s.reason.contains("outside the user profile")),
        "the report must carry the refusal for {}: {:?}",
        scanned_victim.display(),
        report.skipped
    );
    assert_sentinels_intact(&fx, "toctou swap");
}

/// `rules::normalize`. The `..` refusal lives at rule load, so it is proved
/// through the same loader the application uses, not by calling `normalize`
/// directly.
///
/// The path is chosen on purpose: `%TEMP%` is `<profile>\AppData\Local\Temp`,
/// so three `..` land exactly on `<profile>\Documents`. It passes the textual
/// `under_profile` check — only the `..` refusal stands between this rule set
/// and the user's documents.
#[test]
fn a_rule_set_carrying_a_parent_segment_is_rejected_by_the_loader() {
    let fx = fixture();
    let thesis = fx.profile.join("Documents").join("thesis.docx");
    assert!(thesis.exists(), "fixture");

    let lookup = |n: &str| fx.lookup(n);
    let err = load_rules_with(ESCAPE_UP, &lookup)
        .expect_err("a parent segment must make the whole rule set fail to load");
    assert!(
        matches!(err, RuleError::ParentSegment(_)),
        "expected ParentSegment, got {err:?}"
    );
    // Fatal, not merely disabled: a parent segment is a malformed rule set,
    // never a machine-specific condition like MissingVar or OutsideProfile.
    assert!(
        err.to_string().contains(".."),
        "the message must name the offending segment: {err}"
    );

    // The rules that remain valid still run, and still do not reach Documents.
    let rest: String = ESCAPE_UP
        .split("[[rule]]")
        .filter(|s| !s.contains("attack.parent-segment"))
        .collect::<Vec<_>>()
        .join("[[rule]]");
    let rules = load_rules_with(&rest, &lookup).expect("the remaining rule set loads");
    assert_eq!(rules.len(), 1, "one rule survives the removal");
    let report = clean_all_permanent(&fx, &rules);
    assert!(report.deleted > 0, "the remaining rule must still do its job");

    assert!(
        thesis.exists(),
        "USER DOCUMENT DELETED: {}",
        thesis.display()
    );
    assert_eq!(
        std::fs::read(&thesis).unwrap(),
        Fixture::sentinel_content(&thesis),
        "the user document was rewritten"
    );
    assert_sentinels_intact(&fx, "parent-segment rule set");
}
