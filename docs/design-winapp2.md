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
- `ExcludeKeyN=FILE|path|spec` / `PATH|path|spec` → exclude globs, same variable
  rules.
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
- Front: search filter, sort toggle, collapsed category, summary line.
- Existing invariants keep their tests; `cargo clippy --all-targets -D warnings`
  stays clean.
