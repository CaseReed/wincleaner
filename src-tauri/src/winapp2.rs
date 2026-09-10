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
}
