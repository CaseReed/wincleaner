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

## Invariants not to break
- No Tauri command takes a path: the front end only sends `rule_ids` and a
  mode. `clean` re-scans before deleting.
- Every rule lives in `src-tauri/rules.toml`; allowed variables: TEMP,
  LOCALAPPDATA, APPDATA, USERPROFILE; the resolved path must be under the
  profile. Variable values go through `globset::escape` (a `[` in an account
  name used to send the walk outside the profile).
- The textual containment done at load time is not enough: it is **replayed on
  disk**. `scan.rs::confined_root` refuses any root carrying
  `FILE_ATTRIBUTE_REPARSE_POINT` or whose `canonicalize` form leaves the
  canonical profile; `clean.rs::deletable_path` requires, right before every
  deletion, a regular file that is not a reparse point and resolves under the
  profile. Never walk nor delete without going through those two guards
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
  no-op stand-ins, `Trash` mode moves the file to `<root>ecycle-bin` instead
  of calling `trash::delete`, and both startup commands refuse.
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
