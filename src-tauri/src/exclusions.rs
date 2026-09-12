//! User exclusions: paths the user picked out of a scan result and asked the
//! cleaner never to touch again.
//!
//! Three properties hold this module together, and none of them is optional:
//!
//! 1. **No path crosses the IPC boundary.** The front end names a file by its
//!    index into the last scan result of a rule (`commands::LastPaths`); the
//!    absolute path is looked up here, on the Rust side. That is the same
//!    invariant `scan`/`clean` already obey, extended to exclusions — a
//!    free-form glob editor was rejected in `docs/roadmap.md` for exactly this
//!    reason.
//! 2. **What is stored is a pattern, not an absolute path.** The resolved value
//!    of `%LOCALAPPDATA%`, `%APPDATA%`, `%TEMP%` or `%USERPROFILE%` is folded
//!    back into the variable, so the file stays valid if the profile moves and
//!    carries no account name. A path under none of the four is refused.
//! 3. **It fails closed.** A stored file that cannot be read or parsed aborts
//!    the scan and the clean (`load`), it never degrades to "no exclusions" —
//!    the failure mode of a silently ignored exclusion is deleting the very
//!    file the user asked to keep.

use crate::rules::{Rule, ALLOWED_VARS, EnvLookup};
use serde::{Deserialize, Serialize};
use std::path::{Path, PathBuf};

/// Name of the store, in `paths::config_dir` or under a sandbox root.
pub const EXCLUSIONS_FILE: &str = "exclusions.toml";

/// What the user pointed at: the file itself, or the directory holding it.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum Scope {
    File,
    Folder,
}

/// One stored exclusion. `pattern` is always in `%VAR%\…` form: see
/// `pattern_for`.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Exclusion {
    pub rule_id: String,
    pub pattern: String,
    /// `YYYY-MM-DD`, so the Settings list can say when this was added.
    pub added: String,
}

#[derive(Debug, Default, Serialize, Deserialize)]
struct ExclusionFile {
    #[serde(default)]
    exclusion: Vec<Exclusion>,
}

/// Days to `(year, month, day)`, proleptic Gregorian (Howard Hinnant's
/// `civil_from_days`). Written out rather than pulling in `chrono` or `time`
/// for one date a user reads and nothing computes against.
pub(crate) fn civil_from_days(days: i64) -> (i64, u32, u32) {
    let z = days + 719_468;
    let era = if z >= 0 { z } else { z - 146_096 } / 146_097;
    let doe = z - era * 146_097;
    let yoe = (doe - doe / 1460 + doe / 36_524 - doe / 146_096) / 365;
    let y = yoe + era * 400;
    let doy = doe - (365 * yoe + yoe / 4 - yoe / 100);
    let mp = (5 * doy + 2) / 153;
    let d = (doy - (153 * mp + 2) / 5 + 1) as u32;
    let m = if mp < 10 { mp + 3 } else { mp - 9 } as u32;
    (if m <= 2 { y + 1 } else { y }, m, d)
}

/// Today, UTC, as `YYYY-MM-DD`. A clock before the epoch reads as day zero
/// rather than panicking: the date is a label in a list, never a decision.
pub fn today() -> String {
    let secs = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|d| d.as_secs())
        .unwrap_or(0);
    let (y, m, d) = civil_from_days((secs / 86_400) as i64);
    format!("{y:04}-{m:02}-{d:02}")
}

/// Strips `resolved` from the front of `target` when it is a whole-segment
/// prefix, and returns the rest.
///
/// The comparison is ASCII-case-insensitive: Windows path casing differs on
/// the drive letter and on the 8.3/long-form boundary, all of which are ASCII.
/// Folding the full Unicode case would be worse, not better — `to_lowercase`
/// can change a string's byte length, which would make the slice below land
/// mid-character.
fn strip_prefix_ci(target: &str, resolved: &str) -> Option<String> {
    let resolved = resolved.trim_end_matches(['\\', '/']);
    if target.eq_ignore_ascii_case(resolved) {
        return Some(String::new());
    }
    let n = resolved.len();
    let head = target.get(..n)?;
    if !head.eq_ignore_ascii_case(resolved) {
        return None;
    }
    match target.as_bytes().get(n) {
        Some(b'\\') | Some(b'/') => Some(target[n + 1..].to_string()),
        _ => None,
    }
}

/// Turns an absolute path from a scan result back into a stored pattern.
///
/// The relative part is escaped segment by segment with `globset::escape`,
/// exactly as `rules::expand_env_with` escapes a variable's value: a file
/// literally named `report[1].txt` must match itself and not act as a
/// character class. `**` for a folder is appended *after* the escaping, so it
/// stays the one wildcard in the pattern.
///
/// The longest resolved prefix wins: `%LOCALAPPDATA%` sits under
/// `%USERPROFILE%`, and storing `%USERPROFILE%\AppData\Local\…` would be a
/// second spelling for one directory — the same aliasing `winapp2.rs`
/// normalises away.
pub fn pattern_for(path: &str, scope: Scope, lookup: EnvLookup) -> Result<String, String> {
    let path = path.replace('/', "\\");
    let target = match scope {
        Scope::File => path.clone(),
        Scope::Folder => Path::new(&path)
            .parent()
            .ok_or_else(|| format!("\"{path}\" has no parent directory"))?
            .to_string_lossy()
            .to_string(),
    };

    let mut candidates: Vec<(&str, String)> = ALLOWED_VARS
        .iter()
        .filter_map(|name| lookup(name).map(|value| (*name, value.replace('/', "\\"))))
        .collect();
    candidates.sort_by_key(|(_, value)| std::cmp::Reverse(value.trim_end_matches('\\').len()));

    let (name, rel) = candidates
        .iter()
        .find_map(|(name, value)| strip_prefix_ci(&target, value).map(|rel| (*name, rel)))
        .ok_or_else(|| {
            format!(
                "\"{target}\" is not under %TEMP%, %LOCALAPPDATA%, %APPDATA% or %USERPROFILE%, \
                 so it cannot be stored as an exclusion"
            )
        })?;

    let escaped: Vec<String> = rel
        .split('\\')
        .filter(|s| !s.is_empty())
        .map(globset::escape)
        .collect();
    let mut pattern = format!("%{name}%");
    for seg in &escaped {
        pattern.push('\\');
        pattern.push_str(seg);
    }
    if scope == Scope::Folder {
        pattern.push_str("\\**");
    }
    Ok(pattern)
}

/// Stable code, not a sentence, for the one refusal the window turns into its
/// own localized message — the same contract `update.rs` uses for its error
/// codes.
pub const RULE_ROOT_CODE: &str = "exclusion-is-rule-root";

/// Whether the folder holding `path` is one of the roots the rule walks.
///
/// Excluding a rule's own root collapses it to `%VAR%\**`, or to the very
/// directory the rule descends from: the rule then matches nothing at all, and
/// the checkbox still says it is on. A user who wants that wants the rule
/// unchecked, so this is refused rather than silently emptying it.
///
/// The comparison is against `scan::glob_root`, the literal prefix the walk
/// actually starts from — not against the raw pattern, which still carries its
/// wildcards.
pub fn folder_is_rule_root(path: &str, rule: &Rule, lookup: EnvLookup) -> Result<bool, String> {
    let path = path.replace('/', "\\");
    let folder = Path::new(&path)
        .parent()
        .ok_or_else(|| format!("\"{path}\" has no parent directory"))?
        .to_string_lossy()
        .to_string();
    let folder = folder.trim_end_matches('\\');
    let patterns = crate::rules::resolved_paths_with(rule, lookup).map_err(|e| e.to_string())?;
    Ok(patterns.iter().any(|pattern| {
        let root = crate::scan::glob_root(&crate::scan::to_slash(pattern)).replace('/', "\\");
        root.trim_end_matches('\\').eq_ignore_ascii_case(folder)
    }))
}

/// `pattern_for`, plus the one refusal that needs the rule to decide.
pub fn pattern_for_rule(
    path: &str,
    scope: Scope,
    rule: &Rule,
    lookup: EnvLookup,
) -> Result<String, String> {
    if scope == Scope::Folder && folder_is_rule_root(path, rule, lookup)? {
        return Err(RULE_ROOT_CODE.to_string());
    }
    pattern_for(path, scope, lookup)
}

/// Where the store lives. Under a sandbox root when one is active, so a
/// sandbox never reads nor writes the user's real exclusions — that override
/// is checked first and therefore wins over portable mode too. Otherwise
/// wherever `paths::config_dir` says: `%APPDATA%\WinCleaner`, or next to the
/// executable when the portable marker is there.
pub fn store_path(sandbox_root: Option<&Path>) -> Result<PathBuf, String> {
    match sandbox_root {
        Some(root) => Ok(root.join(EXCLUSIONS_FILE)),
        None => Ok(crate::paths::config_dir()?.join(EXCLUSIONS_FILE)),
    }
}

/// Reads the store. A missing file is an empty list — that is the first-run
/// state, not a failure. Anything else (unreadable, malformed) is an error the
/// caller must propagate: see the fail-closed note at the top of this module.
pub fn load(path: &Path) -> Result<Vec<Exclusion>, String> {
    let raw = match std::fs::read_to_string(path) {
        Ok(raw) => raw,
        Err(err) if err.kind() == std::io::ErrorKind::NotFound => return Ok(Vec::new()),
        Err(err) => return Err(format!("{} cannot be read: {err}", path.display())),
    };
    let parsed: ExclusionFile = toml::from_str(&raw)
        .map_err(|err| format!("{} is not a valid exclusions file: {err}", path.display()))?;
    Ok(parsed.exclusion)
}

/// Writes the store atomically: a full temporary file next to the target, then
/// a rename over it. A crash mid-write therefore leaves the previous list
/// intact instead of a half-written file that `load` would refuse — which,
/// failing closed, would block every later scan.
///
/// `std::fs::rename` maps to `MoveFileExW` with `MOVEFILE_REPLACE_EXISTING` on
/// Windows, so the destination is replaced rather than the call failing.
pub fn save(path: &Path, exclusions: &[Exclusion]) -> Result<(), String> {
    if let Some(dir) = path.parent() {
        std::fs::create_dir_all(dir)
            .map_err(|err| format!("{} cannot be created: {err}", dir.display()))?;
    }
    let body = toml::to_string_pretty(&ExclusionFile {
        exclusion: exclusions.to_vec(),
    })
    .map_err(|err| format!("the exclusions cannot be serialised: {err}"))?;

    let tmp = path.with_extension("toml.tmp");
    std::fs::write(&tmp, body)
        .map_err(|err| format!("{} cannot be written: {err}", tmp.display()))?;
    std::fs::rename(&tmp, path).map_err(|err| {
        let _ = std::fs::remove_file(&tmp);
        format!("{} cannot be replaced: {err}", path.display())
    })
}

/// Merges the stored patterns into the rules' own `exclude` list, in place.
///
/// Deliberately just an append: the merged list then goes through
/// `rules::resolved_excludes_with` like any `rules.toml` exclude, so a stored
/// pattern that is not a valid glob, or that resolves outside the profile,
/// is refused by the same code that refuses a bad rule.
pub fn apply(rules: &mut [Rule], exclusions: &[Exclusion]) {
    for rule in rules.iter_mut() {
        for ex in exclusions.iter().filter(|e| e.rule_id == rule.id) {
            rule.exclude.push(ex.pattern.clone());
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::rules::Risk;

    fn env(name: &str) -> Option<String> {
        match name {
            "USERPROFILE" => Some(r"C:\Users\jo".to_string()),
            "LOCALAPPDATA" => Some(r"C:\Users\jo\AppData\Local".to_string()),
            "APPDATA" => Some(r"C:\Users\jo\AppData\Roaming".to_string()),
            "TEMP" => Some(r"C:\Users\jo\AppData\Local\Temp".to_string()),
            _ => None,
        }
    }

    #[test]
    fn a_file_becomes_a_variable_pattern_not_an_absolute_path() {
        let got = pattern_for(r"C:\Users\jo\AppData\Local\Temp\a\b.log", Scope::File, &env)
            .unwrap();
        assert_eq!(got, r"%TEMP%\a\b.log");
        assert!(!got.contains("C:\\"), "no absolute path may be stored: {got}");
        assert!(!got.contains("jo"), "no account name may be stored: {got}");
    }

    #[test]
    fn a_folder_becomes_a_recursive_pattern_on_the_parent() {
        let got = pattern_for(r"C:\Users\jo\AppData\Local\Temp\a\b.log", Scope::Folder, &env)
            .unwrap();
        assert_eq!(got, r"%TEMP%\a\**");
    }

    /// `%TEMP%` resolves *under* `%LOCALAPPDATA%`, which resolves under
    /// `%USERPROFILE%`. One directory must have one spelling, so the deepest
    /// variable wins.
    #[test]
    fn the_longest_resolved_prefix_wins() {
        let got = pattern_for(r"C:\Users\jo\AppData\Local\Temp\x.tmp", Scope::File, &env).unwrap();
        assert!(got.starts_with("%TEMP%"), "{got}");

        let roaming =
            pattern_for(r"C:\Users\jo\AppData\Roaming\app\x.tmp", Scope::File, &env).unwrap();
        assert_eq!(roaming, r"%APPDATA%\app\x.tmp");
    }

    #[test]
    fn the_comparison_ignores_ascii_case() {
        let got = pattern_for(r"c:\users\JO\appdata\local\temp\x.tmp", Scope::File, &env).unwrap();
        assert_eq!(got, r"%TEMP%\x.tmp");
    }

    /// A file named like a glob must match itself and nothing else. The
    /// escaping is the same `globset::escape` the variable value goes through
    /// in `rules::expand_env_with`.
    #[test]
    fn glob_metacharacters_in_the_relative_part_are_escaped() {
        let got = pattern_for(r"C:\Users\jo\AppData\Local\Temp\report[1]*.txt", Scope::File, &env)
            .unwrap();
        assert_eq!(got, r"%TEMP%\report[[]1[]][*].txt");

        // And it survives the trip back through the rule pipeline as a literal.
        let set = crate::scan::build_set(&[crate::rules::expand_env_with(&got, &env).unwrap()])
            .expect("the escaped pattern must be a valid glob");
        assert!(set.is_match(crate::scan::to_slash(
            r"C:\Users\jo\AppData\Local\Temp\report[1]*.txt"
        )));
        assert!(!set.is_match(crate::scan::to_slash(
            r"C:\Users\jo\AppData\Local\Temp\report1x.txt"
        )));
    }

    #[test]
    fn a_path_outside_every_variable_is_refused() {
        let err = pattern_for(r"D:\elsewhere\x.tmp", Scope::File, &env).unwrap_err();
        assert!(err.contains("not under"), "{err}");
    }

    fn temp_rule() -> Rule {
        Rule {
            id: "windows.temp".to_string(),
            category: "System".to_string(),
            label: "Temporary files".to_string(),
            paths: vec![r"%TEMP%\**\*".to_string()],
            exclude: Vec::new(),
            risk: Risk::Low,
            kind: Default::default(),
            default_checked: true,
            note: None,
            unavailable_reason: None,
            label_fr: None,
            description_fr: None,
            category_fr: None,
        }
    }

    /// Excluding the folder of a file that sits directly in the rule's root
    /// would collapse the rule to `%TEMP%\**` and silently empty it, while its
    /// checkbox still read as on. The file itself stays excludable: that is
    /// the narrowing the user actually asked for.
    #[test]
    fn excluding_the_rule_root_as_a_folder_is_refused_but_the_file_is_not() {
        let rule = temp_rule();
        let at_root = r"C:\Users\jo\AppData\Local\Temp\stray.tmp";

        let err = pattern_for_rule(at_root, Scope::Folder, &rule, &env).unwrap_err();
        assert_eq!(err, RULE_ROOT_CODE);

        assert_eq!(
            pattern_for_rule(at_root, Scope::File, &rule, &env).unwrap(),
            r"%TEMP%\stray.tmp",
        );
    }

    /// One level down is a genuine narrowing, not a disabling: it must still
    /// go through.
    #[test]
    fn excluding_a_folder_below_the_rule_root_is_allowed() {
        let rule = temp_rule();
        let nested = r"C:\Users\jo\AppData\Local\Temp\nested\installer.log";

        assert!(!folder_is_rule_root(nested, &rule, &env).unwrap());
        assert_eq!(
            pattern_for_rule(nested, Scope::Folder, &rule, &env).unwrap(),
            r"%TEMP%\nested\**",
        );
    }

    #[test]
    fn a_missing_store_is_an_empty_list_not_an_error() {
        let dir = tempfile::tempdir().unwrap();
        let got = load(&dir.path().join("absent.toml")).unwrap();
        assert!(got.is_empty());
    }

    /// The fail-closed guarantee: a store that exists but does not parse is an
    /// error, never a silent "no exclusions".
    #[test]
    fn a_corrupt_store_is_an_error() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join(EXCLUSIONS_FILE);
        std::fs::write(&path, "this is not = = toml").unwrap();
        let err = load(&path).unwrap_err();
        assert!(err.contains("not a valid exclusions file"), "{err}");
    }

    #[test]
    fn save_then_load_round_trips_and_leaves_no_temporary_behind() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("sub").join(EXCLUSIONS_FILE);
        let list = vec![Exclusion {
            rule_id: "windows.temp".to_string(),
            pattern: r"%TEMP%\keep\**".to_string(),
            added: "2026-09-12".to_string(),
        }];
        save(&path, &list).unwrap();
        assert_eq!(load(&path).unwrap(), list);
        assert!(!path.with_extension("toml.tmp").exists());
    }

    #[test]
    fn save_replaces_an_existing_store() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join(EXCLUSIONS_FILE);
        save(&path, &[]).unwrap();
        let list = vec![Exclusion {
            rule_id: "r".to_string(),
            pattern: r"%TEMP%\a.txt".to_string(),
            added: "2026-09-12".to_string(),
        }];
        save(&path, &list).unwrap();
        assert_eq!(load(&path).unwrap(), list);
    }

    #[test]
    fn the_sandbox_store_lives_under_the_sandbox_root() {
        let root = Path::new(r"C:\Temp\wincleaner-sandbox-1");
        assert_eq!(
            store_path(Some(root)).unwrap(),
            root.join(EXCLUSIONS_FILE),
            "a sandbox must never read the user's real exclusions"
        );
    }

    #[test]
    fn apply_appends_only_the_patterns_of_the_matching_rule() {
        let mut rules = vec![
            Rule {
                id: "a".to_string(),
                category: "c".to_string(),
                label: "A".to_string(),
                paths: vec![r"%TEMP%\*".to_string()],
                exclude: vec![r"%TEMP%\native.txt".to_string()],
                risk: Risk::Low,
                kind: Default::default(),
                default_checked: true,
                note: None,
                unavailable_reason: None,
                label_fr: None,
                description_fr: None,
                category_fr: None,
            },
            Rule {
                id: "b".to_string(),
                category: "c".to_string(),
                label: "B".to_string(),
                paths: vec![r"%TEMP%\*".to_string()],
                exclude: Vec::new(),
                risk: Risk::Low,
                kind: Default::default(),
                default_checked: true,
                note: None,
                unavailable_reason: None,
                label_fr: None,
                description_fr: None,
                category_fr: None,
            },
        ];
        apply(
            &mut rules,
            &[Exclusion {
                rule_id: "a".to_string(),
                pattern: r"%TEMP%\mine.txt".to_string(),
                added: "2026-09-12".to_string(),
            }],
        );
        assert_eq!(
            rules[0].exclude,
            vec![r"%TEMP%\native.txt", r"%TEMP%\mine.txt"],
            "the rule's own excludes must survive the merge"
        );
        assert!(rules[1].exclude.is_empty());
    }

    #[test]
    fn the_date_is_iso_and_matches_known_days() {
        assert_eq!(civil_from_days(0), (1970, 1, 1));
        assert_eq!(civil_from_days(20_708), (2026, 9, 12));
        assert_eq!(civil_from_days(19_814), (2024, 4, 1), "a leap year");
        let today = today();
        assert_eq!(today.len(), 10, "{today}");
        assert_eq!(today.matches('-').count(), 2, "{today}");
    }
}
