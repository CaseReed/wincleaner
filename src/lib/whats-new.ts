/// "What's new" bookkeeping: which version the user has already been told
/// about. Pure logic plus two `localStorage` accessors, so the announcement
/// rule is testable without rendering the application.

export const LAST_SEEN_KEY = "wincleaner.lastSeenVersion";

/// Announce only when a *previous* version was already recorded: on a first
/// install there is nothing new to show, and greeting a brand new user with
/// release notes reads as noise.
export function shouldAnnounce(previous: string | null, current: string): boolean {
  return previous !== null && previous !== "" && previous !== current;
}

export function readLastSeen(): string | null {
  try {
    return localStorage.getItem(LAST_SEEN_KEY);
  } catch {
    // localStorage unavailable: behave like a first install, stay quiet.
    return null;
  }
}

export function markSeen(version: string): void {
  try {
    localStorage.setItem(LAST_SEEN_KEY, version);
  } catch {
    // Nothing to do: the notice may show again on the next launch.
  }
}
