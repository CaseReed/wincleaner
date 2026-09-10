# Third-party notices

WinCleaner itself is MIT licensed (see [`LICENSE`](LICENSE)). It ships the
following third-party material.

## Winapp2.ini

- Source: <https://github.com/MoscaDotTo/Winapp2>, file
  `Non-CCleaner/Winapp2.ini`.
- Copyright: the Winapp2 contributors.
- License: Creative Commons Attribution-ShareAlike 4.0 International
  (CC-BY-SA-4.0) — <https://creativecommons.org/licenses/by-sa/4.0/legalcode.txt>.
- Embedded at `src-tauri/third_party/winapp2/`, together with the full licence
  text and a `NOTICE` recording the snapshot date. Refresh it with
  `npm run winapp2:update`.

WinCleaner converts a subset of those entries into its own cleaning rules at
startup (see `src-tauri/src/winapp2.rs`). The converted rules are data derived
from Winapp2 and stay under CC-BY-SA-4.0; the application code stays MIT.
