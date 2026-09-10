/// Update-check bookkeeping, kept pure so the rules can be tested without
/// rendering the panel or touching the network: release notes are flattened to
/// plain text here, and the "tell the user once per version" rule lives here
/// too.

export const AUTO_CHECK_KEY = "wincleaner.autoCheckUpdates";
export const LAST_NOTIFIED_KEY = "wincleaner.lastNotifiedVersion";

/// Flattens a GitHub release body into text.
///
/// Release notes are attacker-influenced markdown if the answer is ever
/// spoofed, so nothing is ever *rendered*: no markdown dependency, no HTML, no
/// `dangerouslySetInnerHTML` (docs/design-updater.md §3). This only removes the
/// three markers that read badly as raw text — headings, link targets and
/// backticks — and leaves every other character exactly as it came.
export function toPlainText(notes: string): string {
  return notes
    .replace(/^\s{0,3}#{1,6}\s+/gm, "")
    .replace(/\[([^\]]*)\]\([^)]*\)/g, "$1")
    .replace(/`/g, "");
}

/// One toast per newer version, not one per launch.
export function shouldNotify(lastNotified: string | null, latest: string | null): boolean {
  return latest !== null && latest !== "" && lastNotified !== latest;
}

/// The wording of every failure, keyed by the stable code
/// `src-tauri/src/update.rs` sends across the IPC boundary.
export function updateErrorMessage(code: string): string {
  switch (code) {
    case "not-available":
      return "No public release is available yet";
    case "rate-limited":
      return "GitHub rate limit reached, try again later";
    case "malformed":
      return "Could not read GitHub's answer";
    default:
      return "Could not reach GitHub — check your connection";
  }
}

/// Off unless the user has explicitly turned it on: an application whose
/// headline promise is "no network access" does not get to opt itself in.
export function readAutoCheck(): boolean {
  try {
    return localStorage.getItem(AUTO_CHECK_KEY) === "true";
  } catch {
    // localStorage unavailable: stay off, which is the safe answer.
    return false;
  }
}

export function writeAutoCheck(enabled: boolean): void {
  try {
    localStorage.setItem(AUTO_CHECK_KEY, enabled ? "true" : "false");
  } catch {
    // Nothing to do: the choice still holds for this session.
  }
}

export function readLastNotified(): string | null {
  try {
    return localStorage.getItem(LAST_NOTIFIED_KEY);
  } catch {
    return null;
  }
}

export function markNotified(version: string): void {
  try {
    localStorage.setItem(LAST_NOTIFIED_KEY, version);
  } catch {
    // Nothing to do: the toast may show once more on the next launch.
  }
}
