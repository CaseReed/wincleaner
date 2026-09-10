//! Conversion of the embedded Winapp2 rule base into native `Rule` values.
//!
//! Nothing here has a deletion path of its own: an entry becomes a
//! `rules::Rule`, is validated by `rules::check_rule_with`, and is then walked
//! and deleted by `scan.rs` and `clean.rs` exactly like a `rules.toml` rule.

/// One `[Name *]` section of the ini, reduced to the keys we can act on.
/// `RegKeyN`, `Default`, `DetectOS`, `LangSecRef` and `Section` are dropped at
/// parse time: WinCleaner is never a registry cleaner, and the rest carries
/// nothing we use.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct Winapp2Entry {
    pub section: String,
    pub file_keys: Vec<String>,
    pub exclude_keys: Vec<String>,
    pub detects: Vec<String>,
    pub detect_files: Vec<String>,
    pub warning: Option<String>,
}

/// Splits the ini into entries. Never fails: an unreadable line is skipped,
/// because one malformed upstream entry must not cost us the other 2,000.
///
/// Keys are kept in file order. `FileKey1`/`FileKey2` ordering carries no
/// meaning once the patterns are compiled into a single `GlobSet`, so the
/// numeric suffix is only used to recognise the key, never to sort.
pub fn parse_ini(src: &str) -> Vec<Winapp2Entry> {
    let mut out: Vec<Winapp2Entry> = Vec::new();
    let mut current: Option<Winapp2Entry> = None;

    for raw in src.trim_start_matches('\u{feff}').lines() {
        let line = raw.trim();
        if line.is_empty() || line.starts_with(';') {
            continue;
        }
        if let Some(name) = line.strip_prefix('[').and_then(|l| l.strip_suffix(']')) {
            if let Some(entry) = current.take() {
                out.push(entry);
            }
            current = Some(Winapp2Entry {
                section: name.trim().to_string(),
                ..Default::default()
            });
            continue;
        }
        let Some((key, value)) = line.split_once('=') else {
            continue;
        };
        // A key before the first section header belongs to no entry.
        let Some(entry) = current.as_mut() else {
            continue;
        };
        let value = value.trim();
        if value.is_empty() {
            continue;
        }
        let key = key.trim().to_ascii_uppercase();
        // `FileKey1` and `FileKey` are the same key; `DetectOS` keeps its `OS`
        // and therefore never collides with `Detect`.
        let base = key.trim_end_matches(|c: char| c.is_ascii_digit());
        match base {
            "FILEKEY" => entry.file_keys.push(value.to_string()),
            "EXCLUDEKEY" => entry.exclude_keys.push(value.to_string()),
            "DETECT" => entry.detects.push(value.to_string()),
            "DETECTFILE" => entry.detect_files.push(value.to_string()),
            "WARNING" => entry.warning = Some(value.to_string()),
            _ => {}
        }
    }

    if let Some(entry) = current.take() {
        out.push(entry);
    }
    out
}

use crate::rules::{
    check_rule_with, resolved_excludes_with, resolved_paths_with, EnvLookup, Risk, Rule, RuleError,
    RuleKind,
};
use serde::{Deserialize, Serialize};
use std::collections::HashMap;

/// The embedded community rule base, "Non-CCleaner" flavour. A few megabytes
/// of `&'static str`: parsed once at startup, never read from disk.
pub const WINAPP2_INI: &str = include_str!("../third_party/winapp2/Winapp2.ini");

/// The only Winapp2 variables that map onto a rules.toml variable. Everything
/// else is either outside the profile (`%ProgramFiles%`, `%WinDir%`,
/// `%SystemDrive%`, `%CommonAppData%`: they would need elevation) or user data
/// (`%Documents%`, `%Pictures%`, `%Downloads%`, `%Music%`, `%Video%`,
/// `%Public%`), and drops the key that uses it.
const VARIABLES: [(&str, &str); 4] = [
    ("%LOCALAPPDATA%", "%LOCALAPPDATA%"),
    ("%APPDATA%", "%APPDATA%"),
    ("%USERPROFILE%", "%USERPROFILE%"),
    ("%TEMP%", "%TEMP%"),
];

/// Rewrites a Winapp2 path onto a rules.toml path.
///
/// `None` when the leading variable is not one of the four allowed ones, when
/// the path carries a `..` segment, or when it carries a glob metacharacter we
/// cannot keep literal. `*` is kept — Winapp2 uses it for profile directories,
/// and `rules.toml` uses it the same way — but `[`, `]`, `{`, `}` and `?` in a
/// directory name would silently change what the glob matches, so the key is
/// dropped instead of guessed at.
pub fn map_path(raw: &str) -> Option<String> {
    let raw = raw.trim().trim_matches('"').trim_end_matches('\\');
    if !raw.starts_with('%') {
        return None;
    }
    let end = raw[1..].find('%')? + 1;
    let var = raw[..=end].to_ascii_uppercase();
    let mapped = VARIABLES
        .iter()
        .find(|(from, _)| *from == var)
        .map(|(_, to)| *to)?;
    let rest = &raw[end + 1..];
    if !(rest.is_empty() || rest.starts_with('\\')) {
        return None;
    }
    if rest.contains(['[', ']', '{', '}', '?']) {
        return None;
    }
    if rest.split('\\').any(|seg| seg == "..") {
        return None;
    }
    Some(format!("{mapped}{rest}"))
}

/// `*.*` means "every file" in Winapp2 and matches nothing in globset: it
/// becomes `*`. A `;`-separated list becomes one glob each. A spec carrying a
/// metacharacter we cannot keep literal is dropped.
fn specs(field: &str) -> Vec<String> {
    field
        .split(';')
        .map(str::trim)
        .filter(|s| !s.is_empty() && !s.contains(['[', ']', '{', '}']))
        .map(|s| if s == "*.*" { "*" } else { s }.to_string())
        .collect()
}

/// `FileKeyN=path|spec[|RECURSE][|REMOVESELF]` into one glob per spec.
///
/// `REMOVESELF` needs nothing extra: `clean.rs` already removes a directory a
/// rule has emptied. Only `RECURSE` changes the shape of the glob.
fn file_key_globs(value: &str) -> Option<Vec<String>> {
    let mut parts = value.split('|');
    let base = map_path(parts.next()?)?;
    let spec_field = parts.next().unwrap_or("*.*");
    let recurse = parts.any(|flag| flag.trim().eq_ignore_ascii_case("RECURSE"));
    let specs = specs(spec_field);
    if specs.is_empty() {
        return None;
    }
    Some(
        specs
            .into_iter()
            .map(|spec| {
                if recurse {
                    format!(r"{base}\**\{spec}")
                } else {
                    format!(r"{base}\{spec}")
                }
            })
            .collect(),
    )
}

/// `ExcludeKeyN=FILE|path|spec`, `PATH|path[|spec]` or `REG|key`.
///
/// `Some(vec![])` means "nothing to exclude on the file system" (a `REG|`
/// exclude: we never clean the registry, so ignoring it changes nothing).
/// `None` means "this exclude cannot be represented", and the caller must drop
/// the whole entry: keeping the rule without its exclude would delete more
/// than upstream intends. Specs are always applied recursively — excluding too
/// much is safe, excluding too little is not.
fn exclude_key_globs(value: &str) -> Option<Vec<String>> {
    let mut parts = value.split('|');
    let kind = parts.next()?.trim().to_ascii_uppercase();
    if kind == "REG" {
        return Some(Vec::new());
    }
    if kind != "FILE" && kind != "PATH" {
        return None;
    }
    let base = map_path(parts.next()?)?;
    match parts.next().map(str::trim).filter(|s| !s.is_empty()) {
        Some(field) => {
            let specs = specs(field);
            if specs.is_empty() {
                return None;
            }
            Some(
                specs
                    .into_iter()
                    .map(|spec| format!(r"{base}\**\{spec}"))
                    .collect(),
            )
        }
        // `FILE|path` designates one file; `PATH|path` a whole directory.
        None if kind == "FILE" => Some(vec![base]),
        None => Some(vec![format!(r"{base}\**\*")]),
    }
}

/// ASCII, lowercase, hyphen-separated. Non-ASCII characters are dropped: a
/// rule id crosses the IPC and lands in `localStorage`, it stays plain.
pub fn slug(label: &str) -> String {
    let mut out = String::new();
    let mut pending_dash = false;
    for c in label.chars() {
        if c.is_ascii_alphanumeric() {
            if pending_dash {
                out.push('-');
                pending_dash = false;
            }
            out.push(c.to_ascii_lowercase());
        } else if !out.is_empty() {
            pending_dash = true;
        }
    }
    if out.is_empty() {
        "entry".to_string()
    } else {
        out
    }
}

/// Section names repeat in the upstream file: the second `App` becomes
/// `app-2`, the third `app-3`. A duplicate id would be fatal for the whole
/// catalogue.
fn unique_slug(label: &str, used: &mut HashMap<String, u32>) -> String {
    let base = slug(label);
    let count = used.entry(base.clone()).or_insert(0);
    *count += 1;
    if *count == 1 {
        base
    } else {
        format!("{base}-{count}")
    }
}

/// A converted rule, with the detection keys that decide whether it is shown.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ConvertedRule {
    pub rule: Rule,
    pub detects: Vec<String>,
    pub detect_files: Vec<String>,
}

/// What the conversion kept and why it dropped the rest. Surfaced to the user
/// as a summary line: a catalogue that silently shrinks to nothing must be
/// visible.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct ConversionReport {
    pub retained: u32,
    /// No `FileKey` survived the variable allow-list, or the entry had none.
    pub dropped_no_file_key: u32,
    /// An `ExcludeKey` used a variable we refuse, or the rule does not apply
    /// on this machine (`MissingVar`, `OutsideProfile`).
    pub dropped_variable: u32,
    /// Refused by the rule validation or by the glob compiler.
    pub dropped_invalid: u32,
}

impl ConversionReport {
    pub fn dropped(&self) -> u32 {
        self.dropped_no_file_key + self.dropped_variable + self.dropped_invalid
    }
}

/// Validation of a converted rule: the same confinement as a rules.toml rule,
/// plus a glob compilation, because an upstream pattern we cannot compile must
/// be dropped here rather than surface as a scan error later.
fn accept(rule: &Rule, lookup: EnvLookup) -> Result<(), RuleError> {
    check_rule_with(rule, lookup)?;
    crate::scan::build_set(&resolved_paths_with(rule, lookup)?)?;
    crate::scan::build_set(&resolved_excludes_with(rule, lookup)?)?;
    Ok(())
}

/// Converts the whole ini. Never fails: a refused entry is counted, never
/// fatal.
pub fn convert_with(src: &str, lookup: EnvLookup) -> (Vec<ConvertedRule>, ConversionReport) {
    let mut report = ConversionReport::default();
    let mut used: HashMap<String, u32> = HashMap::new();
    let mut out: Vec<ConvertedRule> = Vec::new();

    for entry in parse_ini(src) {
        // Excludes first: one we cannot represent sinks the entry, and there
        // is no point slugging an id we are about to throw away.
        let mut exclude: Vec<String> = Vec::new();
        let mut exclude_failed = false;
        for raw in &entry.exclude_keys {
            match exclude_key_globs(raw) {
                Some(globs) => exclude.extend(globs),
                None => {
                    exclude_failed = true;
                    break;
                }
            }
        }
        if exclude_failed {
            report.dropped_variable += 1;
            continue;
        }

        let mut paths: Vec<String> = Vec::new();
        for raw in &entry.file_keys {
            if let Some(globs) = file_key_globs(raw) {
                paths.extend(globs);
            }
        }
        if paths.is_empty() {
            report.dropped_no_file_key += 1;
            continue;
        }

        let label = entry.section.trim_end_matches('*').trim().to_string();
        let rule = Rule {
            id: format!("winapp2.{}", unique_slug(&label, &mut used)),
            category: "Applications".to_string(),
            label,
            paths,
            exclude,
            risk: Risk::Medium,
            kind: RuleKind::Files,
            default_checked: false,
            note: entry.warning.clone(),
            unavailable_reason: None,
        };
        match accept(&rule, lookup) {
            Ok(()) => {}
            // The machine does not have this variable, or has it outside the
            // profile: the rule would be permanently greyed out. Dropped
            // rather than shown as noise among hundreds of others.
            Err(RuleError::MissingVar(_) | RuleError::OutsideProfile(_)) => {
                report.dropped_variable += 1;
                continue;
            }
            Err(_) => {
                report.dropped_invalid += 1;
                continue;
            }
        }
        report.retained += 1;
        out.push(ConvertedRule {
            rule,
            detects: entry.detects,
            detect_files: entry.detect_files,
        });
    }

    (out, report)
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Every shape the real file throws at the parser: CRLF, comments, a
    /// numbered and an unnumbered key, keys we ignore (LangSecRef, Section,
    /// DetectOS, Default, RegKey), a section name with a trailing `*`, and a
    /// non-ASCII section name.
    const FIXTURE: &str = "\
; a comment\r
[Test App *]\r
LangSecRef=3021\r
Detect=HKCU\\Software\\TestApp\r
Detect2=HKLM\\SOFTWARE\\TestApp\r
DetectFile=%LocalAppData%\\TestApp\r
DetectOS=6.1|\r
Default=False\r
Warning=This deletes the saved sessions.\r
FileKey1=%LocalAppData%\\TestApp\\Cache|*.*|RECURSE\r
FileKey2=%LocalAppData%\\TestApp\\Logs|*.log;*.tmp\r
ExcludeKey1=FILE|%LocalAppData%\\TestApp\\Cache|keep.dat\r
RegKey1=HKCU\\Software\\TestApp\\Recent\r
\r
[Café Cleaner]\r
FileKey1=%AppData%\\Cafe|*.*\r
";

    #[test]
    fn parses_sections_and_the_keys_we_care_about() {
        let entries = parse_ini(FIXTURE);
        assert_eq!(entries.len(), 2);

        let app = &entries[0];
        assert_eq!(app.section, "Test App *");
        assert_eq!(
            app.file_keys,
            vec![
                r"%LocalAppData%\TestApp\Cache|*.*|RECURSE",
                r"%LocalAppData%\TestApp\Logs|*.log;*.tmp",
            ]
        );
        assert_eq!(
            app.exclude_keys,
            vec![r"FILE|%LocalAppData%\TestApp\Cache|keep.dat"]
        );
        assert_eq!(
            app.detects,
            vec![r"HKCU\Software\TestApp", r"HKLM\SOFTWARE\TestApp"]
        );
        assert_eq!(app.detect_files, vec![r"%LocalAppData%\TestApp"]);
        assert_eq!(
            app.warning.as_deref(),
            Some("This deletes the saved sessions.")
        );

        assert_eq!(entries[1].section, "Café Cleaner");
    }

    /// RegKey, Default, DetectOS, LangSecRef and Section carry nothing we can
    /// act on: they must not leak into any of the collected lists.
    #[test]
    fn ignores_the_keys_we_do_not_support() {
        let entries = parse_ini(FIXTURE);
        let joined = format!("{:?}", entries[0]);
        assert!(!joined.contains("RegKey"), "{joined}");
        assert!(!joined.contains("6.1"), "{joined}");
        assert!(!joined.contains("3021"), "{joined}");
    }

    #[test]
    fn skips_a_byte_order_mark_a_key_outside_any_section_and_an_empty_value() {
        let src = "\u{feff}Detect=HKCU\\Orphan\n[A]\nFileKey1=\nFileKey2=%Temp%\\A|*.*\n";
        let entries = parse_ini(src);
        assert_eq!(entries.len(), 1);
        assert_eq!(entries[0].section, "A");
        assert_eq!(entries[0].file_keys, vec![r"%Temp%\A|*.*"]);
        assert!(entries[0].detects.is_empty());
    }

    /// The real upstream file: an 11-line `;` comment preamble before the
    /// first section, and CRLF line endings throughout. Proves both are
    /// handled, not just the crafted fixture above.
    #[test]
    fn parses_the_real_embedded_file() {
        let src = include_str!("../third_party/winapp2/Winapp2.ini");
        let entries = parse_ini(src);
        assert!(
            entries.len() >= 4000,
            "expected at least 4000 sections, got {}",
            entries.len()
        );
        assert!(
            entries.iter().any(|e| !e.file_keys.is_empty()),
            "expected at least one entry with a FileKey"
        );
    }
    fn fake_env(name: &str) -> Option<String> {
        match name {
            "USERPROFILE" => Some(r"C:\Users\Test".to_string()),
            "TEMP" => Some(r"C:\Users\Test\AppData\Local\Temp".to_string()),
            "LOCALAPPDATA" => Some(r"C:\Users\Test\AppData\Local".to_string()),
            "APPDATA" => Some(r"C:\Users\Test\AppData\Roaming".to_string()),
            _ => None,
        }
    }

    #[test]
    fn maps_only_the_four_profile_variables() {
        assert_eq!(
            map_path(r"%LocalAppData%\TestApp\Cache").as_deref(),
            Some(r"%LOCALAPPDATA%\TestApp\Cache")
        );
        assert_eq!(map_path(r"%appdata%\X").as_deref(), Some(r"%APPDATA%\X"));
        assert_eq!(
            map_path(r"%UserProfile%\AppData\Local\X").as_deref(),
            Some(r"%USERPROFILE%\AppData\Local\X")
        );
        assert_eq!(map_path(r"%Temp%\X\").as_deref(), Some(r"%TEMP%\X"));
        // Elevation, user data, and anything we cannot keep literal in a glob.
        assert_eq!(map_path(r"%ProgramFiles%\X"), None);
        assert_eq!(map_path(r"%CommonAppData%\X"), None);
        assert_eq!(map_path(r"%Documents%\X"), None);
        assert_eq!(map_path(r"%LocalAppData%\A[1]\X"), None);
        assert_eq!(map_path(r"%LocalAppData%\..\X"), None);
        assert_eq!(map_path(r"C:\Windows\Temp"), None);
    }

    #[test]
    fn slugs_are_ascii_lowercase_hyphenated() {
        assert_eq!(slug("Test App"), "test-app");
        assert_eq!(slug("Café Cleaner"), "caf-cleaner");
        assert_eq!(slug("7-Zip (x64)"), "7-zip-x64");
        assert_eq!(slug("***"), "entry");
    }

    #[test]
    fn converts_the_fixture_entry() {
        let (converted, report) = convert_with(FIXTURE, &fake_env);
        assert_eq!(report.retained, 2);
        let rule = &converted[0].rule;
        assert_eq!(rule.id, "winapp2.test-app");
        assert_eq!(rule.label, "Test App");
        assert_eq!(rule.category, "Applications");
        assert_eq!(rule.risk, crate::rules::Risk::Medium);
        assert!(!rule.default_checked);
        assert_eq!(rule.note.as_deref(), Some("This deletes the saved sessions."));
        assert_eq!(
            rule.paths,
            vec![
                // `*.*` becomes `*`, RECURSE becomes `\**\`.
                r"%LOCALAPPDATA%\TestApp\Cache\**\*",
                // No RECURSE: one glob per `;`-separated spec, non-recursive.
                r"%LOCALAPPDATA%\TestApp\Logs\*.log",
                r"%LOCALAPPDATA%\TestApp\Logs\*.tmp",
            ]
        );
        assert_eq!(
            rule.exclude,
            vec![r"%LOCALAPPDATA%\TestApp\Cache\**\keep.dat"]
        );
        assert_eq!(converted[0].detects.len(), 2);
        assert_eq!(converted[0].detect_files.len(), 1);
    }

    #[test]
    fn drops_an_entry_with_no_usable_file_key() {
        let src = "[Elevated]\nFileKey1=%ProgramFiles%\\X|*.*\n[Empty]\nDetect=HKCU\\X\n";
        let (converted, report) = convert_with(src, &fake_env);
        assert!(converted.is_empty());
        assert_eq!(report.dropped_no_file_key, 2);
        assert_eq!(report.dropped(), 2);
    }

    /// Silently discarding an exclude would delete MORE than upstream intends:
    /// the entry goes instead.
    #[test]
    fn an_exclude_we_cannot_represent_drops_the_whole_entry() {
        let src = "[A]\nFileKey1=%LocalAppData%\\A|*.*\nExcludeKey1=FILE|%ProgramFiles%\\A|keep.dat\n";
        let (converted, report) = convert_with(src, &fake_env);
        assert!(converted.is_empty());
        assert_eq!(report.dropped_variable, 1);
    }

    /// A `REG|` exclude concerns the registry, which we never clean: ignoring
    /// it changes nothing about which files are deleted.
    #[test]
    fn a_registry_exclude_is_ignored_without_dropping_the_entry() {
        let src = "[A]\nFileKey1=%LocalAppData%\\A|*.*\nExcludeKey1=REG|HKCU\\Software\\A\n";
        let (converted, report) = convert_with(src, &fake_env);
        assert_eq!(report.retained, 1);
        assert!(converted[0].rule.exclude.is_empty());
    }

    #[test]
    fn a_path_exclude_without_a_spec_excludes_the_whole_directory() {
        let src = "[A]\nFileKey1=%LocalAppData%\\A|*.*|RECURSE\nExcludeKey1=PATH|%LocalAppData%\\A\\keep\nExcludeKey2=FILE|%LocalAppData%\\A\\keep.dat\n";
        let (converted, _) = convert_with(src, &fake_env);
        assert_eq!(
            converted[0].rule.exclude,
            vec![
                r"%LOCALAPPDATA%\A\keep\**\*",
                r"%LOCALAPPDATA%\A\keep.dat",
            ]
        );
    }

    #[test]
    fn colliding_section_names_get_distinct_ids() {
        let src = "[App]\nFileKey1=%LocalAppData%\\A|*.*\n[App *]\nFileKey1=%LocalAppData%\\B|*.*\n[App]\nFileKey1=%LocalAppData%\\C|*.*\n";
        let (converted, _) = convert_with(src, &fake_env);
        let ids: Vec<&str> = converted.iter().map(|c| c.rule.id.as_str()).collect();
        assert_eq!(ids, vec!["winapp2.app", "winapp2.app-2", "winapp2.app-3"]);
    }

    /// The real file, converted with the real environment: every rule that
    /// survives must be as safe as a rules.toml rule, and there must be enough
    /// of them for the feature to be worth its weight.
    #[test]
    fn the_embedded_base_converts_into_valid_rules() {
        let (converted, report) = convert_with(WINAPP2_INI, &crate::rules::system_env);
        assert!(
            report.retained >= 500,
            "only {} rules retained (dropped: {})",
            report.retained,
            report.dropped()
        );
        assert_eq!(converted.len(), report.retained as usize);
        let mut ids = std::collections::HashSet::new();
        for c in &converted {
            assert!(c.rule.id.starts_with("winapp2."), "{}", c.rule.id);
            assert!(ids.insert(c.rule.id.clone()), "duplicate id {}", c.rule.id);
            assert_eq!(c.rule.category, "Applications");
            assert!(!c.rule.default_checked, "{}", c.rule.id);
            assert_eq!(c.rule.risk, crate::rules::Risk::Medium);
            assert!(c.rule.unavailable_reason.is_none(), "{}", c.rule.id);
            crate::rules::check_rule_with(&c.rule, &crate::rules::system_env)
                .unwrap_or_else(|e| panic!("{} failed validation: {e}", c.rule.id));
        }
    }
}
