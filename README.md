# WinCleaner

[![CI](https://github.com/CaseReed/wincleaner/actions/workflows/ci.yml/badge.svg)](https://github.com/CaseReed/wincleaner/actions/workflows/ci.yml)
[![License: MIT](https://img.shields.io/badge/license-MIT-blue.svg)](LICENSE)

An open source Windows cleaner that does only what it says: no telemetry, no
registry cleaner, and no network access except one request to GitHub when you
click Check for updates or enable automatic checks (off by default).

![Cleanup screen](docs/images/cleanup-light.png)

## Features

- **Nine audited built-in rules**, all declared in a single readable file
  (`src-tauri/rules.toml`) — nothing is hidden in the code.
- **Measure before you delete.** Analyze reports the reclaimable size per rule,
  with the exact list of files it would touch.
- **Everything is measured, only the checked rules are cleaned.** Analyze sizes
  every rule that applies to this machine, checked or not, so you can see what
  ticking one would free before ticking it.
- **Two deletion modes plus Auto.** Auto sends low-risk items to permanent
  deletion and everything else to the recycle bin.
- **A Space screen, read-only.** The 100 largest files and the 20 largest
  folders of your Downloads, Desktop, Documents, Pictures, Videos and Music,
  with "Reveal in Explorer". It measures and shows; it never deletes.
- **Startup manager.** Enable or disable what starts with your session, without
  ever deleting a registry value.
- **Settings screen.** Version, licence and third-party notices, plus "What's
  new" for the running version, extracted from `CHANGELOG.md` at build time and
  shown offline — nothing is fetched.
- **English and French interface.** Pick the language in Settings or let it
  follow Windows; rule names and release notes come from their own sources and
  stay in English.
- **Light and dark themes**, including the Windows title bar.
- **Keyboard and screen-reader friendly.** Every screen is operable without
  a mouse, every control is named and every result is announced.
- **No installer bloat.** A single Tauri 2 binary, ~10 MB, no runtime to
  install beyond WebView2, which Windows 11 already ships.

## What it cleans

Nine built-in rules across three categories, plus community rules converted
from [Winapp2](https://github.com/MoscaDotTo/Winapp2): each one is shown only
when the application it targets is detected on the machine, always carries the
**medium risk** badge, and is **never checked by default**. When a converted
rule's paths overlap a built-in rule, the built-in rule wins and the converted
one is dropped, so the same bytes are never counted or deleted twice.

| Category | Rule | Default | Risk |
| --- | --- | --- | --- |
| System | Temporary files (`%TEMP%`) | Checked | Low |
| System | Recycle Bin | **Unchecked** | Low |
| System | Thumbnail cache | Checked | Low |
| System | Recent items | **Unchecked** | Medium |
| System | Crash dumps (`%LOCALAPPDATA%\CrashDumps`) | **Unchecked** | Medium |
| System | Error reporting reports (`%LOCALAPPDATA%\Microsoft\Windows\WER\ReportQueue`, `ReportArchive`) | **Unchecked** | Medium |
| Browsers | Microsoft Edge cache | Checked | Low |
| Browsers | Google Chrome cache | Checked | Low |
| Browsers | Mozilla Firefox cache | Checked | Low |
| Applications | npm cache (`%LOCALAPPDATA%\npm-cache`) | Checked | Low |

The browser rules cover HTTP, code, GPU and service-worker caches only. Local
Storage, IndexedDB, Session Storage and service-worker registrations are
deliberately left alone: clearing them breaks offline mode for installed web
apps.

## Safety model

- **No command takes a path.** The front end only ever sends rule ids and a
  mode; cleaning always re-scans before deleting.
- **Profile containment verified on disk**, not merely declared: before walking
  a root, and again before every deletion, the path is resolved with
  `canonicalize` and must stay under the resolved `%USERPROFILE%`. Any
  junction, symbolic link, mount point or cloud sync placeholder is refused
  rather than followed, including at the root.
- **No registry cleaner.** The Startup screen writes only the enable bit of a
  startup program; it never deletes a registry value.
- **No network access, except one request to GitHub when you click Check for
  updates or enable automatic checks (off by default).** The webview itself can
  never reach the network — the CSP forbids it (`connect-src 'self' ipc:
  http://ipc.localhost`) — and that single `GET` is issued from Rust, on
  `https://api.github.com/repos/CaseReed/wincleaner/releases/latest`, with no
  credential, no query string and no identifier: the only thing about this
  machine that GitHub sees is the application version, in the `User-Agent`
  header it requires from every REST client. Automatic checking is off until
  you turn it on in Settings. See `docs/design-updater.md`.
- **Confirmation before cleaning.** The Clean button always goes through a
  confirmation that announces the mode and names what the operation will
  destroy with no way back.
- **Proven on every push.** An automated harness runs the real scan and clean
  code against a synthetic profile full of decoy user data and junctions, and
  fails the build if a single file that should have survived does not — and it
  is built so that removing any one of the containment guards (the walk-root
  refusal, the pre-deletion re-check, the `..` refusal, the non-recursive glob
  boundary) makes it fail, which is checked by applying each removal and
  recording the assertion that fires; one guard is documented as defence in
  depth that no unelevated test can exercise — see
  [`docs/safety-harness.md`](docs/safety-harness.md).
- **Provable on your own machine.** Settings → Sandbox builds that same
  synthetic profile under your temporary directory and points the whole engine
  at it, so you can run a real Analyze and a real Clean and read the verdict —
  sentinels intact, junk removed, nothing outside touched — without a single
  file of your own being reachable.

One exception deserves its own paragraph: the **Recycle Bin** rule goes through
the Windows `SHEmptyRecycleBinW` API, which empties the recycle bin of **every
volume** on the machine, including outside the user profile. That deletion is
permanent whatever the deletion mode chosen. The Cleanup screen flags it on the
rule row, the rule is **unchecked by default**, and it is always processed
first in a pass — otherwise it would permanently destroy what the other rules
had just dropped into the bin in "Recycle Bin" mode.

## Installation

Binaries (MSI and NSIS) are published on the repository
[Releases](https://github.com/CaseReed/wincleaner/releases) page.

These installers are **not signed yet**: Windows Defender SmartScreen will show
a warning, and a machine with Smart App Control (SAC) enabled will refuse to
run them until signing is in place. See [`docs/code-signing.md`](docs/code-signing.md)
for the state of the SignPath setup.

### Portable

`WinCleaner_<version>_x64-portable.exe` on the same Releases page is the bare
executable, nothing to install. Create an empty file named **`portable.txt`**
next to it and WinCleaner stops writing to `%APPDATA%`: the exclusions list and
the Recycle Bin measurement go to `<exe dir>\WinCleaner\` instead, so a copy
on a USB stick leaves nothing behind on the machine it is run from. The content
of the marker is never read. Settings → About says "Portable" when the mode is
active. Only the installers are submitted to SignPath: the portable executable
stays unsigned even on a release whose notes say the installers are signed.

Theme, language and the automatic-update consent are WebView2 `localStorage`
and stay where WebView2 puts them — portable mode does not move those. The
WebView2 runtime is still required; it ships with Windows 11.

## Command line

WinCleaner answers two arguments before it opens anything, so both run in a
terminal without a window:

```
wincleaner --analyze                 # a table: rule, label, files, size, skipped
wincleaner --analyze --json          # one JSON object, for a script or a ticket
```

`--rules id,id` narrows the run to those rule ids; `--help` prints the usage.
Exit codes: `0` a run, `1` a scan error, `2` a bad argument.

Reading that exit code takes one precaution: the binary is built with
`windows_subsystem = "windows"`, so `cmd.exe` does not wait for it and a bare
`%ERRORLEVEL%` or `$LASTEXITCODE` is not the process's own code. Any of these
gives the real one:

```
powershell -c "(Start-Process .\wincleaner.exe -ArgumentList '--analyze','--json' -Wait -PassThru -NoNewWindow).ExitCode"
powershell -c ".\wincleaner.exe --analyze | Out-String; $LASTEXITCODE"   # a pipe makes PowerShell wait
cmd /c start /wait wincleaner.exe --analyze
```

This command line is **read-only**: it deletes nothing and takes no path; the
only file it writes is its own measurement cache (the Recycle Bin figure, so
the next run is not slower for having been asked twice). There is no `--clean`
and there never will be — the confirmation before a deletion is an invariant of
the application — and no option takes a path: `--rules` accepts rule ids and
refuses anything the catalogue does not know.

## Build from source

Prerequisites:

- Windows 11 with WebView2 installed
- Rust stable MSVC (rustup), Node 24 and npm 11
- Visual Studio 2022 with the C++ build tools

```
npm install
npm run tauri dev      # development
npm run tauri build    # installers in src-tauri/target/release/bundle
```

## Running the tests

```
npm test
cd src-tauri
cargo test -- --test-threads=1
```

`--test-threads=1` is mandatory on the Rust side: the startup registry tests
share `HKCU\Software\wincleaner-test`.

The checks that cannot be automated without destroying data (emptying the
recycle bin, actually disabling a startup program) are described in
[`docs/manual-verification.md`](docs/manual-verification.md) and are to be
replayed before every release.

## Contributing

Continuous integration (`.github/workflows/ci.yml`) runs on every pull request
targeting `main`: front-end tests (Vitest), front-end build, Rust tests
(`cargo test -- --test-threads=1`) and lint (`cargo clippy -D warnings`). A pull
request must pass those checks before review.

Adding a cleaning rule usually means editing `src-tauri/rules.toml` alone.
Paths may only use `%TEMP%`, `%LOCALAPPDATA%`, `%APPDATA%` and `%USERPROFILE%`,
and must resolve under the user profile — anything else is refused at load
time. `label_fr`, `description_fr` and `category_fr` are optional: set them to
give the rule a French name, warning and category, shown when the interface is
French (`src/lib/rule-i18n.ts` falls back to the English field otherwise).
Winapp2 (community) rules have none and always read in English.

## Roadmap

The next versions widen what the application is useful for while staying
inside its invariants: exclusions picked from the listed paths, a copyable
cleanup report, a read-only view of the largest files and folders, a portable
build and a read-only command line. Elevation on demand is deliberately
deferred until installers are signed. Details and reasoning in
[`docs/roadmap.md`](docs/roadmap.md).

## License

MIT — see [`LICENSE`](LICENSE).

The bundled [Winapp2](https://github.com/MoscaDotTo/Winapp2) rule base is
CC-BY-SA-4.0; see [`THIRD_PARTY_NOTICES.md`](THIRD_PARTY_NOTICES.md).

## Code signing policy

Free code signing provided by [SignPath.io](https://signpath.io), certificate by
[SignPath Foundation](https://signpath.org).

Team roles:

- **Authors**: [CaseReed](https://github.com/CaseReed)
- **Reviewers**: [CaseReed](https://github.com/CaseReed)
- **Approvers**: [CaseReed](https://github.com/CaseReed)

Every release is built by GitHub Actions from the tagged source of this
repository and signed only after manual approval of the signing request.

Privacy policy: WinCleaner does not collect, store or transmit any user data.
Its only network access is the optional update check (off by default, or one
explicit click), which sends a single request to `api.github.com` carrying no
identifier other than the application version. See the
[Safety model](#safety-model) section above.
