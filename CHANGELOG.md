# Changelog

All notable changes to this project are documented in this file.

The format is based on [Keep a Changelog](https://keepachangelog.com/en/1.1.0/),
and this project adheres to [Semantic Versioning](https://semver.org/spec/v2.0.0.html).

## [Unreleased]

### Added

- Embedded the [Winapp2](https://github.com/MoscaDotTo/Winapp2) community rule
  base (`Non-CCleaner/Winapp2.ini`, snapshot 2026-09-10, CC-BY-SA-4.0) under
  `src-tauri/third_party/winapp2/`, refreshed with `npm run winapp2:update`.

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

[Unreleased]: https://github.com/CaseReed/wincleaner/compare/v0.1.0...HEAD
[0.1.0]: https://github.com/CaseReed/wincleaner/releases/tag/v0.1.0
