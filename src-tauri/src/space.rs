//! The Space screen: where the profile's own data sits, and nothing else.
//!
//! Read-only by construction. Nothing in this module deletes, moves or writes:
//! it walks the user's known folders, ranks what it finds, and hands Explorer a
//! path to show. The gesture that removes a file stays with the user, in
//! Explorer — this is user data, not junk a rule vouched for.
//!
//! The walk goes through the same two guards as `scan.rs`
//! (`confined_root`, `is_reparse_point`): a known folder redirected outside the
//! profile, or planted as a junction, is refused rather than followed.

use crate::rules::{canonical_profile_with, system_env, RuleError};
use crate::scan::{confined_root, is_reparse_point, Containment};
use serde::{Deserialize, Serialize};
use std::cmp::Reverse;
use std::collections::{BTreeMap, BinaryHeap};
use std::path::Path;
use std::sync::Mutex;
use std::time::UNIX_EPOCH;
use walkdir::WalkDir;

/// Largest files handed to the front end. A hundred rows is already more than
/// anyone scrolls; the point is to show where the gigabytes are.
pub const TOP_FILES: usize = 100;
/// Largest folders handed to the front end.
pub const TOP_FOLDERS: usize = 20;
/// How far below a known folder a directory may sit and still be ranked.
/// Deeper than this the answer stops being actionable: nobody tidies
/// `Pictures\2019\raw\export\v2`, they tidy `Pictures\2019`.
pub const FOLDER_DEPTH: usize = 3;

/// One measured known folder. `name` is the stable id below, not a label: the
/// front end translates it.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct SpaceRoot {
    pub name: String,
    pub path: String,
    pub bytes: u64,
    pub files: u64,
}

/// One of the largest files. `index` is its position in this result, and the
/// only thing `space_reveal` accepts: no path ever travels back from the front
/// end (`CLAUDE.md`).
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct SpaceFile {
    pub index: usize,
    pub path: String,
    pub bytes: u64,
    /// Last modification, in milliseconds since the Unix epoch, or null when
    /// the filesystem does not report one. Formatted for the locale on the
    /// front end — a date is one of the few things a dictionary cannot hold.
    pub modified: Option<u64>,
}

/// One of the largest folders, with what it holds below it.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct SpaceFolder {
    pub index: usize,
    pub path: String,
    pub bytes: u64,
    pub files: u64,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct SpaceResult {
    pub roots: Vec<SpaceRoot>,
    pub files: Vec<SpaceFile>,
    pub folders: Vec<SpaceFolder>,
    /// Entries the walk could not read, reparse points included. Never an
    /// error: an unreadable file is not a reason to refuse the other ninety
    /// thousand.
    pub skipped_files: u64,
    /// Known folders refused because they resolve outside the profile, or are
    /// a reparse point. Named so the user knows the total is short.
    pub skipped_roots: Vec<String>,
}

/// One step of a measurement, emitted as each root finishes. Same contract as
/// `ScanProgress`: `total_bytes` is the running total since the start, not the
/// size of the root that has just finished.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct SpaceProgress {
    pub done: u32,
    pub total: u32,
    pub root: String,
    pub total_bytes: u64,
}

/// What one root's walk produced, before the roots are merged and ranked.
pub struct RootScan {
    pub name: String,
    pub path: String,
    pub bytes: u64,
    pub files: u64,
    pub skipped: u64,
    /// Resolved outside the profile, or a reparse point: not walked.
    pub refused: bool,
    /// Absent from disk. Not an anomaly, and not reported: a machine with no
    /// `Music` folder simply has none.
    pub missing: bool,
    /// `(bytes, path, modified)`, largest first.
    top_files: Vec<(u64, String, Option<u64>)>,
    /// `(bytes, path, files)`, largest first.
    top_folders: Vec<(u64, String, u64)>,
}

fn modified_millis(md: &std::fs::Metadata) -> Option<u64> {
    let stamp = md.modified().ok()?;
    Some(stamp.duration_since(UNIX_EPOCH).ok()?.as_millis() as u64)
}

/// Ranks `(bytes, path, extra)` triples: biggest first, then by path so two
/// files of the same size always come back in the same order.
fn rank<T>(mut items: Vec<(u64, String, T)>, keep: usize) -> Vec<(u64, String, T)> {
    items.sort_by(|a, b| b.0.cmp(&a.0).then_with(|| a.1.cmp(&b.1)));
    items.truncate(keep);
    items
}

/// Walks one known folder. Never descends through a reparse point, never
/// leaves the profile, never fails: a root it cannot walk comes back flagged,
/// so one unreadable folder does not cost the user the other five.
pub fn walk_root(name: &str, path: &str, profile_canon: &Path) -> RootScan {
    let mut scan = RootScan {
        name: name.to_string(),
        path: path.to_string(),
        bytes: 0,
        files: 0,
        skipped: 0,
        refused: false,
        missing: false,
        top_files: Vec::new(),
        top_folders: Vec::new(),
    };
    match confined_root(path, profile_canon) {
        Containment::Walkable => {}
        Containment::Missing => {
            scan.missing = true;
            return scan;
        }
        Containment::Refused => {
            scan.refused = true;
            return scan;
        }
    }

    // Only the largest `TOP_FILES` are ever needed: a loaded profile holds
    // hundreds of thousands of files, and keeping every path to sort at the
    // end would be tens of megabytes for a hundred rows. The heap pops the
    // *worst* candidate — `Reverse(bytes)` first, then the largest path, which
    // is the exact inverse of the final "biggest first, then path" order.
    let mut largest: BinaryHeap<(Reverse<u64>, String, Option<u64>)> = BinaryHeap::new();
    // `(bytes, files)` per directory, for the directories at most
    // `FOLDER_DEPTH` below the root.
    let mut folders: BTreeMap<String, (u64, u64)> = BTreeMap::new();

    let reparse_skipped = std::cell::Cell::new(0u64);
    let walk = WalkDir::new(path)
        .follow_links(false)
        .into_iter()
        .filter_entry(|e| {
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
                if err.io_error().map(|e| e.kind()) == Some(std::io::ErrorKind::NotFound) {
                    continue;
                }
                scan.skipped += 1;
                continue;
            }
        };
        if !entry.file_type().is_file() {
            continue;
        }
        let Ok(md) = entry.metadata() else {
            scan.skipped += 1;
            continue;
        };
        let bytes = md.len();
        scan.bytes += bytes;
        scan.files += 1;

        largest.push((
            Reverse(bytes),
            entry.path().to_string_lossy().to_string(),
            modified_millis(&md),
        ));
        if largest.len() > TOP_FILES {
            largest.pop();
        }

        // Every ancestor directory between the root (excluded, depth 0) and
        // `FOLDER_DEPTH` carries this file's size.
        let mut depth = entry.depth().saturating_sub(1);
        let mut dir = entry.path().parent();
        while depth > 0 {
            let Some(current) = dir else { break };
            if depth <= FOLDER_DEPTH {
                let bucket = folders
                    .entry(current.to_string_lossy().to_string())
                    .or_insert((0, 0));
                bucket.0 += bytes;
                bucket.1 += 1;
            }
            depth -= 1;
            dir = current.parent();
        }
    }

    scan.skipped += reparse_skipped.get();
    scan.top_files = rank(
        largest
            .into_iter()
            .map(|(Reverse(bytes), path, modified)| (bytes, path, modified))
            .collect(),
        TOP_FILES,
    );
    scan.top_folders = rank(
        folders
            .into_iter()
            .map(|(path, (bytes, files))| (bytes, path, files))
            .collect(),
        TOP_FOLDERS,
    );
    scan
}

/// Walks every root, one thread each, and merges the rankings.
///
/// The global top N is always a subset of the union of the per-root top N, so
/// truncating inside `walk_root` costs nothing in accuracy. `progress` fires
/// once per root, as it finishes: six roots is six events, and the slowest
/// (`Documents` on a OneDrive profile) is the whole wait.
pub fn scan_space_with(
    roots: &[(String, String)],
    profile_canon: &Path,
    progress: &mut (dyn FnMut(SpaceProgress) + Send),
) -> SpaceResult {
    let total = roots.len() as u32;
    let scans: Mutex<Vec<Option<RootScan>>> = Mutex::new((0..roots.len()).map(|_| None).collect());
    // One lock around the running total, the done count and the callback: the
    // front end trusts `total_bytes` verbatim, so they must advance together.
    let progress_state = Mutex::new((0u32, 0u64, progress));

    std::thread::scope(|scope| {
        for (index, (name, path)) in roots.iter().enumerate() {
            let scans = &scans;
            let progress_state = &progress_state;
            scope.spawn(move || {
                let scan = walk_root(name, path, profile_canon);
                {
                    let mut state = progress_state.lock().unwrap();
                    state.0 += 1;
                    state.1 += scan.bytes;
                    let (done, total_bytes) = (state.0, state.1);
                    (state.2)(SpaceProgress {
                        done,
                        total,
                        root: name.clone(),
                        total_bytes,
                    });
                }
                scans.lock().unwrap()[index] = Some(scan);
            });
        }
    });

    let mut result = SpaceResult {
        roots: Vec::new(),
        files: Vec::new(),
        folders: Vec::new(),
        skipped_files: 0,
        skipped_roots: Vec::new(),
    };
    let mut files: Vec<(u64, String, Option<u64>)> = Vec::new();
    let mut folders: Vec<(u64, String, u64)> = Vec::new();
    for scan in scans.into_inner().unwrap() {
        let scan = scan.expect("every root was walked before the scope joined");
        if scan.refused {
            result.skipped_roots.push(scan.name);
            continue;
        }
        if scan.missing {
            continue;
        }
        result.skipped_files += scan.skipped;
        files.extend(scan.top_files);
        folders.extend(scan.top_folders);
        result.roots.push(SpaceRoot {
            name: scan.name,
            path: scan.path,
            bytes: scan.bytes,
            files: scan.files,
        });
    }

    result.files = rank(files, TOP_FILES)
        .into_iter()
        .enumerate()
        .map(|(index, (bytes, path, modified))| SpaceFile {
            index,
            path,
            bytes,
            modified,
        })
        .collect();
    result.folders = rank(folders, TOP_FOLDERS)
        .into_iter()
        .enumerate()
        .map(|(index, (bytes, path, files))| SpaceFolder {
            index,
            path,
            bytes,
            files,
        })
        .collect();
    result
}

/// The known folders the Space screen measures, in the order it shows them.
/// `SHGetKnownFolderPath` rather than `%USERPROFILE%\Downloads`: a folder
/// redirected into OneDrive still answers here, and a hard-coded join would
/// measure an empty shell.
const KNOWN_ROOTS: [(&str, windows::core::GUID); 6] = [
    ("downloads", windows::Win32::UI::Shell::FOLDERID_Downloads),
    ("desktop", windows::Win32::UI::Shell::FOLDERID_Desktop),
    ("documents", windows::Win32::UI::Shell::FOLDERID_Documents),
    ("pictures", windows::Win32::UI::Shell::FOLDERID_Pictures),
    ("videos", windows::Win32::UI::Shell::FOLDERID_Videos),
    ("music", windows::Win32::UI::Shell::FOLDERID_Music),
];

/// Resolves one known folder. A folder the shell cannot resolve is `None`:
/// that machine simply does not have it.
fn known_folder(id: &windows::core::GUID) -> Option<String> {
    use windows::Win32::Foundation::HANDLE;
    use windows::Win32::System::Com::CoTaskMemFree;
    use windows::Win32::UI::Shell::{SHGetKnownFolderPath, KF_FLAG_DEFAULT};

    unsafe {
        let wide = SHGetKnownFolderPath(id, KF_FLAG_DEFAULT, HANDLE::default()).ok()?;
        let path = wide.to_string().ok();
        // The shell allocated it; we free it whatever happened to the
        // conversion.
        CoTaskMemFree(Some(wide.0 as *const std::ffi::c_void));
        path
    }
}

/// `(name, path)` for every known folder that resolves on this machine.
pub fn known_folder_roots() -> Vec<(String, String)> {
    KNOWN_ROOTS
        .iter()
        .filter_map(|(name, id)| known_folder(id).map(|path| (name.to_string(), path)))
        .collect()
}

/// The measurement as the application runs it: the real known folders, against
/// the real profile.
pub fn scan_space(
    progress: &mut (dyn FnMut(SpaceProgress) + Send),
) -> Result<SpaceResult, RuleError> {
    let profile_canon = canonical_profile_with(&system_env)?;
    Ok(scan_space_with(
        &known_folder_roots(),
        &profile_canon,
        progress,
    ))
}

/// Opens Explorer on `path`: selecting it inside its folder for a file,
/// showing its contents for a folder.
///
/// `CREATE_NO_WINDOW`: without it a console flashes on screen for as long as
/// the process takes to hand over to the running Explorer.
pub fn launch_explorer(path: &Path, select: bool) -> Result<(), String> {
    use std::os::windows::process::CommandExt;
    const CREATE_NO_WINDOW: u32 = 0x0800_0000;

    let mut command = std::process::Command::new("explorer.exe");
    if select {
        // One argument, comma and all: that is the spelling Explorer parses.
        command.arg(format!("/select,{}", path.display()));
    } else {
        command.arg(path);
    }
    command
        .creation_flags(CREATE_NO_WINDOW)
        // Not waited on: `explorer.exe` hands the request to the running
        // instance and exits with a non-zero code that means nothing.
        .spawn()
        .map(|_| ())
        .map_err(|e| format!("could not start Explorer: {e}"))
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::fs;
    use tempfile::TempDir;

    /// `<tmp>/Downloads`, plus whatever the test writes into it.
    fn profile_with_downloads() -> (TempDir, std::path::PathBuf) {
        let dir = TempDir::new().unwrap();
        let downloads = dir.path().join("Downloads");
        fs::create_dir_all(&downloads).unwrap();
        (dir, downloads)
    }

    fn measure(profile: &Path, roots: &[(&str, std::path::PathBuf)]) -> SpaceResult {
        let canon = fs::canonicalize(profile).unwrap();
        let roots: Vec<(String, String)> = roots
            .iter()
            .map(|(name, path)| (name.to_string(), path.to_string_lossy().to_string()))
            .collect();
        scan_space_with(&roots, &canon, &mut |_| {})
    }

    /// Same junction helper as `scan.rs`: `mklink /J` needs no privilege, and
    /// both ends stay inside the test's own `TempDir`.
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
    fn the_largest_files_come_back_biggest_first() {
        let (dir, downloads) = profile_with_downloads();
        fs::write(downloads.join("small.bin"), vec![0u8; 10]).unwrap();
        fs::write(downloads.join("huge.bin"), vec![0u8; 900]).unwrap();
        fs::write(downloads.join("medium.bin"), vec![0u8; 100]).unwrap();

        let res = measure(dir.path(), &[("downloads", downloads)]);
        let sizes: Vec<u64> = res.files.iter().map(|f| f.bytes).collect();
        assert_eq!(sizes, vec![900, 100, 10]);
        assert!(res.files[0].path.ends_with("huge.bin"));
        // The index is the row's own position, and the only handle the front
        // end gets on the path.
        assert_eq!(
            res.files.iter().map(|f| f.index).collect::<Vec<_>>(),
            vec![0, 1, 2]
        );
        assert_eq!(res.roots.len(), 1);
        assert_eq!(res.roots[0].files, 3);
        assert_eq!(res.roots[0].bytes, 1010);
        assert!(res.skipped_roots.is_empty());
    }

    #[test]
    fn files_of_the_same_size_are_ordered_by_path() {
        let (dir, downloads) = profile_with_downloads();
        for name in ["b.bin", "a.bin", "c.bin"] {
            fs::write(downloads.join(name), vec![0u8; 50]).unwrap();
        }
        let res = measure(dir.path(), &[("downloads", downloads)]);
        let names: Vec<String> = res
            .files
            .iter()
            .map(|f| f.path.rsplit('\\').next().unwrap().to_string())
            .collect();
        assert_eq!(names, vec!["a.bin", "b.bin", "c.bin"]);
    }

    #[test]
    fn only_the_hundred_largest_files_come_back() {
        let (dir, downloads) = profile_with_downloads();
        // 120 files of distinct sizes: the twenty smallest must not survive.
        for size in 1..=120u64 {
            fs::write(downloads.join(format!("f{size:03}.bin")), vec![0u8; size as usize]).unwrap();
        }
        let res = measure(dir.path(), &[("downloads", downloads)]);
        assert_eq!(res.files.len(), TOP_FILES);
        assert_eq!(res.files[0].bytes, 120);
        assert_eq!(res.files[TOP_FILES - 1].bytes, 21);
        // The roots still report everything, ranked or not.
        assert_eq!(res.roots[0].files, 120);
    }

    #[test]
    fn folders_stop_three_levels_below_the_root_and_never_name_the_root() {
        let (dir, downloads) = profile_with_downloads();
        let deep = downloads.join("one").join("two").join("three").join("four");
        fs::create_dir_all(&deep).unwrap();
        fs::write(deep.join("a.bin"), vec![0u8; 64]).unwrap();

        let res = measure(dir.path(), &[("downloads", downloads.clone())]);
        let paths: Vec<&str> = res.folders.iter().map(|f| f.path.as_str()).collect();
        assert!(
            !paths.contains(&downloads.to_string_lossy().as_ref()),
            "the root itself is not one of its own folders: {paths:?}"
        );
        assert!(
            paths.iter().all(|p| !p.ends_with("four")),
            "a fourth level must not be ranked: {paths:?}"
        );
        // The three levels above it each carry the file.
        assert_eq!(res.folders.len(), FOLDER_DEPTH);
        assert!(res.folders.iter().all(|f| f.bytes == 64 && f.files == 1));
        assert!(res.folders[0].path.ends_with("one"));
    }

    #[test]
    fn a_root_that_is_a_junction_outside_the_profile_is_skipped_and_named() {
        let base = TempDir::new().unwrap();
        let profile = base.path().join("profile");
        let outside = base.path().join("outside");
        fs::create_dir_all(&profile).unwrap();
        fs::create_dir_all(&outside).unwrap();
        fs::write(outside.join("precious.bin"), vec![0u8; 999]).unwrap();
        let downloads = profile.join("Downloads");
        junction(&downloads, &outside);

        let res = measure(&profile, &[("downloads", downloads)]);
        assert_eq!(res.skipped_roots, vec!["downloads".to_string()]);
        assert!(res.roots.is_empty(), "roots = {:?}", res.roots);
        assert!(res.files.is_empty(), "files = {:?}", res.files);
        assert!(outside.join("precious.bin").exists());
    }

    #[test]
    fn a_junction_under_a_root_is_not_followed() {
        let base = TempDir::new().unwrap();
        let profile = base.path().join("profile");
        let outside = base.path().join("outside");
        let downloads = profile.join("Downloads");
        fs::create_dir_all(&downloads).unwrap();
        fs::create_dir_all(&outside).unwrap();
        fs::write(downloads.join("mine.bin"), vec![0u8; 10]).unwrap();
        fs::write(outside.join("precious.bin"), vec![0u8; 999]).unwrap();
        junction(&downloads.join("link"), &outside);

        let res = measure(&profile, &[("downloads", downloads)]);
        assert_eq!(res.files.len(), 1, "files = {:?}", res.files);
        assert!(res.files[0].path.ends_with("mine.bin"));
        assert!(
            res.files.iter().all(|f| !f.path.contains("precious")),
            "the walk crossed the junction: {:?}",
            res.files
        );
        // The junction contributes nothing to the root's own total either.
        assert_eq!(res.roots[0].bytes, 10);
    }

    #[test]
    fn a_root_that_is_absent_is_skipped_without_a_word() {
        let (dir, downloads) = profile_with_downloads();
        let absent = dir.path().join("Music");
        let res = measure(dir.path(), &[("downloads", downloads), ("music", absent)]);
        assert_eq!(res.roots.len(), 1);
        assert_eq!(res.roots[0].name, "downloads");
        assert!(res.skipped_roots.is_empty());
    }

    #[test]
    fn progress_fires_once_per_root_with_a_running_total() {
        let (dir, downloads) = profile_with_downloads();
        let desktop = dir.path().join("Desktop");
        fs::create_dir_all(&desktop).unwrap();
        fs::write(downloads.join("a.bin"), vec![0u8; 100]).unwrap();
        fs::write(desktop.join("b.bin"), vec![0u8; 50]).unwrap();

        let canon = fs::canonicalize(dir.path()).unwrap();
        let roots = vec![
            ("downloads".to_string(), downloads.to_string_lossy().to_string()),
            ("desktop".to_string(), desktop.to_string_lossy().to_string()),
        ];
        let mut steps: Vec<SpaceProgress> = Vec::new();
        scan_space_with(&roots, &canon, &mut |step| steps.push(step));

        assert_eq!(steps.len(), 2);
        assert!(steps.iter().all(|s| s.total == 2));
        assert_eq!(steps[0].done, 1);
        assert_eq!(steps[1].done, 2);
        assert_eq!(steps[1].total_bytes, 150, "the total is cumulative");
    }
}
