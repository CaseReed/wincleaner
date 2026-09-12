import { useRef, type KeyboardEvent, type ReactNode } from "react";
import { FlaskConical, Moon, Power, Settings, Sparkles, Sun } from "lucide-react";
import { Button } from "@/components/ui/button";
import { useI18n, type TranslationKey } from "@/i18n";
import { cn } from "@/lib/utils";
import type { SandboxSummary } from "@/lib/api";

export type Screen = "clean" | "startup" | "settings";

const SCREENS: { id: Screen; label: TranslationKey; icon: typeof Sparkles }[] = [
  { id: "clean", label: "nav.clean", icon: Sparkles },
  { id: "startup", label: "nav.startup", icon: Power },
  { id: "settings", label: "nav.settings", icon: Settings },
];

function NavItem({
  active,
  label,
  icon: Icon,
  onClick,
  onKeyDown,
  buttonRef,
}: {
  active: boolean;
  label: string;
  icon: typeof Sparkles;
  onClick: () => void;
  onKeyDown: (event: KeyboardEvent<HTMLButtonElement>) => void;
  buttonRef: (node: HTMLButtonElement | null) => void;
}) {
  return (
    <button
      type="button"
      ref={buttonRef}
      aria-current={active ? "page" : undefined}
      /// Roving tabindex: the sidebar is one Tab stop, and the arrow keys move
      /// inside it. Tabbing through three entries to reach the content is the
      /// thing a keyboard user pays for on every screen change.
      tabIndex={active ? 0 : -1}
      onClick={onClick}
      onKeyDown={onKeyDown}
      className={cn(
        "relative flex h-9 w-full items-center gap-2.5 rounded-md px-2.5 text-sm font-medium outline-none transition-colors motion-reduce:transition-none",
        "focus-visible:ring-3 focus-visible:ring-ring",
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
  const { t, tx } = useI18n();
  const items = useRef<(HTMLButtonElement | null)[]>([]);

  /// Arrow keys move the focus and the screen together: the sidebar is a list
  /// of destinations, and a focused entry that is not the shown one would be a
  /// second kind of "current".
  function onNavKeyDown(index: number, event: KeyboardEvent<HTMLButtonElement>) {
    const last = SCREENS.length - 1;
    let next: number;
    switch (event.key) {
      case "ArrowDown":
      case "ArrowRight":
        next = index === last ? 0 : index + 1;
        break;
      case "ArrowUp":
      case "ArrowLeft":
        next = index === 0 ? last : index - 1;
        break;
      case "Home":
        next = 0;
        break;
      case "End":
        next = last;
        break;
      default:
        return;
    }
    event.preventDefault();
    onScreenChange(SCREENS[next].id);
    items.current[next]?.focus();
  }

  return (
    <div className="flex h-screen overflow-hidden bg-background text-foreground">
      <nav
        aria-label={t("nav.label")}
        className="flex w-[220px] shrink-0 flex-col border-r bg-sidebar px-4 py-4"
      >
        <p className="mb-6 px-2.5 text-sm font-semibold tracking-tight">
          WinCleaner
        </p>
        <div className="flex flex-col gap-1">
          {SCREENS.map(({ id, label, icon }, index) => (
            <NavItem
              key={id}
              active={screen === id}
              label={t(label)}
              icon={icon}
              onClick={() => onScreenChange(id)}
              onKeyDown={(event) => onNavKeyDown(index, event)}
              buttonRef={(node) => {
                items.current[index] = node;
              }}
            />
          ))}
        </div>
        <button
          type="button"
          data-testid="theme-toggle"
          /// A toggle, not a command: `aria-pressed` is what tells a screen
          /// reader which theme is on right now, and the label says what the
          /// press will do.
          aria-pressed={dark}
          aria-label={dark ? t("theme.toLight") : t("theme.toDark")}
          onClick={onToggleTheme}
          className="mt-auto flex h-9 w-full items-center gap-2.5 rounded-md px-2.5 text-sm font-medium text-muted-foreground outline-none transition-colors motion-reduce:transition-none hover:bg-sidebar-accent hover:text-foreground focus-visible:ring-3 focus-visible:ring-ring"
        >
          {dark ? <Sun className="size-4" /> : <Moon className="size-4" />}
          {dark ? t("theme.light") : t("theme.dark")}
        </button>
      </nav>
      {/* `relative`: the pane becomes the containing block for
          `position: absolute` descendants (the `.sr-only` nodes), otherwise
          they escape every `overflow` and stretch the document. */}
      <main className="relative flex min-h-0 min-w-0 flex-1 flex-col overflow-hidden">
        {sandbox && (
          <div
            data-testid="sandbox-banner"
            /// The engine has moved off the real profile: announced once, when
            /// it appears, rather than left to be noticed.
            role="status"
            className="flex shrink-0 items-center gap-2.5 border-b border-warning/40 bg-warning/12 px-8 py-2.5 text-sm text-warning-foreground"
          >
            <FlaskConical className="size-4 shrink-0 text-warning" aria-hidden="true" />
            <p className="min-w-0 flex-1">
              {tx("sandbox.banner", {
                root: (
                  <span className="font-mono text-xs break-all">{sandbox.root}</span>
                ),
              })}
            </p>
            <Button
              variant="outline"
              size="sm"
              className="shrink-0"
              onClick={onLeaveSandbox}
            >
              {t("sandbox.leave")}
            </Button>
          </div>
        )}
        {children}
      </main>
    </div>
  );
}
