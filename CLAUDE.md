# WinCleaner — notes for Claude Code

Open source Windows cleaner (MIT). Tauri 2 + Rust (`src-tauri/`), React 19 + TS
+ Tailwind v4 + shadcn/ui (`src/`).
Spec: `docs/design.md`. Manual checklist: `docs/manual-verification.md`.

## Verifications (to run before saying "done")
- Rust: `cd src-tauri && cargo test -- --test-threads=1` (single-threaded: the
  registry tests share `HKCU\Software\wincleaner-test`). Rust lives in
  `%USERPROFILE%\.cargo\bin`.
- Front end: `npm test` (Vitest), `npm run build` (tsc + vite).
- Binary: `npm run tauri build` → `src-tauri/target/release/` (exe, MSI, NSIS).
- `src-tauri/examples/scan_bench.rs` is an opt-in, read-only benchmark of a full Analyze (`cargo run --release --example scan_bench` from `src-tauri`), never run by `cargo test` or CI.
- `src-tauri/examples/space_bench.rs` is an opt-in, read-only benchmark of the
  Space measurement (`cargo run --release --example space_bench` from
  `src-tauri`), never run by `cargo test` or CI.
- `src-tauri/examples/trash_bench.rs` is an opt-in throughput benchmark for
  Recycle Bin deletion (`cargo run --release --example trash_bench` from
  `src-tauri`); it is never run by `cargo test` or CI, and it sends real files
  it creates itself to the real Recycle Bin.

## Invariants not to break
- No Tauri command takes a path: the front end only sends `rule_ids` and a
  mode. `clean` re-scans before deleting. `add_exclusion` obeys the same rule:
  it takes an **index** into the last scan of that rule (`commands::LastPaths`,
  rewritten at every Analyze), never a path — which is also why there is no
  free-form glob editor (`docs/roadmap.md`).
- A user exclusion is stored as a **variable pattern**, never an absolute path
  (`src-tauri/src/exclusions.rs`): the longest resolved prefix among `%TEMP%`,
  `%LOCALAPPDATA%`, `%APPDATA%`, `%USERPROFILE%` is folded back into the
  variable (ASCII-case-insensitive, on the long-path form the scanner uses), a
  file becoming `%VAR%\rel\file.ext` and a folder `%VAR%\rel\dir\**`. The
  relative part goes through `globset::escape` segment by segment, exactly as a
  variable's value does in `expand_env_with`, so a file named `report[1].txt`
  matches itself and not a character class. A path under none of the four is
  refused, so the stored file carries no account name and survives a moved
  profile.
- Exclusions are **merged into the rule's own `exclude` list** before the scan
  AND before the clean (`exclusions::apply`), so they go through
  `resolved_excludes_with` like any `rules.toml` exclude and an invalid stored
  pattern is refused like a bad rule. Merging at clean time is what makes an
  exclusion added after the last Analyze effective: `clean_rule_with_trash`
  re-walks the rule rather than trusting front-end paths.
- One resolver decides where the per-user stores live
  (`src-tauri/src/paths.rs::config_dir`): `<exe dir>\WinCleaner` when a file
  named `portable.txt` sits next to the executable (content never read — a
  marker, not a configuration file), `%APPDATA%\WinCleaner` otherwise. Both
  `exclusions.toml` and `recycle-bin.toml` go through it rather than reading
  `%APPDATA%` themselves, so a store added later cannot quietly stay in the
  roaming profile. The **sandbox override keeps precedence** over both
  branches: `exclusions::store_path` checks the sandbox root first and returns
  before the resolver is ever called. Front-end settings (theme, language,
  update consent) are WebView2 `localStorage` and are not moved by portable
  mode. `app_mode` hands the front end a single flag, never the directory.
- The command line (`src-tauri/src/cli.rs`) is **read-only and takes no
  path**: it deletes nothing, and the only file it writes is its own
  measurement cache (`recycle-bin.toml`, through the same `scan_rules_in` the
  window uses). There is no `--clean`: the confirmation before a deletion is an
  invariant, and an unattended flag would be the way around it. `--rules`
  accepts rule ids validated against the catalogue and nothing else — the same
  rule the IPC boundary obeys, extended to `argv` — and the printed report
  carries no path either. `main.rs` answers it before `tauri::Builder`, so
  `--analyze` opens no window; the measurement is `commands::scan_rules_in`
  itself, so the CLI can never drift from what the window shows. Exit codes:
  0 a run, 1 a scan error, 2 a bad argument.
- The exclusions store **fails closed**: the `exclusions.toml` of the directory
  named above (or `<sandbox root>\exclusions.toml` while a sandbox is active,
  so a sandbox never reads the user's real list) that is absent is an empty
  list, but one that exists and cannot be read or parsed makes scan and clean
  **error**, never proceed as if nothing were excluded. Written atomically:
  temporary file, then rename over the target.
- The Recycle Bin measurement cache (`src-tauri/src/recycle_cache.rs`,
  `recycle-bin.toml` in that same directory) **fails open**, the exact opposite
  of the exclusions store above, and that asymmetry is deliberate: a missing,
  unreadable or corrupt entry is ignored and rewritten, because a lost cache
  costs one slow Analyze while a lost exclusion costs a file. Nothing is ever
  deleted on the strength of a cached figure — `clean.rs` re-queries the bin
  on its own path. The key is the `LastWriteTime` of
  `<drive>\$Recycle.Bin\<current user SID>` per fixed drive plus the drive
  list; the query stays injected (`scan::RecycleQuery`) so tests use a
  `TempDir` store and a fake fingerprint, never the real bin.
- A `clean.rs::SkippedItem` is built through `SkippedItem::new`, never as a
  literal: the stable `code` (`in-use`, `access-denied`, `not-found`, `other`)
  is derived there from the raw message, so no call site can set one that
  disagrees with the sentence next to it. The window shows the translated
  code; the raw message stays in a `title` and in the JSON report.
- `quit_browser` (`commands.rs`) takes an **executable name**, never a path or
  a pid: it is matched against `BROWSER_PROCESSES` and anything else is refused
  (`unknown-browser`), and a browser that owns a visible window is refused too
  (`browser-has-window`), re-checked at the moment of the call rather than
  trusted from the banner. It terminates the main process first — one with no
  child marker on its command line: `--type=` for Chrome and Edge,
  `-contentproc` for Firefox (`commands::marks_child_process`) — and only then
  whatever outlived it. That second pass re-lists the browser by name and
  keeps the intersection with the pids found at the start
  (`commands::leftovers`): a pid alone proves nothing, Windows reuses them
  during the three-second wait. `Machine::list` also keeps only the current
  logon session (`ProcessIdToSessionId`), so the same account logged on twice
  (console plus RDP) does not lose the other session's browser; a session that
  cannot be read is skipped, the opposite of the owner check next to it,
  because such a process is not provably ours. The machine is behind
  `ProcessControl`, so no test ever terminates anything.
- Every rule lives in `src-tauri/rules.toml`; allowed variables: TEMP,
  LOCALAPPDATA, APPDATA, USERPROFILE; the resolved path must be under the
  profile. Variable values go through `globset::escape` (a `[` in an account
  name used to send the walk outside the profile).
- A native rule may set `label_fr`, `description_fr` and `category_fr`
  (`Option<String>`, serde default): the French name, warning and category
  shown when the interface is French. `src/lib/rule-i18n.ts` is the one place
  that picks the French field and falls back to the English one, used
  everywhere a rule's label or description reaches the screen, a live region,
  or the search/sort order — never `rule.label` or `rule.note` directly.
  Winapp2 (community) rules never set these and always read in English.
- The textual containment done at load time is not enough: it is **replayed on
  disk**. `scan.rs::confined_root` refuses any root carrying
  `FILE_ATTRIBUTE_REPARSE_POINT` or whose `canonicalize` form leaves the
  canonical profile; `clean.rs::deletable_path` requires, right before every
  deletion, a regular file that is not a reparse point and resolves under the
  profile — in `Trash` mode that means checked per file immediately before it
  joins a batch of at most 500 (`clean.rs::TRASH_BATCH`), and the batch is sent
  right after the rule's files are checked; a path approved earlier in that
  same batch is not re-checked before the shell call (`docs/safety-harness.md`,
  "What it does not prove"). Never walk nor delete without going through those
  two guards
  (walkdir descends into its root even when that root is a junction, and
  `mklink /J` requires no privilege).
- The guards above do not see hard links: a hard link is one more name on the
  same data, indistinguishable from a regular file (`symlink_metadata` and
  `canonicalize` both resolve it under the profile). A hard link placed under
  the profile pointing at data located elsewhere will therefore be deleted —
  an accepted limitation, for lack of a counter-measure with no cost.
- A machine-specific load error (`MissingVar`, `OutsideProfile`) **disables the
  rule** (`Rule::unavailable_reason`) without preventing startup; only
  structural `rules.toml` errors stay fatal.
- `default_checked = false` in `rules.toml` for anything irreversible or
  hand-curated (recycle bin, recent items, crash dumps). The Clean button
  always goes through a confirmation that names the irreversible parts.
- The `recycle-bin` rule is always cleaned first (`commands.rs::cleaning_order`):
  otherwise it permanently destroys what the previous rules have just dropped
  into the bin.
- `%TEMP%` may be an 8.3 short path: `system_env` resolves it to its long form
  via `GetLongPathNameW`.
- The **Space screen is read-only**, and not merely by convention: there is no
  command behind it that deletes, moves or writes anything
  (`src-tauri/src/space.rs`, `commands.rs::space_scan`/`space_reveal`). It
  measures the six known folders (`SHGetKnownFolderPath`, so an
  OneDrive-redirected folder is honoured), ranks what it finds and hands
  Explorer a path to show. Its roots go through `scan.rs::confined_root` like
  any walk root — one that resolves outside the profile, or is a reparse point,
  is refused and reported by name in `skipped_roots` — and `space_reveal` takes
  an **index** into `commands::LastSpace`, never a path, exactly like
  `add_exclusion`. It refuses while a sandbox is active: the folders it
  measures are the user's real ones.
- Never a registry cleaner. Startup: we only write the `StartupApproved` blob
  (bit 0 = disabled), never a deletion, RunOnce is read-only.
- Tests: never against the real profile, the real recycle bin or the real Run
  keys. `TempDir` and `HKCU\Software\wincleaner-test` only.
- The safety harness (`src-tauri/tests/safety_harness.rs`) must pass; a new
  rule needs a junk fixture in `src-tauri/src/sandbox.rs`, or the harness
  fails with "rule <id> matched no junk". See `docs/safety-harness.md`.
- The fixture builder lives in the **library** (`src-tauri/src/sandbox.rs`),
  not in `tests/`: Sandbox mode builds the very same tree on a user's machine
  so what they watch is what CI proves. While a sandbox is active the four
  rule variables all resolve inside its root, the recycle-bin query/empty are
  no-op stand-ins, `Trash` mode moves the file to `<root>
ecycle-bin` instead
  of calling `trash::delete_all`, and both startup commands refuse.
- One network call and one only: `check_for_updates` (`src-tauri/src/update.rs`)
  does a single unauthenticated `GET` on
  `https://api.github.com/repos/CaseReed/wincleaner/releases/latest`, from
  **Rust**, never from the webview. Ten-second timeout, `User-Agent:
  wincleaner/<version>`, `Accept: application/vnd.github+json`, nothing else —
  no credential, no query string, no second request. Enforced on the agent
  itself, not just by convention (`update.rs::agent_config`):
  `.max_redirects(0)` (a 3xx from the endpoint is read as `Malformed`, never
  followed to a second host), `.https_only(true)`, `.proxy(None)` (ureq
  otherwise honours `HTTPS_PROXY`/`https_proxy` from the environment, which
  would silently add a hop). A second `check_for_updates` call within ten
  seconds of a **successful** previous call returns that previous result
  instead of opening another connection; a previous call that failed
  (offline, rate-limited, malformed...) is never cached, so the next call
  always retries immediately (`commands.rs::check_for_updates_with`) — see
  VM-8b for why the cooldown itself matters in `tauri dev`. It fires on a click, or once at startup when the user has armed
  the Settings switch (off by default, `wincleaner.autoCheckUpdates`).
  Everything but `http_get` is pure and the transport is injected: tests never
  open a socket.
- A GitHub release answer is validated, not trusted verbatim
  (`update.rs::parse_release`): `tag_name` must re-parse as `semver::Version`
  (a non-semver tag is `Malformed`, not silently accepted), and `html_url` is
  kept only when it starts with
  `https://github.com/CaseReed/wincleaner/` byte-for-byte — a homoglyph host
  or a `javascript:` URL becomes `None` instead of reaching the Copy-link
  button. The response body is capped at 256 KiB and `notes` truncated to
  20,000 characters before they ever reach the front end.
- Network-free CSP: `default-src 'self'; connect-src 'self' ipc:
  http://ipc.localhost; style-src 'self'; style-src-attr 'unsafe-inline';
  object-src/base-uri/frame-ancestors/form-action 'none'`. A distinct `devCsp`
  covers Vite HMR. Capabilities limited to `core:event:default` +
  `core:window:allow-set-theme`. `src/lib/tauri-config.test.ts` fails if the
  policy is loosened.
- Fonts and icons are embedded (fontsource, lucide).
- Heavy commands (`scan`, `clean`, startup) run `async` + `spawn_blocking`: a
  sync command blocks the window.
- Winapp2 entries are converted into ordinary `rules::Rule` values and go
  through the same `check_rule_with` validation and the same containment and
  deletion guards as native rules — there is no separate Winapp2 deletion
  path.
- A converted Winapp2 path is denied by segment, not only by variable: the
  first segment under `%USERPROFILE%` may not be `Documents`, `Desktop`,
  `Pictures`, `Videos`, `Music`, `Downloads`, `OneDrive`, `Favorites`, `Links`,
  `Contacts`, `Saved Games` or `Searches`. `%UserProfile%\AppData\Local\`,
  `\AppData\Roaming\` and `%LOCALAPPDATA%\Temp\` are first normalised onto
  `%LOCALAPPDATA%`, `%APPDATA%` and `%TEMP%`, so one directory has one spelling
  and overlap detection cannot be fooled by an alias.
- A converted Winapp2 `FileKey` is also matched against an explicit app-content
  deny-list (`winapp2.rs::APP_CONTENT_DENY`): a path prefix or a file spec that
  names application payload rather than cache (`%LOCALAPPDATA%\Vortex-Updater`,
  `*.nupkg`). One table, one reason per line — never a heuristic on directory
  names. Refused keys are counted by `app_content_keys` (outside `dropped()`),
  entries left with no `FileKey` by `dropped_app_content`.
- The nine curated native rules take precedence: a converted Winapp2 rule
  whose paths overlap a native rule is dropped at conversion time
  (`dropped_overlap`), never counted or deleted twice.
- A Winapp2 rule is shown only when its `Detect`/`DetectFile` key matches on
  this machine, always carries `risk = medium` and `default_checked = false`,
  and lives in the `Applications` category, folded by default.
- `npm run winapp2:update` refreshes the embedded `Winapp2.ini` by hand; the
  application itself never fetches it (no runtime network). The rule base is
  CC-BY-SA-4.0, attributed in `THIRD_PARTY_NOTICES.md`.

## Dev machine
- Smart App Control must be disabled for cargo to work (it blocks any locally
  compiled binary). Check with:
  `Get-ItemProperty 'HKLM:\SYSTEM\CurrentControlSet\Control\CI\Policy' | Select VerifiedAndReputablePolicyState`
  (0 = off).
- Capturing the window without a permission prompt: PowerShell +
  `System.Drawing` `CopyFromScreen` (see `docs/manual-verification.md` or the
  git history).
- Post-MVP: sign the exe (SignPath / Azure Trusted Signing) before any
  distribution, otherwise SAC blocks it for users.
