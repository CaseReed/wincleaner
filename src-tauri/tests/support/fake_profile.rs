//! Synthetic Windows profile for the safety harness.
//!
//! Everything lives under one `TempDir`:
//!
//! ```text
//! <base>\profile\        the fake %USERPROFILE% (and %LOCALAPPDATA%, %APPDATA%, %TEMP%)
//! <base>\outside\        target of the junction planted in %TEMP%
//! <base>\outside2\       target of the junction planted in the Chrome cache
//! <base>\control\        never named by any rule; the snapshot proves it is untouched
//! ```
//!
//! Two families of files, both written with a content marker derived from the
//! path itself so a silent rewrite is caught as well as a deletion:
//!
//! * JUNK — every file a rule is expected to remove.
//! * SENTINELS — every file that must survive, whatever rule is selected.

use std::collections::{BTreeSet, HashMap};
use std::path::{Path, PathBuf};
use tempfile::TempDir;
use walkdir::WalkDir;

#[derive(Debug, Clone)]
pub struct Sentinel {
    pub path: PathBuf,
    /// Why this file must survive. Printed verbatim when it does not.
    pub why: &'static str,
}

pub struct Fixture {
    pub base: PathBuf,
    pub profile: PathBuf,
    pub outside: PathBuf,
    pub outside2: PathBuf,
    pub junk: Vec<PathBuf>,
    pub sentinels: Vec<Sentinel>,
    /// The junction links themselves (not their targets).
    pub junctions: Vec<PathBuf>,
    vars: HashMap<String, String>,
    _dir: TempDir,
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

/// Creates a directory junction. `mklink /J` needs no privilege, unlike
/// `mklink /D`: the harness runs unelevated.
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

impl Fixture {
    /// Content every file of the fixture carries. Derived from the path so it
    /// can be recomputed at assertion time without keeping a second table.
    pub fn sentinel_content(path: &Path) -> Vec<u8> {
        format!("wincleaner-safety-harness::{}", path.display()).into_bytes()
    }

    pub fn strip_verbatim(path: &Path) -> PathBuf {
        strip_verbatim_str(path)
    }

    fn write(path: &Path) {
        std::fs::create_dir_all(path.parent().unwrap()).unwrap();
        std::fs::write(path, Fixture::sentinel_content(path)).unwrap();
    }

    fn junk_file(&mut self, rel: &str) {
        let path = self.profile.join(rel);
        Self::write(&path);
        self.junk.push(path);
    }

    fn sentinel(&mut self, path: PathBuf, why: &'static str) {
        Self::write(&path);
        self.sentinels.push(Sentinel { path, why });
    }

    fn profile_sentinel(&mut self, rel: &str, why: &'static str) {
        let path = self.profile.join(rel);
        self.sentinel(path, why);
    }

    pub fn lookup(&self, name: &str) -> Option<String> {
        self.vars.get(name).cloned()
    }

    pub fn build() -> Self {
        let dir = TempDir::new().unwrap();
        // Canonicalised so the injected variables and what `canonicalize`
        // returns during the walk describe the same directory: `TempDir` sits
        // under `%TMP%`, which Windows may hand out in 8.3 short form.
        let base = strip_verbatim_str(&std::fs::canonicalize(dir.path()).unwrap());
        let profile = base.join("profile");
        let local = profile.join("AppData").join("Local");
        let roaming = profile.join("AppData").join("Roaming");
        let temp = local.join("Temp");
        std::fs::create_dir_all(&temp).unwrap();
        std::fs::create_dir_all(&roaming).unwrap();

        let vars = HashMap::from([
            ("USERPROFILE".to_string(), profile.display().to_string()),
            ("LOCALAPPDATA".to_string(), local.display().to_string()),
            ("APPDATA".to_string(), roaming.display().to_string()),
            ("TEMP".to_string(), temp.display().to_string()),
        ]);

        let mut fx = Fixture {
            outside: base.join("outside"),
            outside2: base.join("outside2"),
            base,
            profile,
            junk: Vec::new(),
            sentinels: Vec::new(),
            junctions: Vec::new(),
            vars,
            _dir: dir,
        };

        fx.populate_junk();
        fx.populate_sentinels();
        fx.populate_detect_files();
        fx.populate_junctions();
        fx
    }

    /// One or more files for EVERY `kind = "files"` rule of `rules.toml`.
    /// A new rule with no entry here fails the harness (the scan finds nothing
    /// for it), which is the point.
    fn populate_junk(&mut self) {
        // windows.temp — `%TEMP%\**\*`, recursive.
        self.junk_file(r"AppData\Local\Temp\stray.tmp");
        self.junk_file(r"AppData\Local\Temp\nested\installer.log");
        self.junk_file(r"AppData\Local\Temp\nested\deeper\chunk.bin");

        // windows.thumbnails — non-recursive, two prefixes.
        self.junk_file(r"AppData\Local\Microsoft\Windows\Explorer\thumbcache_1024.db");
        self.junk_file(r"AppData\Local\Microsoft\Windows\Explorer\iconcache_32.db");

        // windows.explorer-recent — `*.lnk` (non-recursive) + AutomaticDestinations.
        self.junk_file(r"AppData\Roaming\Microsoft\Windows\Recent\report.lnk");
        self.junk_file(
            r"AppData\Roaming\Microsoft\Windows\Recent\AutomaticDestinations\1b4dd67f29cb1962.automaticDestinations-ms",
        );

        // windows.crash-dumps
        self.junk_file(r"AppData\Local\CrashDumps\wincleaner.exe.4242.dmp");

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
            self.junk_file(&format!(r"AppData\Local\Microsoft\Edge\User Data\{rel}"));
        }
        self.junk_file(r"AppData\Local\Microsoft\Edge\User Data\ShaderCache\GPUCache\data_1");

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
            self.junk_file(&format!(r"AppData\Local\Google\Chrome\User Data\{rel}"));
        }
        self.junk_file(r"AppData\Local\Google\Chrome\User Data\ShaderCache\GPUCache\data_1");

        // firefox.cache
        self.junk_file(
            r"AppData\Local\Mozilla\Firefox\Profiles\a1b2c3d4.default-release\cache2\entries\4F2A",
        );
        self.junk_file(
            r"AppData\Local\Mozilla\Firefox\Profiles\a1b2c3d4.default-release\startupCache\startupCache.8.little",
        );

        // npm.cache
        self.junk_file(r"AppData\Local\npm-cache\_cacache\content-v2\sha512\ab\cd\ef01");
    }

    fn populate_sentinels(&mut self) {
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
            self.profile_sentinel(rel, why);
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
            self.profile_sentinel(&format!(r"{ff}\{name}"), why);
        }
        self.profile_sentinel(
            &format!(r"{ff}\extensions\ublock@raymondhill.net.xpi"),
            "installed extension",
        );

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
            self.profile_sentinel(&rel, why);
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
                self.profile_sentinel(&format!(r"{browser}\Default\{name}"), why);
            }
        }

        // npm's own configuration sits next to its cache.
        self.profile_sentinel(r".npmrc", "npm configuration");

        // Pinned taskbar items: the rule stops at Recent\*.lnk on purpose.
        self.profile_sentinel(
            r"AppData\Roaming\Microsoft\Windows\Recent\CustomDestinations\pinned.customDestinations-ms",
            "items the user pinned by hand",
        );

        // (d) Reachable only through a junction, and (e) the control directory
        // no rule ever names.
        let outside = self.outside.join("secret.txt");
        self.sentinel(outside, "outside the profile, behind a junction");
        let outside2 = self.outside2.join("also-secret.txt");
        self.sentinel(
            outside2,
            "outside the profile, behind a junction planted in a cache",
        );
        // Bait: reached through the %TEMP% junction, this name matches
        // `%TEMP%\**\*`; reached through the Chrome cache junction, that one
        // matches `...\Cache\**\*`. They survive only because the walk refuses
        // to cross the junction at all.
        let bait = self.outside.join("bait.tmp");
        self.sentinel(
            bait,
            "would match %TEMP%\\**\\* if the junction were traversed",
        );
        let bait2 = self.outside2.join("Cache_Data").join("f_000009");
        self.sentinel(
            bait2,
            "would match the Chrome cache glob if the junction were traversed",
        );
        let control = self.base.join("control").join("untouched.dat");
        self.sentinel(control, "control directory, named by no rule");
    }

    /// `DetectFile` targets, so a sample of Winapp2 entries becomes "detected"
    /// against this tree and nothing else. The Chromium and Edge entries are
    /// detected as a side effect of the browser cache fixtures above
    /// (`DetectFile=%LocalAppData%\Google\Chrome*`).
    fn populate_detect_files(&mut self) {
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
            std::fs::create_dir_all(self.profile.join(rel)).unwrap();
        }
    }

    /// Two junctions pointing outside the profile: one on the `%TEMP%` walk,
    /// one buried inside the Chrome cache. `mklink /J` needs no privilege,
    /// which is exactly why the containment guards exist.
    fn populate_junctions(&mut self) {
        let temp_link = self.profile.join(r"AppData\Local\Temp\linked");
        junction(&temp_link, &self.outside);
        self.junctions.push(temp_link);

        let cache_link = self
            .profile
            .join(r"AppData\Local\Google\Chrome\User Data\Default\Cache\link");
        junction(&cache_link, &self.outside2);
        self.junctions.push(cache_link);
    }

    /// One concrete file per converted Winapp2 glob, so those rules delete
    /// something instead of walking empty trees. A pattern whose literal form
    /// cannot be derived (a `{a,b}` alternation) is skipped rather than
    /// guessed at, and a path that would collide with an existing file is
    /// never overwritten.
    pub fn add_winapp2_junk(&mut self, patterns: &[String]) {
        // Windows file names are case-insensitive and Winapp2 spells the same
        // directory several ways (`DropBox` and `Dropbox`): the bookkeeping has
        // to be case-insensitive too, or the same file would be created twice
        // and counted once.
        let key = |p: &Path| p.to_string_lossy().to_lowercase();
        let mut known: BTreeSet<String> = self
            .junk
            .iter()
            .chain(self.sentinels.iter().map(|s| &s.path))
            .chain(self.junctions.iter())
            .map(|p| key(p))
            .collect();

        for pattern in patterns {
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
            self.junk.push(path);
        }
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
