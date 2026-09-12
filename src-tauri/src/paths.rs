//! Where WinCleaner's per-user stores live: `%APPDATA%\WinCleaner` normally,
//! `<exe dir>\WinCleaner` when the application is portable.
//!
//! Portable mode is one file: `portable.txt` sitting next to
//! `wincleaner.exe`. Its content is never read — a marker, not a
//! configuration file, so a user creating it by hand cannot get it wrong.
//! While it is there, `exclusions.toml` (`exclusions.rs`) and
//! `recycle-bin.toml` (`recycle_cache.rs`) live next to the executable and
//! the application leaves nothing behind on the machine it runs from.
//!
//! Both stores go through `config_dir` below rather than reading `%APPDATA%`
//! themselves: one resolver, so a store added later cannot quietly stay
//! behind in the roaming profile.
//!
//! The sandbox override is deliberately *not* here. A sandbox root replaces
//! the store wholesale (`exclusions::store_path`) and keeps precedence over
//! both branches below, portable included.
//!
//! Front-end settings — theme, language, update consent — are WebView2
//! `localStorage` and stay where WebView2 puts them; portable mode does not
//! move them (`README.md`).

use std::path::{Path, PathBuf};

/// Marker file that turns portable mode on. Any content, including empty.
pub const PORTABLE_MARKER: &str = "portable.txt";

/// Directory holding the stores, under `%APPDATA%` or next to the executable.
pub const APP_DIR: &str = "WinCleaner";

fn has_marker(dir: &Path) -> bool {
    dir.join(PORTABLE_MARKER).is_file()
}

/// The resolver itself, with both lookups injected: the tests exercise the two
/// branches without a real executable and without touching `%APPDATA%`.
pub fn config_dir_with(exe_dir: Option<&Path>, appdata: Option<&str>) -> Result<PathBuf, String> {
    if let Some(dir) = exe_dir.filter(|dir| has_marker(dir)) {
        return Ok(dir.join(APP_DIR));
    }
    let appdata = appdata.ok_or_else(|| "%APPDATA% is not defined".to_string())?;
    Ok(Path::new(appdata).join(APP_DIR))
}

fn exe_dir() -> Option<PathBuf> {
    std::env::current_exe().ok()?.parent().map(Path::to_path_buf)
}

/// The directory this run's stores live in.
pub fn config_dir() -> Result<PathBuf, String> {
    config_dir_with(
        exe_dir().as_deref(),
        std::env::var("APPDATA").ok().as_deref(),
    )
}

/// Whether this run is portable. Settings → About shows it; nothing else
/// branches on it, since every store already goes through `config_dir`.
pub fn is_portable() -> bool {
    exe_dir().is_some_and(|dir| has_marker(&dir))
}

#[cfg(test)]
mod tests {
    use super::*;
    use tempfile::TempDir;

    #[test]
    fn the_marker_moves_the_stores_next_to_the_executable() {
        let exe = TempDir::new().unwrap();
        std::fs::write(exe.path().join(PORTABLE_MARKER), "").unwrap();

        assert_eq!(
            config_dir_with(Some(exe.path()), Some(r"C:\Users\jo\AppData\Roaming")).unwrap(),
            exe.path().join(APP_DIR),
            "a portable run must not write into the roaming profile"
        );
    }

    #[test]
    fn without_the_marker_the_stores_stay_under_appdata() {
        let exe = TempDir::new().unwrap();

        assert_eq!(
            config_dir_with(Some(exe.path()), Some(r"C:\Users\jo\AppData\Roaming")).unwrap(),
            Path::new(r"C:\Users\jo\AppData\Roaming").join(APP_DIR)
        );
    }

    /// A directory next to the executable named like the marker is not one:
    /// the marker is a file.
    #[test]
    fn a_directory_named_like_the_marker_is_not_the_marker() {
        let exe = TempDir::new().unwrap();
        std::fs::create_dir(exe.path().join(PORTABLE_MARKER)).unwrap();

        assert_eq!(
            config_dir_with(Some(exe.path()), Some(r"C:\Users\jo\AppData\Roaming")).unwrap(),
            Path::new(r"C:\Users\jo\AppData\Roaming").join(APP_DIR)
        );
    }

    #[test]
    fn no_marker_and_no_appdata_is_an_error() {
        let exe = TempDir::new().unwrap();

        assert!(config_dir_with(Some(exe.path()), None)
            .unwrap_err()
            .contains("%APPDATA%"));
    }
}
