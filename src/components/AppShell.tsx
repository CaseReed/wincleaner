import type { ReactNode } from "react";
import { FlaskConical, Moon, Power, Settings, Sparkles, Sun } from "lucide-react";
import { Button } from "@/components/ui/button";
import { cn } from "@/lib/utils";
import type { SandboxSummary } from "@/lib/api";

export type Screen = "clean" | "startup" | "settings";

const SCREENS: { id: Screen; label: string; icon: typeof Sparkles }[] = [
  { id: "clean", label: "Cleanup", icon: Sparkles },
  { id: "startup", label: "Startup", icon: Power },
  { id: "settings", label: "Settings", icon: Settings },
];

function NavItem({
  active,
  label,
  icon: Icon,
  onClick,
}: {
  active: boolean;
  label: string;
  icon: typeof Sparkles;
  onClick: () => void;
}) {
  return (
    <button
      type="button"
      aria-current={active ? "page" : undefined}
      onClick={onClick}
      className={cn(
        "relative flex h-9 w-full items-center gap-2.5 rounded-md px-2.5 text-sm font-medium outline-none transition-colors",
        "focus-visible:ring-3 focus-visible:ring-ring/50",
        active
          ? "bg-primary/12 text-foreground before:absolute before:top-2 before:bottom-2 before:left-0 before:w-[3px] before:rounded-full before:bg-primary before:content-['']"
          : "text-muted-foreground hover:bg-sidebar-accent hover:text-foreground",
      )}
    >
      <Icon className={cn("size-4 shrink-0", active && "text-primary")} />
      {label}
    </button>
  );
}

export function AppShell({
  screen,
  onScreenChange,
  dark,
  onToggleTheme,
  sandbox = null,
  onLeaveSandbox,
  children,
}: {
  screen: Screen;
  onScreenChange: (screen: Screen) => void;
  dark: boolean;
  onToggleTheme: () => void;
  /// The active sandbox, or null when the engine runs against the real
  /// profile. Non-null is a state the user must never be able to forget they
  /// are in: the banner below sits above every screen.
  sandbox?: SandboxSummary | null;
  onLeaveSandbox?: () => void;
  children: ReactNode;
}) {
  return (
    <div className="flex h-screen overflow-hidden bg-background text-foreground">
      <nav className="flex w-[220px] shrink-0 flex-col border-r bg-sidebar px-4 py-4">
        <p className="mb-6 px-2.5 text-sm font-semibold tracking-tight">
          WinCleaner
        </p>
        <div className="flex flex-col gap-1">
          {SCREENS.map(({ id, label, icon }) => (
            <NavItem
              key={id}
              active={screen === id}
              label={label}
              icon={icon}
              onClick={() => onScreenChange(id)}
            />
          ))}
        </div>
        <button
          type="button"
          data-testid="theme-toggle"
          aria-label={dark ? "Switch to light theme" : "Switch to dark theme"}
          onClick={onToggleTheme}
          className="mt-auto flex h-9 w-full items-center gap-2.5 rounded-md px-2.5 text-sm font-medium text-muted-foreground outline-none transition-colors hover:bg-sidebar-accent hover:text-foreground focus-visible:ring-3 focus-visible:ring-ring/50"
        >
          {dark ? <Sun className="size-4" /> : <Moon className="size-4" />}
          {dark ? "Light theme" : "Dark theme"}
        </button>
      </nav>
      {/* `relative`: the pane becomes the containing block for
          `position: absolute` descendants (the `.sr-only` nodes), otherwise
          they escape every `overflow` and stretch the document. */}
      <main className="relative flex min-h-0 min-w-0 flex-1 flex-col overflow-hidden">
        {sandbox && (
          <div
            data-testid="sandbox-banner"
            className="flex shrink-0 items-center gap-2.5 border-b border-warning/40 bg-warning/12 px-8 py-2.5 text-sm text-warning-foreground"
          >
            <FlaskConical className="size-4 shrink-0 text-warning" />
            <p className="min-w-0 flex-1">
              Sandbox mode — cleaning affects only the test profile at{" "}
              <span className="font-mono text-xs break-all">{sandbox.root}</span>
            </p>
            <Button
              variant="outline"
              size="sm"
              className="shrink-0"
              onClick={onLeaveSandbox}
            >
              Leave
            </Button>
          </div>
        )}
        {children}
      </main>
    </div>
  );
}
