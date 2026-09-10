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

use crate::rules::{check_rule_with, EnvLookup, Risk, Rule, RuleError, RuleKind};
use serde::{Deserialize, Serialize};
use std::collections::HashSet;

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
    // A second variable further down the path (`%USERPROFILE%\AppData%\...`,
    // `...\%UserName%`: upstream typos) would stay literal, and an inert
    // literal directory silently matches nothing.
    if rest.contains('%') {
        return None;
    }
    if rest.split('\\').any(|seg| seg == "..") {
        return None;
    }
    Some(format!("{mapped}{rest}"))
}

/// A spec we can keep literal in a glob. `[`, `]`, `{` and `}` would silently
/// change what the pattern matches.
fn representable(spec: &str) -> bool {
    !spec.contains(['[', ']', '{', '}'])
}

/// `*.*` means "every file" in Winapp2 and matches nothing in globset.
fn spec_glob(spec: &str) -> String {
    if spec == "*.*" { "*" } else { spec }.to_string()
}

fn split_specs(field: &str) -> impl Iterator<Item = &str> {
    field.split(';').map(str::trim).filter(|s| !s.is_empty())
}

/// Specs of a `FileKey`: a `;`-separated list becomes one glob each, and a
/// spec we cannot represent is dropped — that only narrows what the rule
/// deletes.
fn specs(field: &str) -> Vec<String> {
    split_specs(field)
        .filter(|s| representable(s))
        .map(spec_glob)
        .collect()
}

/// Specs of an `ExcludeKey`: `None` as soon as ONE spec is unrepresentable.
/// Dropping it would keep the entry with a weaker exclude, and delete a file
/// upstream asks us to keep.
fn exclude_specs(field: &str) -> Option<Vec<String>> {
    split_specs(field)
        .map(|s| representable(s).then(|| spec_glob(s)))
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

/// Why an `ExcludeKey` could not be turned into globs. Both drop the entry —
/// keeping a rule without its exclude would delete more than upstream intends
/// — but they are counted apart, because one says "this path is outside what
/// we clean" and the other "we cannot express this pattern".
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum ExcludeDrop {
    /// The path does not start with one of the four variables we map.
    Variable,
    /// Unknown exclude kind, or a spec we cannot keep literal in a glob.
    Unrepresentable,
}

/// `ExcludeKeyN=FILE|path|spec`, `PATH|path[|spec]` or `REG|key`.
///
/// An empty `Vec` means "nothing to exclude on the file system" (a `REG|`
/// exclude: we never clean the registry, so ignoring it changes nothing).
/// Specs are always applied recursively — excluding too much is safe,
/// excluding too little is not.
fn exclude_key_globs(value: &str) -> Result<Vec<String>, ExcludeDrop> {
    let mut parts = value.split('|');
    let kind = parts
        .next()
        .ok_or(ExcludeDrop::Unrepresentable)?
        .trim()
        .to_ascii_uppercase();
    if kind == "REG" {
        return Ok(Vec::new());
    }
    if kind != "FILE" && kind != "PATH" {
        return Err(ExcludeDrop::Unrepresentable);
    }
    let raw = parts.next().ok_or(ExcludeDrop::Unrepresentable)?;
    let base = map_path(raw).ok_or(ExcludeDrop::Variable)?;
    match parts.next().map(str::trim).filter(|s| !s.is_empty()) {
        Some(field) => {
            let specs = exclude_specs(field).ok_or(ExcludeDrop::Unrepresentable)?;
            if specs.is_empty() {
                return Err(ExcludeDrop::Unrepresentable);
            }
            Ok(specs
                .into_iter()
                .map(|spec| format!(r"{base}\**\{spec}"))
                .collect())
        }
        // `FILE|path` designates one file; `PATH|path` a whole directory.
        None if kind == "FILE" => Ok(vec![base]),
        None => Ok(vec![format!(r"{base}\**\*")]),
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
/// catalogue, so the ids already issued are remembered rather than counted:
/// `[App]`, `[App]`, `[App 2]` would otherwise mint `app-2` twice.
fn unique_slug(label: &str, used: &mut HashSet<String>) -> String {
    let base = slug(label);
    if used.insert(base.clone()) {
        return base;
    }
    let mut n: u32 = 2;
    loop {
        let candidate = format!("{base}-{n}");
        if used.insert(candidate.clone()) {
            return candidate;
        }
        n += 1;
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
    /// An `ExcludeKey` path used a variable outside the four we map.
    pub dropped_variable: u32,
    /// An `ExcludeKey` we cannot express as a glob (unknown kind, or a spec
    /// carrying a metacharacter we cannot keep literal). Kept apart from
    /// `dropped_variable`: this one is a limit of our pattern language, not of
    /// the paths we accept to clean.
    pub dropped_exclude: u32,
    /// Refused by the rule validation — including a variable this machine does
    /// not define or one resolving outside the profile — or by the glob
    /// compiler.
    pub dropped_invalid: u32,
}

impl ConversionReport {
    pub fn dropped(&self) -> u32 {
        self.dropped_no_file_key
            + self.dropped_variable
            + self.dropped_exclude
            + self.dropped_invalid
    }
}

/// Validation of a converted rule: the same confinement as a rules.toml rule,
/// plus a glob compilation, because an upstream pattern we cannot compile must
/// be dropped here rather than surface as a scan error later. The patterns
/// resolved by `check_rule_with` are the ones compiled: resolving them a second
/// time would double the cost of the conversion for nothing.
fn accept(rule: &Rule, lookup: EnvLookup) -> Result<(), RuleError> {
    let resolved = check_rule_with(rule, lookup)?;
    crate::scan::build_set(&resolved.paths)?;
    crate::scan::build_set(&resolved.excludes)?;
    Ok(())
}

/// Converts the whole ini. Never fails: a refused entry is counted, never
/// fatal.
pub fn convert_with(src: &str, lookup: EnvLookup) -> (Vec<ConvertedRule>, ConversionReport) {
    // Every path of every entry is resolved against `%USERPROFILE%` and its own
    // variable: without a cache the real file costs tens of thousands of
    // `GetLongPathNameW` calls. The cache lives exactly as long as this call.
    let memoized = crate::rules::memoized_env(lookup);
    let lookup: EnvLookup = &memoized;

    let mut report = ConversionReport::default();
    let mut used: HashSet<String> = HashSet::new();
    let mut out: Vec<ConvertedRule> = Vec::new();

    for entry in parse_ini(src) {
        // Excludes first: one we cannot represent sinks the entry, and there
        // is no point slugging an id we are about to throw away.
        let mut exclude: Vec<String> = Vec::new();
        let mut exclude_failed = None;
        for raw in &entry.exclude_keys {
            match exclude_key_globs(raw) {
                Ok(globs) => exclude.extend(globs),
                Err(reason) => {
                    exclude_failed = Some(reason);
                    break;
                }
            }
        }
        match exclude_failed {
            Some(ExcludeDrop::Variable) => {
                report.dropped_variable += 1;
                continue;
            }
            Some(ExcludeDrop::Unrepresentable) => {
                report.dropped_exclude += 1;
                continue;
            }
            None => {}
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
        // A rule refused here would be permanently greyed out (the machine does
        // not define the variable, or defines it outside the profile) or would
        // surface as a scan error (an uncompilable glob): dropped rather than
        // shown as noise among thousands of others.
        if accept(&rule, lookup).is_err() {
            report.dropped_invalid += 1;
            continue;
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


/// A detection probe, injected so that tests never depend on what happens to
/// be installed on the machine running them.
pub type Probe<'a> = &'a dyn Fn(&str) -> bool;

/// Read-only existence check on a `DetectN=HKCU\..` / `HKLM\..` key. Opening a
/// key with `KEY_READ` writes nothing and needs no privilege; a hive we do not
/// know is simply "not detected". An empty subkey (`HKCU\`) would otherwise
/// resolve to the hive root, which always exists: rejected explicitly rather
/// than probed. A 32-bit application registers its `HKLM` keys under the
/// WOW6432Node redirector; when the default (64-bit) view misses the key, we
/// retry once, read-only, with `KEY_WOW64_32KEY`.
pub fn registry_key_exists(key: &str) -> bool {
    use winreg::enums::{HKEY_CURRENT_USER, HKEY_LOCAL_MACHINE, KEY_READ, KEY_WOW64_32KEY};
    use winreg::RegKey;

    let key = key.trim().trim_matches('"');
    let Some((root, sub)) = key.split_once('\\') else {
        return false;
    };
    if sub.trim().is_empty() {
        return false;
    }
    let hive = match root.to_ascii_uppercase().as_str() {
        "HKCU" | "HKEY_CURRENT_USER" => HKEY_CURRENT_USER,
        "HKLM" | "HKEY_LOCAL_MACHINE" => HKEY_LOCAL_MACHINE,
        _ => return false,
    };
    let hkey = RegKey::predef(hive);
    if hkey.open_subkey_with_flags(sub, KEY_READ).is_ok() {
        return true;
    }
    if hive == HKEY_LOCAL_MACHINE {
        return hkey
            .open_subkey_with_flags(sub, KEY_READ | KEY_WOW64_32KEY)
            .is_ok();
    }
    false
}

/// Expands `%VAR%\rest` for a probe, WITHOUT `globset::escape`: this string is
/// handed to the file system, not to a glob compiler.
fn expand_raw(mapped: &str, lookup: EnvLookup) -> Option<String> {
    let end = mapped[1..].find('%')? + 1;
    let value = lookup(&mapped[1..end])?;
    Some(format!(
        "{}{}",
        value.trim_end_matches('\\'),
        &mapped[end + 1..]
    ))
}

/// Maps and confines the literal directory a `DetectFile` wildcard is probed
/// against. `None` when the directory is outside the four allowed variables,
/// resolves outside the profile, or (for the "any child" case) is the bare
/// variable root itself — `%LocalAppData%\*` would otherwise always be true,
/// since the profile root is never empty.
fn confined_probe_dir(dir_part: &str, lookup: EnvLookup, profile: &str, bare_variable_ok: bool) -> Option<String> {
    if dir_part.is_empty() {
        return None;
    }
    let mapped = map_path(dir_part)?;
    if !bare_variable_ok && !mapped.contains('\\') {
        return None;
    }
    let path = expand_raw(&mapped, lookup)?;
    if !crate::rules::under_profile(&path, profile) {
        return None;
    }
    Some(path)
}

/// `DetectFileN=<path>`, read-only. Three shapes, and nothing else:
///
/// - no wildcard: the exact path must exist (`symlink_metadata`).
/// - a trailing `\*` on its own: the parent directory must exist AND hold at
///   least one entry, otherwise an empty left-over folder would keep an
///   uninstalled application visible forever. The variable root alone
///   (`%LocalAppData%\*`) does not count: it always has children.
/// - `<prefix>*` as the last segment (e.g. `Adobe Illustrator *`, a trailing
///   space kept literal): the parent directory must hold an entry whose name
///   starts with `prefix`, case-insensitively — upstream often only knows the
///   product name, not the version suffix Windows appends
///   (`Bridge*` matching `Bridge CC 2019`).
///
/// A `*` anywhere else — a middle segment, or not trailing the last one — is
/// refused rather than guessed at: fail closed.
///
/// The probe is confined to the profile like everything else. The exact-path
/// branch reads metadata only (`symlink_metadata`, a one-bit existence
/// oracle that does not follow a reparse point); the two wildcard branches
/// `read_dir` the parent, which does traverse a reparse point sitting at the
/// probe target to list what is inside it — accepted here, since the branch
/// only ever reports existence, never a path that gets walked or deleted.
pub fn detect_file_exists_with(raw: &str, lookup: EnvLookup) -> bool {
    let raw = raw.trim().trim_matches('"');
    let Some(profile) = lookup("USERPROFILE") else {
        return false;
    };

    let (dir_part, last_segment) = match raw.rfind('\\') {
        Some(i) => (&raw[..i], &raw[i + 1..]),
        None => ("", raw),
    };
    // A `*` before the last segment: fail closed rather than guess.
    if dir_part.contains('*') {
        return false;
    }
    if let Some(pos) = last_segment.find('*') {
        if pos != last_segment.len() - 1 {
            // A `*` in the middle of the last segment: same rule.
            return false;
        }
    }

    if last_segment == "*" {
        let Some(path) = confined_probe_dir(dir_part, lookup, &profile, false) else {
            return false;
        };
        return std::fs::read_dir(&path)
            .map(|mut entries| entries.next().is_some())
            .unwrap_or(false);
    }

    if let Some(prefix) = last_segment.strip_suffix('*') {
        if prefix.is_empty() {
            return false;
        }
        let Some(path) = confined_probe_dir(dir_part, lookup, &profile, true) else {
            return false;
        };
        let prefix = prefix.to_ascii_lowercase();
        return std::fs::read_dir(&path)
            .map(|entries| {
                entries.filter_map(|e| e.ok()).any(|e| {
                    e.file_name()
                        .to_string_lossy()
                        .to_ascii_lowercase()
                        .starts_with(&prefix)
                })
            })
            .unwrap_or(false);
    }

    let Some(mapped) = map_path(raw) else {
        return false;
    };
    let Some(path) = expand_raw(&mapped, lookup) else {
        return false;
    };
    if !crate::rules::under_profile(&path, &profile) {
        return false;
    }
    std::fs::symlink_metadata(&path).is_ok()
}

/// One matching `Detect` or `DetectFile` is enough. An entry with none of
/// either stays hidden: showing 2,000 rules for applications that are not
/// installed is how a cleaner ends up deleting something nobody meant to
/// select.
pub fn is_detected_with(entry: &ConvertedRule, registry: Probe, file: Probe) -> bool {
    entry.detects.iter().any(|key| registry(key))
        || entry.detect_files.iter().any(|path| file(path))
}

pub fn detected_rules_with(
    converted: Vec<ConvertedRule>,
    registry: Probe,
    file: Probe,
) -> Vec<Rule> {
    converted
        .into_iter()
        .filter(|c| is_detected_with(c, registry, file))
        .map(|c| c.rule)
        .collect()
}

/// The detected Winapp2 rules and what the conversion did with the rest.
pub struct Winapp2Catalogue {
    pub rules: Vec<Rule>,
    pub report: ConversionReport,
}

/// Parses, converts and probes the embedded base with the real environment.
/// Costs on the order of a second: the caller caches it (see
/// `commands::catalogue`).
pub fn embedded_winapp2() -> Winapp2Catalogue {
    let (converted, report) = convert_with(WINAPP2_INI, &crate::rules::system_env);
    // Same reason as inside `convert_with`: every probe asks for its own
    // variable and for `%USERPROFILE%`, and `system_env` pays a
    // `GetLongPathNameW` per answer.
    let memoized = crate::rules::memoized_env(&crate::rules::system_env);
    let file = |path: &str| detect_file_exists_with(path, &memoized);
    let rules = detected_rules_with(converted, &registry_key_exists, &file);
    Winapp2Catalogue { rules, report }
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

    /// A `;`-separated exclude list where only ONE spec is unrepresentable:
    /// keeping the entry with the remaining specs would delete a file upstream
    /// asks us to keep. The whole entry goes.
    #[test]
    fn one_unrepresentable_spec_in_an_exclude_list_drops_the_whole_entry() {
        let src = "[A]\nFileKey1=%LocalAppData%\\A|*.*\nExcludeKey1=FILE|%LocalAppData%\\A|keep[1].dat;notes.txt\n";
        let (converted, report) = convert_with(src, &fake_env);
        assert!(converted.is_empty());
        assert_eq!(report.dropped_exclude, 1);
        assert_eq!(report.dropped(), 1);
    }

    /// A `FileKey` spec we cannot represent only narrows what the rule
    /// deletes: the key keeps its other specs and the entry survives.
    #[test]
    fn an_unrepresentable_file_key_spec_narrows_the_rule_instead_of_dropping_it() {
        let src = "[A]\nFileKey1=%LocalAppData%\\A|cache[1].dat;*.log\n";
        let (converted, report) = convert_with(src, &fake_env);
        assert_eq!(report.retained, 1);
        assert_eq!(converted[0].rule.paths, vec![r"%LOCALAPPDATA%\A\*.log"]);
    }

    /// The conversion resolves every path of every entry, and every resolution
    /// asks the environment for the variable AND for `%USERPROFILE%`. Without
    /// a cache the real file costs tens of thousands of `GetLongPathNameW`
    /// calls; with one, the count is bounded by the four variables, not by the
    /// number of keys.
    #[test]
    fn the_environment_is_looked_up_once_per_variable_not_once_per_key() {
        let mut src = String::new();
        for i in 0..50 {
            src.push_str(&format!(
                "[App {i}]\nFileKey1=%LocalAppData%\\A{i}|*.*\nFileKey2=%AppData%\\A{i}|*.*\nExcludeKey1=FILE|%Temp%\\A{i}|keep.dat\n"
            ));
        }
        let calls = std::cell::Cell::new(0u32);
        let counting = |name: &str| {
            calls.set(calls.get() + 1);
            fake_env(name)
        };
        let (converted, _) = convert_with(&src, &counting);
        assert_eq!(converted.len(), 50);
        assert!(
            calls.get() <= 4,
            "{} environment lookups for 50 entries",
            calls.get()
        );
    }

    /// Counting occurrences instead of remembering what was issued mints the
    /// same id twice: `[App]`, `[App]`, `[App 2]` gave `app`, `app-2`,
    /// `app-2`. A duplicate id is fatal for the whole catalogue.
    #[test]
    fn a_generated_id_never_collides_with_a_section_named_after_it() {
        let src = "[App]\nFileKey1=%LocalAppData%\\A|*.*\n[App]\nFileKey1=%LocalAppData%\\B|*.*\n[App 2]\nFileKey1=%LocalAppData%\\C|*.*\n";
        let (converted, _) = convert_with(src, &fake_env);
        let ids: Vec<&str> = converted.iter().map(|c| c.rule.id.as_str()).collect();
        let unique: std::collections::HashSet<&&str> = ids.iter().collect();
        assert_eq!(unique.len(), 3, "{ids:?}");
        assert_eq!(ids[0], "winapp2.app");
        assert_eq!(ids[1], "winapp2.app-2");
    }

    /// A variable this machine does not define is a validation failure, not a
    /// mapping failure: it is counted apart from the entries we refuse to map.
    #[test]
    fn a_variable_the_machine_does_not_define_is_counted_as_invalid() {
        let src = "[A]\nFileKey1=%AppData%\\A|*.*\n";
        let without_appdata = |name: &str| match name {
            "APPDATA" => None,
            _ => fake_env(name),
        };
        let (converted, report) = convert_with(src, &without_appdata);
        assert!(converted.is_empty());
        assert_eq!(report.dropped_invalid, 1);
        assert_eq!(report.dropped(), 1);
    }

    /// Upstream typos like `%USERPROFILE%\AppData%\LocalLow\X` or a trailing
    /// `%UserName%`: mapping only the leading variable leaves an inert literal
    /// directory that matches nothing. The key goes instead.
    #[test]
    fn a_variable_left_literal_inside_the_path_drops_the_key() {
        assert_eq!(map_path(r"%USERPROFILE%\AppData%\LocalLow\X"), None);
        assert_eq!(map_path(r"%LocalAppData%\A\%UserName%"), None);
        let src = "[A]\nFileKey1=%UserProfile%\\AppData%\\LocalLow\\A|*.*\n[B]\nFileKey1=%LocalAppData%\\B|*.*\nExcludeKey1=FILE|%LocalAppData%\\B\\%UserName%|keep.dat\n";
        let (converted, report) = convert_with(src, &fake_env);
        assert!(converted.is_empty());
        assert_eq!(report.dropped_no_file_key, 1);
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
        // Every entry is either retained or counted under exactly one reason:
        // a rule that vanishes without a counter would be invisible.
        assert_eq!(
            report.retained + report.dropped(),
            parse_ini(WINAPP2_INI).len() as u32,
            "retained={} dropped={} (no_file_key={} variable={} exclude={} invalid={})",
            report.retained,
            report.dropped(),
            report.dropped_no_file_key,
            report.dropped_variable,
            report.dropped_exclude,
            report.dropped_invalid
        );
        let mut ids = std::collections::HashSet::new();
        // The same real values as `system_env`, asked for once instead of once
        // per path: the check below is unchanged, it just stops paying a
        // syscall per pattern.
        let env = crate::rules::memoized_env(&crate::rules::system_env);
        for c in &converted {
            assert!(c.rule.id.starts_with("winapp2."), "{}", c.rule.id);
            assert!(ids.insert(c.rule.id.clone()), "duplicate id {}", c.rule.id);
            assert_eq!(c.rule.category, "Applications");
            assert!(!c.rule.default_checked, "{}", c.rule.id);
            assert_eq!(c.rule.risk, crate::rules::Risk::Medium);
            assert!(c.rule.unavailable_reason.is_none(), "{}", c.rule.id);
            crate::rules::check_rule_with(&c.rule, &env)
                .unwrap_or_else(|e| panic!("{} failed validation: {e}", c.rule.id));
        }
    }

    /// Test subkey, created and deleted by the test itself. Never point this
    /// at a real application key.
    struct ScratchKey(String);

    impl ScratchKey {
        fn new(suffix: &str) -> Self {
            use winreg::enums::HKEY_CURRENT_USER;
            use winreg::RegKey;
            let path = format!(r"Software\wincleaner-test\{suffix}");
            RegKey::predef(HKEY_CURRENT_USER)
                .create_subkey(&path)
                .unwrap();
            ScratchKey(path)
        }
    }

    impl Drop for ScratchKey {
        fn drop(&mut self) {
            use winreg::enums::HKEY_CURRENT_USER;
            use winreg::RegKey;
            let _ = RegKey::predef(HKEY_CURRENT_USER).delete_subkey_all(&self.0);
        }
    }

    #[test]
    fn detects_an_existing_registry_key_and_only_the_two_user_hives() {
        let scratch = ScratchKey::new("detect-probe");
        assert!(registry_key_exists(&format!(r"HKCU\{}", scratch.0)));
        assert!(registry_key_exists(&format!(
            r"HKEY_CURRENT_USER\{}",
            scratch.0
        )));
        assert!(!registry_key_exists(
            r"HKCU\Software\wincleaner-test\never-created"
        ));
        assert!(!registry_key_exists(r"HKCR\Something"));
        assert!(!registry_key_exists("no-backslash"));
    }

    #[test]
    fn detects_a_file_a_directory_and_a_non_empty_directory() {
        let dir = tempfile::tempdir().unwrap();
        let root = dir.path().to_string_lossy().to_string();
        let lookup = move |name: &str| match name {
            "USERPROFILE" | "LOCALAPPDATA" => Some(root.clone()),
            _ => None,
        };
        std::fs::create_dir(dir.path().join("App")).unwrap();
        std::fs::write(dir.path().join("App").join("x.dat"), b"x").unwrap();
        std::fs::create_dir(dir.path().join("Empty")).unwrap();

        assert!(detect_file_exists_with(r"%LocalAppData%\App", &lookup));
        assert!(detect_file_exists_with(r"%LocalAppData%\App\x.dat", &lookup));
        // A trailing `\*` means "any child": the empty directory does not
        // count as an installed application.
        assert!(detect_file_exists_with(r"%LocalAppData%\App\*", &lookup));
        assert!(!detect_file_exists_with(r"%LocalAppData%\Empty\*", &lookup));
        assert!(!detect_file_exists_with(r"%LocalAppData%\Missing", &lookup));
        // Outside the allow-list: not detectable, never probed.
        assert!(!detect_file_exists_with(r"%ProgramFiles%\App", &lookup));
    }

    #[test]
    fn an_entry_without_any_detect_key_is_hidden() {
        let src = "[Seen]\nDetect=HKCU\\Software\\Yes\nFileKey1=%LocalAppData%\\A|*.*\n\
                   [Unseen]\nDetect=HKCU\\Software\\No\nFileKey1=%LocalAppData%\\B|*.*\n\
                   [NoDetect]\nFileKey1=%LocalAppData%\\C|*.*\n";
        let (converted, report) = convert_with(src, &fake_env);
        assert_eq!(report.retained, 3);
        let registry = |key: &str| key.ends_with("Yes");
        let never = |_: &str| false;
        let rules = detected_rules_with(converted, &registry, &never);
        let ids: Vec<&str> = rules.iter().map(|r| r.id.as_str()).collect();
        assert_eq!(ids, vec!["winapp2.seen"]);
    }

    #[test]
    fn a_detect_file_match_is_enough_on_its_own() {
        let src = "[A]\nDetectFile=%LocalAppData%\\A\nFileKey1=%LocalAppData%\\A|*.*\n";
        let (converted, _) = convert_with(src, &fake_env);
        let never = |_: &str| false;
        let always = |_: &str| true;
        assert_eq!(detected_rules_with(converted, &never, &always).len(), 1);
    }

    /// `Adobe Bridge*` must match `Bridge CC 2019`: upstream often knows only
    /// the product name, not the version suffix Windows appends to it.
    #[test]
    fn a_prefix_wildcard_matches_a_versioned_directory_name() {
        let dir = tempfile::tempdir().unwrap();
        let root = dir.path().to_string_lossy().to_string();
        let lookup = move |name: &str| match name {
            "USERPROFILE" | "LOCALAPPDATA" => Some(root.clone()),
            _ => None,
        };
        std::fs::create_dir(dir.path().join("Bridge CC 2019")).unwrap();
        std::fs::create_dir(dir.path().join("Adobe Illustrator 2024")).unwrap();

        assert!(detect_file_exists_with(r"%LocalAppData%\Bridge*", &lookup));
        // No trailing wildcard: an exact-path probe, which does not match.
        assert!(!detect_file_exists_with(r"%LocalAppData%\Bridge", &lookup));
        // A trailing space before `*` is part of the prefix, not trimmed.
        assert!(detect_file_exists_with(
            r"%LocalAppData%\Adobe Illustrator *",
            &lookup
        ));
        assert!(!detect_file_exists_with(r"%LocalAppData%\Nope*", &lookup));
        // `*` in a middle segment, not trailing the last one: fail closed.
        assert!(!detect_file_exists_with(r"%LocalAppData%\*\Bridge", &lookup));
    }

    /// `%LocalAppData%\*` has no literal segment between the variable and the
    /// wildcard: the profile root is never empty, so this would otherwise
    /// always report "detected".
    #[test]
    fn a_wildcard_directly_under_the_variable_root_is_never_detected() {
        let dir = tempfile::tempdir().unwrap();
        let root = dir.path().to_string_lossy().to_string();
        let lookup = move |name: &str| match name {
            "USERPROFILE" | "LOCALAPPDATA" => Some(root.clone()),
            _ => None,
        };
        std::fs::create_dir(dir.path().join("Something")).unwrap();
        assert!(!detect_file_exists_with(r"%LocalAppData%\*", &lookup));
    }

    #[test]
    fn an_empty_registry_subkey_does_not_resolve_to_the_hive_root() {
        assert!(!registry_key_exists(r"HKCU\"));
        assert!(!registry_key_exists(r"HKCU\ "));
    }
}
