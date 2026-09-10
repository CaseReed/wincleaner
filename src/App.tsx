import { useEffect, useState } from "react";
import { Toaster } from "@/components/ui/sonner";
import { AppShell, type Screen } from "@/components/AppShell";
import { CleanPanel } from "@/components/CleanPanel";
import { StartupPanel } from "@/components/StartupPanel";

const THEME_KEY = "wincleaner.theme";

/// Le choix mémorisé gagne ; sinon on suit le thème du système.
function initialDark(): boolean {
  try {
    const stored = localStorage.getItem(THEME_KEY);
    if (stored === "dark") return true;
    if (stored === "light") return false;
  } catch {
    // localStorage indisponible : on retombe sur le système.
  }
  return (
    typeof window.matchMedia === "function" &&
    window.matchMedia("(prefers-color-scheme: dark)").matches
  );
}

export default function App() {
  const [screen, setScreen] = useState<Screen>("clean");
  const [dark, setDark] = useState(initialDark);

  useEffect(() => {
    document.documentElement.classList.toggle("dark", dark);
    try {
      localStorage.setItem(THEME_KEY, dark ? "dark" : "light");
    } catch {
      // Rien à faire : le thème reste valable pour cette session.
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
