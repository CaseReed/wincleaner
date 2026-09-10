# Changelog

All notable changes to this project are documented in this file.

The format is based on [Keep a Changelog](https://keepachangelog.com/en/1.1.0/),
and this project adheres to [Semantic Versioning](https://semver.org/spec/v2.0.0.html).

## [Unreleased]

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
- No network access: the application CSP forbids it
  (`connect-src 'self' ipc: http://ipc.localhost`).
- Fixes from an internal security audit of the cleanup and startup paths
  (path containment, reparse-point handling, and CSP tightening).

[Unreleased]: https://github.com/CaseReed/wincleaner/compare/v0.2.0...HEAD
[0.2.0]: https://github.com/CaseReed/wincleaner/compare/v0.1.0...v0.2.0
[0.1.0]: https://github.com/CaseReed/wincleaner/releases/tag/v0.1.0
