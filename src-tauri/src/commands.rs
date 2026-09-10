use crate::clean::{clean_rule, CleanMode, CleanReport, SkippedItem};
use crate::rules::{embedded_rules, Risk, Rule, RuleKind};
use crate::scan::{scan_rule, ScanResult};
use crate::startup::StartupEntry;
use serde::{Deserialize, Serialize};
use sysinfo::System;

/// Processes considered "an open browser" for the warning banner.
pub const BROWSER_PROCESSES: [&str; 3] = ["msedge.exe", "chrome.exe", "firefox.exe"];

/// What the front end receives to build the rule list. Never contains a path:
/// the front end has no business knowing what will be deleted.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct RuleSummary {
    pub id: String,
    pub category: String,
    pub label: String,
    pub risk: Risk,
    pub kind: RuleKind,
    /// Checkbox ticked on first launch (see `rules.toml`).
    pub default_checked: bool,
    /// Set when the rule does not apply on this machine. The front end greys
    /// the row out and shows this reason; the rule is neither scanned nor
    /// cleaned, even if its id were sent.
    pub unavailable_reason: Option<String>,
}

fn find_rules(rule_ids: &[String]) -> Result<Vec<Rule>, String> {
    let all = embedded_rules().map_err(|e| e.to_string())?;
    rule_ids
        .iter()
        .map(|id| {
            all.iter()
                .find(|r| &r.id == id)
                .cloned()
                .ok_or_else(|| format!("unknown rule: \"{id}\""))
        })
        .collect()
}

pub fn running_browsers_from(process_names: &[String]) -> Vec<String> {
    let mut out: Vec<String> = Vec::new();
    for name in process_names {
        let lower = name.to_lowercase();
        if BROWSER_PROCESSES.contains(&lower.as_str()) && !out.contains(&lower) {
            out.push(lower);
        }
    }
    out
}

#[tauri::command]
pub fn list_rules() -> Result<Vec<RuleSummary>, String> {
    let rules = embedded_rules().map_err(|e| e.to_string())?;
    Ok(rules
        .into_iter()
        .map(|r| RuleSummary {
            id: r.id,
            category: r.category,
            label: r.label,
            risk: r.risk,
            kind: r.kind,
            default_checked: r.default_checked && r.unavailable_reason.is_none(),
            unavailable_reason: r.unavailable_reason,
        })
        .collect())
}

/// Runs blocking work off the main thread. A synchronous Tauri command runs on
/// the main thread and freezes the webview event loop while it executes: on a
/// loaded profile, a disk walk lasting several seconds would make the window
/// "not responding".
async fn blocking<T, F>(work: F) -> Result<T, String>
where
    F: FnOnce() -> Result<T, String> + Send + 'static,
    T: Send + 'static,
{
    tauri::async_runtime::spawn_blocking(work)
        .await
        .map_err(|e| format!("task interrupted: {e}"))?
}

/// Scans each rule, refusing on the spot those that do not apply to this
/// machine. `run` is injected so that the test exercises that refusal right
/// here, and not in a copy of the loop.
fn scan_rules_with(
    rules: &[Rule],
    mut run: impl FnMut(&Rule) -> Result<ScanResult, String>,
) -> Result<Vec<ScanResult>, String> {
    rules
        .iter()
        .map(|r| match &r.unavailable_reason {
            // The rule does not apply on this machine: nothing to walk, and
            // above all nothing to delete. Counted as "skipped", not an error.
            Some(_) => Ok(ScanResult {
                rule_id: r.id.clone(),
                file_count: 0,
                total_bytes: 0,
                paths: Vec::new(),
                skipped: 1,
            }),
            None => run(r),
        })
        .collect()
}

fn scan_rules(rule_ids: &[String]) -> Result<Vec<ScanResult>, String> {
    scan_rules_with(&find_rules(rule_ids)?, |r| {
        scan_rule(r).map_err(|e| e.to_string())
    })
}

#[tauri::command]
pub async fn scan(rule_ids: Vec<String>) -> Result<Vec<ScanResult>, String> {
    blocking(move || scan_rules(&rule_ids)).await
}

/// The recycle bin is emptied FIRST, whatever the order in rules.toml or the
/// order the front end sends. Otherwise, in "Recycle Bin" mode, the "files"
/// rules drop their files into it and the Recycle Bin rule destroys them
/// permanently in the same pass: the cautious mode no longer protects
/// anything. `sort_by_key` is stable: the relative order of the other rules is
/// preserved.
fn cleaning_order(mut rules: Vec<Rule>) -> Vec<Rule> {
    rules.sort_by_key(|r| r.kind != RuleKind::RecycleBin);
    rules
}

/// Cleans the rules in the order imposed by `cleaning_order`, refusing on the
/// spot those that do not apply to this machine. The failure of one rule never
/// throws away what has already been cleaned: it becomes a `skipped` entry
/// carrying the rule id.
///
/// `run` is injected so that the test exercises this order and this refusal
/// right here: a test replaying the sequence by hand would lock nothing down.
fn clean_rules_with(
    rules: Vec<Rule>,
    mode: CleanMode,
    mut run: impl FnMut(&Rule, CleanMode) -> Result<CleanReport, String>,
) -> CleanReport {
    let mut report = CleanReport::default();
    for rule in cleaning_order(rules) {
        let outcome = match &rule.unavailable_reason {
            Some(reason) => Err(reason.clone()),
            None => run(&rule, mode),
        };
        match outcome {
            Ok(partial) => report.merge(partial),
            Err(reason) => report.skipped.push(SkippedItem {
                path: rule.id.clone(),
                reason,
            }),
        }
    }
    report
}

fn clean_rules(rule_ids: &[String], mode: CleanMode) -> Result<CleanReport, String> {
    Ok(clean_rules_with(
        find_rules(rule_ids)?,
        mode,
        |rule, mode| clean_rule(rule, mode).map_err(|e| e.to_string()),
    ))
}

#[tauri::command]
pub async fn clean(rule_ids: Vec<String>, mode: CleanMode) -> Result<CleanReport, String> {
    blocking(move || clean_rules(&rule_ids, mode)).await
}

#[tauri::command]
pub fn running_browsers() -> Vec<String> {
    let mut system = System::new_all();
    system.refresh_processes(sysinfo::ProcessesToUpdate::All, true);
    let names: Vec<String> = system
        .processes()
        .values()
        .map(|p| p.name().to_string_lossy().to_string())
        .collect();
    running_browsers_from(&names)
}

#[tauri::command]
pub async fn list_startup() -> Result<Vec<StartupEntry>, String> {
    blocking(|| crate::startup::list_startup().map_err(|e| e.to_string())).await
}

#[tauri::command]
pub async fn set_startup_enabled(id: String, enabled: bool) -> Result<(), String> {
    blocking(move || crate::startup::set_startup_enabled(&id, enabled).map_err(|e| e.to_string()))
        .await
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn only_the_targeted_browsers_are_retained() {
        let names: Vec<String> = [
            "explorer.exe",
            "msedge.exe",
            "MSEDGE.EXE",
            "chrome.exe",
            "code.exe",
        ]
        .iter()
        .map(|s| s.to_string())
        .collect();
        let mut got = running_browsers_from(&names);
        got.sort();
        assert_eq!(got, vec!["chrome.exe", "msedge.exe"]);
    }

    #[test]
    fn duplicate_processes_are_deduplicated() {
        let names: Vec<String> = ["firefox.exe", "firefox.exe", "firefox.exe"]
            .iter()
            .map(|s| s.to_string())
            .collect();
        assert_eq!(running_browsers_from(&names), vec!["firefox.exe"]);
    }

    #[test]
    fn no_browser_returns_an_empty_list() {
        let names: Vec<String> = ["explorer.exe".to_string()].to_vec();
        assert!(running_browsers_from(&names).is_empty());
    }

    #[test]
    fn the_rule_summary_carries_the_eight_embedded_rules() {
        let summaries = list_rules().unwrap();
        assert_eq!(summaries.len(), 8);
        assert_eq!(summaries[0].id, "windows.temp");
        assert_eq!(summaries[1].kind, crate::rules::RuleKind::RecycleBin);
    }

    /// The front end builds its checkboxes from this field: if it does not
    /// cross the IPC, everything becomes checked by default again.
    #[test]
    fn the_summary_carries_the_default_checkbox_state() {
        let summaries = list_rules().unwrap();
        let recycle_bin = summaries
            .iter()
            .find(|r| r.id == "windows.recycle-bin")
            .unwrap();
        assert!(!recycle_bin.default_checked);
        let temp = summaries.iter().find(|r| r.id == "windows.temp").unwrap();
        assert!(temp.default_checked);
    }

    /// The whole point of the item: a synchronous command would run on the
    /// calling thread — the main thread in production, the one pumping the
    /// window events. `blocking` must move the work elsewhere.
    #[test]
    fn blocking_work_leaves_the_calling_thread() {
        let caller = std::thread::current().id();
        let inside =
            tauri::async_runtime::block_on(blocking(|| Ok(std::thread::current().id()))).unwrap();
        assert_ne!(caller, inside);
    }

    /// Goes through the async command, and therefore through `spawn_blocking`:
    /// checks that the work moved off the main thread does return its result.
    #[test]
    fn scanning_an_unknown_id_is_an_error() {
        let err = tauri::async_runtime::block_on(scan(vec!["nonexistent".to_string()])).unwrap_err();
        assert!(err.contains("nonexistent"));
    }

    fn rule(id: &str) -> Rule {
        Rule {
            id: id.into(),
            category: "System".into(),
            label: id.into(),
            paths: vec![r"%TEMP%\*".into()],
            exclude: vec![],
            risk: Risk::Low,
            kind: RuleKind::Files,
            default_checked: true,
            note: None,
            unavailable_reason: None,
        }
    }

    #[test]
    fn one_rule_failing_does_not_throw_away_the_report_of_the_previous_ones() {
        let rules = vec![rule("a"), rule("b"), rule("c")];
        let report = clean_rules_with(rules, CleanMode::Auto, |r, _| {
            if r.id == "b" {
                Err("access denied".to_string())
            } else {
                Ok(CleanReport {
                    freed_bytes: 10,
                    deleted: 1,
                    skipped: Vec::new(),
                })
            }
        });
        assert_eq!(report.deleted, 2);
        assert_eq!(report.freed_bytes, 20);
        assert_eq!(report.skipped.len(), 1);
        assert_eq!(report.skipped[0].path, "b");
        assert_eq!(report.skipped[0].reason, "access denied");
    }

    fn recycle_bin_rule() -> Rule {
        Rule {
            id: "windows.recycle-bin".into(),
            category: "System".into(),
            label: "Recycle Bin".into(),
            paths: vec![],
            exclude: vec![],
            risk: Risk::Low,
            kind: RuleKind::RecycleBin,
            default_checked: false,
            note: None,
            unavailable_reason: None,
        }
    }

    /// An unavailable rule must never reach the disk, even if the front end
    /// sends its id. The refusal is exercised where it lives, in
    /// `scan_rules_with` and `clean_rules_with`.
    #[test]
    fn an_unavailable_rule_is_neither_scanned_nor_cleaned() {
        let mut unavailable = rule("x.y");
        unavailable.paths = vec![r"%TEMP%\**\*".into()];
        unavailable.unavailable_reason = Some("%TEMP% is outside the profile".into());

        let scans = scan_rules_with(std::slice::from_ref(&unavailable), |_| {
            panic!("an unavailable rule must not be scanned")
        })
        .unwrap();
        assert_eq!(scans.len(), 1);
        assert_eq!(scans[0].file_count, 0);
        assert!(scans[0].paths.is_empty());
        assert_eq!(scans[0].skipped, 1);

        let report = clean_rules_with(vec![unavailable], CleanMode::Auto, |_, _| {
            panic!("an unavailable rule must not be cleaned")
        });
        assert_eq!(report.deleted, 0);
        assert_eq!(report.skipped.len(), 1);
        assert_eq!(report.skipped[0].path, "x.y");
        assert_eq!(report.skipped[0].reason, "%TEMP% is outside the profile");
    }

    /// Locks the order down at the call site, not in a sequence replayed by
    /// hand: `clean_rules_with` is the function `clean` executes, and the
    /// injected recycle bin API says when the bin was actually emptied.
    #[test]
    fn the_recycle_bin_is_emptied_before_any_files_rule() {
        // Otherwise, in "Recycle Bin" mode, the "files" rule drops its files
        // into it and the Recycle Bin rule destroys them in the same pass.
        let dir = tempfile::TempDir::new().unwrap();
        let root = dir.path().to_string_lossy().to_string();
        std::fs::create_dir_all(dir.path().join(r"AppData\Local\Temp")).unwrap();
        std::fs::write(dir.path().join(r"AppData\Local\Temp\a.txt"), b"aaa").unwrap();
        let lookup = move |name: &str| match name {
            "USERPROFILE" => Some(root.clone()),
            "TEMP" => Some(format!(r"{root}\AppData\Local\Temp")),
            _ => None,
        };
        let log = std::cell::RefCell::new(Vec::new());
        let query = || Ok((2u64, 20u64));
        let empty = || {
            log.borrow_mut().push("recycle bin emptied".to_string());
            Ok(())
        };

        let report = clean_rules_with(
            vec![rule("a"), recycle_bin_rule(), rule("b")],
            CleanMode::Permanent,
            |r, mode| {
                log.borrow_mut().push(r.id.clone());
                crate::clean::clean_rule_with_api(r, mode, &lookup, &query, &empty)
                    .map_err(|e| e.to_string())
            },
        );

        assert_eq!(
            *log.borrow(),
            vec!["windows.recycle-bin", "recycle bin emptied", "a", "b"]
        );
        assert!(report.skipped.is_empty(), "{:?}", report.skipped);
        // 2 items announced by the recycle bin + the file of rule "a".
        assert_eq!(report.deleted, 3);
    }

    #[test]
    fn the_order_of_the_files_rules_is_preserved() {
        let rules = cleaning_order(vec![rule("a"), rule("b"), rule("c")]);
        let ids: Vec<&str> = rules.iter().map(|r| r.id.as_str()).collect();
        assert_eq!(ids, vec!["a", "b", "c"]);
    }

    #[test]
    fn cleaning_an_unknown_id_is_an_error() {
        let err = tauri::async_runtime::block_on(clean(
            vec!["nonexistent".to_string()],
            crate::clean::CleanMode::Auto,
        ))
        .unwrap_err();
        assert!(err.contains("nonexistent"));
    }
}
