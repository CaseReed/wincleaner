/// The English dictionary, and the source of truth for the key set: `fr.ts` is
/// typed as `Dictionary`, so a key added here and forgotten there fails `tsc`.
///
/// What is NOT in here, deliberately: rule labels and descriptions (they come
/// from `src-tauri/rules.toml` and from Winapp2), the "What's new" body
/// (extracted from CHANGELOG.md at build time) and the release notes GitHub
/// returns. Those are data, not UI chrome, and they stay in English whatever
/// the interface language — see the note next to the Language setting.
///
/// A key ending in `.one` always has a `.other` twin: those pairs are the ones
/// `tn`/`txn` pick between.
export const en = {
  // Shell and navigation
  "nav.label": "Main navigation",
  "nav.clean": "Cleanup",
  "nav.startup": "Startup",
  "nav.settings": "Settings",
  "theme.toLight": "Switch to light theme",
  "theme.toDark": "Switch to dark theme",
  "theme.light": "Light theme",
  "theme.dark": "Dark theme",

  // Shared
  "common.retry": "Retry",
  "common.cancel": "Cancel",
  "common.remove": "Remove",
  "common.view": "View",

  // Sandbox banner and toasts
  "sandbox.banner": "Sandbox mode — cleaning affects only the test profile at {root}",
  "sandbox.leave": "Leave",
  "sandbox.created": "Sandbox profile created",
  "sandbox.removed": "Sandbox removed",

  // Cleanup screen
  "clean.title": "Cleanup",
  "clean.rulesError": "Could not load the rules.",
  "clean.hint":
    "Auto mode deletes low-risk items permanently and sends the rest to the Recycle Bin. Change the mode in the bottom bar before cleaning.",
  "clean.hintDismiss": "Got it",
  "clean.freed": "Freed",
  "clean.reclaimable": "Reclaimable",
  "clean.filesDeleted": "{count} files deleted",
  "clean.moreUnchecked": "{bytes} more in unchecked rules",
  "clean.sortBySize": "Sort by size",
  "clean.analyze": "Analyze",
  "clean.analyzing": "Analyzing…",
  "clean.analyzingRules.one": "Analyzing {count} rule…",
  "clean.analyzingRules.other": "Analyzing {count} rules…",
  "clean.cleaning": "Cleaning…",
  "clean.cleaningRules.one": "Cleaning {count} rule…",
  "clean.cleaningRules.other": "Cleaning {count} rules…",
  "clean.progressAnalyzing": "Analyzing {counter} · {label}",
  "clean.progressCleaning": "Cleaning {counter} · {label}",
  "clean.empty": "Analyze to measure what can be freed.",
  "clean.search": "Search rules",
  "clean.searchEmpty": "No rules match your search.",
  "clean.summary":
    "{native} built-in rules · {detected} Winapp2 rules detected out of {retained} converted ({dropped} entries not supported)",
  "clean.clean": "Clean",

  // Browser warning
  "browser.open":
    "{name} is open: its cache files in use will be skipped. Close it for a complete cleanup.",
  "browser.background":
    "{name} is still running in the background ({processes}): quit it from the notification area, or its cache files in use will be skipped.",
  "browser.processes.one": "{count} process",
  "browser.processes.other": "{count} processes",

  // Live-region announcements
  "announce.analyzing": "Analyzing…",
  "announce.cleaning": "Cleaning…",
  "announce.scanProgress": "Analyzing: {done} of {total} rules, {bytes} so far",
  "announce.cleanProgress": "Cleaning: {done} of {total} rules, {bytes} freed so far",
  "announce.scanDone.one": "Analysis complete: {bytes} reclaimable in {count} selected rule",
  "announce.scanDone.other": "Analysis complete: {bytes} reclaimable in {count} selected rules",
  "announce.cleanDone": "Cleanup complete: {bytes} freed, {count} files deleted",
  "toast.cleaned": "Cleaned: {bytes} freed",

  // Deletion modes
  "mode.label": "Deletion mode",
  "mode.auto": "Auto",
  "mode.trash": "Recycle Bin",
  "mode.permanent": "Permanent",
  "mode.autoHelp": "Auto: permanent deletion for low-risk items, recycle bin for the rest.",
  "mode.recycleFirst":
    "The Recycle Bin is emptied first: whatever the other rules drop into it during the same pass is not swept away.",
  "mode.emptyDirs":
    "Directories a rule empties are removed whatever the mode: an empty directory holds no data.",

  // Confirmation
  "confirm.title": "Clean {bytes} in {mode} mode?",
  "confirm.irreversible": "No way back: {rules}.",
  "confirm.reversible": "Everything goes to the recycle bin and stays recoverable.",
  "confirm.confirm": "Confirm cleanup",

  // Cleanup report
  "report.title": "Last cleanup",
  "report.summary": "{bytes} freed · {count} files deleted",
  "report.skipped": "Skipped ({count})",

  // Rule rows
  "rules.count.one": "{count} rule",
  "rules.count.other": "{count} rules",
  "rules.mediumRisk": "medium risk",
  "rules.allVolumes": "all volumes",
  "rules.skipped": "{count} skipped",
  "rules.filesSr": " files",
  "rules.unavailable": "Unavailable on this machine: {reason}",
  "rules.recycleBinNote":
    "Empties the recycle bin of every volume on this machine, including outside the user profile. Permanent and irreversible: the deletion mode does not apply to it.",
  "rules.tempNote": "Close any running installers before cleaning.",
  "rules.showPaths": "Show the paths",
  "rules.hidePaths": "Hide the paths",
  "rules.showPathsOf": "Show the paths of {label}",
  "rules.hidePathsOf": "Hide the paths of {label}",
  "rules.pathsOf": "Paths of {label}",
  "rules.winapp2Attribution":
    "Some of the rules in this category are community rules from Winapp2 (CC-BY-SA 4.0) — {url}",

  // Reclaim gauge
  "gauge.description": "Reclaimable {bytes}: {named}",
  "gauge.more.one": ", and {count} smaller rule",
  "gauge.more.other": ", and {count} smaller rules",
  "gauge.segment": "{label} — {bytes}",

  // Sandbox verdict
  "verdict.title": "Sandbox verdict",
  "verdict.passed": "passed",
  "verdict.failed": "failed",
  "verdict.sentinels": "Sentinels intact",
  "verdict.junk": "Junk removed",
  "verdict.junkScope.one": "for the {count} rule cleaned",
  "verdict.junkScope.other": "for the {count} rules cleaned",
  "verdict.junctions": "Junction baits untouched",
  "verdict.junctionBroken": "A junction the sandbox planted no longer stands.",
  "verdict.damaged": "Deleted or rewritten, and should not have been",
  "verdict.remaining": "Junk still on disk",
  "verdict.unreadable":
    "The cleanup ran; reading the sandbox back failed, so there is no verdict this time.",

  // Startup screen
  "startup.title": "Startup",
  "startup.subtitle": "Decide what starts with your session.",
  "startup.scope":
    "Only the entries of your own session are listed: the HKCU registry (Run, RunOnce) and your Startup folder. Entries shared by all users and scheduled tasks require elevation and stay out of scope.",
  "startup.sandboxTitle": "Unavailable while the sandbox is active.",
  "startup.sandboxBody":
    "Startup programs live in the real Windows registry and in your real Startup folder. The sandbox never touches either, so there is nothing here to show you. Leave the sandbox to manage them.",
  "startup.error": "Could not read the startup programs.",
  "startup.empty": "No program starts with your session.",
  "startup.summary.one": "{count} program, {enabled} enabled",
  "startup.summary.other": "{count} programs, {enabled} enabled",
  "startup.caption": "Programs that start with your session",
  "startup.colName": "Name",
  "startup.colCommand": "Command",
  "startup.colSource": "Source",
  "startup.colEnabled": "Enabled",
  "startup.readOnly": "Read-only",
  "startup.enable": "Enable {name}",
  "startup.enabled": "{name} enabled at startup",
  "startup.disabled": "{name} disabled at startup",
  "startup.changeFailed": "{name} could not be changed",
  "startup.source.run": "Registry (Run)",
  "startup.source.run-once": "Registry (RunOnce)",
  "startup.source.folder": "Startup folder",

  // Settings screen
  "settings.title": "Settings",
  "settings.subtitle": "What this build is, what changed, and what it is made of.",
  "settings.about": "About",
  "settings.aboutBody":
    "Open source, MIT. No telemetry, and no network access except one request to GitHub when you click Check for updates or enable automatic checks (off by default).",
  "settings.whatsNew": "What's new in {version}",
  "settings.updates": "Updates",
  "settings.sandbox": "Sandbox",
  "settings.notices": "Notices",
  "settings.language": "Language",
  "settings.languageSystem": "System",
  "settings.languageEn": "English",
  "settings.languageFr": "Français",
  "settings.languageNote":
    "Native rule names and descriptions are translated; community (Winapp2) rules, release notes and the changelog come from their own sources and stay in English.",

  // Updates section
  "updates.check": "Check for updates",
  "updates.checking": "Checking…",
  "updates.upToDate": "You’re up to date ({version})",
  "updates.available": "WinCleaner {version} is available",
  "updates.published": "Published {date}",
  "updates.copyLink": "Copy link",
  "updates.copied": "Release link copied",
  "updates.copyFailed": "Could not copy the link",
  "updates.autoCheck": "Check automatically at startup",
  "updates.autoCheckPrivacy":
    "When enabled, WinCleaner sends one request to api.github.com at startup with no identifiers other than the app version in the User-Agent.",
  "updates.error.not-available": "No public release is available yet",
  "updates.error.rate-limited": "GitHub rate limit reached, try again later",
  "updates.error.malformed": "Could not read GitHub's answer",
  "updates.error.offline": "Could not reach GitHub — check your connection",

  // Sandbox section
  "sandboxSection.what":
    "A sandbox is a synthetic Windows profile WinCleaner builds under your temporary directory: junk files every rule is meant to remove, plus decoy documents, keys and caches that must survive.",
  "sandboxSection.active":
    "While it is active, Analyze and Clean run for real against that profile and nothing else — nothing in your real profile is touched, and the Recycle Bin and the startup registry keys stay out of reach.",
  "sandboxSection.counts":
    "{junk} junk files · {sentinels} files that must survive · {rules} Winapp2 rules detected",
  "sandboxSection.leave": "Leave the sandbox",
  "sandboxSection.removing": "Removing…",
  "sandboxSection.create": "Create a sandbox profile",
  "sandboxSection.creating": "Creating…",
  "sandboxSection.orphans.one": "{count} old sandbox folder ({bytes})",
  "sandboxSection.orphans.other": "{count} old sandbox folders ({bytes})",
  "sandboxSection.orphansRemoved.one": "{count} old sandbox folder removed",
  "sandboxSection.orphansRemoved.other": "{count} old sandbox folders removed",

  // Notices section
  "notices.mit": "WinCleaner is released under the MIT licence.",
  "notices.winapp2": "Community rules from Winapp2 (CC-BY-SA 4.0) — {url}",
  "notices.bundled":
    "Fonts and icons are bundled with the application; nothing is fetched at runtime.",
} as const;

export type TranslationKey = keyof typeof en;

/// Every dictionary carries exactly the keys of `en`, no more and no fewer.
export type Dictionary = Record<TranslationKey, string>;
