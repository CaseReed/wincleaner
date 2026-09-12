//! Cache of the last Recycle Bin measurement.
//!
//! `SHQueryRecycleBinW` walks every item of every volume's bin: on a full bin
//! it took **243 s** for 110,553 items / 19 GB on the maintainer's machine,
//! while every other rule of the same Analyze finished in seconds. The whole
//! Analyze waited on that one call. This module remembers the last answer and
//! reuses it while the bin has demonstrably not changed.
//!
//! **This cache is a performance aid, never a safety boundary.** Unlike
//! `exclusions.rs`, it deliberately *fails open*: a store that is missing,
//! unreadable, corrupt or written by another version is ignored and rewritten,
//! because the worst a lost cache can do is make one Analyze slow. Nothing is
//! ever deleted on the strength of a cached figure — `clean.rs` re-queries the
//! bin through its own path and `SHEmptyRecycleBinW` empties whatever is
//! actually there.
//!
//! The fingerprint is the `LastWriteTime` of `<drive>\$Recycle.Bin\<SID>` for
//! every fixed drive, plus the drive list itself. Windows stamps that
//! directory whenever an item is filed into the bin or removed from it, so
//! emptying the bin — or dropping one more file into it — invalidates the
//! cache on its own. The one known miss the timestamps alone would not catch
//! is a **bin appearing on a drive that was not there before** (an external
//! disk plugged in between two Analyzes); including the drive list in the
//! fingerprint is what covers it.

use crate::scan::RecycleQuery;
use serde::{Deserialize, Serialize};
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicBool, Ordering};

/// Name of the store, under `%APPDATA%\WinCleaner`.
pub const RECYCLE_CACHE_FILE: &str = "recycle-bin.toml";

/// Directory the store lives in, under `%APPDATA%`. Same one `exclusions.rs`
/// uses.
const APP_DIR: &str = "WinCleaner";

/// The stored measurement and the state of the bin it describes.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Cached {
    pub fingerprint: String,
    pub items: u64,
    pub bytes: u64,
}

pub fn store_path() -> Result<PathBuf, String> {
    let appdata =
        std::env::var("APPDATA").map_err(|_| "%APPDATA% is not defined".to_string())?;
    Ok(Path::new(&appdata).join(APP_DIR).join(RECYCLE_CACHE_FILE))
}

/// Reads the store. Anything that is not a well-formed entry — absent,
/// unreadable, truncated, from an older shape — is simply `None`: see the
/// fail-open note at the top of this module.
pub fn load(path: &Path) -> Option<Cached> {
    let raw = std::fs::read_to_string(path).ok()?;
    toml::from_str(&raw).ok()
}

/// Writes the store atomically — temporary file, then rename over the target —
/// exactly as `exclusions::save` does, so a crash mid-write leaves the previous
/// entry rather than a half-written one.
pub fn save(path: &Path, entry: &Cached) -> Result<(), String> {
    if let Some(dir) = path.parent() {
        std::fs::create_dir_all(dir)
            .map_err(|err| format!("{} cannot be created: {err}", dir.display()))?;
    }
    let body = toml::to_string_pretty(entry)
        .map_err(|err| format!("the measurement cannot be serialised: {err}"))?;
    let tmp = path.with_extension("toml.tmp");
    std::fs::write(&tmp, body)
        .map_err(|err| format!("{} cannot be written: {err}", tmp.display()))?;
    std::fs::rename(&tmp, path).map_err(|err| {
        let _ = std::fs::remove_file(&tmp);
        format!("{} cannot be replaced: {err}", path.display())
    })
}

/// The fingerprint of the bin as it stands right now, or `None` when it cannot
/// be established (no SID, no drive). `None` disables the cache for that
/// Analyze: measuring is slow, guessing is wrong.
pub fn fingerprint() -> Option<String> {
    let sid = current_user_sid()?;
    let drives = fixed_drives();
    if drives.is_empty() {
        return None;
    }
    Some(fingerprint_of(&drives, &sid))
}

/// The fingerprint proper, split out from the machine probes so a test can
/// feed it a drive list and a SID of its own.
fn fingerprint_of(drives: &[String], sid: &str) -> String {
    let mut out = String::new();
    for drive in drives {
        // `symlink_metadata`: the entry's own stamp, never a junction's
        // target. This is a cache key, so it is read and compared, never
        // walked.
        let stamp = std::fs::symlink_metadata(Path::new(drive).join("$Recycle.Bin").join(sid))
            .ok()
            .and_then(|md| md.modified().ok())
            .and_then(|t| t.duration_since(std::time::UNIX_EPOCH).ok())
            .map(|d| d.as_nanos().to_string())
            .unwrap_or_else(|| "-".to_string());
        out.push_str(drive);
        out.push('=');
        out.push_str(&stamp);
        out.push(';');
    }
    out
}

/// `C:\`, `D:\`… for every fixed volume. A removable or network drive is left
/// out: `SHQueryRecycleBinW` counts them, but they come and go, and the drive
/// list is part of the fingerprint precisely so that a drive appearing or
/// disappearing invalidates the cache.
fn fixed_drives() -> Vec<String> {
    use windows::core::PCWSTR;
    use windows::Win32::Storage::FileSystem::{GetDriveTypeW, GetLogicalDrives};
    use windows::Win32::System::WindowsProgramming::DRIVE_FIXED;

    let mask = unsafe { GetLogicalDrives() };
    (0..26u32)
        .filter(|i| mask & (1 << i) != 0)
        .map(|i| format!("{}:\\", (b'A' + i as u8) as char))
        .filter(|root| {
            let wide: Vec<u16> = root.encode_utf16().chain(std::iter::once(0)).collect();
            let kind = unsafe { GetDriveTypeW(PCWSTR(wide.as_ptr())) };
            kind == DRIVE_FIXED
        })
        .collect()
}

/// The current user's SID in `S-1-5-21-…` form: the name of their own
/// directory inside every `$Recycle.Bin`. Read off the process token, so it
/// needs no account name and no extra dependency.
fn current_user_sid() -> Option<String> {
    use windows::Win32::Foundation::{CloseHandle, HANDLE, HLOCAL, LocalFree};
    use windows::Win32::Security::Authorization::ConvertSidToStringSidW;
    use windows::Win32::Security::{GetTokenInformation, TokenUser, TOKEN_QUERY, TOKEN_USER};
    use windows::Win32::System::Threading::{GetCurrentProcess, OpenProcessToken};

    unsafe {
        let mut token = HANDLE::default();
        OpenProcessToken(GetCurrentProcess(), TOKEN_QUERY, &mut token).ok()?;
        let mut needed = 0u32;
        // First call sizes the buffer: it is expected to fail with
        // ERROR_INSUFFICIENT_BUFFER, which is why its result is dropped.
        let _ = GetTokenInformation(token, TokenUser, None, 0, &mut needed);
        let mut buffer = vec![0u8; needed as usize];
        let queried = GetTokenInformation(
            token,
            TokenUser,
            Some(buffer.as_mut_ptr().cast()),
            needed,
            &mut needed,
        );
        let _ = CloseHandle(token);
        queried.ok()?;

        let user = &*(buffer.as_ptr() as *const TOKEN_USER);
        let mut text = windows::core::PWSTR::null();
        ConvertSidToStringSidW(user.User.Sid, &mut text).ok()?;
        let sid = text.to_string().ok();
        let _ = LocalFree(HLOCAL(text.0.cast()));
        sid
    }
}

/// The injected Recycle Bin query, wrapped in the on-disk cache.
///
/// One instance per Analyze: the fingerprint is taken once, up front, so the
/// hit test and the entry that gets written describe the same moment.
pub struct CachedRecycle {
    store: Option<PathBuf>,
    fingerprint: Option<String>,
    hit: AtomicBool,
}

impl CachedRecycle {
    /// For the real engine: the store under `%APPDATA%` and the live
    /// fingerprint. Either being unavailable just means no caching.
    pub fn new() -> Self {
        Self::with(store_path().ok(), fingerprint())
    }

    /// For tests: a `TempDir`-backed store and a fingerprint of their own, so
    /// the suite never reads the real bin nor the user's real store.
    pub fn with(store: Option<PathBuf>, fingerprint: Option<String>) -> Self {
        Self {
            store,
            fingerprint,
            hit: AtomicBool::new(false),
        }
    }

    /// Whether the last `query` answered from the store instead of the shell.
    pub fn was_cached(&self) -> bool {
        self.hit.load(Ordering::Relaxed)
    }

    /// The stored figures when the bin is demonstrably unchanged, the real
    /// measurement otherwise — stored on the way out. A failed measurement is
    /// never stored: it would pin an error's worth of zeroes onto a bin that
    /// is fine.
    pub fn query(&self, real: RecycleQuery) -> Result<(u64, u64), String> {
        let (Some(store), Some(fingerprint)) = (self.store.as_ref(), self.fingerprint.as_ref())
        else {
            return real();
        };
        if let Some(entry) = load(store) {
            if &entry.fingerprint == fingerprint {
                self.hit.store(true, Ordering::Relaxed);
                return Ok((entry.items, entry.bytes));
            }
        }
        let (items, bytes) = real()?;
        // A store we cannot write is one slow Analyze next time, nothing more.
        let _ = save(
            store,
            &Cached {
                fingerprint: fingerprint.clone(),
                items,
                bytes,
            },
        );
        Ok((items, bytes))
    }
}

/// Only here because `clippy::new_without_default` asks for it: the real
/// constructor is `new`.
impl Default for CachedRecycle {
    fn default() -> Self {
        Self::new()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::cell::Cell;
    use tempfile::TempDir;

    /// A query that counts its calls, so "did the cache reach the shell" is an
    /// assertion and not an inference from timing.
    fn counting(calls: &Cell<u32>, answer: (u64, u64)) -> impl Fn() -> Result<(u64, u64), String> + '_ {
        move || {
            calls.set(calls.get() + 1);
            Ok(answer)
        }
    }

    #[test]
    fn a_miss_calls_the_query_once_and_stores_it() {
        let dir = TempDir::new().unwrap();
        let store = dir.path().join(RECYCLE_CACHE_FILE);
        let calls = Cell::new(0);
        let cache = CachedRecycle::with(Some(store.clone()), Some("fp-1".into()));

        let got = cache.query(&counting(&calls, (110_553, 19_000_000_000))).unwrap();
        assert_eq!(got, (110_553, 19_000_000_000));
        assert_eq!(calls.get(), 1);
        assert!(!cache.was_cached());
        assert_eq!(
            load(&store).unwrap(),
            Cached {
                fingerprint: "fp-1".into(),
                items: 110_553,
                bytes: 19_000_000_000
            }
        );
    }

    #[test]
    fn a_hit_returns_the_stored_figures_without_calling_the_query() {
        let dir = TempDir::new().unwrap();
        let store = dir.path().join(RECYCLE_CACHE_FILE);
        save(
            &store,
            &Cached {
                fingerprint: "fp-1".into(),
                items: 42,
                bytes: 4_096,
            },
        )
        .unwrap();

        let cache = CachedRecycle::with(Some(store), Some("fp-1".into()));
        let got = cache
            .query(&|| panic!("a cache hit must not reach the shell"))
            .unwrap();
        assert_eq!(got, (42, 4_096));
        assert!(cache.was_cached());
    }

    /// Emptying the bin, or dropping one more file into it, restamps
    /// `<drive>\$Recycle.Bin\<SID>`: the fingerprint no longer matches and the
    /// stored figures are thrown away rather than shown again.
    #[test]
    fn a_changed_fingerprint_invalidates_the_entry() {
        let dir = TempDir::new().unwrap();
        let store = dir.path().join(RECYCLE_CACHE_FILE);
        save(
            &store,
            &Cached {
                fingerprint: "fp-1".into(),
                items: 42,
                bytes: 4_096,
            },
        )
        .unwrap();

        let calls = Cell::new(0);
        let cache = CachedRecycle::with(Some(store.clone()), Some("fp-2".into()));
        let got = cache.query(&counting(&calls, (0, 0))).unwrap();
        assert_eq!(got, (0, 0));
        assert_eq!(calls.get(), 1);
        assert!(!cache.was_cached());
        assert_eq!(load(&store).unwrap().fingerprint, "fp-2");
    }

    /// Fail open, unlike the exclusions store: a corrupt file is a slow
    /// Analyze, never a refused one, and it is rewritten on the way out.
    #[test]
    fn a_corrupt_store_is_ignored_and_rewritten() {
        let dir = TempDir::new().unwrap();
        let store = dir.path().join(RECYCLE_CACHE_FILE);
        std::fs::write(&store, "this is not toml {{{").unwrap();

        let calls = Cell::new(0);
        let cache = CachedRecycle::with(Some(store.clone()), Some("fp-1".into()));
        let got = cache.query(&counting(&calls, (7, 700))).unwrap();
        assert_eq!(got, (7, 700));
        assert_eq!(calls.get(), 1);
        assert!(!cache.was_cached());
        assert_eq!(load(&store).unwrap().items, 7);
    }

    #[test]
    fn a_failed_measurement_is_not_stored() {
        let dir = TempDir::new().unwrap();
        let store = dir.path().join(RECYCLE_CACHE_FILE);
        let cache = CachedRecycle::with(Some(store.clone()), Some("fp-1".into()));
        assert!(cache.query(&|| Err("SHQueryRecycleBinW failed".into())).is_err());
        assert!(load(&store).is_none());
    }

    /// The drive list is part of the key, so a bin that appears on a drive
    /// plugged in between two Analyzes is a miss — the one case the
    /// timestamps alone would not catch.
    #[test]
    fn the_drive_list_is_part_of_the_fingerprint() {
        let one = fingerprint_of(&["C:\\".to_string()], "S-1-5-21-x");
        let two = fingerprint_of(&["C:\\".to_string(), "D:\\".to_string()], "S-1-5-21-x");
        assert_ne!(one, two);
    }

    /// Environment-coupled: it reads this machine's own SID and drives. It
    /// asserts the shape, not the values, and touches no file.
    #[test]
    fn the_real_fingerprint_names_the_user_and_the_drives() {
        let fp = fingerprint().expect("a Windows session always has a SID and a fixed drive");
        assert!(fp.contains(":\\="), "one entry per drive: {fp}");
        assert!(fp.ends_with(';'), "{fp}");
    }
}
