# WinCleaner MVP — Design

Working name: `wincleaner` (renamable). License: MIT.

## Goal

An open source Windows cleaner, modern, telemetry-free, covering the essentials
of CCleaner: cleaning up junk files and managing startup programs. The MVP runs
without administrator rights and only acts within the user profile.

Out of scope for the MVP: registry cleaner (never), uninstaller, scheduling,
notification-area icon, UAC elevation.

The winapp2.ini import, out of scope for the MVP, was designed and implemented
afterwards: see [`design-winapp2.md`](design-winapp2.md). The rules it produces
are ordinary `Rule` values and go through the validation, containment and
deletion guards described below without exception.

## Stack

- Tauri 2, Rust back end (edition 2021, stable).
- Front end: React + TypeScript + Tailwind + shadcn/ui, Vite bundler.
- Crates: `tauri`, `serde`, `toml`, `walkdir`, `globset`, `trash`,
  `windows-registry` (or `winreg`), `sysinfo` (detecting open browsers).

## Layout

```
wincleaner/
├── src-tauri/
│   ├── src/
│   │   ├── main.rs       registers the Tauri commands
│   │   ├── rules.rs      loading + validation of rules.toml
│   │   ├── scan.rs       glob resolution, walking, sizes
│   │   ├── clean.rs      recycle-bin or permanent deletion
│   │   ├── commands.rs   Tauri command surface (scan/clean/startup)
│   │   └── startup.rs    startup manager
│   └── rules.toml        embedded via include_str!
└── src/                  React
```

## Cleaning rules (rules.toml)

```toml
[[rule]]
id = "windows.temp"          # unique, kebab/dot
category = "System"
label = "Temporary files"
paths = ["%TEMP%\**\*"]    # globs, Windows environment variables
exclude = []                 # excluded globs
risk = "low"                 # low | medium
default_checked = true       # unchecked for anything irreversible or hand-curated
```

Validation at load time (failure = the app refuses to start, with a clear
message):
- each path starts with one of `%TEMP%`, `%LOCALAPPDATA%`, `%APPDATA%`,
  `%USERPROFILE%`;
- after resolution, the path is under `%USERPROFILE%` (canonicalized, no
  `..`);
- variable values go through `globset::escape` before being embedded in a
  glob (a `[` in an account name previously escaped the profile);
- `id` unique, `risk` within the enumeration.

The eight current rules, all within the user profile:

| Category | Rule id | Label | Default | Risk |
| --- | --- | --- | --- | --- |
| System | `windows.temp` | Temporary files (`%TEMP%`) | Checked | Low |
| System | `windows.recycle-bin` | Recycle Bin | **Unchecked** | Low |
| System | `windows.thumbnails` | Thumbnail cache | Checked | Low |
| System | `windows.explorer-recent` | Recent items | **Unchecked** | Medium |
| System | `windows.crash-dumps` | Crash dumps (`%LOCALAPPDATA%\CrashDumps`) | **Unchecked** | Medium |
| Browsers | `edge.cache` | Microsoft Edge cache | Checked | Low |
| Browsers | `chrome.cache` | Google Chrome cache | Checked | Low |
| Browsers | `firefox.cache` | Mozilla Firefox cache (multiple profiles via the `Profiles\*\cache2` glob) | Checked | Low |

`windows.crash-dumps` replaced an earlier `windows.user-logs` rule that also
listed `%LOCALAPPDATA%\Temp\*.log` — a strict subset of `windows.temp` on a
standard install, so it double-counted bytes and produced bogus "skipped"
entries in the report.

The browser rules cover HTTP, code, GPU and service-worker caches only.
`Service Worker\Database`, Local Storage, IndexedDB and Session Storage are
deliberately left alone: they hold application data, and clearing them breaks
offline mode for installed web apps.

`default_checked = false` is set in `rules.toml` for anything irreversible or
hand-curated: recycle bin, recent items, crash dumps. Whatever the checkbox
state, the Clean button always goes through a confirmation dialog that names
the mode and the irreversible parts of the operation before anything is
deleted.

The Recycle Bin is a special case: emptied via the `SHEmptyRecycleBinW` shell
API, not by walking files. That call empties the recycle bin of **every
volume** on the machine, including outside the user profile, and is always
permanent regardless of the chosen deletion mode. The `windows.recycle-bin`
rule is therefore always cleaned first (`commands.rs::cleaning_order`,
implemented as a stable sort keyed on `RuleKind::RecycleBin`): otherwise it
would permanently destroy what the other rules had just dropped into the bin
during the same "Recycle Bin" pass.

A machine-specific load error (`MissingVar`, `OutsideProfile`) disables the
affected rule (`Rule::unavailable_reason`) without preventing the app from
starting; the front end greys the row out and shows the reason, and the rule
is neither scanned nor cleaned even if its id is sent. Only a structural
`rules.toml` error (duplicate id, invalid risk, malformed TOML) stays fatal at
startup.

## On-disk containment

The textual containment check performed at load time is not sufficient on its
own: it is replayed on disk before every operation that touches the
filesystem.

- `scan.rs::confined_root` refuses any root carrying
  `FILE_ATTRIBUTE_REPARSE_POINT`, or whose `canonicalize` form falls outside
  the canonical profile, before `walkdir` is allowed to descend into it.
  `walkdir` otherwise descends into its root even when that root is a
  junction, and `mklink /J` requires no special privilege to create.
- `clean.rs::deletable_path` requires, right before every deletion, that the
  target be a regular file, not a reparse point, and that it resolve under
  the profile.

Neither guard can see hard links: a hard link is one more name on the same
data, indistinguishable from a regular file to `symlink_metadata` and
`canonicalize`, both of which resolve it under the profile. A hard link
placed under the profile but pointing at data located elsewhere will
therefore be deleted — an accepted limitation, there being no
counter-measure available at no cost.

`%TEMP%` may resolve to an 8.3 short path; `system_env` expands it to its
long form via `GetLongPathNameW` before any containment check runs.

## Tauri commands

- `list_rules() -> Vec<RuleSummary>` — `RuleSummary { id, category, label,
  risk, kind, default_checked, unavailable_reason }`. Never carries a path:
  the front end has no business knowing what will be deleted.
- `scan(rule_ids: Vec<String>) -> Result<Vec<ScanResult>, String>` (`async`)
  `ScanResult { rule_id, file_count, total_bytes, paths: Vec<String>,
  skipped: u32 }`. Locked or refused files: skipped, counted in `skipped`.
  A rule with an `unavailable_reason` short-circuits to a zero-result,
  `skipped = 1` entry instead of being walked.
- `clean(rule_ids, mode: CleanMode) -> Result<CleanReport, String>` (`async`)
  `CleanMode` is `Trash | Permanent | Auto`. Internally re-scans right before
  deleting. `Auto` deletes permanently for `risk = low`, sends to the recycle
  bin for `risk = medium`. `CleanReport { freed_bytes, deleted, skipped:
  Vec<SkippedItem { path, reason }> }`. Rules are processed in
  `cleaning_order` (recycle bin first); a rule that fails or is unavailable
  is recorded as `skipped` without discarding what earlier rules already
  cleaned.
- `running_browsers() -> Vec<String>` — for the open-browser warning banner.
- `list_startup() -> Result<Vec<StartupEntry>, String>` (`async`)
  `StartupEntry { id, name, command, source: "run" | "run-once" | "folder",
  enabled }`.
- `set_startup_enabled(id: String, enabled: bool) -> Result<(), String>`
  (`async`).

`scan`, `clean`, `list_startup` and `set_startup_enabled` are declared
`async` and run their blocking work through `tauri::async_runtime::
spawn_blocking` (the `blocking` helper in `commands.rs`): a synchronous
command runs on the main thread and would freeze the webview event loop for
the seconds a disk walk on a loaded profile can take.

## Startup manager

Sources, all HKCU or profile-scoped:
- `HKCU\Software\Microsoft\Windows\CurrentVersion\Run`; `RunOnce` is
  read-only.
- `%APPDATA%\Microsoft\Windows\Start Menu\Programs\Startup\*.lnk`.
- State lives in
  `HKCU\Software\Microsoft\Windows\CurrentVersion\Explorer\StartupApproved\Run`
  and `\StartupFolder`: a 12-byte binary value, byte 0 = 0x02 enabled, 0x03
  disabled, bytes 4..12 = the FILETIME of the disabling. No value present
  means enabled.

The app only ever writes the enable/disable bit of that blob; it never
deletes a registry value, and it is never a registry cleaner.

## Interface

Single window, sidebar: Cleanup, Startup. Light/dark theme, including the
Windows title bar.
- Cleanup: rules grouped by category with checkboxes, an Analyze button;
  after a scan, size and count per rule, an expandable list of paths, a
  running total, a Clean button with a mode choice (Auto by default). The
  Clean button always opens a confirmation dialog before anything is
  deleted. Final report in a side panel. Warning banner if a targeted
  browser is currently running.
- Startup: table of name, command, source, enabled/disabled toggle.

## Security and errors

- No network access, no telemetry. The CSP is network-free: `default-src
  'self'; connect-src 'self' ipc: http://ipc.localhost; style-src 'self';
  style-src-attr 'unsafe-inline'; object-src/base-uri/frame-ancestors/
  form-action 'none'`. A distinct `devCsp` covers Vite HMR in development
  only. `src/lib/tauri-config.test.ts` fails the build if the policy is
  loosened.
- Tauri capabilities are limited to `core:event:default` and
  `core:window:allow-set-theme` — nothing else is granted.
- Every deletion goes through validated rules; there is no "delete this
  path" command. The front end only ever sends `rule_ids` and a mode.
- Per-file errors are non-blocking (recorded as `skipped`); rule-loading
  errors that are structural (not machine-specific) are blocking at
  startup.
- Fonts and icons are embedded (fontsource, lucide) — no CDN fetch.

## Tests

- Rust: unit tests on the rule parser/validator (paths outside the profile
  refused, `default_checked` interaction with `unavailable_reason`),
  integration tests for scan/clean against a temp directory created by the
  test, encoding/decoding of the StartupApproved blob, and the
  `cleaning_order` / confinement guards. 91 tests as of this writing,
  run single-threaded (`cargo test -- --test-threads=1`) because the
  startup registry tests share `HKCU\Software\wincleaner-test`.
- Front end: Vitest on the list and report components, and on the CSP/
  capabilities configuration. 57 tests as of this writing.
- Verification commands: `cargo test -- --test-threads=1` in `src-tauri`,
  `npm test` at the repository root, `npm run tauri build` for the binary.
- Tests never run against the real profile, the real recycle bin or the
  real Run keys — only `TempDir` and `HKCU\Software\wincleaner-test`.

## Post-MVP: signing the binary

Smart App Control (on by default on recent Windows 11 builds) blocks any
executable not signed by a Trusted Root Program authority. Before any public
distribution, sign the executable and the installer (SignPath, free for open
source, or Azure Trusted Signing). Out of scope for the MVP; development
happens with SAC disabled.

## History

The original French design and implementation plan were written on
2026-09-10 and are archived outside this repository.
