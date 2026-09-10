# Manual verifications

These checks cannot be automated without destroying data on the development
machine. They are to be replayed before every release.

## VM-1 — Emptying the recycle bin — to be run by the user

Not run automatically: it really empties the Windows recycle bin.

1. Create a disposable file on the Desktop, then delete it with the Del key (it
   goes to the recycle bin).
2. Open the recycle bin and note the item count and the size.
3. Run `npm run tauri dev`, Cleanup screen.
4. Check "Recycle Bin" — it is **unchecked by default** — and uncheck
   everything else, then click Analyze.
5. Check that the item count and size shown match what was noted at step 2.
6. Click Clean in "Auto" mode, then "Confirm cleanup" in the action bar. The
   confirmation must name "Recycle Bin" as irreversible.
7. Check that the Windows recycle bin is empty and that the report gives the
   number of items deleted, with no entry under "Skipped".

Expected result: no confirmation dialog, no sound, no Windows progress bar
(flags SHERB_NOCONFIRMATION | SHERB_NOPROGRESSUI | SHERB_NOSOUND).

## VM-2 — Cleaning temporary files — to be run by the user

Not run automatically: it really deletes a file (steps 4-5). Steps 1-3
(Analyze) were verified read-only, see the Task 10 report.

1. Create `%TEMP%\wincleaner-probe.txt` with a few bytes.
2. Cleanup screen, check only "Temporary files", click Analyze.
3. Expand the paths and check that `wincleaner-probe.txt` is listed.
4. "Auto" mode, click Clean then "Confirm cleanup".
5. Check that the file is gone and that the report shows freed bytes. The rule
   risk being "low", the file must NOT be in the recycle bin.

## VM-3 — Recycle bin mode on a medium-risk rule — to be run by the user

Not run automatically: it really moves recent items to the recycle bin.

1. Cleanup screen, check only "Recent items" — it is **unchecked by default** —
   then Analyze.
2. "Auto" mode, Clean then "Confirm cleanup". The confirmation must NOT name
   "Recent items": in Auto, a medium-risk rule goes to the recycle bin and
   stays recoverable.
3. Open the recycle bin: the deleted items must be there (rule risk "medium").

## VM-4 — File locked by a browser — to be run by the user

Not run automatically: it really deletes the Edge cache.

1. Open Microsoft Edge and load a few pages.
2. Cleanup screen: the "msedge.exe is open" banner must appear.
3. Check "Microsoft Edge cache", Analyze, Clean, Confirm.
4. Check that the application does not crash, that the report lists the locked
   files under "Skipped" with their error message, and that Edge keeps working
   normally.

## VM-5 — Disabling a startup program — to be run by the user

Not run automatically: it modifies a real startup state on the machine.

1. Startup screen: the list must be an **exact subset** of the Task Manager
   Startup tab, restricted to the current user's entries. `HKLM` entries
   (including WOW6432Node), the common Startup folder
   (`%PROGRAMDATA%\Microsoft\Windows\Start Menu\Programs\StartUp`) and the "at
   startup" scheduled tasks are out of MVP scope (they would require
   elevation): their presence in Task Manager and their absence from the list
   is the expected behaviour. Phrased as a strict match, this step would always
   fail on a real machine and would prove nothing.
2. Toggle a Run entry to "disabled".
3. Open Task Manager, Startup apps tab: the entry must show as "Disabled".
4. Check in `regedit` that the value still exists under
   `HKCU\Software\Microsoft\Windows\CurrentVersion\Run`: it must NOT have been
   deleted.
5. Re-enable the entry and check that Task Manager goes back to "Enabled".

Expected result: the entry toggles in Task Manager without ever disappearing
from the registry, in both directions (disable then re-enable).

## VM-6 — RunOnce is read-only

1. Startup screen: any entry with source "Registry (RunOnce)" must have its
   switch greyed out and not clickable.

## VM-7 — Invalid rules are fatal — to be run on the release binary

The release binary is compiled with `windows_subsystem = "windows"`: it has no
console, so standard error is visible nowhere. The startup gate therefore has
to go through a dialog box. Verify on the release binary, not in `dev`,
otherwise the test proves nothing.

1. Temporarily edit `src-tauri/rules.toml`: set `risk = "high"` on the first
   rule.
2. `npm run tauri build`.
3. Launch `src-tauri/target/release/WinCleaner.exe` from Explorer (double
   click, not from a terminal).
4. Check that a "WinCleaner" dialog with an error icon appears, that it names
   `rules.toml` and carries the error message, and that the main window does
   not open.
5. Close the box: the process must terminate (exit code 1).
6. Restore `rules.toml` (`git checkout -- src-tauri/rules.toml`) and rebuild.

The text of the box is built by `wincleaner_lib::startup_error_message`,
covered by the test `the_startup_gate_message_carries_the_error`. The display
itself (`MessageBoxW`) cannot be tested automatically.

## VM-10 — Profile containment against a junction — to be run by the user

Not run automatically: it creates a junction on a real `%TEMP%`. The automated
version of the same invariant lives in `scan.rs`
(`a_root_that_is_a_junction_outside_the_profile_is_not_walked`).

1. Create `D:\wc-test\precious.txt` (or any other volume/folder outside the
   profile) with a few bytes.
2. Rename `%LOCALAPPDATA%\Temp` to `Temp.bak`, then
   `mklink /J "%LOCALAPPDATA%\Temp" "D:\wc-test"` (no privilege required).
3. Launch the application, Cleanup screen, check "Temporary files", Analyze.
4. Check that the rule reports **0 bytes** and flags "1 skipped", and that
   `precious.txt` still exists after a Clean/Confirm.
5. Remove the junction (`rmdir "%LOCALAPPDATA%\Temp"`, which erases only the
   link) and restore `Temp.bak`.

## VM-11 — Rule unavailable on this machine — to be run by the user

1. `setx TEMP D:\Temp` (or any path outside the profile), then open a new
   Windows session.
2. Launch the release binary: the window must **open**. No error box, no exit
   with code 1.
3. Cleanup screen: "Temporary files" must be greyed out, unchecked, not
   clickable, with "Unavailable on this machine: … is outside the user
   profile".
4. The other rules must stay usable.
5. Restore `TEMP`.

## VM-8 — No network

1. Launch the application, open Windows Resource Monitor, Network tab.
2. Walk through both screens, analyze, clean.
3. Check that no outbound connection is attributed to the WinCleaner process.

## VM-9 — Theme

1. Toggle "Dark theme" / "Light theme": both screens must stay legible, with no
   dark text on a dark background.
