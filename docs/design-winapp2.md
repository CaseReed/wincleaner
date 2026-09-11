# Winapp2 import, size-sorted preview — design

Date: 2026-09-10. Extends `docs/design.md`.

## Goal

Widen what WinCleaner catches by converting the community-maintained
[Winapp2.ini](https://github.com/MoscaDotTo/Winapp2) rule base (2,000+ entries)
into native rules, without loosening any safety invariant, and let users see the
biggest wins first.

Out of scope: registry keys from Winapp2 (`RegKeyN`), entries needing elevation,
user data folders, downloading anything at runtime, a user-facing import button
(may come later).

## Source and licensing

- `src-tauri/third_party/winapp2/Winapp2.ini` (the "Non-CCleaner" flavor) is
  embedded with `include_str!`, next to its `LICENSE` (CC-BY-SA-4.0) and a
  `NOTICE` naming the source and version date. `THIRD_PARTY_NOTICES.md` at the
  repo root and the README credit it. The converted rules are data under
  CC-BY-SA-4.0; the application code stays MIT.
- `scripts/update-winapp2.mjs` (developer tool, Node, no dependency) downloads
  the latest file from the upstream repository, writes the `NOTICE` date and
  reminds to add a CHANGELOG line. The application itself never touches the
  network.

## Conversion (`src-tauri/src/winapp2.rs`)

Parses the ini at startup (milliseconds, kept in memory) into `Vec<Rule>` of the
same type as `rules.toml` rules. Rules are then validated and confined exactly
like native ones (`rules.rs` validation, on-disk containment, reparse-point
refusal, deletion guard).

Per entry `[Name *]`:
- `FileKeyN=path|filespec[|RECURSE][|REMOVESELF]`. Kept only when `path` starts
  with `%LocalAppData%`, `%AppData%`, `%UserProfile%` or `%Temp%` (mapped to
  `%LOCALAPPDATA%`, `%APPDATA%`, `%USERPROFILE%`, `%TEMP%`). Dropped:
  `%ProgramFiles%`, `%WinDir%`, `%SystemDrive%`, `%CommonAppData%` (elevation)
  and `%Documents%`, `%Pictures%`, `%Downloads%`, `%Music%`, `%Video%`,
  `%Public%` (user data). `RECURSE` → `\**\`, `*.*` → `*`, `;`-separated specs →
  one glob each. `REMOVESELF` needs nothing extra (emptied directories are
  already removed).
- User data is denied **by segment**, not only by variable: the allow-list lets
  `%UserProfile%` through, and the very folders `%Documents%` names are also
  reachable spelled out (`%UserProfile%\Documents\Foo\Screenshots`). The first
  segment under the profile is therefore matched, case-insensitively, against
  `Documents, Desktop, Pictures, Videos, Music, Downloads, OneDrive, Favorites,
  Links, Contacts, Saved Games, Searches` and the key is dropped. Only the first
  segment: an application cache holding a `Documents` subfolder stays cleanable.
  The key alone goes; the entry goes only when no `FileKey` is left, counted as
  `dropped_user_data` (entries) — the refused keys themselves are counted by
  `user_data_keys`, which is deliberately outside `dropped()` so that
  `retained + dropped() == entries` keeps holding.
- **App-content deny-list.** A handful of upstream `FileKey`s do not delete a
  cache, they delete the application: `%LocalAppData%\Vortex-Updater` holds the
  staged executables of Vortex's own pending update, and a Squirrel `.nupkg` is
  the package the installed tree is unpacked from and the next delta is computed
  against. `winapp2.rs::APP_CONTENT_DENY` is one explicit table — a path prefix
  (matched on the mapped path, so an alias spelling cannot slip past, and
  stopping at a `\` so a sibling directory is not caught) or a file spec
  (matched anywhere) — each line carrying the reason it is there. A heuristic on
  directory names was rejected: it would silently drop real caches the day an
  application picks an unlucky name. Applied in `file_key_globs`: a denied
  directory sinks the whole key, a denied spec only removes that spec, like a
  spec we cannot represent. A key that loses everything is counted by
  `app_content_keys`, and the entry is dropped as `dropped_app_content` only
  when no `FileKey` is left — same split as user data, `app_content_keys`
  outside `dropped()` so that `retained + dropped() == entries` keeps holding.
  On the embedded file today: 3 keys refused (`[Vortex *]` FileKey8 and
  FileKey9, `[Discord *]` FileKey12), 0 entries dropped — both entries keep
  their real cache keys.
- Before that check, the spelled-out form of a variable is normalised onto the
  variable: `%UserProfile%\AppData\Local\` → `%LOCALAPPDATA%\`,
  `%UserProfile%\AppData\Roaming\` → `%APPDATA%\`, `%LocalAppData%\Temp\` →
  `%TEMP%\`. Overlap detection compares unexpanded strings, so one directory
  must have one spelling; this makes it alias-proof by construction instead of
  by a list of special cases.
- `ExcludeKeyN=FILE|path|spec` / `PATH|path|spec` → exclude globs, same variable
  rules. `ExcludeKeyN=REG|key` is accepted and ignored: WinCleaner never cleans
  the registry, so skipping it changes nothing about which files are deleted.
  An exclude carrying a metacharacter we cannot keep literal (`[`, `]`, `{`,
  `}`, `?`), in its path or in its spec, is counted as `dropped_exclude` — a
  limit of our pattern language — while a path outside the folders we map is
  counted as `dropped_variable`.
- `RegKeyN` ignored. An entry with no retained FileKey is dropped.
- `Warning=` becomes the rule note shown inline. `Default=` is ignored.
- Rule id `winapp2.<slug>` (ASCII, lowercase, hyphens), category `Applications`,
  label = section name without the trailing ` *`, `risk = medium`,
  `default_checked = false`.
- Invalid or unsupported entries are skipped, never fatal. The converter returns
  counts (retained, dropped by reason) exposed to the UI as a summary line.

The curated `rules.toml` rules take precedence: `convert_with` receives them and
drops any converted entry whose globs intersect a native one, counted as
`dropped_overlap`. The intersection test (`winapp2.rs::overlaps`) runs on the
unexpanded, case-insensitive glob strings, split on `\` and walked segment by
segment: two segments are compatible when either is `**`, when they are equal,
or when one carries a `*` matching the other, and a `**` absorbs every following
segment; the patterns overlap when every segment is compatible up to the end of
the shorter list. It is deliberately approximate, and approximate in one
direction only — when both segments carry a `*` they are held compatible without
deciding whether their languages really intersect, so `User Data\*\Login Data*`
counts as overlapping `User Data\ShaderCache\**\*`. Every error is therefore a
false positive that drops a community rule, never a native one; the alternative
(a literal string comparison) errs the other way and lets the same bytes be
counted twice by two rules.

## Detection

An entry is shown only when at least one `DetectN=HKCU\..|HKLM\..` key exists
(read-only registry query) or one `DetectFileN=<path>` exists on disk (same
variable whitelist; a trailing `\*` wildcard is allowed and means "any child").
Entries without any Detect key are hidden. `DetectOS` is ignored. Detection runs
once at startup, off the main thread, and its result is cached for the session.

## UI

- New category `Applications`, collapsed by default with its count and total
  after a scan; a search field above the rule list filters on label across all
  categories.
- Attribution line at the bottom of the category: "Community rules from
  Winapp2 (CC-BY-SA 4.0)" with the upstream link.
- "Sort by size" toggle in the hero, enabled after a scan: sorts rules inside
  each category by `total_bytes` descending and orders categories by their
  total. Off by default; the toggle state persists in `localStorage`.
- Confirmation, modes and the action bar are unchanged.

## Extra native rules

Only what Winapp2 does not already cover after checking the embedded file
(expected: npm cache `%LOCALAPPDATA%\npm-cache`, pip cache
`%LOCALAPPDATA%\pip\Cache`). Added to `rules.toml`, `risk = low`.

## Tests

- Rust: parser on a fixture ini (FileKey with RECURSE, multiple specs, exclude,
  rejected variables, entry without FileKey, Warning, slug collisions),
  detection on a `TempDir` and on `HKCU\Software\wincleaner-test`, and a test
  converting the real embedded file that asserts every produced rule passes
  validation and that the retained count is above a floor (e.g. 500).
- Precedence is proved twice, once by each side: `no_native_rule_overlaps_a_
  converted_winapp2_rule` checks that `convert_with` enforced `overlaps`, and
  `no_retained_converted_rule_resolves_onto_a_native_path` re-proves the same
  guarantee without calling `overlaps` at all — it resolves both catalogues,
  builds a path each pattern is guaranteed to match, and asks a real `GlobSet`
  (built in chunks: thousands of globs do not fit one NFA). Using `overlaps` as
  both the enforcement and the oracle would let an error in it hide itself.
- Front: search filter, sort toggle, collapsed category, summary line.
- Existing invariants keep their tests; `cargo clippy --all-targets -D warnings`
  stays clean.
