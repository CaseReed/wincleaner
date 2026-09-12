# Roadmap

Written after v0.7.0 (2026-09-12). The direction for the next versions is to
widen what the application is useful for while staying inside its invariants
(see `CLAUDE.md`): no elevation, no registry cleaner, no Tauri command that
takes a path, declarative rules, confirmation before every Clean, and never an
automatic update install.

Each version below is a goal, not a promise of a date. Items move between
versions when a measurement or a user report says they should.

## v0.8.0 — Fine control over what leaves

- **Exclusions picked from the listed paths.** In "Show the paths", a
  "Never touch this" action on a file or a folder. The back end receives an
  index into the last scan result, never a typed path; it derives a pattern
  relative to the rule's root and persists it in
  `%APPDATA%\WinCleaner\exclusions.toml`. Settings lists the exclusions and
  lets the user drop one. A free-form glob editor was rejected: it would
  break the "no command takes a path" invariant.
- **Copyable cleanup report** (clipboard, text and JSON): rules, files, bytes,
  skipped entries, deletion mode. No file is written, so no save dialog and
  no path crosses the IPC boundary.
- **French labels for the native rules** through optional `label_fr` and
  `description_fr` fields in `rules.toml`. Winapp2 rules stay English.

## v0.9.0 — See where the space goes (read-only)

- **Largest files in the profile**: the 100 largest files under Downloads,
  Desktop, Documents, Videos and Pictures, with "Reveal in Explorer". No
  deletion from this screen: this is user data, and the gesture stays with
  the user in Explorer.
- **Space by folder**: the 20 largest folders of the profile as horizontal
  bars, no treemap.
- Both reuse the existing walk (walkdir, containment, `GetLongPathNameW`) and
  are measured with `scan_bench` so the time budget holds.

## v0.10.0 — Portable and scriptable

- **Portable build**: a single executable among the release assets, with the
  configuration read next to the executable when present, otherwise from
  `%APPDATA%`.
- **Read-only command line**: `wincleaner --analyze --json` for scripts and
  support tickets. No `--clean`: confirmation before cleaning stays an
  invariant.

## Every version

- Refresh the embedded Winapp2 catalogue (`scripts/update-winapp2.mjs`),
  replay `scan_bench`, and extend the safety harness for every new native
  rule.
- **As soon as SignPath answers**: set the four `SIGNPATH_*` secrets, verify a
  signed release, then ship updater phase 3 (download with an explicit
  confirmation, "skip this version"; see `docs/design-updater.md`). That is
  the gate to 1.0 and is independent of the versions above.

## Deliberately not planned

- **Elevation on demand.** System targets (Windows Update cache, Prefetch,
  the machine-wide WER store) free a few gigabytes once, against an elevated
  helper to secure and sign. The gain-to-risk ratio is poor while the
  installers are unsigned. To be reconsidered after 1.0.
- **Scheduled or unattended cleaning.** It would need to bypass the
  confirmation before Clean.
- **Registry cleaner and application uninstaller.** Out of scope by design.
