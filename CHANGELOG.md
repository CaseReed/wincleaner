# Changelog

All notable changes to this project are documented in this file.

The format is based on [Keep a Changelog](https://keepachangelog.com/en/1.1.0/),
and this project adheres to [Semantic Versioning](https://semver.org/spec/v2.0.0.html).

## [Unreleased]

## [0.9.2] - 2026-09-13

### Added

- **Quitting a browser that only runs in the background.** Chrome and Edge
  keep a handful of processes alive after the last window is closed, and one
  extension is enough to hold them there for the rest of the session: on the
  machine this was written for, nine `chrome.exe` processes with no window
  between them, and a Clean that reported the cache files "in use" every time.
  The banner that already told those two cases apart now does something about
  the second one: a **Quit {browser}** button next to the warning, a
  confirmation that says what it costs — the browser is force-closed, and it
  may offer to restore its session next time — and a count of the processes
  stopped. Chrome has no external "quit" command, so the new `quit_browser`
  terminates its main process (the one with no `--type=` on its command line)
  and gives the children three seconds to follow it out before terminating
  whatever is left. It takes an executable name and matches it against the
  three browsers the detection already knows, never a path, and it re-checks
  at the moment of the click that no window has appeared since the banner was
  painted — a browser you can see is one you close yourself. The banner also
  names the permanent fix now: the "Continue running background apps" setting
  in the browser itself.

## [0.9.1] - 2026-09-12

### Fixed

- **The Recycle Bin was the whole Analyze.** Measured on a loaded profile,
  `SHQueryRecycleBinW` took **243 s** cold for 110,553 items and 19 GB, while
  every other rule of the same run finished in seconds: the Analyze simply
  waited on that one call. The last measurement is now remembered in
  `%APPDATA%\WinCleaner\recycle-bin.toml`, keyed on a cheap fingerprint of the
  bin — the `LastWriteTime` of `<drive>\$Recycle.Bin\<your SID>` on each fixed
  drive, plus the drive list. Windows restamps that directory whenever
  anything is filed into the bin or taken out of it, so emptying the bin, or
  dropping one more file into it, invalidates the cache on its own; the drive
  list covers the one case timestamps alone would miss, a bin on a disk
  plugged in between two runs. A rule answered from the cache says so on its
  row. Unlike the exclusions store this one deliberately fails open: a
  missing, unreadable or corrupt file is ignored and rewritten, because the
  worst a lost cache can cost is one slow Analyze — nothing is ever deleted on
  the strength of a cached figure.

- **The progress bar named the wrong rule.** `scan-progress` carried only the
  rule that had just *finished*, so while the Recycle Bin took its four
  minutes the hero read "84 / 85 · AMD" — a rule that was long done. The event
  now also carries the rules still in flight, and the counter says
  "Analyzing 84 / 85 · still measuring: Recycle Bin" while any of them is,
  falling back to the finished rule on the last event.

- **Excluding a folder hid only the row that was clicked.** The stored pattern
  is `%VAR%\folder\**`, so the whole folder is out of the rule — but its
  siblings stayed on screen, each still offering an Exclude button for a file
  already excluded. Every listed path under that folder now goes at once
  (matched case-insensitively, as everywhere else a Windows path is compared),
  and the file count drops by the number of rows actually hidden. The byte
  total is still left as measured, with the same "Analyze again to refresh the
  figures" note: a scan result carries no per-file size, and inventing one
  would be worse than admitting the figure is stale.

- **The cleanup report quoted raw shell errors.** A locked file read
  ``Error during a `trash` operation: Unknown { description: "Some operations
  were aborted" }``, and a file the system refuses read "(os error 1920)".
  Every skipped item now also carries a stable code — `in-use`,
  `access-denied`, `not-found`, `other`, derived once in Rust from the
  sharing- and lock-violation classes, os errors 5, 32, 33 and 1920, and the
  shell's own "aborted" — which the window turns into its own sentence ("File
  in use or locked"). The raw message stays one hover away, and stays verbatim
  in the JSON report, where a script or a bug report needs it.

- **The deletion-mode help always described Auto.** The footer rendered
  `mode.autoHelp` whatever was selected, so picking Permanent still read
  "recycle bin for the rest". It now shows the help for the mode in the
  picker, in both languages: the Recycle Bin keeps everything recoverable, and
  Permanent recovers nothing.

- **Switching screens threw the analysis away.** The scan results, the last
  cleanup report and the ticked boxes lived inside the Cleanup panel, which
  the screen switch unmounts: a glance at Space or Settings cost a full
  re-Analyze — minutes, on the profile that needs it most. They now live in
  `App`, so they survive the trip. Entering or leaving the sandbox still
  clears them, because the catalogue and the profile both change with it.

- **The Space screen showed "0 B" before measuring anything.** Nothing had
  been measured, and a zero is a figure. It now shows the em dash the Cleanup
  hero already shows in the same situation.

## [0.9.0] - 2026-09-12

### Added

- **A Space screen: where your own files sit, without a delete button.** A
  fourth entry in the sidebar measures the six known folders of your profile —
  Downloads, Desktop, Documents, Pictures, Videos and Music, resolved through
  `SHGetKnownFolderPath` so a folder redirected into OneDrive is the one that
  gets measured — and shows the total, a bar per folder, the 100 largest files
  and the 20 largest folders, each with **Reveal in Explorer**. The walk goes
  through the same two guards as Analyze: a known folder that resolves outside
  the profile, or that is a reparse point, is refused and named instead of
  followed, and a junction below one is never crossed. Nothing on this screen
  deletes anything and the back end offers it no way to: this is your own
  data, and the gesture that removes a file stays with you, in Explorer. The
  reveal takes the row's index, never its path, exactly as `clean` and
  `add_exclusion` do. Unavailable while a sandbox is active, because the
  folders it measures are the real ones.

## [0.8.1] - 2026-09-12

### Fixed

- **Three spots in the French interface still read in English.** The
  copyable cleanup report's per-rule lines now go through `ruleLabel`, so a
  French report says "Fichiers temporaires" instead of "Temporary files" (the
  JSON report is unaffected: its `label` stays English and language-neutral,
  as documented). The Exclusions section in Settings now loads the rule
  catalogue alongside the exclusion list and shows each entry's French label
  when the interface is French, falling back to the back end's own label for
  an id no longer in the catalogue, and formats the "Added on" date for the
  locale instead of printing the raw `YYYY-MM-DD` string (parsed as a local
  date, so the day never shifts). The rule row's sr-only file count is now a
  proper `.one`/`.other` pair, so a screen reader hears "1 fichier" rather
  than "1 fichiers".

## [0.8.0] - 2026-09-12

### Added

- **A file or a folder can be kept out of a rule for good, picked from the
  paths that rule actually found.** "Show the paths" gains two actions on every
  line — Exclude this file, Exclude its folder — and Settings gains an
  Exclusions section listing what was kept out, with the rule it belongs to,
  the date, and a way to drop it again. The back end never receives a path: the
  command takes an index into the last analysis of that rule, the same reason
  `scan` and `clean` take only rule ids, and the same reason there is no
  free-form glob editor. What gets stored is a pattern written with the rule
  variables — `%TEMP%\a\b.log` for a file, `%TEMP%\a\**` for a folder — so the
  file carries no account name and survives a profile that moves; a path under
  none of the four allowed variables is refused. The list lives in
  `%APPDATA%\WinCleaner\exclusions.toml`, written through a temporary file and
  a rename so an interrupted write cannot leave a half-file behind, and under
  the sandbox root instead while a sandbox is active, so the sandbox stays
  isolated. The patterns are merged into the rule's own `exclude` list before
  the scan and again before the clean, and go through the validation any
  `rules.toml` exclude goes through — a stored pattern that is not a valid glob
  is refused exactly like a bad rule. Because `clean` re-walks each rule rather
  than trusting the paths the window is holding, a file excluded after the last
  Analyze is already spared by the clean that follows. A store that exists but
  cannot be read or parsed fails the scan and the clean outright rather than
  proceeding as though nothing were excluded.

- **The ten native rules now have French labels.** `rules.toml` gains optional
  `label_fr`, `description_fr` and `category_fr` fields (`Rule`/`RuleSummary`
  in Rust, `src/lib/rule-i18n.ts::ruleLabel`/`ruleDescription`/`ruleCategory`
  on the front end, falling back to the English field when the French one is
  absent). A native rule is grouped, searched and announced under its French
  name when the interface is French — search still matches the English name
  too — including the "Analyze 3 / 85 · …" progress line, which now looks the
  label up by rule id from the loaded summaries instead of the (deliberately
  unlocalised) event Rust emits. Winapp2 (community) rules stay English, as
  documented in Settings.

- **The last cleanup report can be copied to the clipboard.** "Copy report"
  puts a readable summary on the clipboard — app version, date, deletion mode,
  a total line of what Clean actually freed, the skipped paths if any, and
  per rule how many files and bytes the last Analyze *measured* for it plus
  its skipped count — in the current interface language. `CleanReport` only
  ever returns aggregate totals, never a per-rule breakdown, so the per-rule
  figures are explicitly labelled as measured rather than freed: a rule with
  a skipped file frees less than it measured, and the report says so instead
  of implying otherwise. "Copy as JSON" puts the same data on the clipboard as
  a stable, language-neutral object (`version`, `generated_at`, `mode`, a
  `note` spelling out the same distinction, `rules[]` with `files_measured`/
  `bytes_measured`, `totals` with the real `files_deleted`/`bytes_freed`, and
  `skipped[]`). Both reuse the clipboard mechanism already behind "Copy link"
  in Settings (`navigator.clipboard.writeText`, a success or failure toast)
  and build off a pure, unit-tested `src/lib/report.ts`. No file is written.

## [0.7.0] - 2026-09-12

### Added

- **Added a native rule for per-user Windows Error Reporting reports.**
  `windows.wer-reports` clears `%LOCALAPPDATA%\Microsoft\Windows\WER\ReportQueue`
  and `ReportArchive` — the crash reports Windows queues and archives for this
  user, regenerated on the next crash and not user data. The machine-wide
  `%ProgramData%\Microsoft\Windows\WER\*` store is out of scope: no allowed
  variable resolves there. `risk = medium`, `default_checked = false`, same
  category and voice as `windows.crash-dumps`.

- **The interface speaks English and French.** Every string the application
  writes itself now goes through a small typed dictionary (`src/i18n/en.ts`,
  `src/i18n/fr.ts`, keys typed from the English one, so a missing translation
  fails `tsc`): labels, buttons, toasts, the confirmation, the progress lines,
  the sandbox banner and every accessibility string. Settings gains a Language
  selector — System, English, Français — that follows `navigator.language`
  until it is touched, applies without a reload, is remembered in
  `localStorage` beside the theme and the update-check consent, and sets
  `document.documentElement.lang`. Sizes and counters are formatted for the
  locale (`1.5 KB` / `1,5 Ko`). What stays English whatever the setting: the
  rule labels and descriptions, which come from `rules.toml` and from Winapp2,
  the "What's new" body extracted from `CHANGELOG.md`, the release notes
  GitHub returns, and this changelog.

### Changed

- **Analyze scans its rules concurrently instead of one after another.**
  `commands::scan_rules_with` used to walk the 84 rules of a full catalogue in
  a straight loop, so the wall time was their sum — dominated by a couple of
  slow ones (the Winapp2 catch-all, and the Recycle Bin's own OS call). It now
  runs them on a small worker pool (`std::thread::scope`, sized to
  `available_parallelism` capped at 8), each rule scanned by the exact same
  `scan::scan_rule`/`scan_rule_with_api` function as before. `scan_bench`
  (`cargo run --release --example scan_bench` in `src-tauri`), on this machine:
  26s cold / 17s warm before, 11.2s cold / 7.3s warm after — the warm run now
  lands almost exactly on the slowest single rule (7.3s total against a 7.3s
  Recycle Bin query) instead of their sum. The `scan-progress` event still
  fires once per rule with a running byte total, but the order it arrives in
  now follows completion rather than catalogue order; the result list handed
  back to the front end is unaffected, since it is reassembled by index after
  every rule finishes.

- **Measured the Recycle Bin batching gain: about 2.6x.** The new opt-in
  benchmark (`cargo run --release --example trash_bench` in `src-tauri`)
  creates its own fixture files and times both strategies on this machine: one
  `trash::delete` per file ran at 77-80 files/s, matching the ~75 files/s seen
  on the real 77,000-file run that motivated the batching change; batches of
  500 through `trash::delete_all` ran at about 205 files/s, a 2.5x-2.7x
  speed-up depending on fixture size (2,000 and 5,000 files tested).

## [0.6.0] - 2026-09-12

### Fixed

- **The browser warning now tells the truth about background processes.** Closing every
  Chrome window is not enough when "Continue running background apps" is on; the
  banner used to say the browser was open. It now names the browser, counts its
  processes, distinguishes a visible window from background-only processes, and
  is refreshed every time Analyze is clicked. Closes #5.

### Added

- **The Clean has a progress bar of its own.** Cleaning used to be a disabled
  button reading "Cleaning…" for as long as the work took — thirteen minutes on
  a measured run. `clean` now emits `clean-progress` as each rule closes and
  every 500 files inside a rule, and the hero draws the same determinate bar as
  Analyze: "Cleaning 3 / 9 · Temporary files", with the freed bytes and the
  file count ticking up in the big-number position. The live region follows the
  rules, not the files, so a rule deleting 59,000 of them is not read out a
  hundred times over.

### Changed

- **Recycle Bin mode is far faster: files go to the bin 500 at a time.** Each
  `trash::delete` is one `IFileOperation`, and its fixed cost — COM plumbing,
  the shell's own progress reporting, one undo record — is what dominates a bin
  full of small files: a real run measured about 75 files/s, so 59,000 files
  took thirteen minutes. One `trash::delete_all` per batch of 500 divides that
  by the batch. The `deletable_path` guard still runs per file, immediately
  before the file joins its batch, and a batch the shell refuses is retried one
  file at a time so `skipped` still names exactly what is still on disk.
  Permanent mode is unchanged: one `remove_file` per file.

- **Keyboard and screen-reader pass over the whole interface.** The sidebar is
  a named `Main` landmark with a roving tabindex — the arrow keys, `Home` and
  `End` move between the three screens, and it costs one Tab press to get past
  instead of three; the theme toggle states which theme is on, not only what
  the next press does. On Cleanup, each category header names the rows it
  folds, each "Show the paths" names its rule and opens a named region, and the
  confirmation takes the focus when it appears, cancels on `Escape` and hands
  the focus back to the Clean button. The scan announces itself every ten rules
  and on completion — not on every rule, which would be hundreds of
  interruptions — and the cleanup report, the sandbox verdict and the sandbox
  banner announce themselves too. The reclaim gauge states its total and its
  three largest rules in words. The startup table is named and every row toggle
  reports its outcome. Nothing moved on screen: no layout, no wording, no test
  id changed.
- **Two measured contrast failures fixed.** The focus ring on the controls that
  carry no border — sidebar entries, category headers, paths triggers, the
  search field, the mode selector — was drawn at half opacity (1.66:1 against
  the card in the light theme) and is now solid (4.87:1). The Clean and Confirm
  buttons wrote `--destructive` on a wash of itself: 4.11:1 light, 3.75:1 dark,
  2.87:1 hovered, all under the 4.5:1 body text asks for. A new
  `--destructive-foreground` token takes the worst case to 4.71:1 with the
  fills untouched. Everything else measured clean, `--muted-foreground`
  included (5.11:1 to 6.42:1 on every surface, both themes), and
  `src/theme-contrast.test.ts` recomputes all of it from the stylesheet so the
  next nudge to a colour fails in CI.
- **`prefers-reduced-motion` is now honoured app-wide**, the shadcn transitions
  and the loading spinners included, by one rule in the base layer rather than
  a class to remember on each element.

## [0.5.1] - 2026-09-12

### Fixed

- **Two Winapp2 entries deleted application content, not cache.** `[Vortex *]`
  swept `%LocalAppData%\Vortex-Updater`, where Vortex stages the executables of
  its own pending update, and `[Discord *]` deleted the Squirrel `.nupkg`
  packages the installed tree is unpacked from and the next delta is computed
  against. The converter now carries an explicit app-content deny-list
  (`winapp2.rs::APP_CONTENT_DENY`): a denied directory prefix sinks the whole
  `FileKey`, a denied file spec removes that spec. Refused keys are counted by
  `app_content_keys` and an entry left with no `FileKey` by
  `dropped_app_content`; on the embedded file this refuses 3 keys and drops no
  entry — both applications keep their real cache keys. Closes #2.
- **Sandbox folders are no longer left behind in `%TEMP%`.** Leaving the
  sandbox was the only thing that removed its directory, so a crash, a kill or
  a window closed with a sandbox still open left
  `%TEMP%\wincleaner-sandbox-…` and its few hundred files there for good.
  WinCleaner now sweeps those leftovers at every start — only ever a directory
  whose owning process is gone, never one a running WinCleaner is using — and
  Settings → Sandbox shows a line naming how many are left and how much they
  take, with a Remove button, so no restart is needed. Closing the window with
  a sandbox active also attempts the removal on the way out, within a short
  budget so the window never hangs on it. The sweep unlinks junctions before
  removing a tree, exactly as leaving the sandbox does, and never counts nor
  follows what is on the other side. Closes #3.

## [0.5.0] - 2026-09-11

### Added

- **Sandbox mode.** Settings → Sandbox builds the synthetic profile the safety
  harness runs against — junk, decoy documents, credential stores, lookalike
  cache directories and junctions pointing outside it — under your temporary
  directory, and points the whole engine at it. Analyze and Clean then run for
  real against that profile and nothing else, and a "Sandbox verdict" card
  reads the disk back afterwards: sentinels intact, junk removed, files outside
  the profile untouched, junctions refused. While a sandbox is active, all four
  rule variables resolve inside its root, the Recycle Bin is replaced by no-op
  stand-ins (Recycle Bin mode moves files to a bin inside the sandbox), and the
  Startup screen steps aside because it reads the real registry. See
  [`docs/safety-harness.md`](docs/safety-harness.md).

- **Safety harness.** An end-to-end test (`src-tauri/tests/safety_harness.rs`)
  runs the real scan and clean code against a synthetic Windows profile built
  under a temporary directory — decoy documents, private keys, browser
  credential stores, lookalike cache directories and two junctions pointing
  outside the profile — and fails the build if anything that should have
  survived did not, if any junk survived, or if the walk ever crossed a
  junction. Nothing real is touched: the environment, both recycle-bin calls
  and the move-to-recycle-bin call are injected. See
  [`docs/safety-harness.md`](docs/safety-harness.md).
- **The harness is proven by mutation.** Each containment guard now has a
  fixture that makes it load-bearing, and removing that guard makes the harness
  fail: a junction planted on a walk root (`scan::confined_root`), a directory
  swapped for a junction between the internal re-scan and the deletion
  (`clean::deletable_path`), a rule set carrying `..` segments loaded through
  the real loader (`rules::normalize`), and a sentinel one level below each
  non-recursive rule carrying that rule's own extension
  (`scan::build_set`'s `literal_separator`). The one guard with no
  privilege-free construct to exercise it — the reparse `filter_entry` in
  `scan::collect` — is documented as defence in depth rather than claimed as
  proven.

## [0.4.0] - 2026-09-10

### Added

- **Per-rule progress during Analyze.** Measuring every rule of a loaded
  profile takes ten to thirty seconds; the hero no longer shows a bare spinner
  but a progress bar filled to the rules measured so far, the name of the rule
  being measured ("Analyzing 42 / 84 · Google Chrome cache") and the bytes
  found so far, ticking up as the walk goes. The bar does not animate for a
  user who asked for reduced motion.

### Changed

- Analyze now measures **every rule available on this machine**, checked or
  not — scanning is read-only, so an unchecked rule can tell you what checking
  it would free. Winapp2 rules, the Recycle Bin, Recent items and Crash dumps
  finally show a size without having to be armed for deletion first.
- The checkbox now only decides what **Clean** deletes. The "Reclaimable" total,
  the gauge and the Clean button stay the sum of the checked rules; a muted line
  under the total states how much more sits in the unchecked ones.
- Checking or unchecking a rule after a scan no longer throws the measurements
  away: the total, the gauge, the Clean label and the confirmation update on the
  spot, and the results stand until the next Analyze.

## [0.3.0] - 2026-09-10

### Added

- A one-line, dismissible first-launch hint above the Cleanup hero explaining
  what Auto mode does and where the mode is changed. The dismissal is persisted
  in `localStorage` (`wincleaner.hintDismissed`).
- A loading state while a scan or a cleanup runs: the button carries a spinner
  and reads "Analyzing…" / "Cleaning…", the hero says how many rules are being
  analysed, and the rule list is dimmed and inert.
- A third screen, **Settings**: the application version, the "open source, MIT,
  no network access, no telemetry" statement, the third-party notices, and a
  phase-1 note saying automatic update checks do not exist yet.
- "What's new in <version>" on that screen, built offline from `CHANGELOG.md`:
  `scripts/extract-whats-new.mjs` writes `src/generated/whats-new.json` (not
  committed, regenerated by the `predev`/`prebuild`/`pretest` npm hooks and in
  CI) and the section is rendered as plain text — no markdown renderer, no
  HTML, no new dependency.
- After a version change, one `sonner` toast "What's new in <version>" with a
  **View** action opening Settings, shown once per version and never on a first
  install (`wincleaner.lastSeenVersion` in `localStorage`).
- `npm run version:check` now also fails when `CHANGELOG.md` carries no section
  for the version being released.
- A **Check for updates** button in Settings. It performs exactly one
  unauthenticated `GET` on the GitHub REST API
  (`/repos/CaseReed/wincleaner/releases/latest`), from Rust rather than from
  the webview, with a ten-second timeout and no identifier of any kind: the
  only thing about the machine that leaves it is the application version, in
  the `User-Agent` GitHub requires. It reports being up to date, a newer
  version with its date, notes and link, or one of "no public release yet",
  "could not reach GitHub" and "rate limit reached". Release notes are rendered
  as plain text — no markdown renderer, no HTML.
- A **Check automatically at startup** switch next to it, **off by default**
  and persisted in `localStorage` (`wincleaner.autoCheckUpdates`). When on, one
  check runs at start and a `sonner` toast announces a newer version once per
  version (`wincleaner.lastNotifiedVersion`); a failed background check stays
  silent.

### Changed

- Every category header is a real toggle button carrying `aria-expanded`, a
  rotating chevron (still under `prefers-reduced-motion`), a hover background
  and a visible focus ring — not only the ones folded by default. The
  "Show/Hide the paths" trigger and the rule labels got the same pointer
  treatment.
- After a scan the categories holding reclaimable bytes unfold themselves and
  the empty ones fold, so the biggest wins are the ones on screen. A fold or
  unfold the user asks for afterwards is kept until the next scan.
- The category section moved to its own `src/components/RuleCategory.tsx`; no
  behaviour change.

## [0.2.0] - 2026-09-10

### Added

- Embedded the [Winapp2](https://github.com/MoscaDotTo/Winapp2) community rule
  base (`Non-CCleaner/Winapp2.ini`, snapshot 2026-09-10, CC-BY-SA-4.0) under
  `src-tauri/third_party/winapp2/`, refreshed with `npm run winapp2:update`.
- Winapp2 entries are converted into native rules at startup and shown in a new
  `Applications` category, folded by default, unchecked by default and flagged
  medium risk. A rule appears only when its `Detect`/`DetectFile` key matches
  on this machine; registry keys (`RegKeyN`) are never imported.
- The nine curated native rules take precedence over the community ones: a
  converted Winapp2 rule whose paths overlap a `rules.toml` rule is dropped at
  conversion time (`dropped_overlap`), so the same bytes are never counted or
  deleted twice. This also drops the ~41 Winapp2 browser companion entries
  (history, cookies, saved passwords, sync data): those are user data, not
  cache, and the native browser rules already own that directory.
- One native rule for the package-manager cache Winapp2 does not cover: npm
  cache (`%LOCALAPPDATA%\npm-cache`), low risk and checked by default. pip's
  cache is not duplicated: Winapp2's `[Python *]` section already covers
  `%LOCALAPPDATA%\Pip\cache`, so the candidate pip rule was dropped instead of
  added.
- User-data folders are refused by segment, not only by variable: a converted
  key whose first segment under `%USERPROFILE%` is `Documents`, `Desktop`,
  `Pictures`, `Videos`, `Music`, `Downloads`, `OneDrive`, `Favorites`, `Links`,
  `Contacts`, `Saved Games` or `Searches` is dropped, since the allow-list lets
  `%UserProfile%\Documents\...` through where it refuses `%Documents%\...`. The
  entry is dropped when nothing else remains (`dropped_user_data`); the refused
  keys are counted apart (`user_data_keys`).
- The spelled-out form of a variable is normalised onto the variable itself
  (`%UserProfile%\AppData\Local\` → `%LOCALAPPDATA%\`, `\AppData\Roaming\` →
  `%APPDATA%\`, `%LocalAppData%\Temp\` → `%TEMP%\`), so a community rule can no
  longer shadow a native one just by spelling its path differently.
- A search field filtering rules by label across every category, a summary line
  reporting how many rules were detected, converted and dropped, and a
  "Sort by size" toggle that puts the biggest wins first and is remembered
  between sessions.

## [0.1.0] - 2026-09-10

Initial MVP release.

### Added

- Eight audited cleanup rules declared in `src-tauri/rules.toml`: temporary
  files, recycle bin, thumbnail cache, recent items, crash dumps, and the
  Edge, Chrome and Firefox browser caches.
- Analyze/Clean workflow with per-rule reclaimable size and file list before
  any deletion, plus two deletion modes and an Auto mode that sends low-risk
  items to permanent deletion and everything else to the recycle bin.
- Startup manager that enables or disables what starts with the session
  without ever deleting a registry value.
- Light and dark themes, including the Windows title bar.
- A visual design pass across the Cleanup and Startup screens.

### Security

- Safety model: cleaning rules are confined to variables resolved under the
  user profile (`TEMP`, `LOCALAPPDATA`, `APPDATA`, `USERPROFILE`), and that
  containment is replayed on disk — not just declared — before every walk and
  again immediately before every deletion, rejecting reparse points and any
  path that canonicalizes outside the profile.
- No Tauri command accepts a path from the front end; only rule ids and a
  mode are sent, and `clean` re-scans before deleting.
- The recycle bin rule always runs first in a cleaning pass, so it cannot
  permanently destroy what the other rules just dropped into it.
- The Clean button always goes through a confirmation that names the
  irreversible parts of the selected mode.
- No registry cleaner: the startup manager only flips the `StartupApproved`
  enable bit, never deletes a value; `RunOnce` is read-only.
- No network access from the webview: the application CSP forbids it
  (`connect-src 'self' ipc: http://ipc.localhost`). The only outbound request
  the application can make is the update check described above, issued from
  Rust on an explicit click or with automatic checking turned on.
- Fixes from an internal security audit of the cleanup and startup paths
  (path containment, reparse-point handling, and CSP tightening).

[Unreleased]: https://github.com/CaseReed/wincleaner/compare/v0.9.2...HEAD
[0.9.2]: https://github.com/CaseReed/wincleaner/compare/v0.9.1...v0.9.2
[0.9.1]: https://github.com/CaseReed/wincleaner/compare/v0.9.0...v0.9.1
[0.9.0]: https://github.com/CaseReed/wincleaner/compare/v0.8.1...v0.9.0
[0.8.1]: https://github.com/CaseReed/wincleaner/compare/v0.8.0...v0.8.1
[0.8.0]: https://github.com/CaseReed/wincleaner/compare/v0.7.0...v0.8.0
[0.7.0]: https://github.com/CaseReed/wincleaner/compare/v0.6.0...v0.7.0
[0.6.0]: https://github.com/CaseReed/wincleaner/compare/v0.5.1...v0.6.0
[0.5.1]: https://github.com/CaseReed/wincleaner/compare/v0.5.0...v0.5.1
[0.5.0]: https://github.com/CaseReed/wincleaner/compare/v0.4.0...v0.5.0
[0.4.0]: https://github.com/CaseReed/wincleaner/compare/v0.3.0...v0.4.0
[0.3.0]: https://github.com/CaseReed/wincleaner/compare/v0.2.0...v0.3.0
[0.2.0]: https://github.com/CaseReed/wincleaner/compare/v0.1.0...v0.2.0
[0.1.0]: https://github.com/CaseReed/wincleaner/releases/tag/v0.1.0
