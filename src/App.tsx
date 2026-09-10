import { useEffect, useState } from "react";
import { getCurrentWindow } from "@tauri-apps/api/window";
import { toast } from "sonner";
import { Toaster } from "@/components/ui/sonner";
import { AppShell, type Screen } from "@/components/AppShell";
import { CleanPanel } from "@/components/CleanPanel";
import { SettingsPanel } from "@/components/SettingsPanel";
import { StartupPanel } from "@/components/StartupPanel";
import whatsNew from "@/generated/whats-new.json";
import {
  checkForUpdates,
  sandboxEnter,
  sandboxLeave,
  sandboxStatus,
  type SandboxSummary,
} from "@/lib/api";
import { markSeen, readLastSeen, shouldAnnounce } from "@/lib/whats-new";
import { markNotified, readAutoCheck, readLastNotified, shouldNotify } from "@/lib/updates";

const THEME_KEY = "wincleaner.theme";

/// The remembered choice wins; otherwise we follow the system theme.
function initialDark(): boolean {
  try {
    const stored = localStorage.getItem(THEME_KEY);
    if (stored === "dark") return true;
    if (stored === "light") return false;
  } catch {
    // localStorage unavailable: fall back to the system theme.
  }
  return (
    typeof window.matchMedia === "function" &&
    window.matchMedia("(prefers-color-scheme: dark)").matches
  );
}

/// The title bar is drawn by Windows, not by the WebView: the `dark` class
/// does not reach it, only the Tauri window theme switches it.
function syncWindowTheme(dark: boolean) {
  try {
    void getCurrentWindow()
      .setTheme(dark ? "dark" : "light")
      .catch(() => {});
  } catch {
    // Outside Tauri (tests, browser): the CSS class is enough.
  }
}

export default function App() {
  const [screen, setScreen] = useState<Screen>("clean");
  const [dark, setDark] = useState(initialDark);
  /// The active sandbox, or null when the engine runs against the real
  /// profile. Owned here because three screens read it: the banner, Settings
  /// and Startup — and the Cleanup screen has to remount when it changes,
  /// because the whole rule catalogue changes with it.
  const [sandbox, setSandbox] = useState<SandboxSummary | null>(null);
  /// Creating the sandbox writes a few hundred files and leaving it removes
  /// them: both take long enough for a second click to land.
  const [sandboxBusy, setSandboxBusy] = useState(false);

  /// The backend is the authority: a reload of the webview must not lose a
  /// sandbox that is still open on the Rust side.
  useEffect(() => {
    void sandboxStatus()
      .then(setSandbox)
      .catch(() => {});
  }, []);

  async function onEnterSandbox() {
    setSandboxBusy(true);
    try {
      const summary = await sandboxEnter();
      setSandbox(summary);
      toast.success("Sandbox profile created");
    } catch (err) {
      toast.error(String(err));
    } finally {
      setSandboxBusy(false);
    }
  }

  /// A failed leave is not "still in the sandbox" by assumption: the back end
  /// is the only authority on whether one is still active, so the failure path
  /// asks it rather than guessing. Guessing wrong in either direction is the
  /// dangerous case — a banner over the real profile, or none over the
  /// sandbox.
  async function onLeaveSandbox() {
    setSandboxBusy(true);
    try {
      await sandboxLeave();
      setSandbox(null);
      toast.success("Sandbox removed");
    } catch (err) {
      toast.error(String(err));
      try {
        setSandbox(await sandboxStatus());
      } catch {
        // The status call failed too: the sandbox we know about stands, which
        // is the cautious answer.
      }
    } finally {
      setSandboxBusy(false);
    }
  }

  useEffect(() => {
    document.documentElement.classList.toggle("dark", dark);
    syncWindowTheme(dark);
    try {
      localStorage.setItem(THEME_KEY, dark ? "dark" : "light");
    } catch {
      // Nothing to do: the theme still holds for this session.
    }
  }, [dark]);

  /// Once per version change, never on a first install: the notice exists to
  /// explain what moved under the user's feet, not to greet them.
  useEffect(() => {
    if (shouldAnnounce(readLastSeen(), whatsNew.version)) {
      toast.info(`What's new in ${whatsNew.version}`, {
        action: { label: "View", onClick: () => setScreen("settings") },
      });
    }
    markSeen(whatsNew.version);
  }, []);

  /// Off unless the user armed the switch in Settings. One check, once per
  /// start, and a background failure is never a toast: a cleaner has no
  /// business complaining about its own connectivity
  /// (docs/design-updater.md §4).
  useEffect(() => {
    if (!readAutoCheck()) return;
    void checkForUpdates()
      .then((check) => {
        const latest = check.latest;
        if (!check.is_newer || latest === null) return;
        if (!shouldNotify(readLastNotified(), latest)) return;
        markNotified(latest);
        toast.info(`WinCleaner ${latest} is available`, {
          action: { label: "View", onClick: () => setScreen("settings") },
        });
      })
      .catch(() => {});
  }, []);

  return (
    <AppShell
      screen={screen}
      onScreenChange={setScreen}
      dark={dark}
      onToggleTheme={() => setDark((v) => !v)}
      sandbox={sandbox}
      onLeaveSandbox={() => void onLeaveSandbox()}
    >
      {/* Keyed on the sandbox: entering or leaving swaps the whole catalogue,
          so the panel starts over rather than showing the previous profile's
          rules and measurements. */}
      {screen === "clean" && (
        <CleanPanel key={sandbox ? sandbox.root : "real"} sandbox={sandbox} />
      )}
      {screen === "startup" && <StartupPanel sandbox={sandbox} />}
      {screen === "settings" && (
        <SettingsPanel
          sandbox={sandbox}
          sandboxBusy={sandboxBusy}
          onEnterSandbox={() => void onEnterSandbox()}
          onLeaveSandbox={() => void onLeaveSandbox()}
        />
      )}
      <Toaster theme={dark ? "dark" : "light"} />
    </AppShell>
  );
}
