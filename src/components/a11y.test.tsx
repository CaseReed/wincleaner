import { describe, it, expect, vi, beforeEach } from "vitest";
import { render, screen } from "@testing-library/react";
import userEvent from "@testing-library/user-event";
import axe, { type Result } from "axe-core";

const api = {
  listRules: vi.fn(),
  rulesSummary: vi.fn(),
  scan: vi.fn(),
  clean: vi.fn(),
  runningBrowsers: vi.fn(),
  onScanProgress: vi.fn(),
  listStartup: vi.fn(),
  setStartupEnabled: vi.fn(),
  sandboxOrphans: vi.fn(),
};

vi.mock("@/lib/api", async () => {
  const actual = await vi.importActual<typeof import("@/lib/api")>("@/lib/api");
  return {
    ...actual,
    listRules: () => api.listRules(),
    rulesSummary: () => api.rulesSummary(),
    scan: (ids: string[]) => api.scan(ids),
    clean: (ids: string[], mode: string) => api.clean(ids, mode),
    runningBrowsers: () => api.runningBrowsers(),
    onScanProgress: (cb: (p: unknown) => void) => api.onScanProgress(cb),
    listStartup: () => api.listStartup(),
    setStartupEnabled: (id: string, on: boolean) => api.setStartupEnabled(id, on),
    sandboxOrphans: () => api.sandboxOrphans(),
  };
});

vi.mock("sonner", () => ({ toast: { error: vi.fn(), success: vi.fn(), info: vi.fn() } }));

import { AppShell } from "./AppShell";
import { CleanPanel } from "./CleanPanel";
import { SettingsPanel } from "./SettingsPanel";
import { StartupPanel } from "./StartupPanel";

const RULES = [
  { id: "windows.temp", category: "System", label: "Temporary files", risk: "low", kind: "files", default_checked: true },
  { id: "edge.cache", category: "Browsers", label: "Microsoft Edge cache", risk: "low", kind: "files", default_checked: true },
];

const RESULTS = [
  {
    rule_id: "windows.temp",
    file_count: 2,
    total_bytes: 2048,
    paths: [String.raw`C:\Users\T\AppData\Local\Temp\a.txt`],
    skipped: 0,
  },
  { rule_id: "edge.cache", file_count: 1, total_bytes: 1024, paths: [], skipped: 3 },
];

const ENTRIES = [
  {
    id: "run:OneDrive",
    name: "OneDrive",
    command: String.raw`C:\Program Files\OneDrive\OneDrive.exe /background`,
    source: "run",
    enabled: true,
  },
  {
    id: "run-once:Patch",
    name: "Patch",
    command: String.raw`C:\Temp\patch.exe`,
    source: "run-once",
    enabled: true,
  },
];

/// jsdom computes no layout and paints nothing, so axe's `color-contrast`
/// check has no pixels to sample and would only ever report "incomplete".
/// The palette is measured from the oklch tokens instead, in
/// `src/theme-contrast.test.ts`.
const RULES_OFF = { "color-contrast": { enabled: false } };

async function violations(container: HTMLElement): Promise<Result[]> {
  const report = await axe.run(container, { rules: RULES_OFF });
  return report.violations;
}

function describeViolations(found: Result[]): string {
  return found
    .map((v) => `${v.id}: ${v.help} (${v.nodes.length})`)
    .join("\n");
}

const SANDBOX = {
  root: String.raw`C:\Users\T\AppData\Local\Temp\wincleaner-sandbox-1a2b`,
  sentinels: 52,
  junk: 119,
  winapp2_rules: 14,
};

/// "Cleanup" is the sidebar entry, "Clean 2 KB" the button in the action bar:
/// both start with the same five letters.
const clean = (name: string) => name.startsWith("Clean") && name !== "Cleanup";

/// Everything the Tab key can land on. base-ui backs its checkboxes and
/// switches with a visually hidden `<input>` that exists only to carry a form
/// value — it is `aria-hidden` and takes no focus, so it is not a tab stop.
function tabStops(container: HTMLElement): HTMLElement[] {
  return [
    ...container.querySelectorAll<HTMLElement>(
      'button, input, select, textarea, a[href], [tabindex]:not([tabindex="-1"])',
    ),
  ].filter((el) => el.getAttribute("aria-hidden") !== "true" && !el.style.clipPath);
}

function withoutFocusRing(stops: HTMLElement[]): string[] {
  return stops
    .filter((el) => !/focus-visible:(ring|border)/.test(el.className))
    .map((el) => el.outerHTML.slice(0, 120));
}

/// The shell is part of every screen: a panel is judged where it actually
/// lives, landmarks and sandbox banner included.
function renderScreen(panel: React.ReactNode, sandbox: Parameters<typeof AppShell>[0]["sandbox"] = null) {
  return render(
    <AppShell
      screen="clean"
      onScreenChange={() => {}}
      dark={false}
      onToggleTheme={() => {}}
      sandbox={sandbox}
    >
      {panel}
    </AppShell>,
  );
}

describe("accessibility", () => {
  beforeEach(() => {
    api.onScanProgress.mockReset().mockResolvedValue(vi.fn());
    api.listRules.mockReset().mockResolvedValue(RULES);
    api.scan.mockReset().mockResolvedValue(RESULTS);
    api.clean.mockReset().mockResolvedValue({ freed_bytes: 2048, deleted: 2, skipped: [] });
    api.runningBrowsers.mockReset().mockResolvedValue([]);
    api.rulesSummary.mockReset().mockResolvedValue({
      native: 10,
      winapp2_retained: 1200,
      winapp2_detected: 42,
      winapp2_dropped: 900,
    });
    api.listStartup.mockReset().mockResolvedValue(ENTRIES);
    api.setStartupEnabled.mockReset().mockResolvedValue(undefined);
    api.sandboxOrphans.mockReset().mockResolvedValue([]);
  });

  it("the cleanup screen has no violation before a scan", async () => {
    const { container } = renderScreen(<CleanPanel />);
    await screen.findByLabelText("Temporary files");
    const found = await violations(container);
    expect(describeViolations(found)).toBe("");
  });

  it("the cleanup screen has no violation once it shows results", async () => {
    const user = userEvent.setup();
    const { container } = renderScreen(<CleanPanel />);
    await screen.findByLabelText("Temporary files");
    await user.click(screen.getByRole("button", { name: /Analyze/ }));
    await screen.findByTestId("total-bytes");
    await user.click(screen.getByTestId("toggle-paths-windows.temp"));

    const found = await violations(container);
    expect(describeViolations(found)).toBe("");
  });

  it("the cleanup screen has no violation while the confirmation is up", async () => {
    const user = userEvent.setup();
    const { container } = renderScreen(<CleanPanel />);
    await screen.findByLabelText("Temporary files");
    await user.click(screen.getByRole("button", { name: /Analyze/ }));
    await screen.findByTestId("total-bytes");
    await user.click(screen.getByRole("button", { name: clean }));
    await screen.findByTestId("confirm-clean");

    const found = await violations(container);
    expect(describeViolations(found)).toBe("");
  });

  it("the startup screen has no violation", async () => {
    const { container } = renderScreen(<StartupPanel />);
    await screen.findByText("OneDrive");
    const found = await violations(container);
    expect(describeViolations(found)).toBe("");
  });

  it("the settings screen has no violation", async () => {
    const { container } = renderScreen(<SettingsPanel />);
    await screen.findByTestId("app-version");
    const found = await violations(container);
    expect(describeViolations(found)).toBe("");
  });

  it("the sandbox banner has no violation", async () => {
    const { container } = renderScreen(<StartupPanel sandbox={SANDBOX} />, SANDBOX);
    await screen.findByTestId("startup-sandbox-notice");
    const found = await violations(container);
    expect(describeViolations(found)).toBe("");
  });

  /// axe cannot see a focus ring in jsdom — it has no paint. What it can be
  /// held to is that no tab stop was left without one: this is the check that
  /// catches the next control added with `outline-none` and nothing in its
  /// place.
  it("every tab stop on the cleanup screen has a focus ring", async () => {
    const user = userEvent.setup();
    const { container } = renderScreen(<CleanPanel />);
    await screen.findByLabelText("Temporary files");
    await user.click(screen.getByRole("button", { name: /Analyze/ }));
    await screen.findByTestId("total-bytes");

    const stops = tabStops(container);
    expect(stops.length).toBeGreaterThan(5);
    expect(withoutFocusRing(stops)).toEqual([]);
  });

  it("every tab stop in the shell and on settings has a focus ring", async () => {
    const { container } = renderScreen(<SettingsPanel />);
    await screen.findByTestId("app-version");

    expect(withoutFocusRing(tabStops(container))).toEqual([]);
  });

  /// The Applications category is folded by default. Its `aria-controls`
  /// must still resolve to a mounted element — the rows are hidden, not
  /// removed from the DOM — and axe must see no violation in that state.
  it("keeps every aria-controls target resolvable with a category folded", async () => {
    const APP_RULE = {
      id: "winapp2.7-zip",
      category: "Applications",
      label: "7-Zip",
      risk: "medium",
      kind: "files",
      default_checked: false,
    };
    api.listRules.mockResolvedValue([...RULES, APP_RULE]);
    const { container } = renderScreen(<CleanPanel />);
    await screen.findByLabelText("Temporary files");

    const toggle = screen.getByTestId("toggle-category-Applications");
    expect(toggle).toHaveAttribute("aria-expanded", "false");

    for (const el of container.querySelectorAll("[aria-controls]")) {
      const id = el.getAttribute("aria-controls")!;
      expect(document.getElementById(id)).not.toBeNull();
    }

    const found = await violations(container);
    expect(describeViolations(found)).toBe("");
  });
});
