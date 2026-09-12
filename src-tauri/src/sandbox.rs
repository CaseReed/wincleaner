//! Synthetic Windows profile — the fixture the safety harness runs against,
//! and the profile the Sandbox mode of the application builds so a user can
//! watch the real engine clean a tree that is not theirs.
//!
//! Everything lives under one base directory. The harness passes a `TempDir`;
//! `commands::enter_sandbox` passes a directory it creates under `%TEMP%` and
//! removes again on leave.
//!
//! ```text
//! <base>\profile\        the fake %USERPROFILE% (and %LOCALAPPDATA%, %APPDATA%, %TEMP%)
//! <base>\outside\        target of the junction planted in %TEMP%
//! <base>\outside2\       target of the junction planted in the Chrome cache
//! <base>\outside3\       target of the junction planted ON a walk root (CrashDumps)
//! <base>\outside4\       target of the junction swapped in AFTER the scan (TOCTOU)
//! <base>\control\        never named by any rule; the snapshot proves it is untouched
//! <base>\recycle-bin\    where Trash mode moves a sandbox file (see `sandbox_trash`)
//! ```
//!
//! All four variables the rules may use — `%USERPROFILE%`, `%LOCALAPPDATA%`,
//! `%APPDATA%`, `%TEMP%` — are mapped **inside `<base>\profile`**, so the
//! containment the application already enforces (textual `under_profile`, then
//! `confined_root` and `deletable_path` replayed on disk) applies to the
//! sandbox unchanged: nothing outside `<base>` can be named, walked or deleted.
//!
//! Junction cleanup: every junction is created inside the base directory, so
//! the harness's `TempDir::drop` removes it. That drop runs even when an
//! assertion fails, because the test profile unwinds — `panic = "abort"` is set
//! on the `[profile.release]` of `src-tauri/Cargo.toml` only, and `cargo test`
//! builds the dev profile. A harness moved to a panic-abort profile would leak
//! the junctions of a failing run into `%TEMP%`.
//!
//! Two families of files, both written with a content marker derived from the
//! path itself so a silent rewrite is caught as well as a deletion:
//!
//! * JUNK — every file a rule is expected to remove.
//! * SENTINELS — every file that must survive, whatever rule is selected.

use crate::rules::{
    load_rules_with, memoized_env, EnvLookup, Rule, RULES_TOML,
};
use crate::winapp2::{
    convert_with, detect_file_exists_with, detected_rules_with, ConversionReport, WINAPP2_INI,
};
use serde::{Deserialize, Serialize};
use std::collections::{BTreeSet, HashMap};
use std::path::{Path, PathBuf};
use walkdir::WalkDir;

/// Directory under `%TEMP%` swapped for a junction between the scan and the
/// deletion. `zz_` so it sorts last among the temp junk.
pub const TOCTOU_DIR: &str = "zz_toctou";

#[derive(Debug, Clone)]
pub struct Sentinel {
    pub path: PathBuf,
    /// Why this file must survive. Printed verbatim when it does not.
    pub why: &'static str,
}

/// One junk file, and the rule that is expected to remove it. The attribution
/// is what lets the verdict scope its counts to the rules a user actually
/// cleaned instead of holding them to the whole catalogue.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct JunkFile {
    pub path: PathBuf,
    /// Id of the rule whose pattern this file was written for.
    pub rule: String,
}

pub struct Fixture {
    pub base: PathBuf,
    pub profile: PathBuf,
    pub outside: PathBuf,
    pub outside2: PathBuf,
    pub outside3: PathBuf,
    pub outside4: PathBuf,
    pub junk: Vec<JunkFile>,
    pub sentinels: Vec<Sentinel>,
    /// The junction links themselves (not their targets).
    pub junctions: Vec<PathBuf>,
    /// The sentinels that are reachable ONLY by crossing a junction this
    /// fixture plants. They are what "the walk refused the indirection" is
    /// measured on; the other files outside the profile (`control`, the
    /// junction targets' own `secret.txt`) are ordinary sentinels.
    pub junction_baits: Vec<PathBuf>,
    vars: HashMap<String, String>,
}

/// Every failure of the build reads the same way, and names the path.
fn build_err(path: &Path, e: std::io::Error) -> String {
    format!(
        "Could not create the sandbox profile: {e} at {}",
        path.display()
    )
}

/// Windows hands `canonicalize` back a `\\?\` verbatim path. Rule patterns are
/// built from the injected variables, which are plain paths, so the two forms
/// have to be reconciled before any comparison.
fn strip_verbatim_str(p: &Path) -> PathBuf {
    let s = p.to_string_lossy().to_string();
    PathBuf::from(s.strip_prefix(r"\\?\").unwrap_or(&s).to_string())
}

/// `FILE_ATTRIBUTE_REPARSE_POINT`. A junction is not a symbolic link as far as
/// Rust is concerned, so the attribute is read directly.
fn is_reparse(path: &Path) -> bool {
    use std::os::windows::fs::MetadataExt;
    std::fs::symlink_metadata(path)
        .map(|md| md.file_attributes() & 0x0000_0400 != 0)
        .unwrap_or(false)
}

/// `CREATE_NO_WINDOW`. Without it every `mklink` call flashes a console window
/// over the application: the fixture plants two junctions on enter, and the
/// harness a third and a fourth.
const CREATE_NO_WINDOW: u32 = 0x0800_0000;

/// Creates a directory junction. `mklink /J` needs no privilege, unlike
/// `mklink /D`: the harness — and the sandbox — run unelevated.
///
/// Fallible rather than panicking: this is the one step of the build that can
/// legitimately fail on a user's machine (a policy blocking `cmd`, a volume
/// with no reparse-point support), and `sandbox_enter` has to report that as
/// an error instead of aborting the process.
fn junction(link: &Path, target: &Path) -> Result<(), String> {
    use std::os::windows::process::CommandExt;
    let out = std::process::Command::new("cmd")
        .arg("/C")
        .arg("mklink")
        .arg("/J")
        .arg(link)
        .arg(target)
        .creation_flags(CREATE_NO_WINDOW)
        .output()
        .map_err(|e| format!("mklink could not be started: {e}"))?;
    if !out.status.success() {
        // `mklink` reports its refusals on stdout, not on stderr: dropping it
        // left the error message empty for the most common failure of all.
        return Err(format!(
            "mklink /J failed for {} -> {}: {} {}",
            link.display(),
            target.display(),
            String::from_utf8_lossy(&out.stdout).trim(),
            String::from_utf8_lossy(&out.stderr).trim()
        ));
    }
    Ok(())
}

impl Fixture {
    /// Content every file of the fixture carries. Derived from the path so it
    /// can be recomputed at assertion time without keeping a second table.
    pub fn sentinel_content(path: &Path) -> Vec<u8> {
        format!("wincleaner-safety-harness::{}", path.display()).into_bytes()
    }

    pub fn strip_verbatim(path: &Path) -> PathBuf {
        strip_verbatim_str(path)
    }

    /// Fallible, like every write of the build: `enter_sandbox` runs on a user
    /// machine, where a full or read-only volume is a condition to report, not
    /// a reason to abort the process (release builds are `panic = "abort"`).
    fn write(path: &Path) -> Result<(), String> {
        let parent = path.parent().unwrap_or(path);
        std::fs::create_dir_all(parent).map_err(|e| build_err(parent, e))?;
        std::fs::write(path, Fixture::sentinel_content(path)).map_err(|e| build_err(path, e))
    }

    fn junk_file(&mut self, rule: &str, rel: &str) -> Result<(), String> {
        let path = self.profile.join(rel);
        Self::write(&path)?;
        self.junk.push(JunkFile {
            path,
            rule: rule.to_string(),
        });
        Ok(())
    }

    fn sentinel(&mut self, path: PathBuf, why: &'static str) -> Result<(), String> {
        Self::write(&path)?;
        self.sentinels.push(Sentinel { path, why });
        Ok(())
    }

    /// A sentinel that can only be reached by crossing a junction: it is
    /// counted separately in the verdict, as the measure of the indirection
    /// being refused.
    fn bait(&mut self, path: PathBuf, why: &'static str) -> Result<(), String> {
        self.junction_baits.push(path.clone());
        self.sentinel(path, why)
    }

    fn profile_sentinel(&mut self, rel: &str, why: &'static str) -> Result<(), String> {
        let path = self.profile.join(rel);
        self.sentinel(path, why)
    }

    /// Every junk path, in order. The harness compares whole sets of paths; the
    /// rule attribution only matters to the verdict.
    pub fn junk_paths(&self) -> Vec<PathBuf> {
        self.junk.iter().map(|j| j.path.clone()).collect()
    }

    pub fn lookup(&self, name: &str) -> Option<String> {
        self.vars.get(name).cloned()
    }

    /// Builds the whole fixture under `base`, which must already exist and is
    /// expected to be empty. The caller owns `base` and its removal: the
    /// harness hands a `TempDir`, `commands::enter_sandbox` a directory it
    /// created under `%TEMP%` and deletes again on leave.
    ///
    /// **Every one of the four rule variables is mapped under `base\profile`.**
    /// That is what makes the application's own containment cover the sandbox:
    /// a rule cannot even name a path outside `base`.
    pub fn build_in(base: &Path) -> Result<Self, String> {
        // Canonicalised so the injected variables and what `canonicalize`
        // returns during the walk describe the same directory: the base sits
        // under `%TMP%`, which Windows may hand out in 8.3 short form.
        let base =
            strip_verbatim_str(&std::fs::canonicalize(base).map_err(|e| build_err(base, e))?);
        let profile = base.join("profile");
        let local = profile.join("AppData").join("Local");
        let roaming = profile.join("AppData").join("Roaming");
        let temp = local.join("Temp");
        std::fs::create_dir_all(&temp).map_err(|e| build_err(&temp, e))?;
        std::fs::create_dir_all(&roaming).map_err(|e| build_err(&roaming, e))?;

        let vars = HashMap::from([
            ("USERPROFILE".to_string(), profile.display().to_string()),
            ("LOCALAPPDATA".to_string(), local.display().to_string()),
            ("APPDATA".to_string(), roaming.display().to_string()),
            ("TEMP".to_string(), temp.display().to_string()),
        ]);

        let mut fx = Fixture {
            outside: base.join("outside"),
            outside2: base.join("outside2"),
            outside3: base.join("outside3"),
            outside4: base.join("outside4"),
            base,
            profile,
            junk: Vec::new(),
            sentinels: Vec::new(),
            junctions: Vec::new(),
            junction_baits: Vec::new(),
            vars,
        };

        fx.populate_junk()?;
        fx.populate_sentinels()?;
        fx.populate_detect_files()?;
        fx.populate_junctions()?;
        Ok(fx)
    }

    /// One or more files for EVERY `kind = "files"` rule of `rules.toml`.
    /// A new rule with no entry here fails the harness (the scan finds nothing
    /// for it), which is the point.
    fn populate_junk(&mut self) -> Result<(), String> {
        // windows.temp — `%TEMP%\**\*`, recursive.
        self.junk_file("windows.temp", r"AppData\Local\Temp\stray.tmp")?;
        self.junk_file("windows.temp", r"AppData\Local\Temp\nested\installer.log")?;
        self.junk_file("windows.temp", r"AppData\Local\Temp\nested\deeper\chunk.bin")?;
        // The TOCTOU subject. `zz_` so it sorts last inside %TEMP%: the swap
        // is performed on the FIRST deletion of the rule, and this path must
        // still be ahead of the cursor when it happens.
        self.junk_file(
            "windows.temp",
            &format!(r"AppData\Local\Temp\{TOCTOU_DIR}\victim.txt"),
        )?;

        // windows.thumbnails — non-recursive, two prefixes.
        self.junk_file(
            "windows.thumbnails",
            r"AppData\Local\Microsoft\Windows\Explorer\thumbcache_1024.db",
        )?;
        self.junk_file(
            "windows.thumbnails",
            r"AppData\Local\Microsoft\Windows\Explorer\iconcache_32.db",
        )?;

        // windows.explorer-recent — `*.lnk` (non-recursive) + AutomaticDestinations.
        self.junk_file(
            "windows.explorer-recent",
            r"AppData\Roaming\Microsoft\Windows\Recent\report.lnk",
        )?;
        self.junk_file(
            "windows.explorer-recent",
            r"AppData\Roaming\Microsoft\Windows\Recent\AutomaticDestinations\1b4dd67f29cb1962.automaticDestinations-ms",
        )?;

        // windows.crash-dumps
        self.junk_file(
            "windows.crash-dumps",
            r"AppData\Local\CrashDumps\wincleaner.exe.4242.dmp",
        )?;

        // windows.wer-reports — recursive (`**`), one file nested under each
        // report folder to prove the recursion actually walks.
        self.junk_file(
            "windows.wer-reports",
            r"AppData\Local\Microsoft\Windows\WER\ReportQueue\AppCrash_wincleaner_4242\Report.wer",
        )?;
        self.junk_file(
            "windows.wer-reports",
            r"AppData\Local\Microsoft\Windows\WER\ReportArchive\AppCrash_wincleaner_1337\Report.wer",
        )?;

        // edge.cache — one file per declared pattern.
        for rel in [
            r"Default\Cache\Cache_Data\f_000001",
            r"Default\Code Cache\js\index",
            r"Default\GPUCache\data_0",
            r"Default\DawnGraphiteCache\blob",
            r"Default\DawnWebGPUCache\blob",
            r"Default\Service Worker\CacheStorage\0a1b\index",
            r"Default\Service Worker\ScriptCache\index",
        ] {
            self.junk_file(
                "edge.cache",
                &format!(r"AppData\Local\Microsoft\Edge\User Data\{rel}"),
            )?;
        }
        self.junk_file(
            "edge.cache",
            r"AppData\Local\Microsoft\Edge\User Data\ShaderCache\GPUCache\data_1",
        )?;

        // chrome.cache — same, plus a second profile directory so the `*`
        // level is really expanded.
        for rel in [
            r"Default\Cache\Cache_Data\f_000002",
            r"Default\Code Cache\wasm\index",
            r"Default\GPUCache\data_0",
            r"Default\DawnGraphiteCache\blob",
            r"Default\DawnWebGPUCache\blob",
            r"Default\Service Worker\CacheStorage\9f8e\index",
            r"Default\Service Worker\ScriptCache\index",
            r"Profile 1\Cache\Cache_Data\f_000003",
        ] {
            self.junk_file(
                "chrome.cache",
                &format!(r"AppData\Local\Google\Chrome\User Data\{rel}"),
            )?;
        }
        self.junk_file(
            "chrome.cache",
            r"AppData\Local\Google\Chrome\User Data\ShaderCache\GPUCache\data_1",
        )?;

        // firefox.cache
        self.junk_file(
            "firefox.cache",
            r"AppData\Local\Mozilla\Firefox\Profiles\a1b2c3d4.default-release\cache2\entries\4F2A",
        )?;
        self.junk_file(
            "firefox.cache",
            r"AppData\Local\Mozilla\Firefox\Profiles\a1b2c3d4.default-release\startupCache\startupCache.8.little",
        )?;

        // npm.cache
        self.junk_file(
            "npm.cache",
            r"AppData\Local\npm-cache\_cacache\content-v2\sha512\ab\cd\ef01",
        )?;
        Ok(())
    }

    fn populate_sentinels(&mut self) -> Result<(), String> {
        // (a) User data. No rule, native or converted, may name these: the
        // Winapp2 converter refuses the first segment under %USERPROFILE%.
        for (rel, why) in [
            (r"Documents\thesis.docx", "user document"),
            (r"Desktop\notes.txt", "user document"),
            (r"Pictures\photo.jpg", "user picture"),
            (r"Downloads\installer.exe", "user download"),
            (r"Videos\clip.mp4", "user video"),
            (r"Music\song.mp3", "user music"),
            (r".ssh\id_ed25519", "private key"),
            (r"Documents\project\.git\HEAD", "git repository"),
            (r"Documents\project\.env", "project secret"),
        ] {
            self.profile_sentinel(rel, why)?;
        }

        // (b) Inside app folders, right next to junk, but not matching the rule.
        // Firefox is the browser whose Winapp2 entries stay undetected here
        // (their DetectFile is %AppData%\Mozilla\Firefox\Profiles, which the
        // fixture never creates), so its sentinels hold for every run.
        let ff = r"AppData\Local\Mozilla\Firefox\Profiles\a1b2c3d4.default-release";
        for (name, why) in [
            ("places.sqlite", "browsing history and bookmarks"),
            ("key4.db", "password database"),
            ("logins.json", "saved logins"),
            ("prefs.js", "user preferences"),
            ("cookies.sqlite", "session cookies"),
        ] {
            self.profile_sentinel(&format!(r"{ff}\{name}"), why)?;
        }
        self.profile_sentinel(
            &format!(r"{ff}\extensions\ublock@raymondhill.net.xpi"),
            "installed extension",
        )?;

        // (c) Sibling directories with a lookalike name. Windows file names are
        // case-insensitive, so `Cache2` vs `cache2` would be the SAME
        // directory: the lookalikes differ by a suffix instead.
        for (rel, why) in [
            (
                format!(r"{ff}\cache2.bak\entries\4F2A"),
                "backup of a cache directory, not the cache",
            ),
            (
                format!(r"{ff}\startupCacheKeep\pinned"),
                "directory whose name merely starts like startupCache",
            ),
            (
                format!(r"{ff}\cache2-notes.txt"),
                "file whose name merely starts like cache2",
            ),
        ] {
            self.profile_sentinel(&rel, why)?;
        }

        // Chromium-family internals, sitting right next to the caches the rules
        // do clean. No Winapp2 entry reaches them either: not one Chrome or
        // Edge section survives the conversion, so these hold for the full
        // catalogue as well (see docs/safety-harness.md).
        for browser in [
            r"AppData\Local\Google\Chrome\User Data",
            r"AppData\Local\Microsoft\Edge\User Data",
        ] {
            for (name, why) in [
                ("Login Data", "saved passwords"),
                ("Cookies", "session cookies"),
                ("History", "browsing history"),
                ("Bookmarks", "bookmarks"),
                ("Preferences", "user preferences"),
                (
                    r"Service Worker\Database\000003.log",
                    "service worker registrations, not their cache",
                ),
                (r"Local Storage\leveldb\000005.ldb", "web application data"),
                (
                    r"IndexedDB\https_example.com_0.indexeddb.leveldb\CURRENT",
                    "web application data",
                ),
                (
                    r"Extensions\cjpalhdlnbpafiamejdnhcphjbkeiagm\manifest.json",
                    "installed extension",
                ),
                (
                    r"Cache.bak\keep.bin",
                    "sibling of Cache, not matched by the rule",
                ),
                (
                    "CacheStorage-notes.txt",
                    "file whose name merely starts like CacheStorage",
                ),
            ] {
                self.profile_sentinel(&format!(r"{browser}\Default\{name}"), why)?;
            }
        }

        // npm's own configuration sits next to its cache.
        self.profile_sentinel(r".npmrc", "npm configuration")?;

        // Pinned taskbar items: the rule stops at Recent\*.lnk on purpose.
        self.profile_sentinel(
            r"AppData\Roaming\Microsoft\Windows\Recent\CustomDestinations\pinned.customDestinations-ms",
            "items the user pinned by hand",
        )?;

        // (f) One level deeper than a NON-RECURSIVE rule, and carrying the
        // very extension that rule matches. Each of these is deleted the day
        // its rule becomes recursive — or the day `build_set` loses
        // `literal_separator(true)`, which is what makes a lone `*` stop at a
        // separator.
        //
        // `Recent\CustomDestinations\pinned.lnk` is the sharpest of the three:
        // the sibling pattern `Recent\AutomaticDestinations\*` raises the walk
        // ceiling of the shared `Recent` root to two levels, so the walk DOES
        // reach this file. Only the glob refuses it. The other two sit under a
        // root whose ceiling is one level, so they need the `max_depth` ceiling
        // to fall as well — defence in depth, and the fixture that catches a
        // rule rewritten as `**\*`.
        for (rel, why) in [
            (
                r"AppData\Roaming\Microsoft\Windows\Recent\CustomDestinations\pinned.lnk",
                "a .lnk one level below Recent\\*.lnk, which is not recursive",
            ),
            (
                r"AppData\Local\Microsoft\Windows\Explorer\keep\thumbcache_9.db",
                "a thumbcache_*.db one level below a non-recursive rule",
            ),
            (
                r"AppData\Local\CrashDumps\keep\notes.dmp",
                "a file one level below CrashDumps\\*, which is not recursive",
            ),
        ] {
            self.profile_sentinel(rel, why)?;
        }

        // (d) Reachable only through a junction, and (e) the control directory
        // no rule ever names.
        let outside = self.outside.join("secret.txt");
        self.sentinel(outside, "outside the profile, behind a junction")?;
        let outside2 = self.outside2.join("also-secret.txt");
        self.sentinel(
            outside2,
            "outside the profile, behind a junction planted in a cache",
        )?;
        // Bait: reached through the %TEMP% junction, this name matches
        // `%TEMP%\**\*`; reached through the Chrome cache junction, that one
        // matches `...\Cache\**\*`. They survive only because the walk refuses
        // to cross the junction at all.
        let bait = self.outside.join("bait.tmp");
        self.bait(
            bait,
            "would match %TEMP%\\**\\* if the junction were traversed",
        )?;
        let bait2 = self.outside2.join("Cache_Data").join("f_000009");
        self.bait(
            bait2,
            "would match the Chrome cache glob if the junction were traversed",
        )?;
        // Bait behind the junction that `junction_over_crash_dumps` plants ON
        // a walk root. Its name matches `%LOCALAPPDATA%\CrashDumps\*`, so it
        // survives only because `confined_root` refuses to walk a root that
        // carries FILE_ATTRIBUTE_REPARSE_POINT.
        // Not a junction bait as far as the verdict is concerned: the junction
        // that makes it reachable is planted by the harness
        // (`junction_over_crash_dumps`), never by Sandbox mode. It stays an
        // ordinary sentinel — a file that must survive whatever happens.
        let bait3 = self.outside3.join("bait.dmp");
        self.sentinel(
            bait3,
            "would match %LOCALAPPDATA%\\CrashDumps\\* if a junction AT the walk root were walked",
        )?;
        // Victim of the TOCTOU swap: after the scan, `%TEMP%\<TOCTOU_DIR>`
        // becomes a junction to this directory, so the already-scanned path
        // `%TEMP%\<TOCTOU_DIR>\victim.txt` now resolves here. Only
        // `deletable_path`, re-checked immediately before each deletion,
        // stands between this file and `remove_file`.
        let victim = self.outside4.join("victim.txt");
        self.sentinel(
            victim,
            "would be deleted if deletable_path stopped re-resolving the path before deleting",
        )?;

        let control = self.base.join("control").join("untouched.dat");
        self.sentinel(control, "control directory, named by no rule")?;
        Ok(())
    }

    /// `DetectFile` targets, so a sample of Winapp2 entries becomes "detected"
    /// against this tree and nothing else. The Chromium and Edge entries are
    /// detected as a side effect of the browser cache fixtures above
    /// (`DetectFile=%LocalAppData%\Google\Chrome*`).
    fn populate_detect_files(&mut self) -> Result<(), String> {
        for rel in [
            r"AppData\Local\Postman",
            r"AppData\Roaming\Slack",
            r"AppData\Roaming\Spotify",
            r"AppData\Roaming\Telegram Desktop",
            r"AppData\Roaming\vlc",
            r"AppData\Roaming\Audacity",
            r"AppData\Roaming\obs-studio",
            r"AppData\Roaming\Obsidian",
            r"AppData\Local\GitHub",
            r"AppData\Local\Microsoft\PowerToys\ZoomIt",
            r"AppData\Local\Packages\DropboxInc.Dropbox_xbfy0k16fey96",
        ] {
            let dir = self.profile.join(rel);
            std::fs::create_dir_all(&dir).map_err(|e| build_err(&dir, e))?;
        }
        Ok(())
    }

    /// Two junctions pointing outside the profile: one on the `%TEMP%` walk,
    /// one buried inside the Chrome cache. `mklink /J` needs no privilege,
    /// which is exactly why the containment guards exist.
    fn populate_junctions(&mut self) -> Result<(), String> {
        let temp_link = self.profile.join(r"AppData\Local\Temp\linked");
        junction(&temp_link, &self.outside)?;
        self.junctions.push(temp_link);

        let cache_link = self
            .profile
            .join(r"AppData\Local\Google\Chrome\User Data\Default\Cache\link");
        junction(&cache_link, &self.outside2)?;
        self.junctions.push(cache_link);
        Ok(())
    }

    /// Replaces the `windows.crash-dumps` walk root ITSELF with a junction to
    /// `outside3`. The two junctions planted by `populate_junctions` sit
    /// *inside* a walk; this one IS the walk root, which is the only case
    /// `scan::confined_root` can catch — `walkdir` descends into its own root
    /// even when that root is a reparse point.
    ///
    /// The crash-dump fixtures are dropped from the junk and sentinel lists in
    /// the same move: the directory holding them no longer exists.
    pub fn junction_over_crash_dumps(&mut self) -> PathBuf {
        let root = self.profile.join(r"AppData\Local\CrashDumps");
        std::fs::remove_dir_all(&root).unwrap();
        let prefix = root.to_string_lossy().to_lowercase();
        let under = |p: &Path| p.to_string_lossy().to_lowercase().starts_with(&prefix);
        self.junk.retain(|j| !under(&j.path));
        self.sentinels.retain(|s| !under(&s.path));
        junction(&root, &self.outside3).expect("mklink /J over the crash-dump root");
        self.junctions.push(root.clone());
        root
    }

    /// The TOCTOU swap: `%TEMP%\zz_toctou`, whose `victim.txt` the scan has
    /// just recorded, becomes a junction to `outside4`, which holds a
    /// `victim.txt` of its own. The scanned path is unchanged and still names
    /// a regular file — it simply no longer names the same file, and the file
    /// it names now lives outside the profile.
    ///
    /// Called from inside the injected deletion closure, i.e. after the
    /// internal re-scan `clean_rule_with_trash` performs: this is the exact
    /// window `deletable_path` exists to close.
    pub fn swap_toctou_dir_for_junction(&self) -> PathBuf {
        let dir = self.profile.join(r"AppData\Local\Temp").join(TOCTOU_DIR);
        std::fs::remove_dir_all(&dir).unwrap();
        junction(&dir, &self.outside4).expect("mklink /J over the TOCTOU directory");
        dir
    }

    /// The scanned path of the TOCTOU victim, before the swap.
    pub fn toctou_victim_path(&self) -> PathBuf {
        self.profile
            .join(r"AppData\Local\Temp")
            .join(TOCTOU_DIR)
            .join("victim.txt")
    }

    /// One concrete file per converted Winapp2 glob, so those rules delete
    /// something instead of walking empty trees. A pattern whose literal form
    /// cannot be derived (a `{a,b}` alternation) is skipped rather than
    /// guessed at, and a path that would collide with an existing file is
    /// never overwritten.
    ///
    /// Returns how many files it actually created. The caller asserts a floor
    /// on that number: every `continue` below is silent, so a change that made
    /// them all fire would otherwise leave the Winapp2 rules walking empty
    /// trees while the suite stayed green.
    /// `patterns` pairs each resolved glob with the id of the rule it came
    /// from, so the file created for it is attributed to that rule and the
    /// verdict can scope its counts to what was actually cleaned.
    pub fn add_winapp2_junk(&mut self, patterns: &[(String, String)]) -> usize {
        // Windows file names are case-insensitive and Winapp2 spells the same
        // directory several ways (`DropBox` and `Dropbox`): the bookkeeping has
        // to be case-insensitive too, or the same file would be created twice
        // and counted once.
        let key = |p: &Path| p.to_string_lossy().to_lowercase();
        let mut known: BTreeSet<String> = self
            .junk
            .iter()
            .map(|j| &j.path)
            .chain(self.sentinels.iter().map(|s| &s.path))
            .chain(self.junctions.iter())
            .map(|p| key(p))
            .collect();

        let mut created = 0usize;
        for (rule, pattern) in patterns {
            let Some(path) = concrete_path(pattern) else {
                continue;
            };
            if !known.insert(key(&path)) || path.exists() {
                continue;
            }
            // A junction on the way would take the file outside the profile.
            if self
                .junctions
                .iter()
                .any(|j| path.starts_with(j))
            {
                continue;
            }
            if std::fs::create_dir_all(path.parent().unwrap()).is_err() {
                continue;
            }
            if std::fs::write(&path, Fixture::sentinel_content(&path)).is_err() {
                continue;
            }
            self.junk.push(JunkFile {
                path,
                rule: rule.clone(),
            });
            created += 1;
        }
        created
    }

    /// Every file under the `TempDir`, with its bytes. Junctions are never
    /// traversed: their targets are enumerated once, under their real name.
    pub fn snapshot(&self) -> HashMap<PathBuf, Vec<u8>> {
        let mut out = HashMap::new();
        let walk = WalkDir::new(&self.base)
            .follow_links(false)
            .into_iter()
            .filter_entry(|e| !(e.depth() > 0 && e.file_type().is_dir() && is_reparse(e.path())));
        for entry in walk.filter_map(|e| e.ok()) {
            if !entry.file_type().is_file() {
                continue;
            }
            if let Ok(bytes) = std::fs::read(entry.path()) {
                out.insert(entry.path().to_path_buf(), bytes);
            }
        }
        out
    }
}

/// Turns a resolved rule pattern into one concrete path it matches.
///
/// The pattern is in Windows form and carries `globset::escape` sequences
/// (`[*]`, `[[]`, ...) around the metacharacters of the injected variable
/// values, which must come back as literals. Wildcards are replaced by names
/// carrying a `w2` marker so a materialised path can never collide with a
/// hand-written sentinel name.
fn concrete_path(pattern: &str) -> Option<PathBuf> {
    let mut out = PathBuf::new();
    for (i, seg) in pattern.split('\\').enumerate() {
        if seg == "**" {
            out.push("w2deep");
            continue;
        }
        let literal = concrete_segment(seg)?;
        if literal.is_empty() {
            return None;
        }
        if i == 0 {
            // Drive letter, kept verbatim so `PathBuf` builds an absolute path.
            out.push(format!("{literal}\\"));
        } else {
            out.push(literal);
        }
    }
    Some(out)
}

const ESCAPABLE: [char; 6] = ['?', '*', '[', ']', '{', '}'];

fn concrete_segment(seg: &str) -> Option<String> {
    let mut out = String::new();
    let mut chars = seg.chars().peekable();
    while let Some(c) = chars.next() {
        match c {
            '[' => {
                // `globset::escape` output: `[x]` where x is a metacharacter.
                let mut rest = chars.clone();
                match (rest.next(), rest.next()) {
                    (Some(lit), Some(']')) if ESCAPABLE.contains(&lit) => {
                        out.push(lit);
                        chars.next();
                        chars.next();
                    }
                    // A real character class: not guessed at.
                    _ => return None,
                }
            }
            '*' => out.push_str("w2"),
            '?' => out.push('w'),
            '{' => return None,
            c => out.push(c),
        }
    }
    // Windows refuses a trailing dot or space in a file name.
    let trimmed = out.trim_end_matches([' ', '.']).to_string();
    if trimmed.is_empty() {
        None
    } else {
        Some(trimmed)
    }
}

// ---------------------------------------------------------------------------
// Sandbox mode: what the fixture becomes once the application, and not the
// harness, is the one driving it.
// ---------------------------------------------------------------------------

/// One file that must survive, and the exact bytes it must still carry. The
/// marker is derived from the path (`Fixture::sentinel_content`), so a silent
/// rewrite is caught as well as a deletion.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ManifestSentinel {
    pub path: PathBuf,
    pub marker: String,
}

/// What the sandbox promised to build, kept so `verify` can compare it with
/// what is on disk after a clean. Serialisable: paths cross as strings.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct SandboxManifest {
    pub root: PathBuf,
    pub profile_dir: PathBuf,
    pub sentinels: Vec<ManifestSentinel>,
    /// Every junk file, with the rule it belongs to: the verdict counts only
    /// the entries of the rules that were actually cleaned.
    pub junk: Vec<JunkFile>,
    /// The sentinels reachable ONLY by crossing a junction the sandbox plants
    /// — two of them. The other files outside the fake profile (the junction
    /// targets' own content, the control directory) are ordinary sentinels;
    /// the harness-only junctions of `outside3` / `outside4` are never planted
    /// in Sandbox mode, so their files are not baits here either.
    pub outside: Vec<PathBuf>,
    /// The junction links themselves (not their targets).
    pub junctions: Vec<PathBuf>,
}

/// What the front end is told when a sandbox opens, and while it is active.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct SandboxSummary {
    pub root: String,
    pub sentinels: u32,
    pub junk: u32,
    pub winapp2_rules: u32,
}

/// The verdict a user reads after cleaning the sandbox: what survived, what
/// went, and whether the indirections were refused.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct SandboxVerdict {
    pub sentinels_total: u32,
    pub sentinels_intact: u32,
    /// Paths of the sentinels that were deleted or rewritten. Empty is the
    /// only acceptable answer.
    pub sentinels_damaged: Vec<String>,
    /// Junk **of the rules that were cleaned**, and nothing else: a user who
    /// cleans nine of eighty rules is told about the junk of those nine, not
    /// held to the whole catalogue.
    pub junk_total: u32,
    pub junk_removed: u32,
    pub junk_remaining: Vec<String>,
    /// How many rules the counts above are scoped to.
    pub rules_cleaned: u32,
    /// The junction baits: files that a rule's glob does match, but that can
    /// only be reached by crossing a junction. Untouched is the only
    /// acceptable answer.
    pub outside_total: u32,
    pub outside_intact: u32,
    /// Every junction link still stands and every file behind one is intact:
    /// the walk refused to cross them rather than following them out.
    pub junctions_refused: bool,
}

impl Fixture {
    /// The promise this fixture makes, frozen before anything is cleaned.
    pub fn manifest(&self) -> SandboxManifest {
        let sentinels: Vec<ManifestSentinel> = self
            .sentinels
            .iter()
            .map(|s| ManifestSentinel {
                path: s.path.clone(),
                marker: String::from_utf8_lossy(&Fixture::sentinel_content(&s.path)).into_owned(),
            })
            .collect();
        SandboxManifest {
            root: self.base.clone(),
            profile_dir: self.profile.clone(),
            sentinels,
            junk: self.junk.clone(),
            outside: self.junction_baits.clone(),
            junctions: self.junctions.clone(),
        }
    }
}

/// Compares the disk with the manifest. Pure over `(manifest, cleaned, disk)`:
/// nothing here is remembered from the run that did the cleaning.
///
/// `cleaned` is the list of rule ids the user actually cleaned. The sentinels
/// and the junction baits are held to the whole fixture — no selection can
/// license damaging a file — but the junk counts are scoped to those rules:
/// holding a partial selection to the whole catalogue would paint a correct
/// run red for the only reason that the user did not tick every box.
pub fn verify(manifest: &SandboxManifest, cleaned: &[String]) -> SandboxVerdict {
    let intact =
        |s: &ManifestSentinel| std::fs::read(&s.path).is_ok_and(|b| b == s.marker.as_bytes());
    let damaged: Vec<String> = manifest
        .sentinels
        .iter()
        .filter(|s| !intact(s))
        .map(|s| s.path.display().to_string())
        .collect();

    let scope: BTreeSet<&str> = cleaned.iter().map(|s| s.as_str()).collect();
    let in_scope: Vec<&JunkFile> = manifest
        .junk
        .iter()
        .filter(|j| scope.contains(j.rule.as_str()))
        .collect();
    let remaining: Vec<String> = in_scope
        .iter()
        .filter(|j| j.path.exists())
        .map(|j| j.path.display().to_string())
        .collect();

    let outside_ok = manifest
        .sentinels
        .iter()
        .filter(|s| manifest.outside.contains(&s.path))
        .filter(|s| intact(s))
        .count() as u32;
    let junctions_refused =
        manifest.junctions.iter().all(|j| j.exists()) && outside_ok == manifest.outside.len() as u32;

    SandboxVerdict {
        sentinels_total: manifest.sentinels.len() as u32,
        sentinels_intact: manifest.sentinels.len() as u32 - damaged.len() as u32,
        sentinels_damaged: damaged,
        junk_total: in_scope.len() as u32,
        junk_removed: in_scope.len() as u32 - remaining.len() as u32,
        junk_remaining: remaining,
        rules_cleaned: scope.len() as u32,
        outside_total: manifest.outside.len() as u32,
        outside_intact: outside_ok,
        junctions_refused,
    }
}

/// The native rules, loaded against the fixture's variables through the very
/// loader the application uses.
pub fn native_rules(fx: &Fixture) -> Result<Vec<Rule>, String> {
    let lookup = |n: &str| fx.lookup(n);
    load_rules_with(RULES_TOML, &lookup).map_err(|e| e.to_string())
}

/// The native rules plus the Winapp2 entries this fixture makes "detected".
///
/// The registry probe always answers `false`: detection is decided by the files
/// the fixture created, never by what happens to be installed on the machine.
/// The safety harness and the Sandbox mode share this one builder, so what a
/// user watches is what CI proves.
pub fn full_catalogue(fx: &Fixture) -> Result<(Vec<Rule>, Vec<Rule>, ConversionReport), String> {
    let native = native_rules(fx)?;
    let raw = |n: &str| fx.lookup(n);
    let memo = memoized_env(&raw);
    let (converted, report) = convert_with(WINAPP2_INI, &native, &memo);
    let file = |p: &str| detect_file_exists_with(p, &memo);
    let winapp2 = detected_rules_with(converted, &|_| false, &file);
    Ok((native, winapp2, report))
}

/// Every path the given rules resolve to against this fixture, paired with the
/// id of the rule it came from, so `add_winapp2_junk` can materialise one
/// concrete file per converted glob and attribute it to its rule.
pub fn resolved_patterns(fx: &Fixture, rules: &[Rule]) -> Result<Vec<(String, String)>, String> {
    let lookup: EnvLookup = &|n: &str| fx.lookup(n);
    let mut out = Vec::new();
    for rule in rules {
        for pattern in crate::rules::resolved_paths_with(rule, lookup).map_err(|e| e.to_string())? {
            out.push((rule.id.clone(), pattern));
        }
    }
    Ok(out)
}

/// The sandbox's stand-in for `SHQueryRecycleBinW`. The real bin is never
/// queried and never emptied while a sandbox is active: the
/// `windows.recycle-bin` rule reports nothing rather than reporting the user's
/// own bin.
pub fn sandbox_recycle_query() -> Result<(u64, u64), String> {
    Ok((0, 0))
}

/// The sandbox's stand-in for `SHEmptyRecycleBinW`. Does nothing, on purpose.
pub fn sandbox_recycle_empty() -> Result<(), String> {
    Ok(())
}

/// Where `Trash` mode puts a sandbox file, under the sandbox root.
pub const SANDBOX_BIN: &str = "recycle-bin";

/// `Trash` mode inside the sandbox. The file is **moved** into
/// `<root>\recycle-bin`, never handed to `trash::delete`: the real Recycle Bin
/// of the volume stays out of reach, and the mode still means what it says —
/// the file leaves its place and stays recoverable, which is what a user
/// checking the cautious mode wants to see.
///
/// The destination name is the path relative to the root with its separators
/// flattened, so two files sharing a base name cannot collide. Two *runs* can
/// still land on the same flattened name — the same rule cleaned twice on a
/// re-created file — so an existing destination gets a `~1`, `~2`... suffix
/// rather than being silently overwritten: the bin is supposed to be
/// recoverable. `recycle-bin` sits beside `profile`, not under it, so no rule
/// can ever name what landed there and the verdict cannot mistake it for a
/// survivor.
pub fn sandbox_trash(root: &Path, path: &Path) -> Result<(), String> {
    // `clean::deletable_path` hands its caller the canonical path, verbatim
    // `\\?\` prefix included; the root is a plain path. Reconcile the two forms
    // before comparing, or every file is refused as "outside the sandbox root".
    let path = &strip_verbatim_str(path);
    let relative = path
        .strip_prefix(root)
        .map_err(|_| format!("outside the sandbox root: {}", path.display()))?;
    let flat: String = relative
        .to_string_lossy()
        .chars()
        .map(|c| if c == '\\' || c == '/' || c == ':' { '_' } else { c })
        .collect();
    let bin = root.join(SANDBOX_BIN);
    std::fs::create_dir_all(&bin).map_err(|e| e.to_string())?;
    std::fs::rename(path, free_name(&bin, &flat)).map_err(|e| e.to_string())
}

/// `<bin>\<flat>`, or the first `<bin>\<flat>~n` that does not exist yet.
/// Bounded: after a thousand collisions the last name is returned and `rename`
/// decides — an unbounded loop here would hang a clean instead of failing it.
fn free_name(bin: &Path, flat: &str) -> PathBuf {
    let mut candidate = bin.join(flat);
    for n in 1..1000 {
        if !candidate.exists() {
            break;
        }
        candidate = bin.join(format!("{flat}~{n}"));
    }
    candidate
}

// ---------------------------------------------------------------------------
// Orphaned sandboxes.
//
// `leave_sandbox` removes the tree, but nothing removes it when the process
// never reaches that call: a crash, a kill from the Task Manager, a window
// closed while a sandbox is active. The directory then stays in `%TEMP%` with
// its few hundred files, and nothing in the application ever mentions it
// again. The two functions below are what finds those leftovers and removes
// them — from the startup sweep (`lib.rs`) and from Settings.
// ---------------------------------------------------------------------------

/// The name every sandbox root starts with, under `%TEMP%`. The rest is
/// `commands::sandbox_token`: the process id, the nanoseconds since the epoch
/// and a counter, all in hexadecimal, separated by `-`.
pub const SANDBOX_PREFIX: &str = "wincleaner-sandbox-";

/// A sandbox directory left in `%TEMP%` by a process that is no longer
/// running. `size_bytes` and `files` are what a sweep would reclaim: neither
/// counts anything behind a junction.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Orphan {
    pub path: PathBuf,
    pub size_bytes: u64,
    pub files: u32,
}

/// The process id a sandbox directory name carries, or `None` when the name
/// is not one of ours. Hexadecimal, because that is how `sandbox_token`
/// writes it.
fn pid_of_sandbox_dir(name: &str) -> Option<u32> {
    let token = name.strip_prefix(SANDBOX_PREFIX)?;
    let (pid, rest) = token.split_once('-')?;
    // A bare `wincleaner-sandbox-<pid>` with nothing after it is not a name
    // this application produces; refusing it keeps the sweep off anything a
    // third party happens to have put there under a similar name.
    if rest.is_empty() {
        return None;
    }
    u32::from_str_radix(pid, 16).ok()
}

/// Adds up the ordinary files under `dir`, never crossing a reparse point.
/// `symlink_metadata` throughout: the size of a junction is the size of its
/// link, and what lives on the other side is none of this application's
/// business — it is not what a sweep would reclaim either, since
/// `remove_orphan` unlinks the junction instead of descending into it.
fn measure_tree(dir: &Path, size: &mut u64, files: &mut u32) {
    let Ok(entries) = std::fs::read_dir(dir) else {
        return;
    };
    for entry in entries.flatten() {
        let path = entry.path();
        let Ok(md) = std::fs::symlink_metadata(&path) else {
            continue;
        };
        if is_reparse(&path) {
            continue;
        }
        if md.is_dir() {
            measure_tree(&path, size, files);
        } else {
            *size += md.len();
            *files += 1;
        }
    }
}

/// Every `wincleaner-sandbox-*` directory sitting directly under `temp_dir`
/// whose owning process is gone.
///
/// Four kinds of directory are deliberately left alone:
///
/// * anything not named `wincleaner-sandbox-<hex pid>-…` — not ours;
/// * a directory whose pid is `current_pid` — this very process, whose own
///   sandbox is `active_root` and whose half-built roots `enter_sandbox`
///   already cleans up;
/// * a directory whose pid `is_running` answers `true` for — a second
///   WinCleaner is using it right now. A pid can be recycled, so this errs
///   towards keeping a stranger's directory rather than removing a live one;
/// * `active_root` itself, named explicitly rather than inferred from the pid,
///   so the guarantee holds whatever the name turns out to be.
///
/// A reparse point named like a sandbox root is skipped too: `enter_sandbox`
/// refuses to build on one, so it cannot be ours, and measuring it would mean
/// reading a tree that belongs to someone else.
pub fn orphaned_sandboxes(
    temp_dir: &Path,
    current_pid: u32,
    active_root: Option<&Path>,
    is_running: &dyn Fn(u32) -> bool,
) -> Vec<Orphan> {
    let Ok(entries) = std::fs::read_dir(temp_dir) else {
        return Vec::new();
    };
    let mut orphans = Vec::new();
    for entry in entries.flatten() {
        let path = entry.path();
        let name = entry.file_name().to_string_lossy().into_owned();
        let Some(pid) = pid_of_sandbox_dir(&name) else {
            continue;
        };
        if pid == current_pid || Some(path.as_path()) == active_root {
            continue;
        }
        if !std::fs::symlink_metadata(&path).is_ok_and(|md| md.is_dir()) || is_reparse(&path) {
            continue;
        }
        if is_running(pid) {
            continue;
        }
        let (mut size_bytes, mut files) = (0, 0);
        measure_tree(&path, &mut size_bytes, &mut files);
        orphans.push(Orphan {
            path,
            size_bytes,
            files,
        });
    }
    orphans.sort_by(|a, b| a.path.cmp(&b.path));
    orphans
}

/// Unlinks every reparse point under `dir`, depth first. `remove_dir` on the
/// link, never `remove_dir_all`: the whole point is not to reach what is on
/// the other side.
fn unlink_reparse_points(dir: &Path) {
    let Ok(entries) = std::fs::read_dir(dir) else {
        return;
    };
    for entry in entries.flatten() {
        let path = entry.path();
        if is_reparse(&path) {
            let _ = std::fs::remove_dir(&path).or_else(|_| std::fs::remove_file(&path));
        } else if std::fs::symlink_metadata(&path).is_ok_and(|md| md.is_dir()) {
            unlink_reparse_points(&path);
        }
    }
}

/// Removes one orphaned sandbox, the same way `commands::leave_sandbox`
/// removes a live one: the junctions are unlinked **first**, because
/// `remove_dir_all` is free to descend into a junction and delete what lives
/// on the other side.
///
/// Unlike `leave_sandbox`, there is no manifest to read the junction list
/// from — the process that wrote it is gone — so the tree is walked for
/// reparse points instead.
///
/// `path` is checked rather than trusted: a direct child of `temp_dir`,
/// carrying the sandbox prefix, and nothing else. No caller passes a path that
/// did not come out of `orphaned_sandboxes`, but this function deletes a
/// directory tree and the check costs two comparisons.
pub fn remove_orphan(temp_dir: &Path, path: &Path) -> Result<(), String> {
    let refused = |why: &str| Err(format!("Refused to remove {}: {why}.", path.display()));
    if path.parent() != Some(temp_dir) {
        return refused("it is not directly under the temporary directory");
    }
    let name = path.file_name().unwrap_or_default().to_string_lossy();
    if !name.starts_with(SANDBOX_PREFIX) {
        return refused("its name is not a sandbox name");
    }
    if is_reparse(path) {
        // `orphaned_sandboxes` never lists one, but the root can be swapped
        // for a junction between the listing and this call — the same window
        // `clean::deletable_path` guards. Unlink it, never descend:
        // `remove_dir` on a junction leaves its target alone.
        return std::fs::remove_dir(path).map_err(|e| format!("{}: {e}", path.display()));
    }
    unlink_reparse_points(path);
    std::fs::remove_dir_all(path).map_err(|e| format!("{}: {e}", path.display()))
}

#[cfg(test)]
mod tests {
    use super::*;
    use tempfile::TempDir;

    /// Nothing here touches the real `%TEMP%`: every path is built inside a
    /// `TempDir` that stands in for it.
    fn fake_temp() -> TempDir {
        TempDir::new().expect("temp dir")
    }

    /// A sandbox-shaped directory holding one file, named the way
    /// `sandbox_token` names one: hexadecimal pid, nanoseconds, counter.
    fn sandbox_dir(temp: &Path, pid: u32) -> PathBuf {
        let dir = temp.join(format!("{SANDBOX_PREFIX}{pid:x}-18f2c-0"));
        std::fs::create_dir(&dir).expect("create sandbox dir");
        std::fs::write(dir.join("junk.tmp"), b"0123456789").expect("write junk");
        dir
    }

    /// The pid of a process that cannot be running: above the Windows maximum,
    /// and answered `false` by the injected check in any case.
    const DEAD_PID: u32 = 999_999;
    const LIVE_PID: u32 = 4242;

    fn nothing_runs(_: u32) -> bool {
        false
    }

    #[test]
    fn a_directory_whose_process_is_gone_is_an_orphan_and_the_others_are_left_alone() {
        let temp = fake_temp();
        let orphan = sandbox_dir(temp.path(), DEAD_PID);
        let mine = sandbox_dir(temp.path(), LIVE_PID);
        let active = temp.path().join(format!("{SANDBOX_PREFIX}beef-1-0"));
        std::fs::create_dir(&active).unwrap();
        std::fs::create_dir(temp.path().join("unrelated-folder")).unwrap();
        // A name that carries the prefix but no token: not one of ours.
        std::fs::create_dir(temp.path().join(format!("{SANDBOX_PREFIX}notahex"))).unwrap();

        let found = orphaned_sandboxes(temp.path(), LIVE_PID, Some(&active), &nothing_runs);

        assert_eq!(
            found.iter().map(|o| o.path.clone()).collect::<Vec<_>>(),
            vec![orphan],
            "only the dead process's directory is an orphan"
        );
        assert_eq!(found[0].files, 1);
        assert_eq!(found[0].size_bytes, 10);
        assert!(mine.exists() && active.exists(), "listing removes nothing");
    }

    #[test]
    fn a_directory_whose_process_is_still_running_is_kept() {
        let temp = fake_temp();
        sandbox_dir(temp.path(), DEAD_PID);
        let running = |pid: u32| pid == DEAD_PID;

        assert!(
            orphaned_sandboxes(temp.path(), LIVE_PID, None, &running).is_empty(),
            "a live owner keeps its sandbox"
        );
    }

    #[test]
    fn sizing_stops_at_a_junction_instead_of_counting_what_is_behind_it() {
        let temp = fake_temp();
        let orphan = sandbox_dir(temp.path(), DEAD_PID);
        let outside = temp.path().join("outside");
        std::fs::create_dir(&outside).unwrap();
        std::fs::write(outside.join("secret.txt"), vec![b'x'; 4096]).unwrap();
        let link = orphan.join("link");
        if junction(&link, &outside).is_err() {
            // No reparse-point support on this volume: the guard cannot be
            // exercised, and asserting on a junction that was never created
            // would assert nothing.
            return;
        }

        let found = orphaned_sandboxes(temp.path(), LIVE_PID, None, &nothing_runs);

        assert_eq!(found.len(), 1);
        assert_eq!(found[0].files, 1, "only the file on this side is counted");
        assert_eq!(found[0].size_bytes, 10);
    }

    #[test]
    fn removing_an_orphan_unlinks_a_junction_without_touching_its_target() {
        let temp = fake_temp();
        let orphan = sandbox_dir(temp.path(), DEAD_PID);
        let outside = temp.path().join("outside");
        std::fs::create_dir(&outside).unwrap();
        let secret = outside.join("secret.txt");
        std::fs::write(&secret, b"keep me").unwrap();
        let nested = orphan.join("nested");
        std::fs::create_dir(&nested).unwrap();
        if junction(&nested.join("link"), &outside).is_err() {
            return;
        }

        remove_orphan(temp.path(), &orphan).expect("the orphan is removed");

        assert!(!orphan.exists(), "the sandbox tree is gone");
        assert!(outside.exists(), "the junction target survives");
        assert_eq!(std::fs::read(&secret).unwrap(), b"keep me");
    }

    #[test]
    fn removal_refuses_a_path_outside_the_temporary_directory() {
        let temp = fake_temp();
        let elsewhere = fake_temp();
        let victim = elsewhere.path().join(format!("{SANDBOX_PREFIX}f-1-0"));
        std::fs::create_dir(&victim).unwrap();

        let err = remove_orphan(temp.path(), &victim).unwrap_err();

        assert!(err.contains("not directly under"), "{err}");
        assert!(victim.exists(), "the refusal deleted nothing");
    }

    #[test]
    fn removal_refuses_a_directory_that_is_not_named_like_a_sandbox() {
        let temp = fake_temp();
        let victim = temp.path().join("Documents");
        std::fs::create_dir(&victim).unwrap();

        let err = remove_orphan(temp.path(), &victim).unwrap_err();

        assert!(err.contains("not a sandbox name"), "{err}");
        assert!(victim.exists(), "the refusal deleted nothing");
    }

    /// A sub-directory of a sandbox root is not a sandbox root: the prefix
    /// check alone would let `<root>\profile` through, the parent check is
    /// what stops it.
    #[test]
    fn removal_refuses_a_grandchild_of_the_temporary_directory() {
        let temp = fake_temp();
        let orphan = sandbox_dir(temp.path(), DEAD_PID);
        let inner = orphan.join(format!("{SANDBOX_PREFIX}f-1-0"));
        std::fs::create_dir(&inner).unwrap();

        assert!(remove_orphan(temp.path(), &inner).is_err());
        assert!(inner.exists());
    }
}
