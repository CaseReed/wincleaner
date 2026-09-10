# Safety harness

`src-tauri/tests/safety_harness.rs` is an automated end-to-end proof that
WinCleaner deletes what its rules describe and nothing else. It runs in
`cargo test` and therefore in CI on every push; the test itself is the gate,
there is no artifact to inspect.

It exercises the **real** scan and clean code — `commands::scan_rules_with`,
`commands::clean_rules_with`, `clean::clean_rule_with_trash`,
`scan::scan_rule_with_api` and both containment guards — against a synthetic
Windows profile built under a `TempDir`.

## What is injected, and why nothing real is touched

| Injected | Instead of |
| --- | --- |
| `%USERPROFILE%`, `%LOCALAPPDATA%`, `%APPDATA%`, `%TEMP%` | `rules::system_env` |
| `RecycleQuery` returning a fixed `(3, 4096)` | `SHQueryRecycleBinW` |
| `RecycleEmpty` returning `Ok(())` | `SHEmptyRecycleBinW` |
| `TrashDelete` recording the path | `trash::delete` |
| Winapp2 registry probe always answering `false` | `winapp2::registry_key_exists` |

No test ever reads the real profile, empties the real recycle bin, opens a
registry key, or launches the application.

## The fake profile

```text
<TempDir>\profile\      %USERPROFILE% (and AppData\Local, AppData\Roaming, AppData\Local\Temp)
<TempDir>\outside\      target of the junction planted in %TEMP%
<TempDir>\outside2\     target of the junction planted inside the Chrome cache
<TempDir>\control\      named by no rule; the snapshot proves it is untouched
```

Every file, junk or sentinel, is written with a content marker derived from its
own path, so a silent **rewrite** is caught as well as a deletion.

* **JUNK** — one or more files for every `kind = "files"` rule of
  `rules.toml`, plus one concrete file per glob of every detected Winapp2 rule
  (materialised by `concrete_path`, which turns `*` into `w2` and `**` into a
  `w2deep` level, so a materialised name can never collide with a sentinel).
* **SENTINELS** — 47 files that must survive:
  * user data: `Documents\thesis.docx`, `Desktop\notes.txt`, `Pictures`,
    `Downloads`, `Videos`, `Music`, `.ssh\id_ed25519`,
    `Documents\project\.git\HEAD`, `Documents\project\.env`, `.npmrc`;
  * app internals sitting right next to the junk: Chrome and Edge
    `Login Data`, `Cookies`, `History`, `Bookmarks`, `Preferences`,
    `Service Worker\Database`, `Local Storage`, `IndexedDB`, `Extensions`;
    Firefox `places.sqlite`, `key4.db`, `logins.json`, `prefs.js`,
    `cookies.sqlite`, `extensions\`;
  * lookalike siblings: `Cache.bak`, `CacheStorage-notes.txt`,
    `cache2.bak`, `cache2-notes.txt`, `startupCacheKeep`. Windows file names
    are case-insensitive, so `Cache2` and `cache2` are the *same* directory:
    the lookalikes differ by a suffix, not by case;
  * `Recent\CustomDestinations\` — the items the user pinned by hand, which
    `windows.explorer-recent` deliberately stops short of;
  * behind a junction: `outside\secret.txt`, `outside2\also-secret.txt`, plus
    two **baits** — `outside\bait.tmp` and `outside2\Cache_Data\f_000009` —
    whose names match `%TEMP%\**\*` and the Chrome cache glob respectively.
    They survive only because the walk refuses to cross the junction at all;
  * `control\untouched.dat`.

Two junctions are planted with `mklink /J`, which needs no privilege: one on
the `%TEMP%` walk, one buried inside `Chrome\User Data\Default\Cache\`.

## The three tests

1. **`the_nine_native_rules_delete_their_junk_and_spare_every_sentinel`** —
   loads `rules.toml` against the fake environment, asserts the nine rules are
   available, scans them all, asserts every `files` rule matched at least one
   junk file and that the scan saw *exactly* the junk list, then cleans them
   all in `Permanent` mode (the worst case). Asserts: every junk file gone,
   every sentinel present with identical bytes, `report.deleted` equal to the
   junk count plus the injected bin items, no path walked through a junction,
   and — from a full before/after snapshot of the `TempDir` — that nothing
   else disappeared, changed or appeared.
2. **`the_whole_catalogue_spares_user_data_and_never_crosses_a_junction`** —
   same, with the Winapp2 catalogue converted and probed against the fake
   tree (registry probe always false). Twelve entries are detected —
   Audacity, Dropbox, Obsidian, GitHub Desktop, PowerToys ZoomIt, OBS Studio,
   Postman, Slack, Spotify (×2), Telegram Desktop, VLC — and one junk file is
   materialised per converted glob.
3. **`trash_mode_hands_the_recycle_bin_exactly_the_junk`** — `Trash` mode with
   a recording `TrashDelete`. Asserts the recorded set equals the junk list
   exactly and contains no sentinel, and that nothing was deleted from disk.

Runs in about 5 s.

## What it does NOT prove

* **Hard links.** A hard link under the profile pointing at data elsewhere is
  indistinguishable from a regular file (`symlink_metadata` and `canonicalize`
  both resolve it under the profile) and *would* be deleted. This is the
  accepted limitation already stated in `CLAUDE.md`; the harness documents it
  rather than asserting it.
* **The real Recycle Bin.** `SHQueryRecycleBinW` / `SHEmptyRecycleBinW` and
  `trash::delete` are injected. That the shell really empties the bin is
  covered by `docs/manual-verification.md`, not here.
* **The real registry.** Winapp2 `Detect=HK..` probes always answer `false`,
  so registry-detected entries are never exercised. The startup keys are
  covered by the unit tests in `startup.rs`.
* **Elevation.** Everything runs unelevated, as the application does.
* **Chrome and Edge Winapp2 entries.** Not one of them survives the
  conversion today (checked: no converted rule id contains `chrome` or the
  Edge sections), so the harness cannot exercise them. If a future
  `npm run winapp2:update` lets them through, the sentinels above will start
  failing — which is the signal to look at, not a reason to relax them.
* **Rules whose paths do not resolve under the fake tree.** A rule using a
  variable the fixture does not define is reported `unavailable` and skipped,
  exactly as on a machine that does not have it.

## Adding a rule

**A new rule needs a junk fixture.** Add at least one file matching each of
its patterns in `Fixture::populate_junk`
(`src-tauri/tests/support/fake_profile.rs`). Without one the harness fails at

```
rule "<id>" matched no junk: add a fixture in tests/support/fake_profile.rs
```

If the new rule walks a directory that also holds data a user would miss, add
that data to `Fixture::populate_sentinels` in the same commit, with the
one-line `why` that gets printed when it disappears.
