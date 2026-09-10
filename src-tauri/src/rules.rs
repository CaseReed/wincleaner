use serde::{Deserialize, Serialize};
use std::cell::RefCell;
use std::collections::{HashMap, HashSet};
use std::fmt;

/// Contents of `src-tauri/rules.toml`, embedded in the binary.
pub const RULES_TOML: &str = include_str!("../rules.toml");

/// Exhaustive allow-list of the environment variables a rule path may use.
pub const ALLOWED_VARS: [&str; 4] = ["TEMP", "LOCALAPPDATA", "APPDATA", "USERPROFILE"];

/// Environment variable resolver. Injected so that tests never need the real
/// user profile.
pub type EnvLookup<'a> = &'a dyn Fn(&str) -> Option<String>;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum Risk {
    Low,
    Medium,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, Default)]
#[serde(rename_all = "kebab-case")]
pub enum RuleKind {
    #[default]
    Files,
    RecycleBin,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Rule {
    pub id: String,
    pub category: String,
    pub label: String,
    #[serde(default)]
    pub paths: Vec<String>,
    #[serde(default)]
    pub exclude: Vec<String>,
    pub risk: Risk,
    #[serde(default)]
    pub kind: RuleKind,
    /// Checkbox ticked on first launch. False for anything a user must never
    /// clean without having explicitly asked for it: irreversible, outside the
    /// profile, or hand-curated data.
    #[serde(default = "checked_by_default")]
    pub default_checked: bool,
    /// Free-text warning shown inline under the rule row. Comes from the
    /// Winapp2 `Warning=` key; `rules.toml` rules may set it too.
    #[serde(default)]
    pub note: Option<String>,
    /// Set when the rule does not apply on THIS machine: variable missing, or
    /// pointing outside the profile. The rule is loaded, shown greyed out with
    /// this reason, and never scanned nor cleaned. This is not a faulty
    /// rules.toml, so it is not fatal.
    #[serde(skip)]
    pub unavailable_reason: Option<String>,
}

fn checked_by_default() -> bool {
    true
}

#[derive(Debug, Deserialize)]
struct RuleFile {
    #[serde(default)]
    rule: Vec<Rule>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum RuleError {
    /// The path does not start with `%VAR%\`.
    NotVarPrefixed(String),
    /// Variable missing from the allow-list.
    UnknownVar(String),
    /// Variable on the allow-list but undefined in the environment.
    MissingVar(String),
    /// A `..` segment was found.
    ParentSegment(String),
    /// The resolved path escapes `%USERPROFILE%`.
    OutsideProfile(String),
    /// `%USERPROFILE%` does not resolve on disk: without a real reference no
    /// containment can be checked, so nothing is walked.
    UnresolvableProfile(String),
    /// Two rules share the same `id`.
    DuplicateId(String),
    /// A `kind = "files"` rule declares no path.
    EmptyPaths(String),
    /// TOML syntax or typing error.
    Toml(String),
    /// A path pattern is not a valid glob. Distinct from `Toml`: the file may
    /// be syntactically correct and the pattern still be wrong.
    Glob { pattern: String, cause: String },
}

impl fmt::Display for RuleError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            RuleError::NotVarPrefixed(p) => write!(
                f,
                "path \"{p}\" must start with a variable, for example %TEMP%\\"
            ),
            RuleError::UnknownVar(v) => write!(
                f,
                "variable %{v}% is not allowed (allowed: TEMP, LOCALAPPDATA, APPDATA, USERPROFILE)"
            ),
            RuleError::MissingVar(v) => {
                write!(f, "environment variable %{v}% is not defined")
            }
            RuleError::ParentSegment(p) => {
                write!(f, "path \"{p}\" contains a \"..\" segment")
            }
            RuleError::OutsideProfile(p) => {
                write!(f, "path \"{p}\" is outside the user profile")
            }
            RuleError::UnresolvableProfile(m) => {
                write!(f, "the user profile does not resolve on disk: {m}")
            }
            RuleError::DuplicateId(id) => write!(f, "rule id \"{id}\" is duplicated"),
            RuleError::EmptyPaths(id) => write!(
                f,
                "rule \"{id}\" is of kind \"files\" but declares no path"
            ),
            RuleError::Toml(m) => write!(f, "rules.toml is invalid: {m}"),
            RuleError::Glob { pattern, cause } => {
                write!(f, "invalid glob \"{pattern}\": {cause}")
            }
        }
    }
}

impl std::error::Error for RuleError {}

/// Splits the buffer returned by `GetLongPathNameW`. `None` when the call
/// failed (`written == 0`) or when the announced length exceeds the buffer —
/// the path may have grown between the two calls, and slicing out of bounds
/// would panic, which aborts the process (`panic = "abort"`).
fn long_path_from_buffer(buf: &[u16], written: u32) -> Option<String> {
    let written = written as usize;
    if written == 0 || written > buf.len() {
        return None;
    }
    Some(String::from_utf16_lossy(&buf[..written]))
}

/// Converts a Windows path (possibly in 8.3 short form, like the `%TEMP%`
/// Windows may hand out when the account name contains a space) to its long
/// form. If the path does not exist or the call fails, the input is returned
/// unchanged.
fn long_path(value: &str) -> String {
    use windows::core::PCWSTR;
    use windows::Win32::Storage::FileSystem::GetLongPathNameW;

    let wide: Vec<u16> = value.encode_utf16().chain(std::iter::once(0)).collect();
    let src = PCWSTR(wide.as_ptr());
    let len = unsafe { GetLongPathNameW(src, None) };
    if len == 0 {
        return value.to_string();
    }
    let mut buf = vec![0u16; len as usize];
    let written = unsafe { GetLongPathNameW(src, Some(&mut buf)) };
    long_path_from_buffer(&buf, written).unwrap_or_else(|| value.to_string())
}

/// Real resolution, backed by the process environment.
pub fn system_env(name: &str) -> Option<String> {
    let value = std::env::var(name).ok()?;
    if ALLOWED_VARS.contains(&name) {
        Some(long_path(&value))
    } else {
        Some(value)
    }
}

/// Wraps a lookup in a cache that lives as long as the returned closure.
///
/// Resolving one rule path asks the environment for the path's own variable
/// AND for `%USERPROFILE%`, and `system_env` pays two `GetLongPathNameW` calls
/// per answer. Over the thousands of rules the Winapp2 conversion builds, that
/// is tens of thousands of syscalls for four distinct answers. Deliberately not
/// a process-wide cache: a resolver is injected precisely so that a caller can
/// choose its environment, and a static cache would leak one caller's
/// environment into the next.
pub fn memoized_env<'a>(lookup: EnvLookup<'a>) -> impl Fn(&str) -> Option<String> + 'a {
    let cache: RefCell<HashMap<String, Option<String>>> = RefCell::new(HashMap::new());
    move |name: &str| {
        if let Some(hit) = cache.borrow().get(name) {
            return hit.clone();
        }
        let value = lookup(name);
        cache.borrow_mut().insert(name.to_string(), value.clone());
        value
    }
}

/// Replaces the leading `%VAR%`. A single variable is accepted, and only at
/// the front: a rule path always has the form `%VAR%\rest`.
pub fn expand_env_with(raw: &str, lookup: EnvLookup) -> Result<String, RuleError> {
    if !raw.starts_with('%') {
        return Err(RuleError::NotVarPrefixed(raw.to_string()));
    }
    let end = raw[1..]
        .find('%')
        .ok_or_else(|| RuleError::NotVarPrefixed(raw.to_string()))?
        + 1;
    let name = &raw[1..end];
    let rest = &raw[end + 1..];
    if !ALLOWED_VARS.contains(&name) {
        return Err(RuleError::UnknownVar(name.to_string()));
    }
    if !(rest.starts_with('\\') || rest.starts_with('/') || rest.is_empty()) {
        return Err(RuleError::NotVarPrefixed(raw.to_string()));
    }
    let value = lookup(name).ok_or_else(|| RuleError::MissingVar(name.to_string()))?;
    // Only the variable value is escaped: the suffix is written by the rule
    // and its wildcards must stay wildcards. `globset::escape` wraps every
    // metacharacter in a one-character class (`[*]`), never behind a
    // backslash: the escaping therefore survives the `\` to `/` rewrite done
    // further down the pipeline.
    let value = globset::escape(value.trim_end_matches(['\\', '/']));
    Ok(format!("{value}{rest}"))
}

/// Puts the path in Windows form (`\`) and refuses any `..` segment.
pub fn normalize(raw: &str) -> Result<String, RuleError> {
    let win = raw.replace('/', "\\");
    if win.split('\\').any(|seg| seg == "..") {
        return Err(RuleError::ParentSegment(raw.to_string()));
    }
    Ok(win)
}

pub(crate) fn under_profile(path: &str, profile: &str) -> bool {
    let p = path.to_lowercase();
    let root = profile.trim_end_matches('\\').to_lowercase();
    p == root || p.starts_with(&format!("{root}\\"))
}

fn resolve_one(raw: &str, lookup: EnvLookup) -> Result<String, RuleError> {
    let expanded = expand_env_with(raw, lookup)?;
    let normalized = normalize(&expanded)?;
    // The profile goes through `expand_env_with` so that it undergoes exactly
    // the same escaping as the path it is compared against.
    let profile = expand_env_with("%USERPROFILE%", lookup)?;
    let profile = normalize(&profile)?;
    if !under_profile(&normalized, &profile) {
        return Err(RuleError::OutsideProfile(raw.to_string()));
    }
    Ok(normalized)
}

/// Real user profile path, resolved on disk.
///
/// The containment checked when rules are loaded is purely textual: it says
/// nothing about what the disk actually does with a path (junction, link, 8.3
/// form). This value — and only this value — is the reference for the real
/// containment, both when walking and when deleting.
pub fn canonical_profile_with(lookup: EnvLookup) -> Result<std::path::PathBuf, RuleError> {
    let raw = lookup("USERPROFILE").ok_or_else(|| RuleError::MissingVar("USERPROFILE".into()))?;
    std::fs::canonicalize(&raw).map_err(|e| RuleError::UnresolvableProfile(format!("{raw}: {e}")))
}

pub fn resolved_paths_with(rule: &Rule, lookup: EnvLookup) -> Result<Vec<String>, RuleError> {
    rule.paths.iter().map(|p| resolve_one(p, lookup)).collect()
}

pub fn resolved_excludes_with(rule: &Rule, lookup: EnvLookup) -> Result<Vec<String>, RuleError> {
    rule.exclude.iter().map(|p| resolve_one(p, lookup)).collect()
}

/// The resolved, confined form of a rule: its paths and its excludes, both
/// absolute. Returned by `check_rule_with` so that a caller that needs them —
/// the Winapp2 converter compiles both into a `GlobSet` — does not resolve the
/// very same patterns a second time.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ResolvedRule {
    pub paths: Vec<String>,
    pub excludes: Vec<String>,
}

/// Resolution and confinement of a single rule, whether it comes from
/// `rules.toml` or was built in memory by the Winapp2 converter. The caller
/// decides what to do with the error: `load_rules_with` turns `MissingVar` and
/// `OutsideProfile` into an `unavailable_reason`, the converter drops the
/// entry.
pub fn check_rule_with(rule: &Rule, lookup: EnvLookup) -> Result<ResolvedRule, RuleError> {
    if rule.kind == RuleKind::Files && rule.paths.is_empty() {
        return Err(RuleError::EmptyPaths(rule.id.clone()));
    }
    Ok(ResolvedRule {
        paths: resolved_paths_with(rule, lookup)?,
        excludes: resolved_excludes_with(rule, lookup)?,
    })
}

/// Separates "rules.toml is badly written" — a bug in the binary, fatal —
/// from "this rule does not apply on this machine". A workstation where %TEMP%
/// is redirected to D:\Temp, or %APPDATA% to a network share by group policy,
/// are legitimate setups: they disable the rule concerned, they do not prevent
/// the application from starting.
pub fn load_rules_with(src: &str, lookup: EnvLookup) -> Result<Vec<Rule>, RuleError> {
    let parsed: RuleFile = toml::from_str(src).map_err(|e| RuleError::Toml(e.to_string()))?;
    let mut seen: HashSet<String> = HashSet::new();
    let mut out: Vec<Rule> = Vec::with_capacity(parsed.rule.len());
    for mut rule in parsed.rule {
        if !seen.insert(rule.id.clone()) {
            return Err(RuleError::DuplicateId(rule.id.clone()));
        }
        match check_rule_with(&rule, lookup) {
            Ok(_) => {}
            Err(e @ (RuleError::MissingVar(_) | RuleError::OutsideProfile(_))) => {
                rule.unavailable_reason = Some(e.to_string());
            }
            Err(e) => return Err(e),
        }
        out.push(rule);
    }
    Ok(out)
}

pub fn load_rules(src: &str) -> Result<Vec<Rule>, RuleError> {
    load_rules_with(src, &system_env)
}

/// Loads the embedded rules with the real environment. An error here is
/// fatal: the application must refuse to start.
pub fn embedded_rules() -> Result<Vec<Rule>, RuleError> {
    load_rules(RULES_TOML)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn fake_env(name: &str) -> Option<String> {
        match name {
            "USERPROFILE" => Some(r"C:\Users\Test".to_string()),
            "TEMP" => Some(r"C:\Users\Test\AppData\Local\Temp".to_string()),
            "LOCALAPPDATA" => Some(r"C:\Users\Test\AppData\Local".to_string()),
            "APPDATA" => Some(r"C:\Users\Test\AppData\Roaming".to_string()),
            _ => None,
        }
    }

    fn toml_one(body: &str) -> String {
        format!("[[rule]]\n{}\n", body)
    }

    #[test]
    fn expand_replaces_the_leading_variable() {
        let got = expand_env_with(r"%TEMP%\a\b", &fake_env).unwrap();
        assert_eq!(got, r"C:\Users\Test\AppData\Local\Temp\a\b");
    }

    #[test]
    fn expand_escapes_the_value_metacharacters_but_not_the_suffix() {
        let bracket_profile = |name: &str| match name {
            "USERPROFILE" | "TEMP" => Some(r"C:\Users\a[b]c".to_string()),
            _ => None,
        };
        let got = expand_env_with(r"%TEMP%\**\*", &bracket_profile).unwrap();
        // The value becomes literal, the `**\*` written by the rule stays a wildcard.
        assert_eq!(got, r"C:\Users\a[[]b[]]c\**\*");
    }

    #[test]
    fn expand_escapes_every_glob_metacharacter() {
        // The six globset metacharacters, all neutralised by a one-character
        // class — never by a backslash, which the `\` -> `/` rewrite done when
        // the glob is compiled would destroy.
        let exotic_profile = |name: &str| match name {
            "USERPROFILE" | "TEMP" => Some(r"C:\Users\a?b*c[d]e{f}g".to_string()),
            _ => None,
        };
        let got = expand_env_with(r"%TEMP%\*", &exotic_profile).unwrap();
        assert_eq!(got, r"C:\Users\a[?]b[*]c[[]d[]]e[{]f[}]g\*");
    }

    #[test]
    fn a_rule_whose_profile_contains_brackets_loads() {
        let src = toml_one(
            r#"id = "x.y"
category = "System"
label = "Test"
paths = ["%TEMP%\\**\\*"]
exclude = []
risk = "low""#,
        );
        let bracket_profile = |name: &str| match name {
            "USERPROFILE" | "TEMP" => Some(r"C:\Users\a[b]c".to_string()),
            _ => None,
        };
        assert!(load_rules_with(&src, &bracket_profile).is_ok());
    }

    #[test]
    fn expand_refuses_a_variable_outside_the_allow_list() {
        let err = expand_env_with(r"%WINDIR%\a", &fake_env).unwrap_err();
        assert!(matches!(err, RuleError::UnknownVar(ref v) if v == "WINDIR"));
    }

    #[test]
    fn expand_refuses_a_path_without_a_leading_variable() {
        let err = expand_env_with(r"C:\Windows\Temp\*", &fake_env).unwrap_err();
        assert!(matches!(err, RuleError::NotVarPrefixed(_)));
    }

    #[test]
    fn normalize_converts_slashes_and_refuses_parent_segments() {
        assert_eq!(normalize("C:/a/b").unwrap(), r"C:\a\b");
        let err = normalize(r"C:\a\..\b").unwrap_err();
        assert!(matches!(err, RuleError::ParentSegment(_)));
    }

    #[test]
    fn loads_a_valid_rule() {
        let src = toml_one(
            r#"id = "x.y"
category = "System"
label = "Test"
paths = ["%TEMP%\\**\\*"]
exclude = []
risk = "low""#,
        );
        let rules = load_rules_with(&src, &fake_env).unwrap();
        assert_eq!(rules.len(), 1);
        assert_eq!(rules[0].id, "x.y");
        assert_eq!(rules[0].risk, Risk::Low);
        assert_eq!(rules[0].kind, RuleKind::Files);
    }

    #[test]
    fn a_variable_outside_the_profile_disables_the_rule_without_blocking_loading() {
        // Corporate workstation where %TEMP% is redirected to D:\Temp, or
        // %APPDATA% to a share by group policy: this is not a faulty
        // rules.toml, it is the machine. The application must start.
        let src = toml_one(
            r#"id = "x.y"
category = "System"
label = "Test"
paths = ["%TEMP%\\**\\*"]
exclude = []
risk = "low""#,
        );
        let temp_elsewhere = |name: &str| match name {
            "TEMP" => Some(r"D:\Temp".to_string()),
            _ => fake_env(name),
        };
        let rules = load_rules_with(&src, &temp_elsewhere).unwrap();
        assert_eq!(rules.len(), 1);
        let reason = rules[0]
            .unavailable_reason
            .as_deref()
            .expect("the rule must be marked unavailable");
        assert!(
            reason.contains("outside the user profile"),
            "reason = {reason}"
        );
    }

    #[test]
    fn a_missing_variable_disables_the_rule_without_blocking_loading() {
        let src = toml_one(
            r#"id = "x.y"
category = "System"
label = "Test"
paths = ["%APPDATA%\\*"]
exclude = []
risk = "low""#,
        );
        let without_appdata = |name: &str| match name {
            "APPDATA" => None,
            _ => fake_env(name),
        };
        let rules = load_rules_with(&src, &without_appdata).unwrap();
        assert!(rules[0].unavailable_reason.is_some());
    }

    #[test]
    fn a_resolvable_rule_is_not_marked_unavailable() {
        let src = toml_one(
            r#"id = "x.y"
category = "System"
label = "Test"
paths = ["%TEMP%\\**\\*"]
exclude = []
risk = "low""#,
        );
        assert!(load_rules_with(&src, &fake_env).unwrap()[0]
            .unavailable_reason
            .is_none());
    }

    /// A structural error stays fatal: that is a bug in the binary, not a
    /// quirk of the machine.
    #[test]
    fn a_parent_segment_stays_fatal() {
        let src = toml_one(
            r#"id = "x.y"
category = "System"
label = "Test"
paths = ["%TEMP%\\..\\..\\Windows\\*"]
exclude = []
risk = "low""#,
        );
        assert!(matches!(
            load_rules_with(&src, &fake_env).unwrap_err(),
            RuleError::ParentSegment(_)
        ));
    }

    #[test]
    fn refuses_an_unknown_variable_in_a_rule() {
        let src = toml_one(
            r#"id = "x.y"
category = "System"
label = "Test"
paths = ["%SYSTEMROOT%\\**\\*"]
exclude = []
risk = "low""#,
        );
        let err = load_rules_with(&src, &fake_env).unwrap_err();
        assert!(matches!(err, RuleError::UnknownVar(ref v) if v == "SYSTEMROOT"));
    }

    #[test]
    fn refuses_a_parent_segment_in_a_rule() {
        let src = toml_one(
            r#"id = "x.y"
category = "System"
label = "Test"
paths = ["%TEMP%\\..\\..\\Windows\\*"]
exclude = []
risk = "low""#,
        );
        let err = load_rules_with(&src, &fake_env).unwrap_err();
        assert!(matches!(err, RuleError::ParentSegment(_)));
    }

    #[test]
    fn refuses_a_duplicate_id() {
        let src = format!(
            "{}{}",
            toml_one(
                r#"id = "x.y"
category = "System"
label = "A"
paths = ["%TEMP%\\*"]
exclude = []
risk = "low""#
            ),
            toml_one(
                r#"id = "x.y"
category = "System"
label = "B"
paths = ["%TEMP%\\*"]
exclude = []
risk = "low""#
            )
        );
        let err = load_rules_with(&src, &fake_env).unwrap_err();
        assert!(matches!(err, RuleError::DuplicateId(ref v) if v == "x.y"));
    }

    #[test]
    fn refuses_an_unknown_risk() {
        let src = toml_one(
            r#"id = "x.y"
category = "System"
label = "Test"
paths = ["%TEMP%\\*"]
exclude = []
risk = "high""#,
        );
        assert!(matches!(
            load_rules_with(&src, &fake_env).unwrap_err(),
            RuleError::Toml(_)
        ));
    }

    #[test]
    fn the_recycle_bin_rule_has_no_path() {
        let src = toml_one(
            r#"id = "windows.recycle-bin"
category = "System"
label = "Recycle Bin"
kind = "recycle-bin"
paths = []
exclude = []
risk = "low""#,
        );
        let rules = load_rules_with(&src, &fake_env).unwrap();
        assert_eq!(rules[0].kind, RuleKind::RecycleBin);
        assert!(rules[0].paths.is_empty());
    }

    #[test]
    fn refuses_a_files_rule_without_a_path() {
        let src = toml_one(
            r#"id = "x.y"
category = "System"
label = "Test"
paths = []
exclude = []
risk = "low""#,
        );
        assert!(matches!(
            load_rules_with(&src, &fake_env).unwrap_err(),
            RuleError::EmptyPaths(_)
        ));
    }

    #[test]
    fn a_rule_is_checked_by_default_unless_stated_otherwise() {
        let src = toml_one(
            r#"id = "x.y"
category = "System"
label = "Test"
paths = ["%TEMP%\\*"]
exclude = []
risk = "low""#,
        );
        assert!(load_rules_with(&src, &fake_env).unwrap()[0].default_checked);

        let src = toml_one(
            r#"id = "x.y"
category = "System"
label = "Test"
paths = ["%TEMP%\\*"]
exclude = []
risk = "low"
default_checked = false"#,
        );
        assert!(!load_rules_with(&src, &fake_env).unwrap()[0].default_checked);
    }

    #[test]
    fn irreversible_rules_are_not_checked_by_default() {
        // The shortest path to data loss was: open, Analyze, Clean. Two
        // clicks, and the recycle bin of a plugged-in USB stick was emptied.
        let rules = load_rules_with(RULES_TOML, &fake_env).unwrap();
        let unchecked: Vec<&str> = rules
            .iter()
            .filter(|r| !r.default_checked)
            .map(|r| r.id.as_str())
            .collect();
        assert_eq!(
            unchecked,
            vec![
                "windows.recycle-bin",
                "windows.explorer-recent",
                "windows.crash-dumps"
            ]
        );
    }

    #[test]
    fn the_embedded_rules_toml_is_valid() {
        let rules = load_rules_with(RULES_TOML, &fake_env).unwrap();
        assert_eq!(rules.len(), 10);
        // Declared risk included: any risk change changes the deletion mode in
        // Auto, so it must be a deliberate test change.
        let seen: Vec<(&str, Risk)> = rules.iter().map(|r| (r.id.as_str(), r.risk)).collect();
        assert_eq!(
            seen,
            vec![
                ("windows.temp", Risk::Low),
                ("windows.recycle-bin", Risk::Low),
                ("windows.thumbnails", Risk::Low),
                ("windows.explorer-recent", Risk::Medium),
                ("windows.crash-dumps", Risk::Medium),
                ("edge.cache", Risk::Low),
                ("chrome.cache", Risk::Low),
                ("firefox.cache", Risk::Low),
                ("npm.cache", Risk::Low),
                ("pip.cache", Risk::Low),
            ]
        );
    }

    /// A path this pattern is guaranteed to match.
    fn example_from(pattern: &str) -> String {
        pattern.replace(r"**\*", r"x\y").replace('*', "x")
    }

    #[test]
    fn no_embedded_rule_overlaps_another() {
        // Two overlapping rules count the same bytes twice in the
        // "Reclaimable" total, and the second one fails to delete what the
        // first already deleted: the report then shows bogus "Skipped" entries
        // for files that were handled correctly.
        let rules = load_rules_with(RULES_TOML, &fake_env).unwrap();
        let file_rules: Vec<&Rule> = rules.iter().filter(|r| r.kind == RuleKind::Files).collect();
        for a in &file_rules {
            let set = crate::scan::build_set(&resolved_paths_with(a, &fake_env).unwrap()).unwrap();
            for b in &file_rules {
                if a.id == b.id {
                    continue;
                }
                for pattern in resolved_paths_with(b, &fake_env).unwrap() {
                    let example = crate::scan::to_slash(&example_from(&pattern));
                    assert!(
                        !set.is_match(&example),
                        "\"{}\" matches \"{example}\", which belongs to \"{}\"",
                        a.id,
                        b.id
                    );
                }
            }
        }
    }

    #[test]
    fn long_path_converts_a_short_path_to_its_long_form() {
        use windows::Win32::Storage::FileSystem::GetShortPathNameW;

        let dir = tempfile::tempdir().unwrap();
        let long_dir = dir.path().join("Long Name With Spaces");
        std::fs::create_dir(&long_dir).unwrap();
        // `tempfile` builds its path from %TEMP% as-is, which may already be
        // an 8.3 short form on this machine: we go through `canonicalize` to
        // get a long reference independent of `long_path`, the function under
        // test.
        let canonical = std::fs::canonicalize(&long_dir).unwrap();
        let long_dir = canonical
            .to_str()
            .unwrap()
            .trim_start_matches(r"\\?\")
            .to_string();

        let mut wide: Vec<u16> = long_dir.encode_utf16().chain(std::iter::once(0)).collect();
        let mut buf = vec![0u16; 260];
        let len = unsafe {
            GetShortPathNameW(windows::core::PCWSTR(wide.as_mut_ptr()), Some(&mut buf))
        };
        assert!(len > 0, "GetShortPathNameW failed");
        let short: String = String::from_utf16_lossy(&buf[..len as usize]);

        if short.eq_ignore_ascii_case(&long_dir) {
            // 8.3 name generation disabled on this volume: nothing to convert.
            assert_eq!(long_path(&long_dir), long_dir);
            return;
        }

        let got = long_path(&short);
        let got_trim = got.trim_end_matches('\\');
        let want_trim = long_dir.trim_end_matches('\\');
        assert!(
            got_trim.eq_ignore_ascii_case(want_trim),
            "got={got_trim} want={want_trim}"
        );
    }

    #[test]
    fn long_path_from_buffer_refuses_an_out_of_bounds_length() {
        // `GetLongPathNameW` can report a length larger than the buffer if the
        // path grew between the two calls: slicing the buffer would panic, and
        // `panic = "abort"` kills the process.
        let buf = [0x41u16, 0x42];
        assert_eq!(long_path_from_buffer(&buf, 5), None);
        assert_eq!(long_path_from_buffer(&buf, 0), None);
        assert_eq!(long_path_from_buffer(&buf, 2), Some("AB".to_string()));
        assert_eq!(long_path_from_buffer(&buf, 1), Some("A".to_string()));
    }

    #[test]
    fn long_path_returns_the_input_unchanged_when_the_path_does_not_exist() {
        let input = r"C:\path\that\does\not\exist\ABCDEF~1";
        assert_eq!(long_path(input), input);
    }

    #[test]
    fn the_embedded_rules_load_with_the_real_environment() {
        let rules = load_rules_with(RULES_TOML, &system_env).unwrap();
        assert_eq!(rules.len(), 10);
    }

    #[test]
    fn check_rule_accepts_a_rule_built_in_memory_and_refuses_an_unknown_variable() {
        // The Winapp2 converter builds `Rule` values by hand: they must go
        // through exactly the same confinement as a rules.toml rule, not a
        // second, looser copy of it.
        let mut rule = Rule {
            id: "winapp2.test".into(),
            category: "Applications".into(),
            label: "Test".into(),
            paths: vec![r"%LOCALAPPDATA%\TestApp\**\*".into()],
            exclude: vec![],
            risk: Risk::Medium,
            kind: RuleKind::Files,
            default_checked: false,
            note: None,
            unavailable_reason: None,
        };
        assert!(check_rule_with(&rule, &fake_env).is_ok());

        rule.paths = vec![r"%WINDIR%\Temp\*".into()];
        assert!(matches!(
            check_rule_with(&rule, &fake_env).unwrap_err(),
            RuleError::UnknownVar(_)
        ));

        rule.paths = vec![r"%LOCALAPPDATA%\TestApp\*".into()];
        rule.exclude = vec![r"%PROGRAMFILES%\x\*".into()];
        assert!(matches!(
            check_rule_with(&rule, &fake_env).unwrap_err(),
            RuleError::UnknownVar(_)
        ));

        rule.exclude = vec![];
        rule.paths = vec![];
        assert!(matches!(
            check_rule_with(&rule, &fake_env).unwrap_err(),
            RuleError::EmptyPaths(_)
        ));
    }

    #[test]
    fn a_note_survives_the_toml_round_trip_and_defaults_to_none() {
        let src = toml_one(
            r#"id = "x.y"
category = "System"
label = "Test"
paths = ["%TEMP%\\*"]
exclude = []
risk = "low"
note = "Closes the saved sessions.""#,
        );
        assert_eq!(
            load_rules_with(&src, &fake_env).unwrap()[0].note.as_deref(),
            Some("Closes the saved sessions.")
        );
        assert_eq!(
            load_rules_with(RULES_TOML, &fake_env).unwrap()[0].note,
            None
        );
    }

    #[test]
    fn resolved_paths_returns_absolute_paths() {
        let rule = Rule {
            id: "x.y".into(),
            category: "System".into(),
            label: "Test".into(),
            paths: vec![r"%TEMP%\**\*".into()],
            exclude: vec![],
            risk: Risk::Low,
            kind: RuleKind::Files,
            default_checked: true,
            note: None,
            unavailable_reason: None,
        };
        let got = resolved_paths_with(&rule, &fake_env).unwrap();
        assert_eq!(got, vec![r"C:\Users\Test\AppData\Local\Temp\**\*"]);
    }
}
