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
2. Walk through the Cleanup and Startup screens, analyze, clean. Open Settings
   but do **not** click Check for updates, and leave "Check automatically at
   startup" off.
3. Check that no outbound connection is attributed to the WinCleaner process.

## VM-8b — The update check, and nothing else

Run step 3 (the "exactly one connection" check) against a **release build**
(`npm run tauri build`, then the installed exe), not `npm run tauri dev`:
React StrictMode double-invokes effects in development, so the same startup
check can appear to fire twice there even though the 10-second cooldown in
`commands.rs::check_for_updates` collapses the second call into the first
result without a second connection.

1. Launch the application, open Windows Resource Monitor, Network tab.
2. Settings screen. Confirm "Check automatically at startup" is **off** on a
   fresh profile and that nothing has been sent yet.
3. Click **Check for updates**. Exactly **one** connection must appear, to
   `api.github.com`, and nothing else — no second request, no other host.
4. Expected result **while the repository is private**: "No public release is
   available yet". Once the repository is public and a release exists, the
   panel shows either "You're up to date (<version>)" or "WinCleaner <version>
   is available" with its date, its notes as plain text, and its release URL
   with a **Copy link** button.
5. Disconnect the network and click again: "Could not reach GitHub — check your
   connection". No toast, no crash, the button returns to its idle state.
6. Turn the switch on, close the application, reopen it: one check runs at
   start. Turn it off, reopen: no connection at all is attributed to the
   process.

## VM-9 — Theme

1. Toggle "Dark theme" / "Light theme": both screens must stay legible, with no
   dark text on a dark background.

## VM-12 — Applications category and Winapp2 detection — to be run by the user

Not run automatically: the result depends on what is installed on the machine.

1. Launch the release binary, Cleanup screen.
2. The summary line under the search field must report a non-zero number of
   built-in rules, a number of detected Winapp2 rules, and a number of
   converted rules at least as large as the detected one.
3. The "Applications" category must be **folded**, showing its name and its
   rule count.
4. Unfold it: every row must be **unchecked**, carry the "medium risk" badge,
   and the attribution "Community rules from Winapp2 (CC-BY-SA 4.0)" must sit
   under the list.
5. Pick a rule whose application you know is **not** installed (for example a
   browser you have never used): it must be absent from the list.
6. Type a few letters of a rule label in the search field: matching rules from
   every category must show, including from the folded Applications category,
   and non-matching categories must disappear entirely.
7. Clear the search field: the Applications category must be folded again.
8. Check one Applications rule, Analyze, and expand its paths: every listed
   path must be under the user profile.

## VM-13 — Sort by size — to be run by the user

1. Cleanup screen, check a few rules across two categories, Analyze.
2. Tick "Sort by size" in the hero: inside each category the largest rule must
   come first, and the category with the largest total must come first.
3. Close and relaunch the application: the toggle must still be ticked, and
   untick it before a scan — it must be greyed out until the next Analyze.

## VM-14 — What's new after a version change — to be run on a release binary

Not testable in development: `localStorage` belongs to the release WebView and
DevTools is unavailable there, so the stored version cannot be edited by hand.
The only honest check is a real upgrade.

1. Install 0.2.0 (NSIS installer), launch it, close it. **No** "What's new"
   toast must appear: this is a first install, and
   `wincleaner.lastSeenVersion` was only just written.
2. Install 0.3.0 over it and launch.
3. A toast **"What's new in 0.3.0"** must appear once, with a **View** action
   that opens the Settings screen.
4. On Settings: the version must read 0.3.0, and "What's new in 0.3.0" must
   show the `CHANGELOG.md` section of 0.3.0 as plain text — bullets as literal
   `- ` lines, no rendered links, no HTML.
5. Close and relaunch: the toast must **not** come back.

## VM-15 — Everything is measured, only the checked rules are cleaned — to be run by the user

1. Cleanup screen, before any scan: leave the default checkboxes as they are
   and unfold **Applications**. Analyze.
2. The hero must read "Analyzing N rules…" where N is the number of rules that
   are **not** greyed out as unavailable — not the number of checked ones.
3. After the scan, unchecked rows (Winapp2 entries, Recycle Bin, Recent items,
   Crash dumps) must show a size and a file count, exactly like the checked
   ones. A rule greyed out as unavailable must still show no result.
4. The "Reclaimable" total, the gauge and the size on the **Clean** button must
   equal the sum of the **checked** rules only. Under the total, the line
   "X more in unchecked rules" must state the rest; it is absent when
   everything measured is checked.
5. Check one rule that was measured but unchecked: the total, the gauge and the
   Clean label must jump immediately, with **no** new scan (no spinner, no
   "Analyzing…") and every row keeping the size it was measured at. Uncheck it:
   everything must go back.
6. Uncheck **every** rule: the Clean button must go disabled while the sizes
   stay on screen.
7. Click Clean with a mix of checked and unchecked measured rules: the
   confirmation must name only checked rules under "No way back", and the
   cleanup must free at most the checked total.

## VM-16 — Progress during Analyze — to be run by the user

1. Cleanup screen, click **Analyze** and watch the hero without touching
   anything else.
2. Before the first rule comes back, the hero reads "Analyzing N rules…" and
   the bar is empty. Within a second it must switch to
   "Analyzing 1 / N · <rule name>" and the bar must start filling.
3. The counter must only ever go up, reach exactly N / N, and name a different
   rule as it goes. The big number above it must climb with it — that is the
   bytes measured so far, not the final total.
4. When the scan ends, the bar and the counter must give the slot back to the
   normal hero: "Reclaimable", the total of the checked rules and the gauge.
5. Turn on **Settings > Ease of Access > Show animations in Windows = Off**
   (`prefers-reduced-motion`), relaunch and Analyze again: the bar must jump
   from step to step with no sliding animation, and still reach N / N.
6. Analyze a second time straight after the first: the counter must restart at
   1 / N, never resume where the previous run stopped.

## VM-17 — Sandbox mode — to be run by the user

Not run automatically: it is the whole point of the item that a human watches
the real engine clean a real tree. Nothing of yours is reachable while it runs,
but only your own eyes can confirm that.

1. Note what your Windows Recycle Bin holds, and open **Task Manager > Startup
   apps** to note what is enabled there. Both must be untouched at the end.
2. Run `npm run tauri dev`, **Settings > Sandbox**. Read the two sentences,
   then click **Create a sandbox profile**.
3. A banner must appear above every screen: "Sandbox mode — cleaning affects
   only the test profile at `%TEMP%\wincleaner-sandbox-<id>`". Switch screens:
   the banner follows.
4. Open that directory in Explorer. It must hold `profile\`, `outside\`,
   `outside2\`, `outside3\`, `outside4\` and `control\`. Open
   `profile\Documents` and confirm `thesis.docx` is there — that file must
   still be there at the end, byte for byte.
5. **Startup** screen: it must show the notice, not the table. Nothing in Task
   Manager > Startup apps may have changed.
6. **Cleanup** screen: the rule list is the sandbox's own, and **every rule
   that applies starts checked** — the sandbox exists to exercise the whole
   catalogue. Click **Analyze**. The reclaimable total must be non-zero and
   every native rule must report something.
7. Choose **Permanent** in the bottom bar, click **Clean**, then **Confirm
   cleanup**.
8. A **Sandbox verdict** card must appear, green, reading "Sentinels intact
   N / N", "Junk removed M / M for the R rules cleaned" and "Junction baits
   untouched 2 / 2". If it is red, read the paths it lists and stop — that is a
   real containment failure.
8b. Uncheck most of the rules, **Analyze** and **Clean** again on a fresh
   sandbox: the verdict must still be green, and "Junk removed" must name the
   smaller number of rules you cleaned. A partial selection is not a failure.
9. Check Explorer again: `profile\Documents\thesis.docx`, `profile\.ssh\`,
   `outside\secret.txt` and `control\untouched.dat` must all still be there;
   `profile\AppData\Local\Temp` must be empty of the junk.
10. Repeat steps 6–8 with **Recycle Bin** mode on a fresh sandbox: the removed
    files must land in `<root>\recycle-bin`, and your **real** Recycle Bin must
    still hold exactly what it held at step 1 — nothing added, nothing emptied.
10b. While the sandbox is active, open one of its files in an editor that
    keeps a lock on it and click **Leave**: the leave must fail with a message
    naming the directory, **and the banner must stay**. Close the file and
    leave again.
11. Click **Leave** in the banner. The banner disappears, the rule list goes
    back to the machine's own catalogue, the Startup screen lists your real
    entries again, and `%TEMP%\wincleaner-sandbox-<id>` is gone from Explorer.
12. Confirm one last time that your Recycle Bin and your startup entries are
    exactly as they were at step 1.

## VM-18 — Keyboard only, and a screen reader — to be run by the user

A browser test proves the markup; only a real pass proves the experience.
Unplug the mouse, or simply do not touch it.

1. Cleanup screen. Press `Tab` once from a fresh start: the focus must land on
   the sidebar, on **Cleanup**, with a ring you can see without hunting for it.
   Press `ArrowDown` twice: **Startup**, then **Settings**, each screen opening
   as the focus reaches it. `ArrowUp` twice to come back, `End` and `Home` to
   jump to the last and the first.
2. From the sidebar, one more `Tab` must leave it altogether — not three.
3. Keep tabbing through the Cleanup screen. Every stop must show a ring: the
   theme toggle, the Sort by size checkbox, Analyze, the search field, each
   category header, each rule checkbox, the mode selector, Clean. Nothing may
   be reachable without a visible ring, and nothing may be skipped.
4. On a rule checkbox, press `Space`: it must tick and untick. On a category
   header, press `Enter`: the rows must fold and unfold, chevron included.
5. `Enter` on **Analyze**. The progress must run to the end and the focus must
   stay where you left it.
6. `Enter` on **Clean**. The focus must jump to the confirmation, which reads
   what is about to be deleted. `Tab` must reach **Cancel** then **Confirm
   cleanup**, in that order. Press `Escape`: the confirmation goes, nothing is
   deleted, and the focus is back on **Clean** — press `Enter` again to check
   it really is.
7. Repeat step 6 and use **Cancel** instead of `Escape`: the focus must come
   back to **Clean** the same way.
8. Turn on **Settings > Ease of Access > Show animations in Windows = Off**
   (`prefers-reduced-motion`) and relaunch: the chevrons must snap, the gauge
   and the progress bar must jump, and the "Checking…" and "Cleaning…" loaders
   must stop spinning. Everything must still say what it is doing in words.
9. With Narrator on (`Win`+`Ctrl`+`Enter`), Analyze once: it must speak the
   progress a few times, not on every rule, and state the total at the end.
   Clean once: the report must be spoken without you going to look for it. The
   reclaim gauge must read as "Reclaimable X: <rule> Y, <rule> Z…", not as a
   silent image.
10. Startup screen with Narrator: the table must be announced by its caption,
    and flipping a row must speak "<name> enabled at startup" or "disabled".

## VM-19 — Progress during Clean, and batched Recycle Bin deletion — to be run by the user

Not run automatically: it deletes for real, and the whole point is the time it
takes on a profile that is actually loaded.

1. Cleanup screen. Set the mode to **Recycle Bin**, check **Temporary files**
   and whatever else is large, leave **Recycle Bin** itself unchecked, and
   click **Analyze**. Note the total and the file count.
2. Click **Clean**, then **Confirm cleanup**, and watch the hero without
   touching anything else. Start a stopwatch.
3. Before the first event, the hero reads "Cleaning N rules…" with an empty
   bar. Within a second it must switch to "Cleaning 1 / N · <rule name>" and
   the bar must start filling.
4. The counter must only ever go up, name a different rule as it goes, and
   reach exactly N / N. The big number must climb with it — that is what has
   been freed so far — and the file count under it likewise. A rule holding
   tens of thousands of files must keep reporting **while** it works: the
   numbers must move several times a second, not once when the rule ends.
5. The bottom bar must keep its disabled "Cleaning…" button throughout: the
   progress belongs to the hero, the action bar does not move.
6. Stop the stopwatch. A rule of a few tens of thousands of small files should
   take **tens of seconds, not tens of minutes** — the measured run this
   replaces was 59,000 files in thirteen minutes, one file per call.
7. Open the Windows Recycle Bin: every file must be in it, restorable, with the
   right count. Nothing may have been deleted permanently.
8. Check the report: "X freed · N files deleted", with "Skipped" empty, or
   naming only files that really are still on disk (open ones, typically a
   running browser). A file listed under "Skipped" that is no longer there is
   the bug this item exists to catch.
9. Run it again with a browser open so a batch fails: the other files of that
   batch must still be counted as deleted, and only the locked one named.
10. Turn on **Settings > Ease of Access > Show animations in Windows = Off**
    (`prefers-reduced-motion`), relaunch and clean again: the bar must jump
    from step to step with no sliding animation, and still reach N / N.
11. With Narrator on, clean once: it must speak the progress a few times, not
    on every batch, and state the report at the end.

## VM-20 — Exclusions — to be run by the user

Not run automatically: the point is that a real file the user pointed at is
still on disk after a real clean, and that the stored file carries nothing
machine-specific.

Run it in **Sandbox mode** first (Settings > Sandbox > Create): the store then
lives in the sandbox root and nothing real is at stake. Then repeat steps 1-5
outside the sandbox on a rule you are happy to clean.

1. Cleanup screen, **Analyze**. On a rule with several files, open **Show the
   paths**. Each line must show two icon actions on hover — a file and a folder
   — and they must also appear when you reach the line with <kbd>Tab</kbd>
   alone, never only on hover.
2. With the keyboard only, tab to the first line's file action. Narrator must
   read **"Exclude &lt;the full path&gt; from &lt;the rule&gt;"** — not just
   "Exclude this file", which would name nothing in particular. Press
   <kbd>Enter</kbd>.
3. A toast must confirm, naming the pattern in its `%VAR%\…` form. The line
   must disappear from the list, the rule's **file count must drop by one**,
   and a "Analyze again to refresh the figures" note must appear under the
   rule. The byte figure must **not** change: a scan result carries no
   per-file size, and inventing one would be worse than saying it is stale.
4. Exclude a second file from the same rule, a few lines down. It must be that
   line that disappears, not its neighbour — the indices of the remaining rows
   must not have shifted.
5. **Clean** that rule without re-analyzing. Once it finishes, look on disk:
   the two excluded files must still be there, everything else the rule matched
   must be gone, and the report's deleted count must be exactly the analyzed
   count minus two. This is the item that matters most: it proves `clean`
   re-walks the rule instead of trusting what the window was holding.
6. Use **Exclude its folder** on a file sitting in a subdirectory. Analyze
   again: nothing under that folder may appear in the list any more.
7. Settings > **Exclusions**: every entry must be listed with its pattern, its
   rule label and its date. No absolute path, and no account name, may appear
   anywhere in that section.
8. Open the store by hand — `%APPDATA%\WinCleaner\exclusions.toml`, or
   `<sandbox root>\exclusions.toml` in the sandbox. Every `pattern` must start
   with `%TEMP%`, `%LOCALAPPDATA%`, `%APPDATA%` or `%USERPROFILE%`. A drive
   letter or your account name in that file is the bug this item exists to
   catch.
9. Remove one entry from the Settings list. The row must go, and a fresh
   Analyze must count the file it was sparing again.
10. **Fail closed.** With the application closed, replace the store's contents
    with `garbage = = =` and relaunch. Analyze must **fail with an error
    naming the exclusions file**, and Clean likewise — neither may quietly
    proceed as though nothing were excluded. Restore the file (or delete it)
    afterwards: a deleted store is a legitimate empty list and must analyze
    normally again.
11. Leave the sandbox and confirm the exclusions you made inside it are gone
    from Settings: the sandbox has its own store and must never have touched
    the real one.

## VM-21 — The Space screen — to be run by the user

Not run automatically: the point is that the figures match the user's own
disk, that Explorer really comes up on the right item, and that the screen
offers no way to delete anything.

1. Sidebar: a fourth entry, **Space** (**Espace** in French), between Cleanup
   and Startup. Reach it with <kbd>Tab</kbd> then the arrow keys alone — one
   tab stop for the whole sidebar, as on the other screens.
2. Click **Measure**. The bar must fill in steps, one per folder, and the
   counter must read `1 / 6` … `6 / 6` — never sit at zero and then jump to
   the end. Narrator must announce the progress a few times, and once more
   when it finishes.
3. When it finishes: a total, then one bar per folder with its size and file
   count. Compare two of them with the folder's own **Properties > Size** in
   Explorer. They must agree to within the odd file Windows itself skips.
   A folder you do not have (no `Music`) must simply not be listed — not a
   zero row, not an error.
4. **Largest files**: one hundred rows at most, biggest first, with a real
   absolute path, a size and a date formatted for the interface language
   (`4 mai 2023` in French, `May 4, 2023` in English).
5. Click the reveal button on the first file. Explorer must open **with that
   file selected**, and no console window may flash on the way.
6. **Largest folders**: twenty rows at most, none of them one of the six known
   folders themselves, and none more than three levels below one. Click a
   reveal button: Explorer must open **inside** that folder.
7. Delete one of the listed files in Explorer, come back **without measuring
   again**, and click its reveal button. A toast must say the path no longer
   exists. Nothing may crash, and the row may stay: the list is a snapshot.
8. Look over the whole screen: there must be no checkbox, no delete button and
   no clean button anywhere on it. This screen shows; it never removes.
9. Settings > Sandbox > **Create**, then Space. The screen must state that it
   is unavailable while the sandbox is active, and offer no Measure button at
   all. Leave the sandbox: the screen must work again.
10. If one of your known folders is redirected outside your profile (a
    `Videos` folder moved to `D:\`), it must be **named** under the bars as
    not measured, and contribute nothing to the total — never silently walked.

## VM-22 — The Recycle Bin measurement cache — to be run by the user

Not run automatically: the whole point is the real `SHQueryRecycleBinW` on a
real, loaded bin. The suite only ever sees a `TempDir` store and a made-up
fingerprint.

1. Make sure the Recycle Bin actually holds something — a few thousand files
   if you can, otherwise the difference is not visible. Delete
   `%APPDATA%\WinCleaner\recycle-bin.toml` if it exists, so this is a cold
   start.
2. Cleanup > tick **Recycle Bin** > **Analyze**, and time it. On a full bin
   the run takes as long as the bin does — minutes, in the case this was
   written for. While it runs, the counter must read **still measuring:
   Recycle Bin** (French: **en cours : Corbeille**), not the name of whatever
   rule happened to finish last.
3. `%APPDATA%\WinCleaner\recycle-bin.toml` must now exist and hold a
   `fingerprint`, an `items` count and a `bytes` figure. The file must carry
   no path and no account name — only drive letters, your SID and timestamps.
4. **Analyze again, changing nothing.** It must come back in seconds, the
   Recycle Bin row must show the same figures as before, and that row must be
   marked **cached** (French: **en cache**). Hover it: the tooltip says the
   bin has not changed since.
5. Now empty the Recycle Bin from Explorer and **Analyze** again. The row must
   be re-measured — no **cached** mark — and must read 0 bytes. Send one file
   to the bin and Analyze once more: re-measured again, and the figure follows
   the file.
6. Corrupt the store on purpose (`echo not toml > "%APPDATA%\WinCleaner\
   recycle-bin.toml"`) and Analyze. Nothing may fail and nothing may be
   refused: the Analyze simply measures again, and the file is replaced with a
   valid one. This store fails **open** — unlike `exclusions.toml`, which
   aborts the scan — because a lost cache costs time, never a file.
7. Clean with the Recycle Bin ticked. What gets emptied must be whatever is
   actually in the bin, never the cached figure: the report's freed total is
   allowed to differ from what the row showed, and the bin must come back
   empty in Explorer.
