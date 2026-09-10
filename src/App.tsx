import { useEffect, useState } from "react";
import { Button } from "@/components/ui/button";
import { Toaster } from "@/components/ui/sonner";
import { CleanPanel } from "@/components/CleanPanel";
import { StartupPanel } from "@/components/StartupPanel";

type Screen = "clean" | "startup";

export default function App() {
  const [screen, setScreen] = useState<Screen>("clean");
  const [dark, setDark] = useState(false);

  useEffect(() => {
    document.documentElement.classList.toggle("dark", dark);
  }, [dark]);

  return (
    <div className="flex h-screen bg-background text-foreground">
      <nav className="flex w-52 flex-col gap-1 border-r p-3">
        <h1 className="mb-4 px-2 text-lg font-semibold">WinCleaner</h1>
        <Button
          variant={screen === "clean" ? "secondary" : "ghost"}
          className="justify-start"
          onClick={() => setScreen("clean")}
        >
          Nettoyage
        </Button>
        <Button
          variant={screen === "startup" ? "secondary" : "ghost"}
          className="justify-start"
          onClick={() => setScreen("startup")}
        >
          Démarrage
        </Button>
        <Button
          variant="ghost"
          className="mt-auto justify-start"
          onClick={() => setDark((v) => !v)}
        >
          {dark ? "Thème clair" : "Thème sombre"}
        </Button>
      </nav>
      <main className="flex-1 overflow-auto p-6">
        {screen === "clean" ? <CleanPanel /> : <StartupPanel />}
      </main>
      <Toaster />
    </div>
  );
}
