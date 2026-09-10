import { useEffect, useState } from "react";
import { getCurrentWindow } from "@tauri-apps/api/window";
import { Toaster } from "@/components/ui/sonner";
import { AppShell, type Screen } from "@/components/AppShell";
import { CleanPanel } from "@/components/CleanPanel";
import { StartupPanel } from "@/components/StartupPanel";

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

  useEffect(() => {
    document.documentElement.classList.toggle("dark", dark);
    syncWindowTheme(dark);
    try {
      localStorage.setItem(THEME_KEY, dark ? "dark" : "light");
    } catch {
      // Nothing to do: the theme still holds for this session.
    }
  }, [dark]);

  return (
    <AppShell
      screen={screen}
      onScreenChange={setScreen}
      dark={dark}
      onToggleTheme={() => setDark((v) => !v)}
    >
      {screen === "clean" ? <CleanPanel /> : <StartupPanel />}
      <Toaster theme={dark ? "dark" : "light"} />
    </AppShell>
  );
}
