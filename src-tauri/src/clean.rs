use crate::rules::{canonical_profile_with, system_env, EnvLookup, Risk, Rule, RuleError, RuleKind};
use crate::scan::{
    build_set, confined_root, is_reparse_point, query_recycle_bin, scan_rule_with_api, to_slash,
    walk_roots, Containment, RecycleQuery,
};
use serde::{Deserialize, Serialize};
use std::path::{Path, PathBuf};
use walkdir::WalkDir;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum CleanMode {
    Trash,
    Permanent,
    Auto,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct SkippedItem {
    pub path: String,
    pub reason: String,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, Default)]
pub struct CleanReport {
    pub freed_bytes: u64,
    pub deleted: u64,
    pub skipped: Vec<SkippedItem>,
}

impl CleanReport {
    pub fn merge(&mut self, other: CleanReport) {
        self.freed_bytes += other.freed_bytes;
        self.deleted += other.deleted;
        self.skipped.extend(other.skipped);
    }
}

/// Emptying of the recycle bin, injected to stay testable.
pub type RecycleEmpty<'a> = &'a dyn Fn() -> Result<(), String>;

/// Moving files to the recycle bin, injected to stay testable.
///
/// `trash::delete_all` is the only deletion path that reaches outside the tree
/// it is given: it hands the files to the shell, which files them in the real
/// recycle bin of the volume. A test exercising `CleanMode::Trash` for real
/// would therefore litter the recycle bin of whoever runs the suite. This
/// injection point exists so the safety harness can run the whole `Trash`
/// path — re-scan, `deletable_path` guard, emptied-directory sweep — while
/// recording the paths instead of handing them to the shell.
///
/// The slice is what makes the mode usable at all: see `TRASH_BATCH`.
pub type TrashDelete<'a> = &'a dyn Fn(&[PathBuf]) -> Result<(), String>;

/// How many approved paths go to the recycle bin in one call.
///
/// On Windows one `trash::delete_all` is one `IFileOperation`, whose fixed cost
/// — COM plumbing, the shell's own progress reporting, one undo record — is
/// what dominates a bin full of small files. Handing them over one at a time
/// measured about 75 files/s on a real profile: 59,000 files took thirteen
/// minutes. The batch is what that number is divided by.
pub const TRASH_BATCH: usize = 500;

/// Called after each deletion step with the running counts of the rule being
/// cleaned: `(deleted, freed_bytes)`. A step is one file in `Permanent` mode,
/// one batch in `Trash` mode. Injected, like `TrashDelete`, so the sequence the
/// front end draws is asserted without a Tauri application — the caller decides
/// how often that becomes an event (`commands.rs::PROGRESS_EVERY`).
pub type CleanTick<'a> = &'a mut dyn FnMut(u64, u64);

/// The real one. Separate function so `clean_rule_with_api` can name it.
fn trash_delete(paths: &[PathBuf]) -> Result<(), String> {
    trash::delete_all(paths).map_err(|e| e.to_string())
}

/// One entry approved by `deletable_path` and waiting for its batch: the
/// canonical path handed to the shell, the size credited on success, and the
/// scanned path a failure is reported against (`skipped` never shows the
/// verbatim `\\?\` form).
type Approved = (PathBuf, u64, String);

/// Hands one batch to the recycle bin, then falls back to one call per file if
/// that fails.
///
/// The shell reports a failed `IFileOperation` as a single error for the whole
/// lot: without the retry, one locked file would cost the other 499 their place
/// in `deleted` and name none of them in `skipped`. The fallback pays a batch's
/// worth of calls only on the batch that failed.
fn send_to_trash(batch: &[Approved], trash: TrashDelete, report: &mut CleanReport) {
    let paths: Vec<PathBuf> = batch.iter().map(|(real, _, _)| real.clone()).collect();
    if trash(&paths).is_ok() {
        report.deleted += batch.len() as u64;
        report.freed_bytes += batch.iter().map(|(_, size, _)| size).sum::<u64>();
        return;
    }
    for (real, size, scanned) in batch {
        let reason = match trash(std::slice::from_ref(real)) {
            Ok(()) => None,
            // A file the failed batch did move is gone from the disk: the retry
            // cannot find it any more, and calling that "skipped" would be a
            // lie about what is still there.
            Err(reason) if real.exists() => Some(reason),
            Err(_) => None,
        };
        match reason {
            None => {
                report.deleted += 1;
                report.freed_bytes += size;
            }
            Some(reason) => report.skipped.push(SkippedItem {
                path: scanned.clone(),
                reason,
            }),
        }
    }
}

/// Resolves `Auto` from the rule risk. Never returns `Auto`.
pub fn effective_mode(mode: CleanMode, risk: Risk) -> CleanMode {
    match mode {
        CleanMode::Auto => match risk {
            Risk::Low => CleanMode::Permanent,
            Risk::Medium => CleanMode::Trash,
        },
        other => other,
    }
}

/// Last check before deleting: immediate as the re-scan is, a process running
/// under the same account can replace a name between the walk's `metadata()`
/// and the delete call. We therefore require, on the path itself and not on
/// what it points to, a regular file whose real location stays under the
/// profile.
///
/// Returns the canonical path to delete, verbatim `\\?\` prefix included, and
/// its size. Both `trash::delete` and `remove_file` accept that form; nothing
/// is displayed from it (`skipped` entries carry the scanned path), so nothing
/// justifies shortening it.
fn deletable_path(path: &str, profile_canon: &Path) -> Result<(PathBuf, u64), String> {
    let md = std::fs::symlink_metadata(path).map_err(|e| e.to_string())?;
    if is_reparse_point(&md) {
        return Err("the path has become a reparse point".to_string());
    }
    if !md.is_file() {
        return Err("the path is no longer a regular file".to_string());
    }
    let real = std::fs::canonicalize(path).map_err(|e| e.to_string())?;
    if !real.starts_with(profile_canon) {
        return Err("the path is outside the user profile at deletion time".to_string());
    }
    Ok((real, md.len()))
}

/// Removes the directories the rule has just emptied.
///
/// A directory is a candidate only if the rule patterns would match it: the
/// sweep has exactly the scope of the rule, never wider — and the walk root
/// itself is never removed (`min_depth(1)`). `remove_dir`, rather than
/// `remove_dir_all`, makes the operation safe by construction: a non-empty
/// directory fails the call, which is ignored. An empty directory holds no
/// data, so the pass runs in both deletion modes.
fn remove_emptied_directories(
    patterns: &[String],
    excludes: &[String],
    profile_canon: &Path,
) -> Result<(), RuleError> {
    let include = build_set(patterns)?;
    let exclude = build_set(excludes)?;
    for root in walk_roots(patterns) {
        let win_root = root.path.replace('/', "\\");
        if confined_root(&win_root, profile_canon) != Containment::Walkable {
            continue;
        }
        let mut walk = WalkDir::new(&win_root)
            .follow_links(false)
            .min_depth(1)
            .contents_first(true);
        if let Some(depth) = root.depth {
            walk = walk.max_depth(depth);
        }
        for entry in walk.into_iter().filter_map(|e| e.ok()) {
            if !entry.file_type().is_dir() {
                continue;
            }
            // A reparse point is not an empty directory: `remove_dir` would
            // erase the link, not its contents.
            if entry
                .metadata()
                .map(|m| is_reparse_point(&m))
                .unwrap_or(true)
            {
                continue;
            }
            let slash = to_slash(&entry.path().to_string_lossy());
            if !include.is_match(&slash) || exclude.is_match(&slash) {
                continue;
            }
            let _ = std::fs::remove_dir(entry.path());
        }
    }
    Ok(())
}

pub fn clean_rule_with_api(
    rule: &Rule,
    mode: CleanMode,
    lookup: EnvLookup,
    recycle_query: RecycleQuery,
    recycle_empty: RecycleEmpty,
    tick: CleanTick,
) -> Result<CleanReport, RuleError> {
    clean_rule_with_trash(
        rule,
        mode,
        lookup,
        recycle_query,
        recycle_empty,
        &trash_delete,
        tick,
    )
}

/// Same as `clean_rule_with_api`, with the move-to-recycle-bin call injected
/// too. Only the safety harness passes anything but `trash_delete`.
pub fn clean_rule_with_trash(
    rule: &Rule,
    mode: CleanMode,
    lookup: EnvLookup,
    recycle_query: RecycleQuery,
    recycle_empty: RecycleEmpty,
    trash: TrashDelete,
    tick: CleanTick,
) -> Result<CleanReport, RuleError> {
    // Internal re-scan just before deleting: the front end never sent a path,
    // and the state of the disk may have changed since the scan.
    let scan = scan_rule_with_api(rule, lookup, recycle_query)?;

    if rule.kind == RuleKind::RecycleBin {
        return Ok(match recycle_empty() {
            Ok(()) => CleanReport {
                freed_bytes: scan.total_bytes,
                deleted: scan.file_count,
                skipped: Vec::new(),
            },
            Err(reason) => CleanReport {
                freed_bytes: 0,
                deleted: 0,
                skipped: vec![SkippedItem {
                    path: rule.label.clone(),
                    reason,
                }],
            },
        });
    }

    let target = effective_mode(mode, rule.risk);
    let profile_canon = canonical_profile_with(lookup)?;
    let patterns = crate::rules::resolved_paths_with(rule, lookup)?;
    let excludes = crate::rules::resolved_excludes_with(rule, lookup)?;
    let mut report = CleanReport::default();
    let mut pending: Vec<Approved> = Vec::new();

    for path in &scan.paths {
        let (real, size) = match deletable_path(path, &profile_canon) {
            Ok(v) => v,
            Err(reason) => {
                report.skipped.push(SkippedItem {
                    path: path.clone(),
                    reason,
                });
                continue;
            }
        };
        match target {
            CleanMode::Permanent => {
                match std::fs::remove_file(&real) {
                    Ok(()) => {
                        report.deleted += 1;
                        report.freed_bytes += size;
                    }
                    Err(e) => report.skipped.push(SkippedItem {
                        path: path.clone(),
                        reason: e.to_string(),
                    }),
                }
                tick(report.deleted, report.freed_bytes);
            }
            CleanMode::Trash => {
                pending.push((real, size, path.clone()));
                if pending.len() == TRASH_BATCH {
                    send_to_trash(&pending, trash, &mut report);
                    pending.clear();
                    tick(report.deleted, report.freed_bytes);
                }
            }
            CleanMode::Auto => unreachable!("effective_mode never returns Auto"),
        }
    }
    if !pending.is_empty() {
        send_to_trash(&pending, trash, &mut report);
        tick(report.deleted, report.freed_bytes);
    }

    remove_emptied_directories(&patterns, &excludes, &profile_canon)?;
    Ok(report)
}

pub fn clean_rule(rule: &Rule, mode: CleanMode, tick: CleanTick) -> Result<CleanReport, RuleError> {
    clean_rule_with_api(
        rule,
        mode,
        &system_env,
        &query_recycle_bin,
        &empty_recycle_bin,
        tick,
    )
}

/// Empties the recycle bin of every volume, without confirmation, progress bar
/// or sound. Irreversible: only called by the `windows.recycle-bin` rule.
pub fn empty_recycle_bin() -> Result<(), String> {
    use windows::core::PCWSTR;
    use windows::Win32::UI::Shell::{
        SHEmptyRecycleBinW, SHERB_NOCONFIRMATION, SHERB_NOPROGRESSUI, SHERB_NOSOUND,
    };

    let flags = SHERB_NOCONFIRMATION | SHERB_NOPROGRESSUI | SHERB_NOSOUND;
    unsafe { SHEmptyRecycleBinW(None, PCWSTR::null(), flags) }
        .map_err(|e| format!("SHEmptyRecycleBinW failed: {e}"))
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::rules::{Risk, Rule, RuleKind};
    use std::fs;
    use std::fs::OpenOptions;
    use std::path::Path;
    use tempfile::TempDir;

    fn fake_profile() -> TempDir {
        let dir = TempDir::new().unwrap();
        let temp = dir.path().join("AppData").join("Local").join("Temp");
        fs::create_dir_all(temp.join("sub")).unwrap();
        fs::write(temp.join("a.txt"), b"aaa").unwrap();
        fs::write(temp.join("sub").join("b.txt"), b"bbbbb").unwrap();
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

    fn temp_rule(risk: Risk) -> Rule {
        Rule {
            id: "windows.temp".into(),
            category: "System".into(),
            label: "Temporary files".into(),
            paths: vec![r"%TEMP%\**\*".into()],
            exclude: vec![],
            risk,
            kind: RuleKind::Files,
            default_checked: true,
            note: None,
            unavailable_reason: None,
        }
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

    fn forbidden_recycle_query() -> Result<(u64, u64), String> {
        panic!("the recycle bin API must not be called for a \"files\" rule");
    }

    fn forbidden_recycle_empty() -> Result<(), String> {
        panic!("the recycle bin API must not be called for a \"files\" rule");
    }

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

    #[test]
    fn a_real_file_under_the_profile_is_deletable() {
        let dir = TempDir::new().unwrap();
        let profile = std::fs::canonicalize(dir.path()).unwrap();
        let f = dir.path().join("a.txt");
        fs::write(&f, b"aaa").unwrap();
        let (_, size) =
            deletable_path(&f.to_string_lossy(), &profile).expect("should be deletable");
        assert_eq!(size, 3);
    }

    #[test]
    fn a_path_that_became_a_directory_between_scan_and_delete_is_skipped() {
        let dir = TempDir::new().unwrap();
        let profile = std::fs::canonicalize(dir.path()).unwrap();
        let d = dir.path().join("a.txt");
        fs::create_dir(&d).unwrap();
        let err = deletable_path(&d.to_string_lossy(), &profile).unwrap_err();
        assert!(err.contains("regular file"), "reason = {err}");
    }

    #[test]
    fn a_path_that_became_a_junction_between_scan_and_delete_is_skipped() {
        let base = TempDir::new().unwrap();
        let profile_dir = base.path().join("profile");
        let outside = base.path().join("outside");
        fs::create_dir_all(&profile_dir).unwrap();
        fs::create_dir_all(&outside).unwrap();
        fs::write(outside.join("precious.txt"), b"precious").unwrap();
        let profile = std::fs::canonicalize(&profile_dir).unwrap();
        // The name that had been scanned as a file has become a junction.
        let trap = profile_dir.join("a.txt");
        junction(&trap, &outside);

        let err = deletable_path(&trap.to_string_lossy(), &profile).unwrap_err();
        assert!(err.contains("reparse point"), "reason = {err}");
        assert!(outside.join("precious.txt").exists());
    }

    #[test]
    fn a_file_outside_the_profile_is_not_deletable() {
        let base = TempDir::new().unwrap();
        let profile_dir = base.path().join("profile");
        let outside = base.path().join("outside");
        fs::create_dir_all(&profile_dir).unwrap();
        fs::create_dir_all(&outside).unwrap();
        let f = outside.join("precious.txt");
        fs::write(&f, b"precious").unwrap();
        let profile = std::fs::canonicalize(&profile_dir).unwrap();

        let err = deletable_path(&f.to_string_lossy(), &profile).unwrap_err();
        assert!(err.contains("outside the user profile"), "reason = {err}");
        assert!(f.exists());
    }

    #[test]
    fn auto_becomes_permanent_for_a_low_risk() {
        assert_eq!(
            effective_mode(CleanMode::Auto, Risk::Low),
            CleanMode::Permanent
        );
    }

    #[test]
    fn auto_becomes_trash_for_a_medium_risk() {
        assert_eq!(
            effective_mode(CleanMode::Auto, Risk::Medium),
            CleanMode::Trash
        );
    }

    #[test]
    fn an_explicit_mode_is_not_reinterpreted() {
        assert_eq!(effective_mode(CleanMode::Trash, Risk::Low), CleanMode::Trash);
        assert_eq!(
            effective_mode(CleanMode::Permanent, Risk::Medium),
            CleanMode::Permanent
        );
    }

    #[test]
    fn permanent_deletion_erases_the_files() {
        let dir = fake_profile();
        let lookup = lookup_for(dir.path());
        let rule = temp_rule(Risk::Low);
        let report = clean_rule_with_api(
            &rule,
            CleanMode::Permanent,
            &lookup,
            &forbidden_recycle_query,
            &forbidden_recycle_empty,
            &mut |_, _| {},
        )
        .unwrap();
        assert_eq!(report.deleted, 2);
        assert_eq!(report.freed_bytes, 8);
        assert!(report.skipped.is_empty());
        let temp = dir.path().join("AppData").join("Local").join("Temp");
        assert!(!temp.join("a.txt").exists());
        assert!(!temp.join("sub").join("b.txt").exists());
    }

    #[test]
    fn trash_deletion_erases_the_files_from_the_directory() {
        let dir = fake_profile();
        let lookup = lookup_for(dir.path());
        let rule = temp_rule(Risk::Medium);
        let report = clean_rule_with_api(
            &rule,
            CleanMode::Trash,
            &lookup,
            &forbidden_recycle_query,
            &forbidden_recycle_empty,
            &mut |_, _| {},
        )
        .unwrap();
        assert_eq!(report.deleted, 2);
        assert_eq!(report.freed_bytes, 8);
        let temp = dir.path().join("AppData").join("Local").join("Temp");
        assert!(!temp.join("a.txt").exists());
    }

    /// Builds `count` files under `%TEMP%`, and answers their total size.
    fn temp_files(dir: &Path, count: usize) -> u64 {
        let temp = dir.join("AppData").join("Local").join("Temp");
        fs::create_dir_all(&temp).unwrap();
        for n in 0..count {
            fs::write(temp.join(format!("f{n:05}.txt")), b"junk").unwrap();
        }
        (count * 4) as u64
    }

    /// The item itself: 59,000 files handed over one at a time measured about
    /// 75 files/s. What the rule owes the shell is one call per `TRASH_BATCH`,
    /// and the counts must stay exactly what the per-file loop reported.
    #[test]
    fn trash_mode_hands_the_shell_one_call_per_batch() {
        let dir = TempDir::new().unwrap();
        let bytes = temp_files(dir.path(), 5_000);
        let lookup = lookup_for(dir.path());
        let sizes = std::cell::RefCell::new(Vec::<usize>::new());
        let trash = |paths: &[PathBuf]| {
            sizes.borrow_mut().push(paths.len());
            paths.iter().try_for_each(|p| fs::remove_file(p).map_err(|e| e.to_string()))
        };

        let report = clean_rule_with_trash(
            &temp_rule(Risk::Medium),
            CleanMode::Trash,
            &lookup,
            &forbidden_recycle_query,
            &forbidden_recycle_empty,
            &trash,
            &mut |_, _| {},
        )
        .unwrap();

        assert_eq!(report.deleted, 5_000);
        assert_eq!(report.freed_bytes, bytes);
        assert!(report.skipped.is_empty());
        assert_eq!(
            *sizes.borrow(),
            vec![TRASH_BATCH; 5_000 / TRASH_BATCH],
            "one call per batch of {TRASH_BATCH}, and nothing left over"
        );
    }

    /// A batch that does not divide evenly ends on a short call, not on a
    /// silently dropped remainder.
    #[test]
    fn the_last_batch_carries_whatever_is_left() {
        let dir = TempDir::new().unwrap();
        temp_files(dir.path(), TRASH_BATCH + 3);
        let lookup = lookup_for(dir.path());
        let sizes = std::cell::RefCell::new(Vec::<usize>::new());
        let trash = |paths: &[PathBuf]| {
            sizes.borrow_mut().push(paths.len());
            paths.iter().try_for_each(|p| fs::remove_file(p).map_err(|e| e.to_string()))
        };

        let report = clean_rule_with_trash(
            &temp_rule(Risk::Medium),
            CleanMode::Trash,
            &lookup,
            &forbidden_recycle_query,
            &forbidden_recycle_empty,
            &trash,
            &mut |_, _| {},
        )
        .unwrap();

        assert_eq!(report.deleted, (TRASH_BATCH + 3) as u64);
        assert_eq!(*sizes.borrow(), vec![TRASH_BATCH, 3]);
    }

    /// The shell reports a failed batch as one error for the whole lot. Without
    /// the per-file retry, one unusable file would cost the other 499 their
    /// place in `deleted` and name none of them in `skipped`.
    #[test]
    fn a_failed_batch_falls_back_to_one_call_per_file() {
        let dir = TempDir::new().unwrap();
        temp_files(dir.path(), 4);
        let lookup = lookup_for(dir.path());
        let calls = std::cell::RefCell::new(Vec::<usize>::new());
        let trash = |paths: &[PathBuf]| {
            calls.borrow_mut().push(paths.len());
            if paths.len() > 1 {
                return Err("the operation could not be completed".to_string());
            }
            if paths[0].to_string_lossy().contains("f00002") {
                return Err("the file is in use".to_string());
            }
            fs::remove_file(&paths[0]).map_err(|e| e.to_string())
        };

        let report = clean_rule_with_trash(
            &temp_rule(Risk::Medium),
            CleanMode::Trash,
            &lookup,
            &forbidden_recycle_query,
            &forbidden_recycle_empty,
            &trash,
            &mut |_, _| {},
        )
        .unwrap();

        // One failed batch of four, then the four retries.
        assert_eq!(*calls.borrow(), vec![4, 1, 1, 1, 1]);
        assert_eq!(report.deleted, 3);
        assert_eq!(report.freed_bytes, 12);
        assert_eq!(report.skipped.len(), 1);
        assert!(report.skipped[0].path.ends_with("f00002.txt"));
        assert_eq!(report.skipped[0].reason, "the file is in use");
    }

    /// A batch that failed halfway has still moved some of its files. The retry
    /// cannot find those any more, and calling them "skipped" would claim they
    /// are still on disk.
    #[test]
    fn a_file_the_failed_batch_did_move_is_still_counted_as_deleted() {
        let dir = TempDir::new().unwrap();
        temp_files(dir.path(), 2);
        let lookup = lookup_for(dir.path());
        let trash = |paths: &[PathBuf]| {
            // The shell moved the first file, then gave up on the batch; the
            // per-file retries that follow move nothing at all.
            if paths.len() > 1 {
                let _ = fs::remove_file(&paths[0]);
            }
            Err("the operation could not be completed".to_string())
        };

        let report = clean_rule_with_trash(
            &temp_rule(Risk::Medium),
            CleanMode::Trash,
            &lookup,
            &forbidden_recycle_query,
            &forbidden_recycle_empty,
            &trash,
            &mut |_, _| {},
        )
        .unwrap();

        assert_eq!(report.deleted, 1);
        assert_eq!(report.freed_bytes, 4);
        assert_eq!(report.skipped.len(), 1);
        assert!(report.skipped[0].path.ends_with("f00001.txt"));
    }

    /// `Permanent` mode stays one `remove_file` per file — and therefore one
    /// tick per file, which is what the harness uses to reach inside the loop.
    #[test]
    fn permanent_mode_ticks_once_per_file_and_trash_mode_once_per_batch() {
        let dir = TempDir::new().unwrap();
        temp_files(dir.path(), TRASH_BATCH + 1);
        let lookup = lookup_for(dir.path());
        let mut ticks = Vec::<(u64, u64)>::new();
        clean_rule_with_api(
            &temp_rule(Risk::Low),
            CleanMode::Permanent,
            &lookup,
            &forbidden_recycle_query,
            &forbidden_recycle_empty,
            &mut |d, b| ticks.push((d, b)),
        )
        .unwrap();
        assert_eq!(ticks.len(), TRASH_BATCH + 1);
        assert_eq!(*ticks.last().unwrap(), ((TRASH_BATCH + 1) as u64, (TRASH_BATCH + 1) as u64 * 4));

        let dir = TempDir::new().unwrap();
        temp_files(dir.path(), TRASH_BATCH + 1);
        let lookup = lookup_for(dir.path());
        let trash = |paths: &[PathBuf]| {
            paths.iter().try_for_each(|p| fs::remove_file(p).map_err(|e| e.to_string()))
        };
        let mut ticks = Vec::<(u64, u64)>::new();
        clean_rule_with_trash(
            &temp_rule(Risk::Medium),
            CleanMode::Trash,
            &lookup,
            &forbidden_recycle_query,
            &forbidden_recycle_empty,
            &trash,
            &mut |d, b| ticks.push((d, b)),
        )
        .unwrap();
        assert_eq!(
            ticks,
            vec![
                (TRASH_BATCH as u64, TRASH_BATCH as u64 * 4),
                ((TRASH_BATCH + 1) as u64, (TRASH_BATCH + 1) as u64 * 4),
            ]
        );
    }

    #[test]
    fn a_locked_file_ends_up_in_skipped() {
        let dir = fake_profile();
        let lookup = lookup_for(dir.path());
        let temp = dir.path().join("AppData").join("Local").join("Temp");
        let locked = temp.join("a.txt");
        // Opened for writing with sharing denied: Windows returns
        // ERROR_SHARING_VIOLATION to any delete attempt.
        let _handle = {
            #[cfg(windows)]
            {
                use std::os::windows::fs::OpenOptionsExt;
                OpenOptions::new()
                    .write(true)
                    .share_mode(0)
                    .open(&locked)
                    .unwrap()
            }
            #[cfg(not(windows))]
            {
                OpenOptions::new().write(true).open(&locked).unwrap()
            }
        };

        let rule = temp_rule(Risk::Low);
        let report = clean_rule_with_api(
            &rule,
            CleanMode::Permanent,
            &lookup,
            &forbidden_recycle_query,
            &forbidden_recycle_empty,
            &mut |_, _| {},
        )
        .unwrap();

        assert_eq!(report.deleted, 1);
        assert_eq!(report.freed_bytes, 5);
        assert_eq!(report.skipped.len(), 1);
        assert!(report.skipped[0].path.ends_with("a.txt"));
        assert!(!report.skipped[0].reason.is_empty());
        assert!(locked.exists());
    }

    #[test]
    fn cleaning_removes_the_emptied_directories_but_not_the_root() {
        // Without this pass, %TEMP% keeps tens of thousands of empty
        // directories: every scan has to walk them again to find nothing, and
        // the user sees a folder that still looks full.
        let dir = fake_profile();
        let lookup = lookup_for(dir.path());
        let temp = dir.path().join("AppData").join("Local").join("Temp");
        fs::create_dir_all(temp.join("keep").join("deep")).unwrap();
        fs::write(temp.join("keep").join("c.keep"), b"c").unwrap();

        let rule = Rule {
            exclude: vec![r"%TEMP%\**\*.keep".into()],
            ..temp_rule(Risk::Low)
        };
        clean_rule_with_api(
            &rule,
            CleanMode::Permanent,
            &lookup,
            &forbidden_recycle_query,
            &forbidden_recycle_empty,
            &mut |_, _| {},
        )
        .unwrap();

        assert!(temp.exists(), "the rule root is never removed");
        assert!(!temp.join("sub").exists(), "sub, emptied, must disappear");
        assert!(
            !temp.join("keep").join("deep").exists(),
            "deep, empty, must disappear"
        );
        assert!(
            temp.join("keep").exists(),
            "keep still holds c.keep: remove_dir must fail and be ignored"
        );
    }

    /// The empty-directory sweep has exactly the scope of the rule: without
    /// the `include.is_match` guard it would erase directories the rule was
    /// never allowed to touch.
    #[test]
    fn an_empty_directory_outside_the_rule_patterns_survives() {
        let dir = fake_profile();
        let lookup = lookup_for(dir.path());
        let temp = dir.path().join("AppData").join("Local").join("Temp");
        // Empty, under the walked root, but does not match `*.txt`.
        fs::create_dir_all(temp.join("stranger")).unwrap();

        let rule = Rule {
            paths: vec![r"%TEMP%\**\*.txt".into()],
            ..temp_rule(Risk::Low)
        };
        let report = clean_rule_with_api(
            &rule,
            CleanMode::Permanent,
            &lookup,
            &forbidden_recycle_query,
            &forbidden_recycle_empty,
            &mut |_, _| {},
        )
        .unwrap();

        assert_eq!(report.deleted, 2);
        assert!(
            temp.join("stranger").exists(),
            "an empty directory outside the rule patterns must not be removed"
        );
        assert!(
            temp.join("sub").exists(),
            "neither must sub: emptied by the rule, but outside its patterns"
        );
    }

    #[test]
    fn cleaning_an_empty_directory_does_nothing() {
        let dir = TempDir::new().unwrap();
        let lookup = lookup_for(dir.path());
        let rule = temp_rule(Risk::Low);
        let report = clean_rule_with_api(
            &rule,
            CleanMode::Permanent,
            &lookup,
            &forbidden_recycle_query,
            &forbidden_recycle_empty,
            &mut |_, _| {},
        )
        .unwrap();
        assert_eq!(report.deleted, 0);
        assert_eq!(report.freed_bytes, 0);
        assert!(report.skipped.is_empty());
    }

    #[test]
    fn merge_adds_two_reports_together() {
        let mut a = CleanReport {
            freed_bytes: 10,
            deleted: 2,
            skipped: vec![SkippedItem {
                path: "x".into(),
                reason: "y".into(),
            }],
        };
        a.merge(CleanReport {
            freed_bytes: 5,
            deleted: 1,
            skipped: vec![SkippedItem {
                path: "z".into(),
                reason: "w".into(),
            }],
        });
        assert_eq!(a.freed_bytes, 15);
        assert_eq!(a.deleted, 3);
        assert_eq!(a.skipped.len(), 2);
    }

    #[test]
    fn the_recycle_bin_rule_calls_the_recycle_bin_api() {
        let dir = TempDir::new().unwrap();
        let lookup = lookup_for(dir.path());
        let query = || Ok((4u64, 4096u64));
        let empty = || Ok(());
        let report = clean_rule_with_api(
            &recycle_bin_rule(),
            CleanMode::Auto,
            &lookup,
            &query,
            &empty,
            &mut |_, _| {},
        )
        .unwrap();
        assert_eq!(report.deleted, 4);
        assert_eq!(report.freed_bytes, 4096);
        assert!(report.skipped.is_empty());
    }

    #[test]
    fn a_failed_recycle_bin_api_call_ends_up_in_skipped() {
        let dir = TempDir::new().unwrap();
        let lookup = lookup_for(dir.path());
        let query = || Ok((4u64, 4096u64));
        let empty = || Err("access denied".to_string());
        let report = clean_rule_with_api(
            &recycle_bin_rule(),
            CleanMode::Auto,
            &lookup,
            &query,
            &empty,
            &mut |_, _| {},
        )
        .unwrap();
        assert_eq!(report.deleted, 0);
        assert_eq!(report.freed_bytes, 0);
        assert_eq!(report.skipped.len(), 1);
        assert_eq!(report.skipped[0].reason, "access denied");
    }
}
