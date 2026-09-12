//! The read-only command line: `wincleaner --analyze [--json] [--rules …]`.
//!
//! Three properties hold this module together.
//!
//! 1. **It never deletes.** There is no `--clean` and there never will be:
//!    the confirmation before a deletion is an invariant of the application
//!    (`docs/roadmap.md`), and an unattended flag would be exactly the way
//!    around it. The only thing this module can do is measure and print.
//! 2. **No argument names a path.** `--rules` takes rule ids, checked against
//!    the catalogue, and nothing else — the same rule the IPC boundary obeys,
//!    extended to `argv`. A run therefore walks what `rules.toml` and the
//!    detected Winapp2 entries declare, never what the caller typed.
//! 3. **It opens no window.** `main.rs` answers here *before*
//!    `tauri::Builder`, so `--analyze` in a script costs no WebView2 and no
//!    message loop.
//!
//! The measurement is the application's own (`commands::scan_rules_in` with no
//! sandbox): same catalogue, same stored exclusions with the same fail-closed
//! rule, same concurrent `scan_rules_with`, same Recycle Bin cache. A figure
//! printed here is the figure the window would show.

use crate::commands::{catalogue, scan_rules_in, LastPaths, SandboxState};
use crate::exclusions;
use crate::rules::{Risk, Rule};
use crate::scan::ScanResult;
use serde::Serialize;

pub const EXIT_OK: i32 = 0;
pub const EXIT_ERROR: i32 = 1;
pub const EXIT_USAGE: i32 = 2;

pub const USAGE: &str = "\
Usage:
  wincleaner --analyze [--json] [--rules <id,id,...>]
  wincleaner --help

  --analyze       Measure every rule and print what it found. Read-only:
                  there is no --clean, and no option takes a path.
  --json          Print one JSON object instead of a table.
  --rules <ids>   Restrict the analysis to these rule ids, comma separated.
  --help          Print this message.

With no argument at all, WinCleaner opens its window as usual.
";

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Command {
    Analyze {
        json: bool,
        /// `None` means the whole catalogue.
        rules: Option<Vec<String>>,
    },
    Help,
}

/// Parses `argv` minus the program name. Called only when there is at least
/// one argument: an empty `argv` is the window, decided in `main.rs`.
pub fn parse(args: &[String]) -> Result<Command, String> {
    let mut analyze = false;
    let mut json = false;
    let mut rules = None;
    let mut args = args.iter();
    while let Some(arg) = args.next() {
        match arg.as_str() {
            // Answered on the spot: `--help` next to a typo still helps.
            "--help" | "-h" => return Ok(Command::Help),
            "--analyze" => analyze = true,
            "--json" => json = true,
            "--rules" => {
                let value = args
                    .next()
                    .ok_or("--rules needs a comma-separated list of rule ids")?;
                rules = Some(rule_ids(value)?);
            }
            other => return Err(format!("unknown option \"{other}\"")),
        }
    }
    if !analyze {
        return Err("--analyze is required".to_string());
    }
    Ok(Command::Analyze { json, rules })
}

fn rule_ids(value: &str) -> Result<Vec<String>, String> {
    let ids: Vec<String> = value
        .split(',')
        .map(str::trim)
        .filter(|id| !id.is_empty())
        .map(str::to_string)
        .collect();
    if ids.is_empty() {
        return Err("--rules needs at least one rule id".to_string());
    }
    Ok(ids)
}

/// The ids to measure: those asked for, or the whole catalogue. An id the
/// catalogue does not know is refused here rather than half-way through a
/// scan, and every unknown one is named at once so a typo in a list of ten
/// takes one run to find.
pub fn select_ids(rules: &[Rule], requested: Option<&[String]>) -> Result<Vec<String>, String> {
    let Some(requested) = requested else {
        return Ok(rules.iter().map(|r| r.id.clone()).collect());
    };
    let unknown: Vec<&str> = requested
        .iter()
        .map(String::as_str)
        .filter(|id| !rules.iter().any(|r| r.id == *id))
        .collect();
    if !unknown.is_empty() {
        return Err(format!("unknown rule id: {}", unknown.join(", ")));
    }
    Ok(requested.to_vec())
}

/// One rule of the report. Deliberately the same fields the window shows, and
/// no path: a report pasted into a support ticket carries no file name.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct RuleReport {
    pub id: String,
    pub label: String,
    pub category: String,
    pub risk: Risk,
    pub file_count: u64,
    pub total_bytes: u64,
    pub skipped: u32,
    pub cached: bool,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
pub struct Totals {
    pub rules: u32,
    pub files: u64,
    pub bytes: u64,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct AnalyzeReport {
    pub version: String,
    pub generated_at: String,
    pub rules: Vec<RuleReport>,
    pub totals: Totals,
    pub exclusions: usize,
}

/// Builds the report from the rules and the results of their measurement,
/// paired by the caller. The shape is the contract a script parses, so it is
/// built here, away from the printing, and asserted by its own test.
pub fn build_report(
    measured: &[(Rule, ScanResult)],
    exclusions: usize,
    generated_at: &str,
) -> AnalyzeReport {
    let rules: Vec<RuleReport> = measured
        .iter()
        .map(|(rule, result)| RuleReport {
            id: result.rule_id.clone(),
            label: rule.label.clone(),
            category: rule.category.clone(),
            risk: rule.risk,
            file_count: result.file_count,
            total_bytes: result.total_bytes,
            skipped: result.skipped,
            cached: result.cached,
        })
        .collect();
    let totals = Totals {
        rules: rules.len() as u32,
        files: rules.iter().map(|r| r.file_count).sum(),
        bytes: rules.iter().map(|r| r.total_bytes).sum(),
    };
    AnalyzeReport {
        version: env!("CARGO_PKG_VERSION").to_string(),
        generated_at: generated_at.to_string(),
        rules,
        totals,
        exclusions,
    }
}

/// Binary units, like the window's own formatter (`src/lib/format.ts`).
fn format_bytes(bytes: u64) -> String {
    const UNITS: [&str; 5] = ["KB", "MB", "GB", "TB", "PB"];
    if bytes < 1024 {
        return format!("{bytes} B");
    }
    let mut value = bytes as f64 / 1024.0;
    let mut unit = 0;
    while value >= 1024.0 && unit + 1 < UNITS.len() {
        value /= 1024.0;
        unit += 1;
    }
    format!("{value:.1} {}", UNITS[unit])
}

pub fn render_table(report: &AnalyzeReport) -> String {
    let id_width = report
        .rules
        .iter()
        .map(|r| r.id.len())
        .chain(std::iter::once("RULE".len()))
        .max()
        .unwrap_or(4);
    let label_width = report
        .rules
        .iter()
        .map(|r| r.label.chars().count())
        .chain(std::iter::once("LABEL".len()))
        .max()
        .unwrap_or(5);

    let mut out = format!(
        "{:<id_width$}  {:<label_width$}  {:>9}  {:>10}  {:>7}  {}\n",
        "RULE", "LABEL", "FILES", "SIZE", "SKIPPED", "CACHED"
    );
    for rule in &report.rules {
        out.push_str(&format!(
            "{:<id_width$}  {:<label_width$}  {:>9}  {:>10}  {:>7}  {}\n",
            rule.id,
            rule.label,
            rule.file_count,
            format_bytes(rule.total_bytes),
            rule.skipped,
            if rule.cached { "yes" } else { "" }
        ));
    }
    out.push_str(&format!(
        "\n{} rules, {} files, {}, {} exclusion(s) applied.\n",
        report.totals.rules,
        report.totals.files,
        format_bytes(report.totals.bytes),
        report.exclusions
    ));
    out
}

/// `YYYY-MM-DDTHH:MM:SSZ`. Same reasoning as `exclusions::today`, whose date
/// arithmetic this reuses: a timestamp a human reads in a ticket, not a value
/// anything computes against, so it does not pull in a date crate.
fn generated_at() -> String {
    let secs = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|d| d.as_secs())
        .unwrap_or(0);
    let (y, m, d) = exclusions::civil_from_days((secs / 86_400) as i64);
    let day = secs % 86_400;
    format!(
        "{y:04}-{m:02}-{d:02}T{:02}:{:02}:{:02}Z",
        day / 3600,
        (day % 3600) / 60,
        day % 60
    )
}

/// Release builds carry `windows_subsystem = "windows"`: with no console
/// attached, every print goes to a handle that does not exist. Attaching the
/// parent's console is what makes `wincleaner --analyze` in a terminal print
/// anything at all. A run with no parent console — a double click — fails here
/// and stays silent, which is the wanted outcome; a redirected or piped run
/// already has its handles and does not need this.
fn attach_console() {
    use windows::Win32::System::Console::{AttachConsole, ATTACH_PARENT_PROCESS};

    unsafe {
        let _ = AttachConsole(ATTACH_PARENT_PROCESS);
    }
}

fn analyze(json: bool, requested: Option<Vec<String>>) -> Result<i32, (i32, String)> {
    let catalogue = catalogue().map_err(|e| (EXIT_ERROR, e))?;
    let ids = select_ids(&catalogue.rules, requested.as_deref())
        .map_err(|message| (EXIT_USAGE, message))?;

    // Read for the count only; `scan_rules_in` loads them again for the scan
    // itself, with the same fail-closed rule — a store that exists and cannot
    // be read stops the run here rather than reporting on a profile whose
    // exclusions were silently dropped.
    let store = exclusions::store_path(None).map_err(|e| (EXIT_ERROR, e))?;
    let exclusions = exclusions::load(&store).map_err(|e| (EXIT_ERROR, e))?.len();

    if !json {
        eprintln!("Analyzing {} rules…", ids.len());
    }
    let results = scan_rules_in(
        &SandboxState::default(),
        &LastPaths::default(),
        &ids,
        &mut |_| {},
    )
    .map_err(|e| (EXIT_ERROR, e))?;

    // `scan_rules_with` assembles its `Vec` by index, so the results come back
    // in the order of the ids handed to it, which is the order of `picked`.
    let picked: Vec<Rule> = ids
        .iter()
        .filter_map(|id| catalogue.rules.iter().find(|r| &r.id == id).cloned())
        .collect();
    let measured: Vec<(Rule, ScanResult)> = picked.into_iter().zip(results).collect();
    let report = build_report(&measured, exclusions, &generated_at());

    if json {
        let body = serde_json::to_string_pretty(&report)
            .map_err(|e| (EXIT_ERROR, format!("the report cannot be serialised: {e}")))?;
        println!("{body}");
    } else {
        print!("{}", render_table(&report));
    }
    Ok(EXIT_OK)
}

/// Entry point of a run that carries arguments. Returns the process exit code.
pub fn main(args: &[String]) -> i32 {
    attach_console();
    match parse(args) {
        Ok(Command::Help) => {
            print!("{USAGE}");
            EXIT_OK
        }
        Ok(Command::Analyze { json, rules }) => match analyze(json, rules) {
            Ok(code) => code,
            Err((code, message)) => {
                eprintln!("wincleaner: {message}");
                if code == EXIT_USAGE {
                    eprint!("{USAGE}");
                }
                code
            }
        },
        Err(message) => {
            eprintln!("wincleaner: {message}");
            eprint!("{USAGE}");
            EXIT_USAGE
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::rules::RuleKind;

    fn rule(id: &str) -> Rule {
        Rule {
            id: id.to_string(),
            category: "Temporary files".to_string(),
            label: format!("Label of {id}"),
            paths: vec![r"%TEMP%\*".to_string()],
            exclude: vec![],
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

    fn result(id: &str, files: u64, bytes: u64) -> ScanResult {
        ScanResult {
            rule_id: id.to_string(),
            file_count: files,
            total_bytes: bytes,
            paths: Vec::new(),
            skipped: 0,
            cached: false,
        }
    }

    #[test]
    fn analyze_alone_measures_the_whole_catalogue_as_a_table() {
        assert_eq!(
            parse(&["--analyze".to_string()]).unwrap(),
            Command::Analyze {
                json: false,
                rules: None
            }
        );
    }

    #[test]
    fn json_and_rules_are_read_off_the_command_line() {
        let args = ["--analyze", "--json", "--rules", "a, b"].map(str::to_string);
        assert_eq!(
            parse(&args).unwrap(),
            Command::Analyze {
                json: true,
                rules: Some(vec!["a".to_string(), "b".to_string()])
            }
        );
    }

    #[test]
    fn help_wins_over_the_rest_of_the_line() {
        let args = ["--analyze", "--help"].map(str::to_string);
        assert_eq!(parse(&args).unwrap(), Command::Help);
    }

    #[test]
    fn an_unknown_option_is_a_usage_error() {
        assert!(parse(&["--clean".to_string()])
            .unwrap_err()
            .contains("--clean"));
    }

    /// `--json` without `--analyze` would otherwise print an empty report and
    /// exit 0, which a script would read as "nothing to clean".
    #[test]
    fn an_option_without_analyze_is_a_usage_error() {
        assert!(parse(&["--json".to_string()])
            .unwrap_err()
            .contains("--analyze"));
    }

    #[test]
    fn rules_without_a_value_is_a_usage_error() {
        let args = ["--analyze", "--rules"].map(str::to_string);
        assert!(parse(&args).unwrap_err().contains("--rules"));
    }

    #[test]
    fn an_unknown_rule_id_is_refused_before_anything_is_walked() {
        let catalogue = [rule("temp"), rule("cache")];
        let asked = ["temp".to_string(), "typo".to_string()];
        assert_eq!(
            select_ids(&catalogue, Some(&asked)).unwrap_err(),
            "unknown rule id: typo"
        );
    }

    #[test]
    fn no_rules_option_selects_the_whole_catalogue_in_order() {
        let catalogue = [rule("temp"), rule("cache")];
        assert_eq!(
            select_ids(&catalogue, None).unwrap(),
            vec!["temp".to_string(), "cache".to_string()]
        );
    }

    /// The JSON shape is what a script parses: every key and every total is
    /// pinned here, with the scan injected rather than run.
    #[test]
    fn the_json_report_has_the_documented_shape() {
        let measured = vec![
            (rule("temp"), result("temp", 3, 2048)),
            (rule("cache"), result("cache", 1, 512)),
        ];
        let report = build_report(&measured, 2, "2026-09-13T10:00:00Z");
        let json: serde_json::Value =
            serde_json::from_str(&serde_json::to_string(&report).unwrap()).unwrap();

        assert_eq!(json["version"], env!("CARGO_PKG_VERSION"));
        assert_eq!(json["generated_at"], "2026-09-13T10:00:00Z");
        assert_eq!(json["exclusions"], 2);
        assert_eq!(json["totals"]["rules"], 2);
        assert_eq!(json["totals"]["files"], 4);
        assert_eq!(json["totals"]["bytes"], 2560);
        assert_eq!(json["rules"][0]["id"], "temp");
        assert_eq!(json["rules"][0]["label"], "Label of temp");
        assert_eq!(json["rules"][0]["category"], "Temporary files");
        assert_eq!(json["rules"][0]["risk"], "low");
        assert_eq!(json["rules"][0]["file_count"], 3);
        assert_eq!(json["rules"][0]["total_bytes"], 2048);
        assert_eq!(json["rules"][0]["skipped"], 0);
        assert_eq!(json["rules"][0]["cached"], false);
        assert!(
            json["rules"][0].get("paths").is_none(),
            "a report must never carry a path"
        );
    }

    #[test]
    fn the_table_names_every_rule_and_closes_on_the_totals() {
        let measured = vec![(rule("temp"), result("temp", 3, 2048))];
        let table = render_table(&build_report(&measured, 0, "2026-09-13T10:00:00Z"));

        assert!(table.starts_with("RULE"));
        assert!(table.contains("temp"));
        assert!(table.contains("2.0 KB"));
        assert!(table.contains("1 rules, 3 files, 2.0 KB, 0 exclusion(s) applied."));
    }
}
