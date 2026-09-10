# Safety harness

`src-tauri/tests/safety_harness.rs` is an automated end-to-end proof that
WinCleaner deletes what its rules describe and nothing else. It runs in
`cargo test` and therefore in CI on every push; the test itself is the gate,
there is no artifact to inspect.

It exercises the **real** scan and clean code — `commands::scan_rules_with`,
`commands::clean_rules_with`, `clean::clean_rule_with_trash`,
`scan::scan_rule_with_api` and both containment guards — against a synthetic
Windows profile built under a `TempDir`.

## What is proven, and how that claim is itself checked

"Green suite" is not the claim. The claim is that **removing any one of the
guards below makes the harness fail**, and it is checked by applying each
removal to the working tree, running `cargo test --test safety_harness`, and
recording which assertion fires. The mutation table lives in
`.superpowers/harness/report.md`; the guards it covers are:

| Guard | Fixture that makes it load-bearing |
| --- | --- |
| `scan::confined_root` | a junction planted **on the walk root itself** (`%LOCALAPPDATA%\CrashDumps` → `outside3\bait.dmp`). `walkdir` descends into its own root even when that root is a reparse point, so this is the only place the guard can act |
| `clean::deletable_path` | a directory swapped for a junction **between the internal re-scan and the deletion**, from inside the injected deletion call (`%TEMP%\zz_toctou` → `outside4\victim.txt`) |
| `rules::normalize`'s `..` refusal | a rule set carrying `%TEMP%\..\..\..\Documents\*` — which lands exactly on `<profile>\Documents` and passes the textual containment check — loaded through `rules::load_rules_with`, the loader the application uses |
| `scan::build_set`'s `literal_separator(true)` | `Recent\CustomDestinations\pinned.lnk`: the sibling pattern `Recent\AutomaticDestinations\*` raises the shared walk ceiling to two levels, so the walk *reaches* this file and only the glob refuses it |
| the non-recursive rule patterns of `rules.toml` | one sentinel one level below each: `Recent\CustomDestinations\pinned.lnk`, `Explorer\keep\thumbcache_9.db`, `CrashDumps\keep\notes.dmp`, each carrying the extension its rule matches |

**One guard is not proven here.** The reparse `filter_entry` inside
`scan::collect` has **no privilege-free construct that exercises it**, and
removing it leaves the harness green. Rust classifies a Windows junction as a
symbolic link, so `walkdir`'s `follow_links(false)` already refuses it before
`filter_entry` is consulted — and the branch is guarded by
`file_type().is_dir()`, which is false for a junction, so it never even fires.
What `filter_entry` closes is the *other* reparse tags — cloud placeholders,
volume mount points, app-execution links — none of which an unelevated test can
create. It is **defence in depth, kept deliberately and asserted by nothing**.
The day one of those tags becomes creatable without privilege, this is the
fixture to add.

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
<TempDir>\outside3\     target of the junction planted ON a walk root (CrashDumps)
<TempDir>\outside4\     target of the junction swapped in AFTER the scan (TOCTOU)
<TempDir>\control\      named by no rule; the snapshot proves it is untouched
```

Every file, junk or sentinel, is written with a content marker derived from its
own path, so a silent **rewrite** is caught as well as a deletion.

* **JUNK** — one or more files for every `kind = "files"` rule of
  `rules.toml`, plus one concrete file per glob of every detected Winapp2 rule
  (materialised by `concrete_path`, which turns `*` into `w2` and `**` into a
  `w2deep` level, so a materialised name can never collide with a sentinel).
* **SENTINELS** — 52 files that must survive:
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
  * one level below each **non-recursive** rule, carrying the very extension
    that rule matches: `Recent\CustomDestinations\pinned.lnk`,
    `Explorer\keep\thumbcache_9.db`, `CrashDumps\keep\notes.dmp`;
  * behind a junction: `outside\secret.txt`, `outside2\also-secret.txt`, plus
    two **baits** — `outside\bait.tmp` and `outside2\Cache_Data\f_000009` —
    whose names match `%TEMP%\**\*` and the Chrome cache glob respectively.
    They survive only because the walk refuses to cross the junction at all;
  * `control\untouched.dat`.

Two junctions are planted with `mklink /J`, which needs no privilege: one on
the `%TEMP%` walk, one buried inside `Chrome\User Data\Default\Cache\`.

## The six tests

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
4. **`a_junction_at_a_rule_root_is_refused_never_walked_and_never_deleted`** —
   `scan::confined_root`. `%LOCALAPPDATA%\CrashDumps` is replaced by a junction
   to `outside3`, which holds a `bait.dmp` matching `CrashDumps\*`. Asserts the
   scan returns no path at all, `file_count == 0`, `skipped == 1` (the refusal
   *is* reported, it is not silent), the clean deletes nothing, and the bait is
   byte-for-byte intact.
5. **`a_directory_swapped_for_a_junction_after_the_scan_is_refused_at_deletion`**
   — `clean::deletable_path`. The scan runs and records
   `%TEMP%\zz_toctou\victim.txt`; then, from *inside* the injected deletion
   call — after the internal re-scan, in the one window the re-scan cannot see
   — `zz_toctou` becomes a junction to `outside4`. The stand-in deletes for
   real (`remove_file`), so the victim survives only if the guard refuses.
   Asserts the victim is intact, the hijacked path never reached the deletion
   call, and `CleanReport.skipped` carries it with the reason "outside the user
   profile at deletion time".
6. **`a_rule_set_carrying_a_parent_segment_is_rejected_by_the_loader`** —
   `rules::normalize`. A rule set whose first entry is
   `%TEMP%\..\..\..\Documents\*` is loaded through `rules::load_rules_with`.
   Asserts the whole load fails with `ParentSegment` (fatal, not merely
   disabled), then runs the remaining valid rule and asserts `Documents` is
   untouched.

Runs in about 6 s.

Ordering inside each test is deliberate: **containment first, arithmetic
second.** A walk that crossed a junction must be reported as a breach of
containment, not as a count that no longer adds up.

## What it does NOT prove

* **The reparse `filter_entry` in `scan::collect`.** See the table above:
  defence in depth, no privilege-free construct exercises it, removing it
  leaves the harness green.
* **The real `trash::delete`.** Every test passes an injected deletion call, and
  the `Permanent`-mode tests pass a panic stub (`forbidden_trash`) so reaching
  the move-to-bin aborts the run rather than being caught after the fact. Two
  doors cannot be closed from an integration test: `clean::clean_rule` and
  `clean::clean_rule_with_api` are public and wired to the real, private
  `trash_delete`, which cannot be shadowed from `tests/`. Nothing in the
  harness names them; a test that reached the real bin would have had to type
  the name.
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
