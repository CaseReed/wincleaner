use crate::clean::{
    clean_rule, clean_rule_with_trash, CleanMode, CleanReport, CleanTick, SkippedItem,
};
use crate::rules::{embedded_rules, Risk, Rule, RuleKind};
use crate::sandbox::{
    full_catalogue, orphaned_sandboxes, remove_orphan, resolved_patterns, sandbox_recycle_empty,
    sandbox_recycle_query, sandbox_trash, verify, Fixture, Orphan, SandboxManifest, SandboxSummary,
    SandboxVerdict, SANDBOX_PREFIX,
};
use crate::scan::{scan_rule, scan_rule_with_api, ScanResult};
use crate::startup::StartupEntry;
use crate::update::{check_with, http_get, UpdateCheck, LATEST_RELEASE_URL};
use serde::{Deserialize, Serialize};
use std::collections::HashMap;
use std::sync::{Arc, Mutex, OnceLock};
use std::time::{Duration, Instant};
use sysinfo::System;
use tauri::Emitter;

/// Processes considered "an open browser" for the warning banner.
pub const BROWSER_PROCESSES: [&str; 3] = ["msedge.exe", "chrome.exe", "firefox.exe"];

/// One tracked browser found among the running processes: how many processes
/// it has, and whether any of them still owns a visible window. Chrome (and
/// Edge) keep several background processes alive after every window is
/// closed ("Continue running background apps") — `has_window` is what tells
/// the front end whether the user actually has the browser open, or just its
/// leftover background processes.
#[derive(Debug, Clone, PartialEq, Serialize)]
pub struct RunningBrowser {
    pub process: String,
    pub name: String,
    pub processes: u32,
    pub has_window: bool,
}

fn browser_display_name(process: &str) -> &'static str {
    match process {
        "chrome.exe" => "Google Chrome",
        "msedge.exe" => "Microsoft Edge",
        "firefox.exe" => "Mozilla Firefox",
        _ => "Unknown browser",
    }
}

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
    /// Inline warning shown under the rule row (Winapp2 `Warning=`).
    pub note: Option<String>,
    /// French translation of `label`, null for a Winapp2 rule or a native rule
    /// that has none. The front end falls back to `label` when this is null.
    pub label_fr: Option<String>,
    /// French translation of `note`, on the same terms as `label_fr`.
    pub description_fr: Option<String>,
    /// French translation of `category`, on the same terms as `label_fr`.
    pub category_fr: Option<String>,
    /// Set when the rule does not apply on this machine. The front end greys
    /// the row out and shows this reason; the rule is neither scanned nor
    /// cleaned, even if its id were sent.
    pub unavailable_reason: Option<String>,
}

/// What the rule list is made of, for the summary line in the UI.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, Serialize, Deserialize)]
pub struct RulesSummary {
    /// Rules declared in `rules.toml`.
    pub native: u32,
    /// Winapp2 entries that survived conversion and validation.
    pub winapp2_retained: u32,
    /// ... of which are shown, because the application was detected.
    pub winapp2_detected: u32,
    /// Winapp2 entries dropped, all reasons together.
    pub winapp2_dropped: u32,
}

/// One step of an Analyze, sent to the front end after each measured rule.
/// Measuring eighty-odd rules takes ten to thirty seconds on a loaded profile:
/// without this, the only feedback is a spinner that says nothing about how
/// far along the walk is.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ScanProgress {
    /// Rules measured so far, this one included. Starts at 1.
    pub done: u32,
    /// Rules this scan will measure. `done == total` marks the last event.
    pub total: u32,
    /// The rule that has just been measured.
    pub rule_id: String,
    /// Its label, shown as is next to the counter.
    pub label: String,
    /// Bytes measured **since the start of this scan**, not the size of this
    /// one rule: the hero shows this number verbatim, so a dropped or
    /// duplicated event cannot make the running total drift.
    pub total_bytes: u64,
}

/// One step of a Clean, sent to the front end while `clean` runs. Emptying a
/// loaded Recycle Bin is minutes of work, not seconds: without this the only
/// feedback is a disabled button reading "Cleaning…".
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct CleanProgress {
    /// Rules finished or in flight, this one included. Starts at 1.
    pub done_rules: u32,
    /// Rules this clean will go through. `done_rules == total_rules` marks the
    /// last event.
    pub total_rules: u32,
    /// The rule being cleaned.
    pub rule_id: String,
    /// Its label, shown as is next to the counter.
    pub label: String,
    /// Files deleted **since the start of this clean**, not by this one rule:
    /// the hero shows this number verbatim, so a dropped or duplicated event
    /// cannot make the running total drift. Same for `bytes_freed`.
    pub files_deleted: u64,
    pub bytes_freed: u64,
}

/// How many files a rule has to delete before it says so again. One event per
/// file would be 59,000 IPC messages for a single Recycle Bin; one per rule
/// leaves that rule looking frozen for thirteen minutes.
const PROGRESS_EVERY: u64 = 500;

/// The rule list of this session: the native rules first, then the Winapp2
/// rules whose application was detected.
pub struct Catalogue {
    pub rules: Vec<Rule>,
    pub summary: RulesSummary,
}

fn build_catalogue() -> Result<Catalogue, String> {
    let native = embedded_rules().map_err(|e| e.to_string())?;
    let winapp2 = crate::winapp2::embedded_winapp2(&native);
    let summary = RulesSummary {
        native: native.len() as u32,
        winapp2_retained: winapp2.report.retained,
        winapp2_detected: winapp2.rules.len() as u32,
        winapp2_dropped: winapp2.report.dropped(),
    };
    let mut rules = native;
    rules.extend(winapp2.rules);
    Ok(Catalogue { rules, summary })
}

static CATALOGUE: OnceLock<Result<Catalogue, String>> = OnceLock::new();

/// Built once and cached for the session: parsing a few megabytes of ini and
/// probing the registry for 2,000 entries costs about a second, and the answer
/// cannot change while the application runs.
pub fn catalogue() -> Result<&'static Catalogue, String> {
    CATALOGUE
        .get_or_init(build_catalogue)
        .as_ref()
        .map_err(|e| e.clone())
}

/// The catalogue holds thousands of Winapp2 rules: a linear scan per requested
/// id turns "clean everything selected" into thousands of scans of thousands of
/// rules. The index is built once per call and dropped with it.
fn pick_rules(catalogue: &Catalogue, rule_ids: &[String]) -> Result<Vec<Rule>, String> {
    let by_id: HashMap<&str, &Rule> = catalogue
        .rules
        .iter()
        .map(|r| (r.id.as_str(), r))
        .collect();
    rule_ids
        .iter()
        .map(|id| {
            by_id
                .get(id.as_str())
                .map(|r| (*r).clone())
                .ok_or_else(|| format!("unknown rule: \"{id}\""))
        })
        .collect()
}

fn find_rules(rule_ids: &[String]) -> Result<Vec<Rule>, String> {
    pick_rules(catalogue()?, rule_ids)
}

fn summarize(rule: Rule) -> RuleSummary {
    RuleSummary {
        id: rule.id,
        category: rule.category,
        label: rule.label,
        risk: rule.risk,
        kind: rule.kind,
        default_checked: rule.default_checked && rule.unavailable_reason.is_none(),
        note: rule.note,
        label_fr: rule.label_fr,
        description_fr: rule.description_fr,
        category_fr: rule.category_fr,
        unavailable_reason: rule.unavailable_reason,
    }
}

pub fn rule_summaries() -> Result<Vec<RuleSummary>, String> {
    Ok(catalogue()?.rules.iter().cloned().map(summarize).collect())
}

/// Pure core of `running_browsers`: groups `(process name, pid)` pairs by the
/// browsers we track, counting their processes and asking `has_window`
/// (injected so this needs no real desktop) whether any of a browser's pids
/// still owns a visible top-level window.
pub fn summarise(processes: &[(String, u32)], has_window: &dyn Fn(u32) -> bool) -> Vec<RunningBrowser> {
    let mut order: Vec<String> = Vec::new();
    let mut pids: HashMap<String, Vec<u32>> = HashMap::new();
    for (name, pid) in processes {
        let lower = name.to_lowercase();
        if !BROWSER_PROCESSES.contains(&lower.as_str()) {
            continue;
        }
        if !pids.contains_key(&lower) {
            order.push(lower.clone());
        }
        pids.entry(lower).or_default().push(*pid);
    }
    order
        .into_iter()
        .map(|process| {
            let list = &pids[&process];
            RunningBrowser {
                name: browser_display_name(&process).to_string(),
                processes: list.len() as u32,
                has_window: list.iter().any(|&pid| has_window(pid)),
                process,
            }
        })
        .collect()
}

/// Whether any window still visible on screen belongs to `pid`. Walks every
/// top-level window with `EnumWindows`, matching each one's owning process
/// with `GetWindowThreadProcessId`. A background-only process (Chrome's
/// "Continue running background apps") owns no such window.
fn pid_has_visible_window(pid: u32) -> bool {
    use windows::Win32::Foundation::{BOOL, HWND, LPARAM};
    use windows::Win32::UI::WindowsAndMessaging::{
        EnumWindows, GetWindowThreadProcessId, IsWindowVisible,
    };

    struct Search {
        target: u32,
        found: bool,
    }

    unsafe extern "system" fn visit(hwnd: HWND, lparam: LPARAM) -> BOOL {
        let search = &mut *(lparam.0 as *mut Search);
        let mut owner_pid: u32 = 0;
        GetWindowThreadProcessId(hwnd, Some(&mut owner_pid));
        if owner_pid == search.target && IsWindowVisible(hwnd).as_bool() {
            search.found = true;
            return BOOL(0); // stop the walk: one visible window is enough
        }
        BOOL(1)
    }

    let mut search = Search {
        target: pid,
        found: false,
    };
    unsafe {
        // A stopped enumeration returns an error from the last callback
        // return value, not a real failure: the answer is in `search.found`
        // either way.
        let _ = EnumWindows(Some(visit), LPARAM(&mut search as *mut Search as isize));
    }
    search.found
}

/// Async like the other heavy commands: the first call builds the catalogue if
/// the startup warm-up has not finished yet, and that must not happen on the
/// thread pumping the window events.
#[tauri::command]
pub async fn list_rules(
    state: tauri::State<'_, SandboxState>,
) -> Result<Vec<RuleSummary>, String> {
    let state = state.inner().clone();
    blocking(move || rule_summaries_in(&state)).await
}

#[tauri::command]
pub async fn rules_summary(
    state: tauri::State<'_, SandboxState>,
) -> Result<RulesSummary, String> {
    let state = state.inner().clone();
    blocking(move || rules_summary_in(&state)).await
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

/// Caps the worker pool: past a handful of threads, disk and Recycle Bin I/O
/// stop scaling and only add contention. `available_parallelism` can also
/// return a number larger than makes sense to spawn for a handful of rules.
const MAX_SCAN_WORKERS: usize = 8;

/// Scans every rule concurrently, refusing on the spot those that do not
/// apply to this machine. `run` is injected so that the test exercises that
/// refusal right here, and not in a copy of the loop; it is called from
/// worker threads, so it must be `Sync` — the two callers close over either
/// nothing but free functions (`scan_rule`) or a read-only `Fixture`/lookup,
/// never a `RefCell` or other single-threaded cache.
///
/// The wall time of an Analyze used to be the sum of every rule (dominated by
/// a couple of slow ones, e.g. the Winapp2 catch-all and the Recycle Bin's own
/// OS call); scanning them in parallel brings it down to roughly the slowest
/// rule instead. `progress` is still called exactly once per rule, unavailable
/// ones included — they cost nothing to "measure", but dropping them from the
/// count would leave the bar short of its end — but the order it fires in now
/// follows completion, not the catalogue: two rules finishing on different
/// threads can report in either order. The returned `Vec` is unaffected: it is
/// assembled by index, so it always comes back in catalogue order.
pub fn scan_rules_with(
    rules: &[Rule],
    run: impl Fn(&Rule) -> Result<ScanResult, String> + Sync,
    progress: &mut (dyn FnMut(ScanProgress) + Send),
) -> Result<Vec<ScanResult>, String> {
    let total = rules.len() as u32;
    let workers = std::thread::available_parallelism()
        .map(|n| n.get())
        .unwrap_or(1)
        .min(MAX_SCAN_WORKERS)
        .min(rules.len());

    let next_index = std::sync::atomic::AtomicUsize::new(0);
    let results: Mutex<Vec<Option<ScanResult>>> = Mutex::new(vec![None; rules.len()]);
    // One lock around the running byte total, the done count and the actual
    // callback: `total_bytes` is a cumulative sum the front end trusts
    // verbatim, so the count, the sum and the emit must advance together or a
    // race could hand out a `done` further along than its `total_bytes`.
    let progress_state = Mutex::new((0u32, 0u64, progress));
    let first_error: Mutex<Option<String>> = Mutex::new(None);

    std::thread::scope(|scope| {
        for _ in 0..workers {
            scope.spawn(|| loop {
                let index = next_index.fetch_add(1, std::sync::atomic::Ordering::Relaxed);
                if index >= rules.len() {
                    return;
                }
                let r = &rules[index];
                let result = match &r.unavailable_reason {
                    // The rule does not apply on this machine: nothing to
                    // walk, and above all nothing to delete. Counted as
                    // "skipped", not an error.
                    Some(_) => ScanResult {
                        rule_id: r.id.clone(),
                        file_count: 0,
                        total_bytes: 0,
                        paths: Vec::new(),
                        skipped: 1,
                    },
                    None => match run(r) {
                        Ok(result) => result,
                        Err(e) => {
                            let mut first_error = first_error.lock().unwrap();
                            if first_error.is_none() {
                                *first_error = Some(e);
                            }
                            continue;
                        }
                    },
                };
                {
                    let mut state = progress_state.lock().unwrap();
                    state.0 += 1;
                    state.1 += result.total_bytes;
                    let (done, total_bytes) = (state.0, state.1);
                    (state.2)(ScanProgress {
                        done,
                        total,
                        rule_id: r.id.clone(),
                        label: r.label.clone(),
                        total_bytes,
                    });
                }
                results.lock().unwrap()[index] = Some(result);
            });
        }
    });

    if let Some(e) = first_error.into_inner().unwrap() {
        return Err(e);
    }
    Ok(results
        .into_inner()
        .unwrap()
        .into_iter()
        .map(|r| r.expect("every non-errored index was filled before the scope joined"))
        .collect())
}

fn scan_rules(
    rule_ids: &[String],
    progress: &mut (dyn FnMut(ScanProgress) + Send),
) -> Result<Vec<ScanResult>, String> {
    scan_rules_with(
        &find_rules(rule_ids)?,
        |r| scan_rule(r).map_err(|e| e.to_string()),
        progress,
    )
}

/// Emits `scan-progress` after each rule. The event goes out from inside the
/// blocking closure — the point is to reach the window *while* the walk runs,
/// not once it has returned. An emit that fails (window already gone) must
/// never abort a scan that is still legitimate work.
#[tauri::command]
pub async fn scan(
    app: tauri::AppHandle,
    state: tauri::State<'_, SandboxState>,
    rule_ids: Vec<String>,
) -> Result<Vec<ScanResult>, String> {
    let state = state.inner().clone();
    blocking(move || {
        scan_rules_in(&state, &rule_ids, &mut |step| {
            let _ = app.emit("scan-progress", step);
        })
    })
    .await
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
/// right here: a test replaying the sequence by hand would lock nothing down;
/// `progress` likewise, so the sequence the front end draws is asserted without
/// a Tauri application. One event closes every rule, unavailable ones included
/// — they cost nothing to "clean", but dropping them from the count would leave
/// the bar short of its end — plus one every `PROGRESS_EVERY` files inside a
/// rule, which is the only feedback a Recycle Bin holding 59,000 files gives
/// for minutes on end.
pub fn clean_rules_with(
    rules: Vec<Rule>,
    mode: CleanMode,
    mut run: impl FnMut(&Rule, CleanMode, CleanTick) -> Result<CleanReport, String>,
    progress: &mut dyn FnMut(CleanProgress),
) -> CleanReport {
    let ordered = cleaning_order(rules);
    let total_rules = ordered.len() as u32;
    let mut report = CleanReport::default();
    // Running totals across the rules already finished: the mid-rule events add
    // the rule's own counts on top, so what the front end reads never goes
    // backwards between two rules.
    let mut files = 0u64;
    let mut bytes = 0u64;
    for (index, rule) in ordered.iter().enumerate() {
        let step = |files_deleted, bytes_freed| CleanProgress {
            done_rules: index as u32 + 1,
            total_rules,
            rule_id: rule.id.clone(),
            label: rule.label.clone(),
            files_deleted,
            bytes_freed,
        };
        let outcome = match &rule.unavailable_reason {
            Some(reason) => Err(reason.clone()),
            None => {
                let mut announced = 0u64;
                let mut tick = |deleted: u64, freed: u64| {
                    if deleted >= announced + PROGRESS_EVERY {
                        announced = deleted;
                        progress(step(files + deleted, bytes + freed));
                    }
                };
                run(rule, mode, &mut tick)
            }
        };
        match outcome {
            Ok(partial) => {
                files += partial.deleted;
                bytes += partial.freed_bytes;
                report.merge(partial);
            }
            Err(reason) => report.skipped.push(SkippedItem {
                path: rule.id.clone(),
                reason,
            }),
        }
        progress(step(files, bytes));
    }
    report
}

fn clean_rules(
    rule_ids: &[String],
    mode: CleanMode,
    progress: &mut dyn FnMut(CleanProgress),
) -> Result<CleanReport, String> {
    Ok(clean_rules_with(
        find_rules(rule_ids)?,
        mode,
        |rule, mode, tick| clean_rule(rule, mode, tick).map_err(|e| e.to_string()),
        progress,
    ))
}

/// Emits `clean-progress` while it deletes, for the same reason `scan` emits
/// `scan-progress`: the event has to reach the window *during* the work, not
/// once it has returned. An emit that fails (window already gone) must never
/// abort a deletion that is still legitimate work.
#[tauri::command]
pub async fn clean(
    app: tauri::AppHandle,
    state: tauri::State<'_, SandboxState>,
    rule_ids: Vec<String>,
    mode: CleanMode,
) -> Result<CleanReport, String> {
    let state = state.inner().clone();
    blocking(move || {
        clean_rules_in(&state, &rule_ids, mode, &mut |step| {
            let _ = app.emit("clean-progress", step);
        })
    })
    .await
}

#[tauri::command]
pub fn running_browsers() -> Vec<RunningBrowser> {
    let mut system = System::new_all();
    system.refresh_processes(sysinfo::ProcessesToUpdate::All, true);
    let processes: Vec<(String, u32)> = system
        .processes()
        .iter()
        .map(|(pid, p)| (p.name().to_string_lossy().to_string(), pid.as_u32()))
        .collect();
    summarise(&processes, &pid_has_visible_window)
}

/// The minimum spacing between two GitHub requests this process will make: a
/// user mashing the button, or a StrictMode double-invoke, must not turn "one
/// request" into two.
const CHECK_COOLDOWN: Duration = Duration::from_secs(10);

/// The last time `check_for_updates` actually reached the network, and what
/// it got back. `None` until the first call, or after every call so far has
/// failed: only a successful check is cached.
static LAST_CHECK: Mutex<Option<(Instant, UpdateCheck)>> = Mutex::new(None);

/// Refuses to run `check` again within `cooldown` of the last time it ran:
/// the previous result is handed back instead, rather than opening a second
/// connection. `now` and `check` are injected so the cooldown itself is
/// tested without a real clock or a real socket.
///
/// Only an `Ok` result starts or extends the cooldown: an `Err` (offline,
/// rate-limited, malformed...) is a transient failure, not a cached answer —
/// caching it would make a click that failed once refuse to retry for ten
/// seconds. A second click after a failure must reach the network again.
fn check_for_updates_with(
    state: &Mutex<Option<(Instant, UpdateCheck)>>,
    cooldown: Duration,
    now: Instant,
    check: impl FnOnce() -> Result<UpdateCheck, String>,
) -> Result<UpdateCheck, String> {
    {
        let guard = state.lock().unwrap_or_else(|e| e.into_inner());
        if let Some((at, result)) = guard.as_ref() {
            if now.saturating_duration_since(*at) < cooldown {
                return Ok(result.clone());
            }
        }
    }
    let result = check();
    if let Ok(ok) = &result {
        *state.lock().unwrap_or_else(|e| e.into_inner()) = Some((now, ok.clone()));
    }
    result
}

/// The single network-touching command: one GET on the GitHub REST API, run
/// off the main thread like every other blocking call here. The error crossing
/// the IPC boundary is a stable code (`offline`, `not-available`,
/// `rate-limited`, `malformed`); the wording lives in the front end. A call
/// within `CHECK_COOLDOWN` of the last one returns that last result again
/// instead of opening a second connection — the front end needs no special
/// handling for this, the button is already disabled while a check is
/// pending.
#[tauri::command]
pub async fn check_for_updates() -> Result<UpdateCheck, String> {
    blocking(|| {
        check_for_updates_with(&LAST_CHECK, CHECK_COOLDOWN, Instant::now(), || {
            check_with(env!("CARGO_PKG_VERSION"), http_get, LATEST_RELEASE_URL)
                .map_err(|e| e.code().to_string())
        })
    })
    .await
}

#[tauri::command]
pub async fn list_startup(
    state: tauri::State<'_, SandboxState>,
) -> Result<Vec<StartupEntry>, String> {
    let state = state.inner().clone();
    blocking(move || {
        refuse_startup_in_sandbox(&state)?;
        crate::startup::list_startup().map_err(|e| e.to_string())
    })
    .await
}

#[tauri::command]
pub async fn set_startup_enabled(
    state: tauri::State<'_, SandboxState>,
    id: String,
    enabled: bool,
) -> Result<(), String> {
    let state = state.inner().clone();
    blocking(move || {
        refuse_startup_in_sandbox(&state)?;
        crate::startup::set_startup_enabled(&id, enabled).map_err(|e| e.to_string())
    })
    .await
}


// ---------------------------------------------------------------------------
// Sandbox mode.
//
// A user who wants proof that WinCleaner deletes only junk should not have to
// take CI's word for it. `sandbox_enter` builds, on their own machine, the very
// fixture the safety harness runs against, points the whole engine at it, and
// lets them Analyze and Clean for real; `sandbox_verify` then reads the disk
// back against the manifest.
//
// Nothing real is reachable while a sandbox is active:
//
// * the four rule variables all resolve inside the sandbox root, so the
//   containment the application already enforces confines every walk and every
//   deletion to it (`sandbox.rs`, module doc);
// * the recycle-bin query and empty are the sandbox stand-ins, so
//   `SHQueryRecycleBinW` and `SHEmptyRecycleBinW` are never called;
// * `Trash` mode moves the file into `<root>\recycle-bin` instead of calling
//   `trash::delete`, so the real Recycle Bin of the volume is never written to
//   either;
// * `list_startup` and `set_startup_enabled` refuse outright, because they read
//   and write the real `HKCU` keys and no sandbox can stand in for those.
// ---------------------------------------------------------------------------

/// A sandbox that is open right now: the fixture on disk, the catalogue built
/// against it, and the promise `sandbox_verify` checks the disk against.
pub struct ActiveSandbox {
    fixture: Fixture,
    catalogue: Catalogue,
    manifest: SandboxManifest,
    summary: SandboxSummary,
}

/// Managed by Tauri. The `Arc` is what lets a command clone the handle and take
/// it into `spawn_blocking`: a `State` borrow cannot cross that boundary, and
/// every sandbox operation is disk work that has no business on the thread
/// pumping the window events.
#[derive(Clone, Default)]
pub struct SandboxState(Arc<Mutex<Option<ActiveSandbox>>>);

fn lock(state: &SandboxState) -> std::sync::MutexGuard<'_, Option<ActiveSandbox>> {
    state.0.lock().unwrap_or_else(|e| e.into_inner())
}

/// Enough entropy to keep two runs apart, with no new dependency: the process
/// id, the nanoseconds since the epoch, and a counter so two sandboxes created
/// inside the same clock tick cannot land on the same name.
fn sandbox_token() -> String {
    static COUNTER: std::sync::atomic::AtomicU64 = std::sync::atomic::AtomicU64::new(0);
    let nanos = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|d| d.as_nanos())
        .unwrap_or(0);
    let n = COUNTER.fetch_add(1, std::sync::atomic::Ordering::Relaxed);
    format!("{:x}-{:x}-{:x}", std::process::id(), nanos, n)
}

/// `FILE_ATTRIBUTE_REPARSE_POINT` on the directory itself, never followed.
fn is_reparse_point(path: &std::path::Path) -> bool {
    use std::os::windows::fs::MetadataExt;
    std::fs::symlink_metadata(path)
        .map(|md| md.file_attributes() & 0x0000_0400 != 0)
        .unwrap_or(false)
}

fn build_sandbox(root: &std::path::Path) -> Result<ActiveSandbox, String> {
    let mut fixture = Fixture::build_in(root)?;
    let (native, winapp2, report) = full_catalogue(&fixture)?;

    // One concrete file per converted glob, exactly as the harness does, so the
    // Winapp2 rules delete something instead of walking empty trees.
    let patterns = resolved_patterns(&fixture, &winapp2)?;
    fixture.add_winapp2_junk(&patterns);

    let rules_summary = RulesSummary {
        native: native.len() as u32,
        winapp2_retained: report.retained,
        winapp2_detected: winapp2.len() as u32,
        winapp2_dropped: report.dropped(),
    };
    let mut rules = native;
    rules.extend(winapp2);

    let manifest = fixture.manifest();
    let summary = SandboxSummary {
        root: manifest.root.display().to_string(),
        sentinels: manifest.sentinels.len() as u32,
        junk: manifest.junk.len() as u32,
        winapp2_rules: rules_summary.winapp2_detected,
    };
    Ok(ActiveSandbox {
        fixture,
        catalogue: Catalogue {
            rules,
            summary: rules_summary,
        },
        manifest,
        summary,
    })
}

/// Creates the sandbox and switches the engine onto it. Refuses if one is
/// already active: two sandboxes would mean two catalogues and one verdict.
pub fn enter_sandbox(state: &SandboxState) -> Result<SandboxSummary, String> {
    let mut guard = lock(state);
    if guard.is_some() {
        return Err("A sandbox is already active. Leave it before creating another.".to_string());
    }
    let root = std::env::temp_dir().join(format!("{SANDBOX_PREFIX}{}", sandbox_token()));
    // `create_dir`, not `create_dir_all`: the name carries a random suffix, so
    // the directory must not exist yet. `create_dir_all` on a name someone
    // else planted — a junction to somewhere else, say — would succeed
    // silently and the whole fixture would be built on the other side of it.
    std::fs::create_dir(&root).map_err(|e| format!("{}: {e}", root.display()))?;
    if is_reparse_point(&root) {
        let _ = std::fs::remove_dir(&root);
        return Err(format!(
            "The sandbox root is a reparse point and was refused: {}",
            root.display()
        ));
    }
    match build_sandbox(&root) {
        Ok(active) => {
            let summary = active.summary.clone();
            *guard = Some(active);
            Ok(summary)
        }
        Err(err) => {
            // A half-built sandbox is not left behind in `%TEMP%`.
            let _ = std::fs::remove_dir_all(&root);
            Err(err)
        }
    }
}

/// Removes the sandbox directory and hands the real catalogue back.
///
/// The junctions are unlinked with `remove_dir` **before** the tree goes:
/// `remove_dir_all` on a junction is free to descend into it, which would
/// delete what lives on the other side.
///
/// The tree goes first and the state is cleared **only on success**. Clearing
/// it first would leave a failed leave — a file still open somewhere under the
/// root — with the back end already back on the real catalogue while the
/// directory is still there and the front end still shows the banner: the next
/// Clean would then run against the user's own profile with a sandbox banner
/// on screen.
pub fn leave_sandbox(state: &SandboxState) -> Result<(), String> {
    let mut guard = lock(state);
    let (root, junctions) = {
        let active = guard.as_ref().ok_or("No sandbox is active.")?;
        (
            active.manifest.root.clone(),
            active.manifest.junctions.clone(),
        )
    };
    for link in &junctions {
        let _ = std::fs::remove_dir(link);
    }
    std::fs::remove_dir_all(&root).map_err(|e| {
        format!(
            "The sandbox is still active: its directory could not be removed ({e}) at {}",
            root.display()
        )
    })?;
    *guard = None;
    Ok(())
}

pub fn sandbox_status_of(state: &SandboxState) -> Option<SandboxSummary> {
    lock(state).as_ref().map(|a| a.summary.clone())
}

/// `rule_ids` are the rules the user just cleaned: the junk counts are scoped
/// to them, so a partial selection reads correctly instead of red. The
/// sentinels and the junction baits are checked whatever the selection.
pub fn verify_sandbox(
    state: &SandboxState,
    rule_ids: &[String],
) -> Result<SandboxVerdict, String> {
    let guard = lock(state);
    let active = guard.as_ref().ok_or("No sandbox is active.")?;
    Ok(verify(&active.manifest, rule_ids))
}

// ---------------------------------------------------------------------------
// Orphaned sandboxes.
//
// `leave_sandbox` is the only thing that removes a sandbox tree, and it only
// runs when the user asks for it. A process that dies before that — a crash, a
// kill from the Task Manager, a window closed with a sandbox still active —
// leaves a few hundred files in `%TEMP%` that nothing will ever mention again.
//
// Two sweeps, and they are not equivalent:
//
// * `lib.rs` runs `sweep_orphans` at every start, off the main thread. This is
//   the safety net, and the only one that is guaranteed: whatever happened to
//   the previous process, the next start cleans up after it.
// * `sandbox_orphans` / `sandbox_remove_orphans` put the same thing in
//   Settings, so a user who has just seen a sandbox survive a crash does not
//   have to restart the application to be rid of it.
//
// Neither ever touches a directory whose owning process is still running: a
// second WinCleaner with an open sandbox is not an orphan.
// ---------------------------------------------------------------------------

/// The process check the application runs with. Injected everywhere else, so
/// no test ever asks the real machine what is running.
fn pid_is_running(pid: u32) -> bool {
    let mut system = System::new();
    system.refresh_processes(
        sysinfo::ProcessesToUpdate::Some(&[sysinfo::Pid::from_u32(pid)]),
        true,
    );
    system.process(sysinfo::Pid::from_u32(pid)).is_some()
}

/// The sandbox directories left behind in `%TEMP%`, excluding the one this
/// process has open.
pub fn orphans_of(state: &SandboxState) -> Vec<Orphan> {
    let active = lock(state).as_ref().map(|a| a.manifest.root.clone());
    orphaned_sandboxes(
        &std::env::temp_dir(),
        std::process::id(),
        active.as_deref(),
        &pid_is_running,
    )
}

/// Removes each of `orphans` and answers how many went. A failure on one — a
/// file still open, a permission the sweep does not have — is not allowed to
/// stop the others: the point is to reclaim what can be reclaimed, and the
/// next start tries the rest again.
fn remove_all(temp_dir: &std::path::Path, orphans: &[Orphan]) -> u32 {
    orphans
        .iter()
        .filter(|orphan| match remove_orphan(temp_dir, &orphan.path) {
            Ok(()) => true,
            Err(err) => {
                eprintln!("orphaned sandbox: {err}");
                false
            }
        })
        .count() as u32
}

/// What the Settings button does: sweep, minus whatever this process has open.
pub fn remove_orphans_of(state: &SandboxState) -> u32 {
    remove_all(&std::env::temp_dir(), &orphans_of(state))
}

/// The startup sweep. No sandbox can be active this early, so there is nothing
/// to exclude beyond this process's own pid.
pub fn sweep_orphans() -> u32 {
    let temp = std::env::temp_dir();
    let orphans = orphaned_sandboxes(&temp, std::process::id(), None, &pid_is_running);
    remove_all(&temp, &orphans)
}

/// Why the Startup screen steps aside while a sandbox is active. It reads and
/// writes the real `HKCU\Software\Microsoft\Windows\CurrentVersion\Run` keys;
/// there is nothing to point at a sandbox root, so it refuses instead of
/// pretending.
pub const STARTUP_IN_SANDBOX: &str =
    "Startup programs are unavailable while the sandbox is active: they live in the real \
     Windows registry, which the sandbox never touches. Leave the sandbox to manage them.";

pub fn refuse_startup_in_sandbox(state: &SandboxState) -> Result<(), String> {
    if lock(state).is_some() {
        return Err(STARTUP_IN_SANDBOX.to_string());
    }
    Ok(())
}

/// The rule list the user is looking at: the sandbox catalogue while one is
/// active, the machine's own otherwise.
pub fn rule_summaries_in(state: &SandboxState) -> Result<Vec<RuleSummary>, String> {
    {
        let guard = lock(state);
        if let Some(active) = guard.as_ref() {
            return Ok(active
                .catalogue
                .rules
                .iter()
                .cloned()
                .map(summarize)
                .collect());
        }
    }
    rule_summaries()
}

pub fn rules_summary_in(state: &SandboxState) -> Result<RulesSummary, String> {
    {
        let guard = lock(state);
        if let Some(active) = guard.as_ref() {
            return Ok(active.catalogue.summary);
        }
    }
    Ok(catalogue()?.summary)
}

pub fn scan_rules_in(
    state: &SandboxState,
    rule_ids: &[String],
    progress: &mut (dyn FnMut(ScanProgress) + Send),
) -> Result<Vec<ScanResult>, String> {
    {
        let guard = lock(state);
        if let Some(active) = guard.as_ref() {
            let rules = pick_rules(&active.catalogue, rule_ids)?;
            let lookup = |n: &str| active.fixture.lookup(n);
            return scan_rules_with(
                &rules,
                |r| {
                    scan_rule_with_api(r, &lookup, &sandbox_recycle_query).map_err(|e| e.to_string())
                },
                progress,
            );
        }
    }
    scan_rules(rule_ids, progress)
}

pub fn clean_rules_in(
    state: &SandboxState,
    rule_ids: &[String],
    mode: CleanMode,
    progress: &mut dyn FnMut(CleanProgress),
) -> Result<CleanReport, String> {
    {
        let guard = lock(state);
        if let Some(active) = guard.as_ref() {
            let rules = pick_rules(&active.catalogue, rule_ids)?;
            let lookup = |n: &str| active.fixture.lookup(n);
            let root = active.manifest.root.clone();
            // The sandbox bin is a directory, not a shell operation: a batch is
            // one rename per path, and a failure on one stops the batch so the
            // per-file fallback can name it.
            let trash = |paths: &[std::path::PathBuf]| {
                paths.iter().try_for_each(|p| sandbox_trash(&root, p))
            };
            return Ok(clean_rules_with(
                rules,
                mode,
                |rule, mode, tick| {
                    clean_rule_with_trash(
                        rule,
                        mode,
                        &lookup,
                        &sandbox_recycle_query,
                        &sandbox_recycle_empty,
                        &trash,
                        tick,
                    )
                    .map_err(|e| e.to_string())
                },
                progress,
            ));
        }
    }
    clean_rules(rule_ids, mode, progress)
}

#[tauri::command]
pub async fn sandbox_enter(state: tauri::State<'_, SandboxState>) -> Result<SandboxSummary, String> {
    let state = state.inner().clone();
    blocking(move || enter_sandbox(&state)).await
}

#[tauri::command]
pub async fn sandbox_leave(state: tauri::State<'_, SandboxState>) -> Result<(), String> {
    let state = state.inner().clone();
    blocking(move || leave_sandbox(&state)).await
}

#[tauri::command]
pub async fn sandbox_status(
    state: tauri::State<'_, SandboxState>,
) -> Result<Option<SandboxSummary>, String> {
    let state = state.inner().clone();
    blocking(move || Ok(sandbox_status_of(&state))).await
}

#[tauri::command]
pub async fn sandbox_verify(
    state: tauri::State<'_, SandboxState>,
    rule_ids: Vec<String>,
) -> Result<SandboxVerdict, String> {
    let state = state.inner().clone();
    blocking(move || verify_sandbox(&state, &rule_ids)).await
}

#[tauri::command]
pub async fn sandbox_orphans(
    state: tauri::State<'_, SandboxState>,
) -> Result<Vec<Orphan>, String> {
    let state = state.inner().clone();
    // Measuring the trees is disk work: off the window thread like every other
    // sandbox command.
    blocking(move || Ok(orphans_of(&state))).await
}

#[tauri::command]
pub async fn sandbox_remove_orphans(state: tauri::State<'_, SandboxState>) -> Result<u32, String> {
    let state = state.inner().clone();
    blocking(move || Ok(remove_orphans_of(&state))).await
}

#[cfg(test)]
mod tests {
    use super::*;

    /// No pid ever owns a window: the default stand-in for `has_window` in
    /// tests that only care about which browsers and counts come out.
    fn no_windows(_pid: u32) -> bool {
        false
    }

    #[test]
    fn only_the_targeted_browsers_are_retained() {
        let names: Vec<(String, u32)> = [
            ("explorer.exe", 1),
            ("msedge.exe", 2),
            ("MSEDGE.EXE", 3),
            ("chrome.exe", 4),
            ("code.exe", 5),
        ]
        .iter()
        .map(|(name, pid)| (name.to_string(), *pid))
        .collect();
        let mut got: Vec<String> = summarise(&names, &no_windows)
            .into_iter()
            .map(|b| b.process)
            .collect();
        got.sort();
        assert_eq!(got, vec!["chrome.exe", "msedge.exe"]);
    }

    #[test]
    fn duplicate_processes_are_counted_not_deduplicated() {
        let names: Vec<(String, u32)> = [("firefox.exe", 1), ("firefox.exe", 2), ("firefox.exe", 3)]
            .iter()
            .map(|(name, pid)| (name.to_string(), *pid))
            .collect();
        let got = summarise(&names, &no_windows);
        assert_eq!(got.len(), 1);
        assert_eq!(got[0].process, "firefox.exe");
        assert_eq!(got[0].name, "Mozilla Firefox");
        assert_eq!(got[0].processes, 3);
        assert!(!got[0].has_window);
    }

    #[test]
    fn no_browser_returns_an_empty_list() {
        let names: Vec<(String, u32)> = [("explorer.exe".to_string(), 1)].to_vec();
        assert!(summarise(&names, &no_windows).is_empty());
    }

    #[test]
    fn a_process_owning_a_window_marks_the_browser_as_having_a_window() {
        let names: Vec<(String, u32)> = [("chrome.exe".to_string(), 7), ("chrome.exe".to_string(), 8)].to_vec();
        let got = summarise(&names, &|pid| pid == 8);
        assert_eq!(got.len(), 1);
        assert_eq!(got[0].processes, 2);
        assert!(got[0].has_window);
    }

    #[test]
    fn browser_process_names_map_to_a_display_name() {
        let names: Vec<(String, u32)> = [
            ("chrome.exe".to_string(), 1),
            ("msedge.exe".to_string(), 2),
            ("firefox.exe".to_string(), 3),
        ]
        .to_vec();
        let got = summarise(&names, &no_windows);
        let name_of = |process: &str| {
            got.iter()
                .find(|b| b.process == process)
                .map(|b| b.name.clone())
        };
        assert_eq!(name_of("chrome.exe"), Some("Google Chrome".to_string()));
        assert_eq!(name_of("msedge.exe"), Some("Microsoft Edge".to_string()));
        assert_eq!(name_of("firefox.exe"), Some("Mozilla Firefox".to_string()));
    }

    #[test]
    fn the_catalogue_starts_with_the_native_rules() {
        let summaries = rule_summaries().unwrap();
        let native: Vec<&RuleSummary> = summaries
            .iter()
            .filter(|s| !s.id.starts_with("winapp2."))
            .collect();
        assert_eq!(native.len() as u32, catalogue().unwrap().summary.native);
        assert_eq!(summaries[0].id, "windows.temp");
        assert_eq!(summaries[1].kind, crate::rules::RuleKind::RecycleBin);
    }

    /// The front end builds its checkboxes from this field: if it does not
    /// cross the IPC, everything becomes checked by default again.
    #[test]
    fn the_summary_carries_the_default_checkbox_state() {
        let summaries = rule_summaries().unwrap();
        let recycle_bin = summaries
            .iter()
            .find(|r| r.id == "windows.recycle-bin")
            .unwrap();
        assert!(!recycle_bin.default_checked);
        let temp = summaries.iter().find(|r| r.id == "windows.temp").unwrap();
        assert!(temp.default_checked);
    }

    /// A duplicate id would make `find_rules` return the wrong rule — the
    /// wrong paths — for one of the two.
    #[test]
    fn every_catalogue_id_is_unique_and_winapp2_rules_are_unchecked() {
        let mut seen = std::collections::HashSet::new();
        for rule in &catalogue().unwrap().rules {
            assert!(seen.insert(rule.id.clone()), "duplicate id {}", rule.id);
            if rule.id.starts_with("winapp2.") {
                assert!(!rule.default_checked, "{}", rule.id);
                assert_eq!(rule.category, "Applications");
            }
        }
    }

    #[test]
    fn the_rules_summary_counts_add_up() {
        let summary = catalogue().unwrap().summary;
        assert!(summary.native >= 8);
        assert!(summary.winapp2_retained >= 500);
        assert!(summary.winapp2_detected <= summary.winapp2_retained);
        assert!(summary.winapp2_dropped > 0);
    }

    /// The catalogue is built once: a second call must not re-parse the ini.
    #[test]
    fn the_catalogue_is_cached() {
        let first = catalogue().unwrap();
        let second = catalogue().unwrap();
        assert!(std::ptr::eq(first, second));
    }

    #[test]
    fn a_winapp2_warning_reaches_the_front_end_as_a_note() {
        // Not every machine has a rule carrying a Warning, so this asserts the
        // plumbing on a rule built here, not on the machine's catalogue.
        let rule = Rule {
            id: "winapp2.x".into(),
            category: "Applications".into(),
            label: "X".into(),
            paths: vec![r"%LOCALAPPDATA%\X\*".into()],
            exclude: vec![],
            risk: Risk::Medium,
            kind: RuleKind::Files,
            default_checked: false,
            note: Some("This deletes the saved sessions.".into()),
            label_fr: None,
            description_fr: None,
            category_fr: None,
            unavailable_reason: None,
        };
        assert_eq!(
            summarize(rule).note.as_deref(),
            Some("This deletes the saved sessions.")
        );
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

    /// Goes through `blocking`, and therefore through `spawn_blocking`, exactly
    /// like the `scan` command: checks that the work moved off the main thread
    /// does return its result. The command itself is not called here because it
    /// now takes an `AppHandle`, which only a running Tauri application has.
    #[test]
    fn scanning_an_unknown_id_is_an_error() {
        let err = tauri::async_runtime::block_on(blocking(|| {
            scan_rules(&["nonexistent".to_string()], &mut |_| {})
        }))
        .unwrap_err();
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
            label_fr: None,
            description_fr: None,
            category_fr: None,
            unavailable_reason: None,
        }
    }

    #[test]
    fn one_rule_failing_does_not_throw_away_the_report_of_the_previous_ones() {
        let rules = vec![rule("a"), rule("b"), rule("c")];
        let report = clean_rules_with(
            rules,
            CleanMode::Auto,
            |r, _, _| {
                if r.id == "b" {
                    Err("access denied".to_string())
                } else {
                    Ok(CleanReport {
                        freed_bytes: 10,
                        deleted: 1,
                        skipped: Vec::new(),
                    })
                }
            },
            &mut |_| {},
        );
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
            label_fr: None,
            description_fr: None,
            category_fr: None,
            unavailable_reason: None,
        }
    }

    /// Rules scan concurrently, so completion order is not the catalogue's:
    /// two rules finishing on different threads can report in either order.
    /// What must still hold is that every rule reports exactly once, `done`
    /// counts up to `total` with no gap or repeat, `total_bytes` accumulates
    /// correctly whatever the order, and the returned `Vec` — unlike the
    /// progress stream — always comes back in catalogue order.
    #[test]
    fn every_rule_reports_its_progress_exactly_once() {
        let rules = vec![rule("a"), rule("b"), rule("c")];
        let seen: Mutex<Vec<ScanProgress>> = Mutex::new(Vec::new());
        let scans = scan_rules_with(
            &rules,
            |r| {
                Ok(ScanResult {
                    rule_id: r.id.clone(),
                    file_count: 1,
                    total_bytes: 100,
                    paths: Vec::new(),
                    skipped: 0,
                })
            },
            &mut |p| seen.lock().unwrap().push(p),
        )
        .unwrap();

        assert_eq!(
            scans.iter().map(|s| s.rule_id.as_str()).collect::<Vec<_>>(),
            ["a", "b", "c"],
            "the result Vec keeps catalogue order regardless of scan order"
        );

        let seen = seen.into_inner().unwrap();
        assert_eq!(seen.len(), 3);
        let mut ids: Vec<&str> = seen.iter().map(|p| p.rule_id.as_str()).collect();
        ids.sort_unstable();
        assert_eq!(ids, ["a", "b", "c"], "every rule reports exactly once");
        let mut done: Vec<u32> = seen.iter().map(|p| p.done).collect();
        done.sort_unstable();
        assert_eq!(done, [1, 2, 3], "done counts up with no gap or repeat");
        assert!(seen.iter().all(|p| p.total == 3));
        // `total_bytes` is the running total, so the hero can show it verbatim
        // without adding events up itself: whatever the order, it must land on
        // the full sum once every rule has reported.
        let last = seen.iter().find(|p| p.done == p.total).unwrap();
        assert_eq!(last.total_bytes, 300);
    }

    /// A rule that does not apply to this machine is measured by nobody, but
    /// the user still asked for it: it counts in `total` and reports zero
    /// bytes, so the bar never stalls short of its end.
    #[test]
    fn an_unavailable_rule_still_counts_in_the_progress_total() {
        let mut unavailable = rule("b");
        unavailable.unavailable_reason = Some("%TEMP% is outside the profile".into());
        let rules = vec![rule("a"), unavailable];
        let seen: Mutex<Vec<ScanProgress>> = Mutex::new(Vec::new());
        scan_rules_with(
            &rules,
            |r| {
                Ok(ScanResult {
                    rule_id: r.id.clone(),
                    file_count: 1,
                    total_bytes: 512,
                    paths: Vec::new(),
                    skipped: 0,
                })
            },
            &mut |p| seen.lock().unwrap().push(p),
        )
        .unwrap();

        // Rules scan concurrently: "b" (unavailable, free) can report before
        // or after "a" (which actually runs). Either way, both report once,
        // and the total settles on the full sum once the second one lands.
        let seen = seen.into_inner().unwrap();
        assert_eq!(seen.len(), 2);
        assert!(seen.iter().all(|p| p.total == 2));
        let b = seen.iter().find(|p| p.rule_id == "b").unwrap();
        assert!(b.done == 1 || b.done == 2);
        let last = seen.iter().find(|p| p.done == 2).unwrap();
        assert_eq!(last.total_bytes, 512);
    }

    /// An unavailable rule must never reach the disk, even if the front end
    /// sends its id. The refusal is exercised where it lives, in
    /// `scan_rules_with` and `clean_rules_with`.
    #[test]
    fn an_unavailable_rule_is_neither_scanned_nor_cleaned() {
        let mut unavailable = rule("x.y");
        unavailable.paths = vec![r"%TEMP%\**\*".into()];
        unavailable.unavailable_reason = Some("%TEMP% is outside the profile".into());

        let scans = scan_rules_with(
            std::slice::from_ref(&unavailable),
            |_| panic!("an unavailable rule must not be scanned"),
            &mut |_| {},
        )
        .unwrap();
        assert_eq!(scans.len(), 1);
        assert_eq!(scans[0].file_count, 0);
        assert!(scans[0].paths.is_empty());
        assert_eq!(scans[0].skipped, 1);

        let report = clean_rules_with(
            vec![unavailable],
            CleanMode::Auto,
            |_, _, _| panic!("an unavailable rule must not be cleaned"),
            &mut |_| {},
        );
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
            |r, mode, tick| {
                log.borrow_mut().push(r.id.clone());
                crate::clean::clean_rule_with_api(r, mode, &lookup, &query, &empty, tick)
                    .map_err(|e| e.to_string())
            },
            &mut |_| {},
        );

        assert_eq!(
            *log.borrow(),
            vec!["windows.recycle-bin", "recycle bin emptied", "a", "b"]
        );
        assert!(report.skipped.is_empty(), "{:?}", report.skipped);
        // 2 items announced by the recycle bin + the file of rule "a".
        assert_eq!(report.deleted, 3);
    }

    /// The progress the front end draws during a Clean is produced here, in
    /// cleaning order — recycle bin first — with running totals the hero can
    /// show verbatim, and a last event whose `done_rules` equals `total_rules`.
    #[test]
    fn every_rule_reports_its_cleaning_progress_in_cleaning_order() {
        let rules = vec![rule("a"), recycle_bin_rule(), rule("b")];
        let mut seen: Vec<CleanProgress> = Vec::new();
        clean_rules_with(
            rules,
            CleanMode::Auto,
            |_, _, _| {
                Ok(CleanReport {
                    freed_bytes: 100,
                    deleted: 2,
                    skipped: Vec::new(),
                })
            },
            &mut |p| seen.push(p),
        );

        assert_eq!(
            seen.iter().map(|p| p.rule_id.as_str()).collect::<Vec<_>>(),
            ["windows.recycle-bin", "a", "b"]
        );
        assert_eq!(seen.iter().map(|p| p.done_rules).collect::<Vec<_>>(), [1, 2, 3]);
        assert!(seen.iter().all(|p| p.total_rules == 3));
        assert_eq!(seen.iter().map(|p| p.files_deleted).collect::<Vec<_>>(), [2, 4, 6]);
        assert_eq!(seen.iter().map(|p| p.bytes_freed).collect::<Vec<_>>(), [100, 200, 300]);
        let last = seen.last().unwrap();
        assert_eq!(last.done_rules, last.total_rules);
        assert_eq!(last.label, "b");
    }

    /// The thirteen-minute case: one rule deleting tens of thousands of files
    /// has to say so while it runs, not once it has returned. The rule reports
    /// each deletion step; only every `PROGRESS_EVERY`-th file becomes an
    /// event, and the totals it carries already include the rules before it.
    #[test]
    fn a_rule_reports_from_inside_its_own_deletion_loop() {
        let rules = vec![rule("a"), rule("b")];
        let mut seen: Vec<CleanProgress> = Vec::new();
        clean_rules_with(
            rules,
            CleanMode::Auto,
            |r, _, tick| {
                // "a" deletes one file, "b" deletes 1,200 one at a time.
                let count = if r.id == "a" { 1 } else { 1_200 };
                for n in 1..=count {
                    tick(n, n * 10);
                }
                Ok(CleanReport {
                    freed_bytes: count * 10,
                    deleted: count,
                    skipped: Vec::new(),
                })
            },
            &mut |p| seen.push(p),
        );

        // "a": its single tick is short of the threshold, so only its closing
        // event. "b": one at 500, one at 1,000, then its closing event.
        assert_eq!(
            seen.iter()
                .map(|p| (p.rule_id.as_str(), p.done_rules, p.files_deleted))
                .collect::<Vec<_>>(),
            [
                ("a", 1, 1),
                ("b", 2, 501),
                ("b", 2, 1_001),
                ("b", 2, 1_201),
            ]
        );
        assert_eq!(seen[1].bytes_freed, 5_010);
    }

    /// A rule that does not apply is cleaned by nobody, but the user still
    /// asked for it: it closes with an event of its own, so the bar never
    /// stalls short of its end.
    #[test]
    fn an_unavailable_rule_still_counts_in_the_cleaning_progress_total() {
        let mut unavailable = rule("b");
        unavailable.unavailable_reason = Some("%TEMP% is outside the profile".into());
        let mut seen: Vec<CleanProgress> = Vec::new();
        clean_rules_with(
            vec![rule("a"), unavailable],
            CleanMode::Auto,
            |_, _, _| {
                Ok(CleanReport {
                    freed_bytes: 7,
                    deleted: 1,
                    skipped: Vec::new(),
                })
            },
            &mut |p| seen.push(p),
        );

        assert_eq!(seen.len(), 2);
        assert_eq!(seen[1].rule_id, "b");
        assert_eq!(seen[1].done_rules, 2);
        assert_eq!(seen[1].total_rules, 2);
        assert_eq!(seen[1].files_deleted, 1);
        assert_eq!(seen[1].bytes_freed, 7);
    }

    #[test]
    fn the_order_of_the_files_rules_is_preserved() {
        let rules = cleaning_order(vec![rule("a"), rule("b"), rule("c")]);
        let ids: Vec<&str> = rules.iter().map(|r| r.id.as_str()).collect();
        assert_eq!(ids, vec!["a", "b", "c"]);
    }

    fn fake_check(n: u32) -> Result<UpdateCheck, String> {
        Ok(UpdateCheck {
            current: n.to_string(),
            latest: None,
            is_newer: false,
            notes: None,
            url: None,
            published_at: None,
        })
    }

    /// Two calls inside the cooldown must not run `check` twice: the second
    /// gets the first result back, verbatim.
    #[test]
    fn a_second_check_within_the_cooldown_reuses_the_first_result() {
        let state: Mutex<Option<(Instant, UpdateCheck)>> = Mutex::new(None);
        let calls = std::cell::Cell::new(0u32);
        let t0 = Instant::now();

        let first = check_for_updates_with(&state, Duration::from_secs(10), t0, || {
            calls.set(calls.get() + 1);
            fake_check(1)
        });
        let second = check_for_updates_with(
            &state,
            Duration::from_secs(10),
            t0 + Duration::from_secs(1),
            || {
                calls.set(calls.get() + 1);
                fake_check(2)
            },
        );

        assert_eq!(first, second);
        assert_eq!(calls.get(), 1);
    }

    /// Once the cooldown has elapsed, the next call runs `check` again.
    #[test]
    fn a_check_after_the_cooldown_runs_again() {
        let state: Mutex<Option<(Instant, UpdateCheck)>> = Mutex::new(None);
        let calls = std::cell::Cell::new(0u32);
        let t0 = Instant::now();

        let first = check_for_updates_with(&state, Duration::from_secs(10), t0, || {
            calls.set(calls.get() + 1);
            fake_check(1)
        });
        let second = check_for_updates_with(
            &state,
            Duration::from_secs(10),
            t0 + Duration::from_secs(11),
            || {
                calls.set(calls.get() + 1);
                fake_check(2)
            },
        );

        assert_ne!(first, second);
        assert_eq!(calls.get(), 2);
    }

    /// A failure must not be cached: a second call within the cooldown after
    /// an `Err` (offline, rate-limited, malformed...) has to reach the
    /// network again, not get the same failure handed back for ten seconds.
    #[test]
    fn a_check_after_a_failure_retries_immediately_within_the_cooldown() {
        let state: Mutex<Option<(Instant, UpdateCheck)>> = Mutex::new(None);
        let calls = std::cell::Cell::new(0u32);
        let t0 = Instant::now();

        let first = check_for_updates_with(&state, Duration::from_secs(10), t0, || {
            calls.set(calls.get() + 1);
            Err("offline".to_string())
        });
        let second = check_for_updates_with(
            &state,
            Duration::from_secs(10),
            t0 + Duration::from_secs(1),
            || {
                calls.set(calls.get() + 1);
                fake_check(2)
            },
        );

        assert_eq!(first, Err("offline".to_string()));
        assert_eq!(second, fake_check(2));
        assert_eq!(calls.get(), 2);
    }


    // -----------------------------------------------------------------------
    // Sandbox mode.
    //
    // Every test here builds its own state and its own directory under
    // `%TEMP%`, and leaves the sandbox at the end. Nothing reaches the real
    // profile, the real Recycle Bin or the real registry: the four rule
    // variables are mapped inside the sandbox root, both recycle-bin calls are
    // the sandbox stand-ins, and the startup commands are refused outright.
    // -----------------------------------------------------------------------

    /// Scans every rule of the active sandbox, discarding the progress events.
    fn sandbox_scan_all(state: &SandboxState) -> Vec<ScanResult> {
        let ids: Vec<String> = rule_summaries_in(state)
            .unwrap()
            .into_iter()
            .map(|r| r.id)
            .collect();
        scan_rules_in(state, &ids, &mut |_| {}).unwrap()
    }

    #[test]
    fn entering_a_sandbox_builds_the_native_rules_and_the_detected_winapp2_ones() {
        let state = SandboxState::default();
        let summary = enter_sandbox(&state).unwrap();
        let rules = rule_summaries_in(&state).unwrap();

        let native = rules.iter().filter(|r| !r.id.starts_with("winapp2.")).count();
        assert_eq!(native, 10, "rules.toml declares ten native rules");
        assert!(
            summary.winapp2_rules >= 10,
            "the fixture must make at least ten Winapp2 entries detected, got {}",
            summary.winapp2_rules
        );
        assert_eq!(rules.len(), native + summary.winapp2_rules as usize);
        assert!(summary.sentinels > 0 && summary.junk > 0);
        assert!(
            std::path::Path::new(&summary.root).is_dir(),
            "the sandbox root must exist: {}",
            summary.root
        );
        assert_eq!(sandbox_status_of(&state).as_ref(), Some(&summary));

        leave_sandbox(&state).unwrap();
    }

    /// The containment claim of the whole item, asserted where it can actually
    /// be broken: a scan driven by the sandbox lookup may name nothing outside
    /// the sandbox root, junction targets included.
    #[test]
    fn a_sandbox_scan_never_lists_a_path_outside_the_sandbox_root() {
        let state = SandboxState::default();
        let summary = enter_sandbox(&state).unwrap();
        let root = summary.root.to_lowercase();

        let scans = sandbox_scan_all(&state);
        let mut seen = 0usize;
        for scan in &scans {
            for path in &scan.paths {
                seen += 1;
                assert!(
                    path.to_lowercase().starts_with(&root),
                    "rule \"{}\" named a path outside the sandbox root: {path}",
                    scan.rule_id
                );
            }
        }
        assert!(seen > 0, "the scan must have found something to compare");

        leave_sandbox(&state).unwrap();
    }

    /// The end-to-end promise: the real engine, driven through the real
    /// `scan_rules_with` and `clean_rules_with`, removes every junk file and
    /// damages no sentinel — and the verdict says so.
    #[test]
    fn cleaning_the_whole_sandbox_removes_the_junk_and_spares_every_sentinel() {
        let state = SandboxState::default();
        let summary = enter_sandbox(&state).unwrap();

        let ids: Vec<String> = rule_summaries_in(&state)
            .unwrap()
            .into_iter()
            .map(|r| r.id)
            .collect();
        let scans = scan_rules_in(&state, &ids, &mut |_| {}).unwrap();
        assert!(scans.iter().map(|s| s.file_count).sum::<u64>() > 0);

        let report = clean_rules_in(&state, &ids, CleanMode::Permanent, &mut |_| {}).unwrap();
        assert!(report.deleted > 0);

        let verdict = verify_sandbox(&state, &ids).unwrap();
        assert_eq!(
            verdict.sentinels_damaged,
            Vec::<String>::new(),
            "a sentinel was deleted or rewritten"
        );
        assert_eq!(verdict.sentinels_intact, verdict.sentinels_total);
        assert_eq!(verdict.sentinels_total, summary.sentinels);
        assert_eq!(
            verdict.junk_remaining,
            Vec::<String>::new(),
            "junk survived the clean"
        );
        assert_eq!(verdict.junk_removed, verdict.junk_total);
        // Every rule was cleaned, so the scope is the whole junk list.
        assert_eq!(verdict.junk_total, summary.junk);
        assert_eq!(verdict.rules_cleaned, ids.len() as u32);
        assert_eq!(verdict.outside_intact, verdict.outside_total);
        assert_eq!(
            verdict.outside_total, 2,
            "the sandbox plants exactly two junction baits"
        );
        assert!(verdict.junctions_refused);

        leave_sandbox(&state).unwrap();
    }

    /// The false-red case: cleaning a subset must be judged on that subset.
    /// The verdict used to compare the whole junk list with the disk, so any
    /// selection short of "everything" read as "junk survived".
    #[test]
    fn the_verdict_counts_only_the_junk_of_the_rules_that_were_cleaned() {
        let state = SandboxState::default();
        let summary = enter_sandbox(&state).unwrap();

        let ids = vec!["windows.temp".to_string()];
        scan_rules_in(&state, &ids, &mut |_| {}).unwrap();
        clean_rules_in(&state, &ids, CleanMode::Permanent, &mut |_| {}).unwrap();

        let verdict = verify_sandbox(&state, &ids).unwrap();
        assert_eq!(verdict.rules_cleaned, 1);
        assert!(
            verdict.junk_total > 0 && verdict.junk_total < summary.junk,
            "one rule owns some of the junk, not all of it: {} of {}",
            verdict.junk_total,
            summary.junk
        );
        assert_eq!(
            verdict.junk_remaining,
            Vec::<String>::new(),
            "the junk of the cleaned rule must be gone"
        );
        assert_eq!(verdict.junk_removed, verdict.junk_total);
        // The sentinels and the baits are never scoped: a selection cannot
        // license damaging a file.
        assert_eq!(verdict.sentinels_total, summary.sentinels);
        assert_eq!(verdict.sentinels_intact, verdict.sentinels_total);
        assert_eq!(verdict.outside_intact, verdict.outside_total);

        // Nothing cleaned: nothing to answer for.
        let none = verify_sandbox(&state, &[]).unwrap();
        assert_eq!(none.junk_total, 0);
        assert_eq!(none.rules_cleaned, 0);

        leave_sandbox(&state).unwrap();
    }

    /// A leave that cannot remove the tree must leave the sandbox ACTIVE: the
    /// back end going back to the real catalogue while the directory is still
    /// there — and the banner still on screen — is how a Clean meant for the
    /// sandbox reaches the user's own profile.
    #[test]
    fn a_leave_that_cannot_remove_the_tree_keeps_the_sandbox_active() {
        use std::os::windows::fs::OpenOptionsExt;

        let state = SandboxState::default();
        let summary = enter_sandbox(&state).unwrap();
        let root = std::path::PathBuf::from(&summary.root);

        // `share_mode(0)`: no other handle may open this file, so the deletion
        // is refused outright rather than deferred.
        let locked = root.join("profile").join("Documents").join("thesis.docx");
        let handle = std::fs::OpenOptions::new()
            .read(true)
            .share_mode(0)
            .open(&locked)
            .unwrap();

        let err = leave_sandbox(&state).unwrap_err();
        assert!(err.contains("still active"), "{err}");
        assert_eq!(
            sandbox_status_of(&state).as_ref(),
            Some(&summary),
            "a failed leave must not switch the engine back"
        );
        assert!(root.is_dir(), "the tree is still there");
        assert!(verify_sandbox(&state, &[]).is_ok());

        drop(handle);
        leave_sandbox(&state).unwrap();
        assert!(sandbox_status_of(&state).is_none());
        assert!(!root.exists());
    }

    /// Every write of the build is fallible now: a base that does not exist is
    /// reported, not a panic — and nothing is left behind.
    #[test]
    fn a_fixture_built_on_a_missing_base_reports_it_and_leaves_nothing() {
        let base = std::env::temp_dir().join(format!("wincleaner-absent-{}", sandbox_token()));
        assert!(!base.exists());

        // `.err()`, not `unwrap_err()`: `Fixture` is not `Debug`, and giving it
        // a derive only so a test can print it is the wrong way round.
        let err = crate::sandbox::Fixture::build_in(&base)
            .err()
            .expect("a base that does not exist must be reported");
        assert!(
            err.starts_with("Could not create the sandbox profile:"),
            "{err}"
        );
        assert!(err.contains(&base.display().to_string()), "{err}");
        assert!(!base.exists(), "the failed build must leave nothing behind");
    }

    /// `Trash` mode never reaches `trash::delete`, and therefore never the real
    /// Recycle Bin: the file is moved into `<root>\recycle-bin` instead.
    #[test]
    fn trash_mode_moves_sandbox_files_into_the_sandbox_bin() {
        let state = SandboxState::default();
        let summary = enter_sandbox(&state).unwrap();
        let bin = std::path::Path::new(&summary.root).join(crate::sandbox::SANDBOX_BIN);

        let ids = vec!["windows.temp".to_string()];
        scan_rules_in(&state, &ids, &mut |_| {}).unwrap();
        let report = clean_rules_in(&state, &ids, CleanMode::Trash, &mut |_| {}).unwrap();
        assert!(report.deleted > 0);

        let moved = std::fs::read_dir(&bin).unwrap().count();
        assert_eq!(
            moved as u64, report.deleted,
            "every file Trash mode removed must be in {}",
            bin.display()
        );

        leave_sandbox(&state).unwrap();
    }

    #[test]
    fn a_second_sandbox_is_refused_while_one_is_active() {
        let state = SandboxState::default();
        enter_sandbox(&state).unwrap();
        let err = enter_sandbox(&state).unwrap_err();
        assert!(err.contains("already active"), "{err}");
        leave_sandbox(&state).unwrap();
    }

    #[test]
    fn leaving_removes_the_sandbox_directory_and_gives_the_real_catalogue_back() {
        let state = SandboxState::default();
        let summary = enter_sandbox(&state).unwrap();
        let root = std::path::PathBuf::from(&summary.root);
        assert!(root.is_dir());

        leave_sandbox(&state).unwrap();

        assert!(!root.exists(), "the sandbox directory must be gone");
        assert!(sandbox_status_of(&state).is_none());
        assert!(verify_sandbox(&state, &[]).is_err());
        // Back on the machine's own catalogue, which holds far more rules than
        // the sandbox's nine natives plus a handful of detected entries.
        let rules = rule_summaries_in(&state).unwrap();
        assert_eq!(rules.len(), catalogue().unwrap().rules.len());
    }

    /// The startup screen writes to the real `HKCU\...\Run` keys, which no
    /// sandbox can stand in for: while one is active, both commands refuse.
    #[test]
    fn the_startup_commands_are_refused_while_a_sandbox_is_active() {
        let state = SandboxState::default();
        assert!(refuse_startup_in_sandbox(&state).is_ok());

        enter_sandbox(&state).unwrap();
        let err = refuse_startup_in_sandbox(&state).unwrap_err();
        assert!(err.to_lowercase().contains("sandbox"), "{err}");
        assert!(err.to_lowercase().contains("registry"), "{err}");

        leave_sandbox(&state).unwrap();
        assert!(refuse_startup_in_sandbox(&state).is_ok());
    }

    /// Goes through `blocking`, and therefore through `spawn_blocking`, exactly
    /// like the `clean` command. The command itself is not called here because
    /// it now takes a `tauri::State`, which only a running Tauri application
    /// can hand out.
    #[test]
    fn cleaning_an_unknown_id_is_an_error() {
        let state = SandboxState::default();
        let err = tauri::async_runtime::block_on(blocking(move || {
            clean_rules_in(&state, &["nonexistent".to_string()], CleanMode::Auto, &mut |_| {})
        }))
        .unwrap_err();
        assert!(err.contains("nonexistent"));
    }
}
