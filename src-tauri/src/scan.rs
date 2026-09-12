use crate::rules::{
    canonical_profile_with, resolved_excludes_with, resolved_paths_with, system_env, EnvLookup,
    Rule, RuleError, RuleKind,
};
use globset::{GlobBuilder, GlobSet, GlobSetBuilder};
use serde::{Deserialize, Serialize};
use std::collections::BTreeMap;
use std::path::Path;
use walkdir::WalkDir;

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ScanResult {
    pub rule_id: String,
    pub file_count: u64,
    pub total_bytes: u64,
    pub paths: Vec<String>,
    pub skipped: u32,
}

/// Recycle bin query, injected to stay testable.
/// Returns `(file_count, total_bytes)`.
pub type RecycleQuery<'a> = &'a dyn Fn() -> Result<(u64, u64), String>;

/// `globset` treats `\` as an escape character: patterns and paths are
/// therefore always compared in `/` notation.
pub fn to_slash(path: &str) -> String {
    path.replace('\\', "/")
}

/// Metacharacters that `globset::escape` neutralises by wrapping them in a
/// one-character class.
const ESCAPABLE: [char; 6] = ['?', '*', '[', ']', '{', '}'];

/// Returns the literal form of the segment when it contains no wildcard.
///
/// The `[c]` sequences produced by `globset::escape` are literals, not
/// wildcards: treating them as wildcards would raise the walk root (a profile
/// named `a[b]c` would take it back to `C:/Users`, and therefore to every
/// profile on the machine).
fn literal_segment(seg: &str) -> Option<String> {
    let mut literal = String::new();
    let mut chars = seg.chars().peekable();
    while let Some(c) = chars.next() {
        match c {
            '[' => {
                let mut rest = chars.clone();
                match (rest.next(), rest.next()) {
                    (Some(lit), Some(']')) if ESCAPABLE.contains(&lit) => {
                        literal.push(lit);
                        chars.next();
                        chars.next();
                    }
                    // A real character class: that is a wildcard.
                    _ => return None,
                }
            }
            '*' | '?' | '{' => return None,
            c => literal.push(c),
        }
    }
    Some(literal)
}

/// Longest literal prefix of the pattern, in whole segments.
pub fn glob_root(pattern_slash: &str) -> String {
    let mut parts: Vec<String> = Vec::new();
    for seg in pattern_slash.split('/') {
        match literal_segment(seg) {
            Some(l) => parts.push(l),
            None => break,
        }
    }
    parts.join("/")
}

/// A walk root and the depth ceiling that goes with it.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct WalkRoot {
    pub path: String,
    /// Maximum depth under the root; `None` for a pattern containing `**`,
    /// which bounds nothing.
    pub depth: Option<usize>,
}

/// Walk roots of a pattern.
///
/// `glob_root` alone cuts at the first wildcard: for `User Data/*/Cache/**/*`
/// the walk started from the whole of `User Data` and enumerated tens of
/// thousands of entries — history, cookies, credentials — before the `GlobSet`
/// rejected them. Every single-wildcard level is therefore expanded by
/// enumerating the disk, up to the first `**`: the walk then starts from the
/// directory actually concerned. The last segment designates the files to
/// keep, there is nothing to expand there.
///
/// An entry that is not a real directory (junction, link) is never expanded:
/// `DirEntry::file_type` does not follow links.
fn pattern_roots(pattern: &str) -> Vec<WalkRoot> {
    let segs: Vec<&str> = pattern.split('/').collect();
    let mut i = 0;
    let mut base = String::new();
    while i < segs.len() {
        match literal_segment(segs[i]) {
            Some(l) => {
                if i > 0 {
                    base.push('/');
                }
                base.push_str(&l);
                i += 1;
            }
            None => break,
        }
    }

    let mut current = vec![base];
    while i + 1 < segs.len() && segs[i] != "**" {
        let Ok(glob) = GlobBuilder::new(segs[i]).literal_separator(true).build() else {
            break;
        };
        let matcher = glob.compile_matcher();
        let mut next = Vec::new();
        for root in &current {
            let Ok(entries) = std::fs::read_dir(root.replace('/', "\\")) else {
                continue;
            };
            for e in entries.flatten() {
                if !e.file_type().map(|t| t.is_dir()).unwrap_or(false) {
                    continue;
                }
                let name = e.file_name().to_string_lossy().to_string();
                if matcher.is_match(&name) {
                    next.push(format!("{root}/{name}"));
                }
            }
        }
        current = next;
        i += 1;
    }

    let depth = if segs[i..].contains(&"**") {
        None
    } else {
        Some(segs.len() - i)
    };
    current
        .into_iter()
        .map(|path| WalkRoot { path, depth })
        .collect()
}

pub(crate) fn build_set(patterns: &[String]) -> Result<GlobSet, RuleError> {
    let mut builder = GlobSetBuilder::new();
    for p in patterns {
        // `literal_separator`: a lone `*` must not cross a `/` (otherwise
        // `%TEMP%\*.txt`, non-recursive, would still descend into
        // subdirectories). `**` keeps its special treatment in globset and
        // still spans several levels.
        let glob = GlobBuilder::new(&to_slash(p))
            .literal_separator(true)
            .build()
            .map_err(|e| RuleError::Glob {
                pattern: p.clone(),
                cause: e.to_string(),
            })?;
        builder.add(glob);
    }
    builder.build().map_err(|e| RuleError::Glob {
        pattern: patterns.join(", "),
        cause: e.to_string(),
    })
}

/// `FILE_ATTRIBUTE_REPARSE_POINT`: junction, symbolic link, volume mount
/// point, cloud sync placeholder. All of them make a path designate something
/// other than the directory it appears to designate.
const FILE_ATTRIBUTE_REPARSE_POINT: u32 = 0x0000_0400;

pub(crate) fn is_reparse_point(md: &std::fs::Metadata) -> bool {
    #[cfg(windows)]
    {
        use std::os::windows::fs::MetadataExt;
        md.file_attributes() & FILE_ATTRIBUTE_REPARSE_POINT != 0
    }
    #[cfg(not(windows))]
    {
        md.file_type().is_symlink()
    }
}

/// Verdict of the containment check on a walk root.
#[derive(Debug, PartialEq, Eq)]
pub(crate) enum Containment {
    /// Real directory, resolved under the profile: safe to walk.
    Walkable,
    /// Absent from disk: the rule does not apply on this machine. This is not
    /// an anomaly, nothing is counted.
    Missing,
    /// Reparse point, or path resolved outside the profile: do not walk.
    Refused,
}

/// Confronts a walk root with the disk before descending into it.
///
/// This is the heart of the containment: the check done when rules are loaded
/// is textual, and `walkdir` descends into its walk root even when that root
/// is a reparse point. A junction placed on `%TEMP%` — which `mklink /J`
/// creates without any privilege — would otherwise be enough to have a whole
/// tree deleted outside the profile, or even outside the system volume.
pub(crate) fn confined_root(win_root: &str, profile_canon: &Path) -> Containment {
    let md = match std::fs::symlink_metadata(win_root) {
        Ok(md) => md,
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => return Containment::Missing,
        Err(_) => return Containment::Refused,
    };
    // Refused even when the target stays under the profile: we only walk real
    // directories, never an indirection.
    if is_reparse_point(&md) {
        return Containment::Refused;
    }
    match std::fs::canonicalize(win_root) {
        Ok(real) if real.starts_with(profile_canon) => Containment::Walkable,
        Ok(_) => Containment::Refused,
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => Containment::Missing,
        Err(_) => Containment::Refused,
    }
}

/// Minimal walk roots for a set of patterns.
pub(crate) fn walk_roots(patterns: &[String]) -> Vec<WalkRoot> {
    let mut all: Vec<WalkRoot> = patterns
        .iter()
        .flat_map(|p| pattern_roots(&to_slash(p)))
        .collect();
    all.sort_by(|a, b| a.path.cmp(&b.path));

    let mut minimal: Vec<WalkRoot> = Vec::new();
    for r in all {
        // A root contained in another would be walked twice: it is absorbed,
        // raising the depth ceiling of the one containing it accordingly. The
        // sort puts parents before their children.
        let parent = minimal
            .iter_mut()
            .find(|p| r.path == p.path || r.path.starts_with(&format!("{}/", p.path)));
        match parent {
            Some(p) => {
                let gap = r.path.matches('/').count() - p.path.matches('/').count();
                p.depth = match (p.depth, r.depth) {
                    (Some(a), Some(b)) => Some(a.max(b + gap)),
                    _ => None,
                };
            }
            None => minimal.push(r),
        }
    }
    minimal
}

/// Walks the rule patterns and returns, for each retained file, its Windows
/// path and its size. Unreadable entries, refused roots and reparse points
/// encountered are counted in `skipped` and never propagated.
fn collect(
    patterns: &[String],
    excludes: &[String],
    profile_canon: &Path,
) -> Result<(BTreeMap<String, u64>, u32), RuleError> {
    let include = build_set(patterns)?;
    let exclude = build_set(excludes)?;
    let mut found: BTreeMap<String, u64> = BTreeMap::new();
    let mut skipped: u32 = 0;

    for root in walk_roots(patterns) {
        let win_root = root.path.replace('/', "\\");
        match confined_root(&win_root, profile_canon) {
            Containment::Walkable => {}
            Containment::Missing => continue,
            Containment::Refused => {
                skipped += 1;
                continue;
            }
        }

        // `follow_links(false)` already stops junctions and symbolic links
        // under the root; `filter_entry` closes the other reparse points
        // (cloud placeholders, containers), which Rust does not classify as
        // links and into which `walkdir` would descend.
        let reparse_skipped = std::cell::Cell::new(0u32);
        let mut walk = WalkDir::new(&win_root).follow_links(false);
        // A pattern without `**` cannot match anything deeper than its number
        // of segments: no point in descending further.
        if let Some(depth) = root.depth {
            walk = walk.max_depth(depth);
        }
        let walk = walk.into_iter().filter_entry(|e| {
            if e.depth() > 0 && e.file_type().is_dir() {
                if let Ok(md) = e.metadata() {
                    if is_reparse_point(&md) {
                        reparse_skipped.set(reparse_skipped.get() + 1);
                        return false;
                    }
                }
            }
            true
        });

        for entry in walk {
            let entry = match entry {
                Ok(e) => e,
                Err(err) => {
                    // A missing root directory is not an anomaly: the rule
                    // simply does not apply on this machine.
                    if err.io_error().map(|e| e.kind()) == Some(std::io::ErrorKind::NotFound) {
                        continue;
                    }
                    skipped += 1;
                    continue;
                }
            };
            if !entry.file_type().is_file() {
                continue;
            }
            let win_path = entry.path().to_string_lossy().to_string();
            let slash = to_slash(&win_path);
            if !include.is_match(&slash) || exclude.is_match(&slash) {
                continue;
            }
            match entry.metadata() {
                Ok(md) => {
                    found.insert(win_path, md.len());
                }
                Err(_) => skipped += 1,
            }
        }
        skipped += reparse_skipped.get();
    }
    Ok((found, skipped))
}

pub fn scan_rule_with_api(
    rule: &Rule,
    lookup: EnvLookup,
    recycle: RecycleQuery,
) -> Result<ScanResult, RuleError> {
    if rule.kind == RuleKind::RecycleBin {
        return Ok(match recycle() {
            Ok((count, bytes)) => ScanResult {
                rule_id: rule.id.clone(),
                file_count: count,
                total_bytes: bytes,
                paths: Vec::new(),
                skipped: 0,
            },
            Err(_) => ScanResult {
                rule_id: rule.id.clone(),
                file_count: 0,
                total_bytes: 0,
                paths: Vec::new(),
                skipped: 1,
            },
        });
    }

    let patterns = resolved_paths_with(rule, lookup)?;
    let excludes = resolved_excludes_with(rule, lookup)?;
    let profile_canon = canonical_profile_with(lookup)?;
    let (found, skipped) = collect(&patterns, &excludes, &profile_canon)?;
    Ok(ScanResult {
        rule_id: rule.id.clone(),
        file_count: found.len() as u64,
        total_bytes: found.values().sum(),
        paths: found.keys().cloned().collect(),
        skipped,
    })
}

pub fn scan_rule(rule: &Rule) -> Result<ScanResult, RuleError> {
    scan_rule_with_api(rule, &system_env, &query_recycle_bin)
}

/// Queries the recycle bin of every volume. Read-only.
/// Returns `(item count, bytes used)`.
pub fn query_recycle_bin() -> Result<(u64, u64), String> {
    use windows::core::PCWSTR;
    use windows::Win32::UI::Shell::{SHQueryRecycleBinW, SHQUERYRBINFO};

    let mut info = SHQUERYRBINFO {
        cbSize: std::mem::size_of::<SHQUERYRBINFO>() as u32,
        i64Size: 0,
        i64NumItems: 0,
    };
    // PCWSTR::null() => every volume on the machine.
    unsafe { SHQueryRecycleBinW(PCWSTR::null(), &mut info) }
        .map_err(|e| format!("SHQueryRecycleBinW failed: {e}"))?;
    Ok((info.i64NumItems.max(0) as u64, info.i64Size.max(0) as u64))
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::rules::{Risk, Rule, RuleKind};
    use std::fs;
    use tempfile::TempDir;

    /// Builds a fake profile:
    ///   <tmp>\AppData\Local\Temp\a.txt      (3 bytes)
    ///   <tmp>\AppData\Local\Temp\sub\b.txt  (5 bytes)
    ///   <tmp>\AppData\Local\Temp\keep.log   (7 bytes)
    fn fake_profile() -> TempDir {
        let dir = TempDir::new().unwrap();
        let temp = dir.path().join("AppData").join("Local").join("Temp");
        fs::create_dir_all(temp.join("sub")).unwrap();
        fs::write(temp.join("a.txt"), b"aaa").unwrap();
        fs::write(temp.join("sub").join("b.txt"), b"bbbbb").unwrap();
        fs::write(temp.join("keep.log"), b"ccccccc").unwrap();
        dir
    }

    fn lookup_for(root: &Path) -> impl Fn(&str) -> Option<String> + '_ {
        let root = root.to_string_lossy().to_string();
        move |name: &str| match name {
            "USERPROFILE" => Some(root.clone()),
            "TEMP" => Some(format!(r"{root}\AppData\Local\Temp")),
            "LOCALAPPDATA" => Some(format!(r"{root}\AppData\Local")),
            "APPDATA" => Some(format!(r"{root}\AppData\Roaming")),
            _ => None,
        }
    }

    fn temp_rule(paths: Vec<&str>, exclude: Vec<&str>) -> Rule {
        Rule {
            id: "windows.temp".into(),
            category: "System".into(),
            label: "Temporary files".into(),
            paths: paths.into_iter().map(String::from).collect(),
            exclude: exclude.into_iter().map(String::from).collect(),
            risk: Risk::Low,
            kind: RuleKind::Files,
            default_checked: true,
            note: None,
            label_fr: None,
            description_fr: None,
            category_fr: None,
            unavailable_reason: None,
        }
    }

    fn no_recycle() -> Result<(u64, u64), String> {
        panic!("the recycle bin API must not be called for a \"files\" rule");
    }

    /// Creates a directory junction. `mklink /J` requires no privilege, unlike
    /// `mklink /D`: the test runs without elevation. Both the link and its
    /// target live inside the test `TempDir`.
    fn junction(link: &Path, target: &Path) {
        let out = std::process::Command::new("cmd")
            .arg("/C")
            .arg("mklink")
            .arg("/J")
            .arg(link)
            .arg(target)
            .output()
            .expect("mklink could not be started");
        assert!(
            out.status.success(),
            "mklink /J failed: {}",
            String::from_utf8_lossy(&out.stderr)
        );
    }

    /// `<base>/profile` (fake profile), `<base>/outside/precious.txt`.
    fn profile_and_outside(base: &Path) -> (std::path::PathBuf, std::path::PathBuf) {
        let profile = base.join("profile");
        let outside = base.join("outside");
        fs::create_dir_all(profile.join("AppData").join("Local")).unwrap();
        fs::create_dir_all(&outside).unwrap();
        fs::write(outside.join("precious.txt"), b"precious").unwrap();
        (profile, outside)
    }

    #[test]
    fn a_root_that_is_a_junction_outside_the_profile_is_not_walked() {
        let base = TempDir::new().unwrap();
        let (profile, outside) = profile_and_outside(base.path());
        // %TEMP% is a junction to a directory outside the fake profile:
        // exactly the setup of a machine where Temp has been moved.
        junction(&profile.join("AppData").join("Local").join("Temp"), &outside);

        let lookup = lookup_for(&profile);
        let rule = temp_rule(vec![r"%TEMP%\**\*"], vec![]);
        let res = scan_rule_with_api(&rule, &lookup, &no_recycle).unwrap();

        assert_eq!(res.file_count, 0, "paths = {:?}", res.paths);
        assert_eq!(res.total_bytes, 0);
        assert_eq!(res.skipped, 1, "the refused root must be reported");
        assert!(outside.join("precious.txt").exists());
    }

    #[test]
    fn a_junction_under_the_root_is_not_followed() {
        let base = TempDir::new().unwrap();
        let (profile, outside) = profile_and_outside(base.path());
        let temp = profile.join("AppData").join("Local").join("Temp");
        fs::create_dir_all(&temp).unwrap();
        fs::write(temp.join("a.txt"), b"aaa").unwrap();
        junction(&temp.join("link"), &outside);

        let lookup = lookup_for(&profile);
        let rule = temp_rule(vec![r"%TEMP%\**\*"], vec![]);
        let res = scan_rule_with_api(&rule, &lookup, &no_recycle).unwrap();

        assert_eq!(res.file_count, 1, "paths = {:?}", res.paths);
        assert!(res.paths[0].ends_with("a.txt"));
        assert!(res.paths.iter().all(|p| !p.contains("precious")));
    }

    #[test]
    fn a_root_that_is_a_junction_into_the_profile_is_refused_too() {
        // Even when pointing inside the profile, a reparse point at the walk
        // root is refused: we only walk real directories.
        let base = TempDir::new().unwrap();
        let profile = base.path().join("profile");
        let elsewhere = profile.join("Elsewhere");
        fs::create_dir_all(profile.join("AppData").join("Local")).unwrap();
        fs::create_dir_all(&elsewhere).unwrap();
        fs::write(elsewhere.join("a.txt"), b"aaa").unwrap();
        junction(
            &profile.join("AppData").join("Local").join("Temp"),
            &elsewhere,
        );

        let lookup = lookup_for(&profile);
        let rule = temp_rule(vec![r"%TEMP%\**\*"], vec![]);
        let res = scan_rule_with_api(&rule, &lookup, &no_recycle).unwrap();

        assert_eq!(res.file_count, 0, "paths = {:?}", res.paths);
        assert_eq!(res.skipped, 1);
        assert!(elsewhere.join("a.txt").exists());
    }

    #[test]
    fn the_recent_items_rule_spares_custom_destinations() {
        // CustomDestinations holds the items the user pinned by right-clicking
        // a taskbar icon: hand-curated data, not junk.
        let dir = TempDir::new().unwrap();
        let recent = dir
            .path()
            .join("AppData")
            .join("Roaming")
            .join("Microsoft")
            .join("Windows")
            .join("Recent");
        fs::create_dir_all(recent.join("AutomaticDestinations")).unwrap();
        fs::create_dir_all(recent.join("CustomDestinations")).unwrap();
        fs::write(recent.join("doc.lnk"), b"aaa").unwrap();
        fs::write(
            recent
                .join("AutomaticDestinations")
                .join("a.automaticDestinations-ms"),
            b"bbbbb",
        )
        .unwrap();
        fs::write(
            recent
                .join("CustomDestinations")
                .join("pinned.customDestinations-ms"),
            b"ccccccc",
        )
        .unwrap();

        // The real embedded rule, not a copy: it is the one that must be kept
        // from descending back into CustomDestinations.
        let lookup = lookup_for(dir.path());
        let rules = crate::rules::load_rules_with(crate::rules::RULES_TOML, &lookup).unwrap();
        let rule = rules
            .iter()
            .find(|r| r.id == "windows.explorer-recent")
            .unwrap();
        let res = scan_rule_with_api(rule, &lookup, &no_recycle).unwrap();

        assert_eq!(res.file_count, 2, "paths = {:?}", res.paths);
        assert!(
            res.paths.iter().all(|p| !p.contains("CustomDestinations")),
            "pinned items must stay out of reach: {:?}",
            res.paths
        );
    }

    #[test]
    fn walk_roots_expand_the_single_wildcard_levels() {
        // `glob_root` cuts at the first wildcard: the walk root of a
        // `User Data/*/Cache/**/*` pattern was the whole of `User Data`, which
        // walkdir traversed entirely — History, Cookies, Login Data included —
        // before the GlobSet filtered. A single-wildcard level is expanded by
        // enumerating the disk.
        let dir = TempDir::new().unwrap();
        let ud = dir.path().join("User Data");
        for profile in ["Default", "Profile 1"] {
            fs::create_dir_all(ud.join(profile).join("Cache")).unwrap();
        }
        fs::create_dir_all(ud.join("Crashpad").join("very").join("deep")).unwrap();

        let pattern = format!(r"{}\User Data\*\Cache\**\*", dir.path().display());
        // The literal prefix alone — the old walk root — is the whole of
        // "User Data", which walkdir enumerated in full.
        assert_eq!(
            glob_root(&to_slash(&pattern)),
            to_slash(&ud.to_string_lossy())
        );
        let roots = walk_roots(&[pattern]);
        let mut paths: Vec<String> = roots.iter().map(|r| r.path.clone()).collect();
        paths.sort();
        let expected: Vec<String> = ["Default", "Profile 1"]
            .iter()
            .map(|p| to_slash(&ud.join(p).join("Cache").to_string_lossy()))
            .collect();
        assert_eq!(paths, expected);
        // `**`: no depth ceiling under the expanded root.
        assert!(roots.iter().all(|r| r.depth.is_none()));
    }

    #[test]
    fn a_pattern_without_a_double_star_caps_the_depth() {
        let dir = fake_profile();
        let temp = to_slash(
            &dir.path()
                .join("AppData")
                .join("Local")
                .join("Temp")
                .to_string_lossy(),
        );
        let roots = walk_roots(&[format!(r"{}\*.txt", temp.replace('/', "\\"))]);
        assert_eq!(roots.len(), 1);
        assert_eq!(roots[0].path, temp);
        assert_eq!(roots[0].depth, Some(1));
    }

    #[test]
    fn a_root_contained_in_another_is_absorbed_and_raises_the_ceiling() {
        let dir = fake_profile();
        let temp = dir.path().join("AppData").join("Local").join("Temp");
        let roots = walk_roots(&[
            format!(r"{}\*.txt", temp.display()),
            format!(r"{}\sub\*.txt", temp.display()),
        ]);
        assert_eq!(roots.len(), 1);
        assert_eq!(roots[0].path, to_slash(&temp.to_string_lossy()));
        assert_eq!(roots[0].depth, Some(2));
    }

    #[test]
    fn glob_root_cuts_at_the_first_wildcard() {
        assert_eq!(glob_root("C:/Users/T/Temp/**/*"), "C:/Users/T/Temp");
        assert_eq!(glob_root("C:/Users/T/E/thumb_*.db"), "C:/Users/T/E");
        assert_eq!(glob_root("C:/Users/T/Temp"), "C:/Users/T/Temp");
    }

    #[test]
    fn glob_root_treats_escaped_metacharacters_as_literals() {
        // `a[[]b[]]c` is the escaped form of `a[b]c`: the walk root must be
        // the real directory, not `C:/Users`.
        assert_eq!(
            glob_root("C:/Users/a[[]b[]]c/Temp/**/*"),
            "C:/Users/a[b]c/Temp"
        );
        // A real character class written by the rule stays a wildcard.
        assert_eq!(glob_root("C:/Users/T/Temp/[ab]*.txt"), "C:/Users/T/Temp");
    }

    #[test]
    fn a_profile_with_brackets_does_not_spill_onto_the_neighbouring_profile() {
        // Two sibling profiles: the name of the targeted one contains `[` and
        // `]`, which would form a character class matching the neighbour.
        let base = TempDir::new().unwrap();
        let target = base.path().join("a[b]c");
        let neighbour = base.path().join("abc");
        for profile in [&target, &neighbour] {
            let temp = profile.join("AppData").join("Local").join("Temp");
            fs::create_dir_all(&temp).unwrap();
        }
        fs::write(
            target
                .join("AppData")
                .join("Local")
                .join("Temp")
                .join("a.txt"),
            b"aaa",
        )
        .unwrap();
        fs::write(
            neighbour
                .join("AppData")
                .join("Local")
                .join("Temp")
                .join("intruder.txt"),
            b"bbbbb",
        )
        .unwrap();

        let lookup = lookup_for(&target);
        let rule = temp_rule(vec![r"%TEMP%\**\*"], vec![]);
        let res = scan_rule_with_api(&rule, &lookup, &no_recycle).unwrap();

        assert_eq!(res.file_count, 1, "paths = {:?}", res.paths);
        assert_eq!(res.total_bytes, 3);
        assert!(res.paths[0].ends_with("a.txt"));
        assert!(
            res.paths.iter().all(|p| !p.contains("intruder.txt")),
            "the walk spilled onto the neighbouring profile: {:?}",
            res.paths
        );
    }

    #[test]
    fn scan_counts_files_and_bytes() {
        let dir = fake_profile();
        let lookup = lookup_for(dir.path());
        let rule = temp_rule(vec![r"%TEMP%\**\*"], vec![]);
        let res = scan_rule_with_api(&rule, &lookup, &no_recycle).unwrap();
        assert_eq!(res.rule_id, "windows.temp");
        assert_eq!(res.file_count, 3);
        assert_eq!(res.total_bytes, 3 + 5 + 7);
        assert_eq!(res.skipped, 0);
        assert_eq!(res.paths.len(), 3);
    }

    #[test]
    fn scan_applies_the_exclusions() {
        let dir = fake_profile();
        let lookup = lookup_for(dir.path());
        let rule = temp_rule(vec![r"%TEMP%\**\*"], vec![r"%TEMP%\*.log"]);
        let res = scan_rule_with_api(&rule, &lookup, &no_recycle).unwrap();
        assert_eq!(res.file_count, 2);
        assert_eq!(res.total_bytes, 3 + 5);
    }

    #[test]
    fn scan_does_not_retain_directories() {
        let dir = fake_profile();
        let lookup = lookup_for(dir.path());
        let rule = temp_rule(vec![r"%TEMP%\**\*"], vec![]);
        let res = scan_rule_with_api(&rule, &lookup, &no_recycle).unwrap();
        assert!(res.paths.iter().all(|p| !p.ends_with("sub")));
    }

    #[test]
    fn scanning_a_non_recursive_glob_does_not_descend() {
        let dir = fake_profile();
        let lookup = lookup_for(dir.path());
        let rule = temp_rule(vec![r"%TEMP%\*.txt"], vec![]);
        let res = scan_rule_with_api(&rule, &lookup, &no_recycle).unwrap();
        assert_eq!(res.file_count, 1);
        assert_eq!(res.total_bytes, 3);
    }

    #[test]
    fn scanning_a_missing_directory_returns_an_empty_result() {
        let dir = TempDir::new().unwrap();
        let lookup = lookup_for(dir.path());
        let rule = temp_rule(vec![r"%TEMP%\**\*"], vec![]);
        let res = scan_rule_with_api(&rule, &lookup, &no_recycle).unwrap();
        assert_eq!(res.file_count, 0);
        assert_eq!(res.total_bytes, 0);
        assert_eq!(res.skipped, 0);
    }

    #[test]
    fn scan_does_not_count_twice_a_file_covered_by_two_globs() {
        let dir = fake_profile();
        let lookup = lookup_for(dir.path());
        let rule = temp_rule(vec![r"%TEMP%\**\*", r"%TEMP%\*.txt"], vec![]);
        let res = scan_rule_with_api(&rule, &lookup, &no_recycle).unwrap();
        assert_eq!(res.file_count, 3);
        assert_eq!(res.total_bytes, 15);
    }

    #[test]
    fn scan_propagates_a_rule_error() {
        let rule = temp_rule(vec![r"%WINDIR%\*"], vec![]);
        let dir = TempDir::new().unwrap();
        let lookup = lookup_for(dir.path());
        assert!(scan_rule_with_api(&rule, &lookup, &no_recycle).is_err());
    }

    #[test]
    fn scanning_a_recycle_bin_rule_uses_the_api_and_ignores_the_globs() {
        let dir = fake_profile();
        let lookup = lookup_for(dir.path());
        let rule = Rule {
            id: "windows.recycle-bin".into(),
            category: "System".into(),
            label: "Recycle Bin".into(),
            paths: vec![],
            exclude: vec![],
            risk: Risk::Low,
            kind: RuleKind::RecycleBin,
            default_checked: false,
            note: None,
            label_fr: None,
            description_fr: None,
            category_fr: None,
            unavailable_reason: None,
        };
        let query = || Ok((7u64, 700u64));
        let res = scan_rule_with_api(&rule, &lookup, &query).unwrap();
        assert_eq!(res.file_count, 7);
        assert_eq!(res.total_bytes, 700);
        assert!(res.paths.is_empty());
        assert_eq!(res.skipped, 0);
    }

    #[test]
    fn a_failed_recycle_bin_query_counts_one_skipped() {
        let dir = fake_profile();
        let lookup = lookup_for(dir.path());
        let rule = Rule {
            id: "windows.recycle-bin".into(),
            category: "System".into(),
            label: "Recycle Bin".into(),
            paths: vec![],
            exclude: vec![],
            risk: Risk::Low,
            kind: RuleKind::RecycleBin,
            default_checked: false,
            note: None,
            label_fr: None,
            description_fr: None,
            category_fr: None,
            unavailable_reason: None,
        };
        let query = || Err("failure".to_string());
        let res = scan_rule_with_api(&rule, &lookup, &query).unwrap();
        assert_eq!(res.file_count, 0);
        assert_eq!(res.total_bytes, 0);
        assert_eq!(res.skipped, 1);
    }

    /// Environment-coupled test: it queries the machine's real recycle bin, so
    /// its result depends on the workstation and not only on the code. It
    /// checks one thing only: the FFI call neither panics nor returns an
    /// error. No assertion on the values, which vary.
    #[test]
    fn query_recycle_bin_does_not_panic() {
        // Read-only call on the real recycle bin: erases nothing.
        let res = query_recycle_bin();
        assert!(res.is_ok(), "SHQueryRecycleBinW failed: {res:?}");
    }
}
