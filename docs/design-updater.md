# In-app updates — design

Status: **proposal**, nothing implemented. Everything below was verified on
2026-09-10 against the sources listed in "Sources verified" at the end.

## 1. Goals and privacy stance

WinCleaner's headline promise (`README.md`, `CLAUDE.md`) is *no telemetry, no
network access*. An updater is, unavoidably, network access. The promise has to
be **narrowed honestly, not quietly dropped**. What we commit to:

- Exactly **one** outbound request per check: `GET` on the manifest, plus a
  second `GET` on the installer **only after the user clicks**. Nothing else.
- **No identifiers**: no user/machine/install id, no query string. GitHub sees
  an IP, a date and the plugin's `User-Agent` (`tauri-plugin-updater/<version>`,
  `updater.rs`). The current version is *not* sent — the endpoint is a static
  file, not a `{{current_version}}` template.
- **Nothing is uploaded, logged or reported.** We run no server to ping.

Recommended consent model: **off by default, first-launch choice.** On first
launch (reusing the `localStorage` hint pattern already in `CHANGELOG.md`),
one row: *"Check for updates automatically? WinCleaner will fetch one file from
GitHub, with no identifier attached. [Yes] [No, I'll check manually]"*. Until
answered, automatic checking is off; a manual **Check for updates** button
always exists, and the switch can be turned off later. Rationale: silently
enabling network access in an app sold on "no network access" is the single most
damaging thing we could do to its credibility. A default of off is recoverable;
an opt-out default is not.

### Wording changes required

- `README.md` "Safety model": **No network access** → **No network access
  unless you ask for it**, describing the single GET, the no-identifier
  property, the off-by-default switch, and linking here.
- `README.md` "Installation": the updater cannot work while the repo is private
  (§4).
- `CLAUDE.md` "Invariants": the "Network-free CSP" bullet stays *true as
  written*, but gains a sentence saying the updater does its HTTP in Rust
  (`reqwest`), never from the webview.
- `src/lib/tauri-config.test.ts` asserts every capability starts with `core:`
  ("declares no plugin granting disk or network access"). It **will fail** the
  moment `updater:default` is added. Rewrite it to an explicit allowlist
  (`core:*` plus exactly `updater:default` and `process:default`) — not delete
  it.

**The CSP does not need to change.** `tauri-plugin-updater` builds a `reqwest`
client in Rust (`ClientBuilder::new().user_agent(UPDATER_USER_AGENT)`,
`updater.rs`); the webview never issues the request, so `connect-src 'self'
ipc: http://ipc.localhost` stays exactly as it is. A defensible property: a
compromised front end still cannot reach the network.

## 2. Architecture

### Plugin choice — use `tauri-plugin-updater` + `tauri-plugin-process`

Recommended over a custom implementation, which would mean reimplementing
minisign verification, download, NSIS/MSI argument construction and the
exit-then-install dance. The plugin's signature validation **cannot be
disabled** (docs page), comparison is semver, and the installer invocation is
battle-tested. `tauri-plugin-process` is needed only for `relaunch()`.

### Endpoint

```
https://github.com/CaseReed/wincleaner/releases/latest/download/latest.json
```

Two caveats, both verified:

- `/releases/latest/` resolves to the latest **non-prerelease**. `release.yml`
  already marks hyphenated tags as prereleases, so `v0.3.0-beta.1` is correctly
  not offered to stable users — deliberate, not accidental.
- The `platforms[*].url` `tauri-action` writes is **not** a
  `releases/download/...` URL. Per `src/upload-version-json.ts` it is
  `https://api.github.com/repos/CaseReed/wincleaner/releases/assets/<asset_id>`,
  fetched with `Accept: application/octet-stream` (`updater.rs`) as that
  endpoint requires. Consequence: **the installer download counts against the
  unauthenticated REST limit of 60 requests/hour/IP.** Fine for a person; not
  behind a large NAT.

### Manifest format (what `tauri-action` produces)

```json
{
  "version": "0.3.0",
  "notes": "<the GitHub release body, verbatim>",
  "pub_date": "2026-09-10T12:00:00.000Z",
  "platforms": {
    "windows-x86_64": {
      "signature": "<base64 minisign .sig contents>",
      "url": "https://api.github.com/repos/CaseReed/wincleaner/releases/assets/123456"
    },
    "windows-x86_64-nsis": { "signature": "…", "url": "…" }
  }
}
```

`notes` is the release `body` passed straight through (`src/index.ts` calls
`uploadVersionJSON(info.version, body, …)`). Today `releaseBody` in
`release.yml` is a two-paragraph *pointer* to the CHANGELOG — **that is what
users would see as release notes.** Recommended: make `release.yml` inject the
tag's actual `CHANGELOG.md` section instead. Useful on its own, independent of
the updater.

### Signing keys

```
npm run tauri signer generate -- -w %USERPROFILE%\.tauri\wincleaner.key
```

Produces `wincleaner.key` (private, password-protected) and `.key.pub`.
**The user generates this, not Claude** — it is a secret.

- Private key content + password → repository secrets
  `TAURI_SIGNING_PRIVATE_KEY` and `TAURI_SIGNING_PRIVATE_KEY_PASSWORD`.
- Keep an offline copy. **Losing it means no installed copy can ever be updated
  again** — the pubkey is baked into every shipped binary ("Critical" on the
  plugin docs page).
- The `.pub` content goes into `tauri.conf.json` and is committed.

### `src-tauri/tauri.conf.json`

```json
{
  "bundle": {
    "createUpdaterArtifacts": true
  },
  "plugins": {
    "updater": {
      "pubkey": "<contents of wincleaner.key.pub>",
      "endpoints": [
        "https://github.com/CaseReed/wincleaner/releases/latest/download/latest.json"
      ],
      "windows": {
        "installMode": "passive"
      }
    }
  }
}
```

`installMode` is `passive` (default, progress bar), `basicUi` (needs
interaction) or `quiet` (no feedback). Verified in `config.rs`: `passive` → NSIS
`/P`, `quiet` → `/S`, `basicUi` → no flag; unless `restart_after_install(false)`
is set, `/R` is appended for `passive` and `quiet`. **Recommend `passive`** —
`quiet` leaves several seconds with nothing on screen after the app vanishes,
which reads as a crash.

### `src-tauri/capabilities/default.json`

```json
{
  "permissions": [
    "core:event:default",
    "core:window:allow-set-theme",
    "updater:default",
    "process:default"
  ]
}
```

`updater:default` grants `allow-check`, `allow-download`, `allow-install`,
`allow-download-and-install`. Consider narrowing `process:default` to
`process:allow-restart` alone — we never need `exit`.

### `src-tauri/src/lib.rs`

```rust
tauri::Builder::default()
    .plugin(tauri_plugin_updater::Builder::new().build())
    .plugin(tauri_plugin_process::init())
```

### `.github/workflows/release.yml`

Add to the `tauri-apps/tauri-action@v0` step:

```yaml
        env:
          GITHUB_TOKEN: ${{ secrets.GITHUB_TOKEN }}
          TAURI_SIGNING_PRIVATE_KEY: ${{ secrets.TAURI_SIGNING_PRIVATE_KEY }}
          TAURI_SIGNING_PRIVATE_KEY_PASSWORD: ${{ secrets.TAURI_SIGNING_PRIVATE_KEY_PASSWORD }}
        with:
          updaterJsonPreferNsis: true
```

`uploadUpdaterJson` already defaults to `true` (verified in `action.yml`).
`updaterJsonPreferNsis` defaults to `false`, which picks the **MSI** since we
build both. Set it to `true`: our NSIS bundle is `installMode: currentUser`,
installs under `%LOCALAPPDATA%` and needs **no UAC elevation**, whereas an MSI
update raises a UAC prompt *after* the app has already exited — the worst
possible moment for a consent dialog.

**Ordering problem with SignPath.** `tauri-action` minisigns the bundles and
uploads `latest.json` *before* the SignPath step replaces the `.msi`/`.exe`
assets with Authenticode-signed ones. The signature in `latest.json` would then
describe the *unsigned* binary and every update would fail verification. Fix
before phase 3: sign with SignPath before `tauri-action` uploads, or regenerate
and re-upload `latest.json` after signing. **Must not be discovered in
production.**

### What "verify everything is OK during installation" can mean

- **Before install (built in, not optional):** downloaded bytes are checked
  against the minisign signature using the compiled-in pubkey. A tampered or
  truncated download is rejected and never executed.
- **Downgrade protection (built in):** `updater.rs` compares
  `release.version > self.current_version` as semver. Equal or older is never
  offered.
- **After install:** the plugin verifies nothing — it calls `ShellExecuteW` then
  `std::process::exit(0)`, so our process is gone. The only honest check is **on
  next launch**: persist the expected version before exiting, compare it with
  `app.package_info().version` at start. Match → "Updated to x.y.z", clear the
  marker. Mismatch → the install silently failed; show a notice and a link to
  the release page.
- **Rollback: there is none.** NSIS `currentUser` runs the previous version's
  uninstaller before installing; the old binary is not kept. Recovery from a bad
  update is "download the previous release and run it". Do not promise a
  rollback we do not have.

## 3. In-app UX

**Never a modal.** A cleaner that interrupts you to talk about itself is the
genre's worst habit.

- **Sidebar badge.** `AppShell.tsx` gains a third nav item, *Settings*, with a
  small dot when an update is available. That is the entire ambient signal.
- **Toast.** One `sonner` toast (already a dependency, already CSP-wired):
  *"WinCleaner 0.3.0 is available"* with a **View** action. Once per version,
  not per launch.
- **Update panel** (in Settings): current version, new version, `pub_date`,
  release notes, a primary **Download & install** button, and the two controls
  (automatic-check switch, manual **Check for updates**). During download the
  button becomes a determinate progress bar fed by the `Started`
  (`contentLength`) / `Progress` (`chunkLength`) / `Finished` events of
  `downloadAndInstall`. After `Finished`: *"WinCleaner will close to install,
  then reopen"* — because it will, whether we say so or not.

### Rendering release notes safely

`latest.json.notes` is a GitHub release body — **attacker-influenced markdown if
the manifest is ever spoofed.** Recommendation: **do not render markdown and do
not add a markdown dependency.** Render plain text with `white-space: pre-wrap`,
blank lines splitting paragraphs; no HTML parsing, no `dangerouslySetInnerHTML`,
no link auto-detection. Our CHANGELOG sections are bullet lists that read
perfectly as plain text. Zero cost, zero dependency, one whole injection surface
removed — instead of relying on the CSP to catch it.

### "What's new" for the current version — offline

Separate feature, **no network at all**, worth shipping first. At build time
extract the `CHANGELOG.md` section matching `tauri.conf.json`'s version into a
bundled asset (a small `scripts/` script, in the spirit of `version:check`) and
show it in Settings as *What's new in 0.2.0*, plus once automatically after a
version change is detected. This is phase 1 and depends on none of the decisions
below.

## 4. Failure modes and tests

| Case | Expected behaviour |
| --- | --- |
| Offline / DNS failure | Check fails silently on automatic runs; manual check shows "Could not reach GitHub". Never a toast for a background failure. |
| Rate limited (60/h/IP unauth.) | Same as offline. The *download* is the API endpoint, so it counts too. |
| Tampered `latest.json` | A forged `version`/`notes` is accepted; a forged `url`/`signature` is not — the payload fails minisign verification. The manifest is trusted for *display*, never for *execution*. This is why notes are plain text. |
| Signature mismatch | `downloadAndInstall` errors before executing anything. Surface as "Update rejected: signature invalid" and do not retry automatically. |
| Download interrupted | Error surfaced, button returns to its idle state, partial file discarded. Retry is a click. |
| Installer blocked by SAC | **The minisign signature is irrelevant to Windows.** SAC and SmartScreen judge Authenticode. An unsigned NSIS installer is blocked on a SAC machine exactly as today's manual download is (`docs/code-signing.md`). The updater cannot fix this; SignPath must land first. |
| Downgrade / equal version | Never offered (`release.version > current_version`). |
| Prerelease tag | Not served by `/releases/latest/`; stable users are unaffected. |
| **Private repo** | `https://github.com/CaseReed/wincleaner/releases/latest/download/latest.json` returns **404** unauthenticated while the repo is private, and so does the `api.github.com` asset URL. **The updater cannot work at all until the repo is public**, short of a proxy that holds a token — which would mean running a server, seeing every user's IP, and breaking the no-telemetry stance far more than GitHub does. Recommendation: no proxy; ship the updater when the repo goes public. |

Unit-testable in Rust: none of the plugin itself (upstream). Ours: the
post-install version marker (write expected version → read back → compare, pure
logic over a stored string) and the CHANGELOG extraction script. Vitest: the
panel state machine (idle / checking / available / downloading / error), the
plain-text notes renderer against a body containing `<script>` and
`[x](javascript:…)`, and the updated `tauri-config.test.ts` allowlist.

Manual, and it must be a **VM** (new section in `docs/manual-verification.md`):
install vN, publish vN+1, check, download, watch the app exit and come back at
vN+1, confirm the "Updated to" notice. Then the same with the network cut
mid-download; the same on a SAC-enabled VM (expect a block until signed); and a
run with automatic checking off, confirming with a network monitor that **zero**
requests leave the machine.

## 5. Sequencing

Blocking prerequisites, in order:

1. **Repo goes public.** Without it, nothing in phase 3 functions. Non-negotiable.
2. **Authenticode signing live** (SignPath, `docs/code-signing.md`). Shipping an
   auto-updater that hands users an unsigned installer is worse than no updater.
3. **Minisign keypair generated by the user** and stored as repository secrets.
4. **SignPath / `latest.json` ordering fixed** in `release.yml` (§2).

Phases:

- **Phase 1 — offline, ships today.** Bundled "What's new" from `CHANGELOG.md`
  plus a Settings screen. No network, no plugin, no new capability, no README
  change. Independent of every prerequisite above.
- **Phase 2 — manual check only.** Add the plugin and `updater:default`, wire
  the **Check for updates** button and the panel, but **no
  `downloadAndInstall`** — an available update links to the release page.
  Needs prerequisites 1 and 3. First-launch consent lands here, automatic
  checking still off by default. Most of the value at a fraction of the risk.
- **Phase 3 — download & install.** Add `tauri-plugin-process`, progress,
  relaunch, post-install verification. Needs all four prerequisites.

## 6. Risks and open questions — for the user to decide

1. **Consent default.** Off-by-default with a first-launch choice (recommended)
   costs adoption; opt-out costs credibility. Decide before phase 2.
2. **When does the repo go public?** Phases 2 and 3 are blocked until then, and
   no proxy workaround preserves the privacy stance.
3. **Key custody.** Where does `wincleaner.key` live outside GitHub, and who
   else can restore it? A lost key permanently ends updates for every installed
   copy — the highest-consequence, lowest-visibility risk here.
4. Inject the real `CHANGELOG.md` section as the release body so `notes` is
   useful? (Recommended; cheap; useful on its own.)
5. Plain-text release notes, or a markdown dependency? (Recommended: plain.)
6. `passive` vs `quiet` install mode? (Recommended: `passive`.)
7. No rollback exists: accept it, or gate updates behind a longer prerelease
   soak?

## Sources verified (2026-09-10)

- https://v2.tauri.app/plugin/updater/ — install, `tauri signer generate`,
  `TAURI_SIGNING_PRIVATE_KEY`, `plugins.updater`, `createUpdaterArtifacts`,
  `latest.json` shape, `updater:default`, `passive`/`basicUi`/`quiet`, "the app
  automatically exits when the install step executes", "signature validation
  cannot be disabled".
- https://v2.tauri.app/plugin/process/ — `process:default`,
  `process:allow-restart`, `relaunch()`.
- https://github.com/tauri-apps/tauri-action + its `action.yml` and
  `src/upload-version-json.ts`, `src/index.ts` (branch `dev`) —
  `uploadUpdaterJson` default `true`, `updaterJsonPreferNsis` default `false`;
  `notes` comes from the release `body`; asset URLs are
  `https://api.github.com/repos/{owner}/{repo}/releases/assets/{id}`.
- `tauri-apps/plugins-workspace` (branch `v2`) `plugins/updater/src/updater.rs`
  and `config.rs` — `reqwest` + `UPDATER_USER_AGENT`; `Accept:
  application/json` for the manifest, `application/octet-stream` for the
  payload; `release.version > self.current_version`; `ShellExecuteW` then
  `std::process::exit(0)`; NSIS `/P` `/S` `/R` `/UPDATE` mapping.
- https://docs.github.com/en/rest/releases/assets — asset download endpoint,
  `Accept: application/octet-stream`.
- https://docs.github.com/en/rest/using-the-rest-api/rate-limits-for-the-rest-api
  — 60 requests/hour unauthenticated, per IP.
