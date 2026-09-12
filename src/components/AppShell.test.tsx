import { describe, it, expect, vi } from "vitest";
import { render, screen } from "@testing-library/react";
import userEvent from "@testing-library/user-event";
import { I18nProvider, LANGUAGE_KEY } from "@/i18n";
import { AppShell } from "./AppShell";

const SANDBOX = {
  root: String.raw`C:\Users\T\AppData\Local\Temp\wincleaner-sandbox-1a2b`,
  sentinels: 52,
  junk: 119,
  winapp2_rules: 14,
};

function renderShell(props: Partial<Parameters<typeof AppShell>[0]> = {}) {
  const onScreenChange = vi.fn();
  render(
    <AppShell
      screen="clean"
      onScreenChange={onScreenChange}
      dark={false}
      onToggleTheme={() => {}}
      {...props}
    >
      <p>panel</p>
    </AppShell>,
  );
  return { onScreenChange };
}

describe("AppShell", () => {
  it("lists the four screens", () => {
    renderShell();
    expect(screen.getByRole("button", { name: "Cleanup" })).toBeInTheDocument();
    expect(screen.getByRole("button", { name: "Space" })).toBeInTheDocument();
    expect(screen.getByRole("button", { name: "Startup" })).toBeInTheDocument();
    expect(screen.getByRole("button", { name: "Settings" })).toBeInTheDocument();
  });

  it("routes to Space, and names it in French too", async () => {
    const user = userEvent.setup();
    const { onScreenChange } = renderShell();
    await user.click(screen.getByRole("button", { name: "Space" }));
    expect(onScreenChange).toHaveBeenCalledWith("space");

    localStorage.setItem(LANGUAGE_KEY, "fr");
    render(
      <I18nProvider>
        <AppShell screen="space" onScreenChange={() => {}} dark={false} onToggleTheme={() => {}}>
          <p>panel</p>
        </AppShell>
      </I18nProvider>,
    );
    expect(screen.getByRole("button", { name: "Espace" })).toHaveAttribute(
      "aria-current",
      "page",
    );
  });

  it("routes to Settings when its entry is clicked", async () => {
    const user = userEvent.setup();
    const { onScreenChange } = renderShell();
    await user.click(screen.getByRole("button", { name: "Settings" }));
    expect(onScreenChange).toHaveBeenCalledWith("settings");
  });

  it("marks the current screen, and only it", () => {
    renderShell({ screen: "settings" });
    expect(screen.getByRole("button", { name: "Settings" })).toHaveAttribute(
      "aria-current",
      "page",
    );
    expect(screen.getByRole("button", { name: "Cleanup" })).not.toHaveAttribute("aria-current");
  });

  it("keeps the theme toggle", () => {
    renderShell();
    expect(screen.getByTestId("theme-toggle")).toHaveAccessibleName("Switch to dark theme");
  });

  /// A screen reader lands on the sidebar by landmark, not by hunting for the
  /// first button of the page.
  it("names the sidebar as the main navigation landmark", () => {
    renderShell();
    expect(screen.getByRole("navigation", { name: "Main navigation" })).toBeInTheDocument();
  });

  /// One Tab stop for the whole sidebar: the active entry is the only one in
  /// the tab order, the arrows reach the other two.
  it("keeps a single tab stop in the sidebar", () => {
    renderShell({ screen: "startup" });
    expect(screen.getByRole("button", { name: "Startup" })).toHaveAttribute("tabindex", "0");
    expect(screen.getByRole("button", { name: "Cleanup" })).toHaveAttribute("tabindex", "-1");
    expect(screen.getByRole("button", { name: "Space" })).toHaveAttribute("tabindex", "-1");
    expect(screen.getByRole("button", { name: "Settings" })).toHaveAttribute("tabindex", "-1");
  });

  it("moves between the entries with the arrow keys", async () => {
    const user = userEvent.setup();
    const { onScreenChange } = renderShell();
    screen.getByRole("button", { name: "Cleanup" }).focus();
    await user.keyboard("{ArrowDown}");
    expect(onScreenChange).toHaveBeenCalledWith("space");
    expect(screen.getByRole("button", { name: "Space" })).toHaveFocus();
  });

  it("wraps around from the first entry to the last with ArrowUp", async () => {
    const user = userEvent.setup();
    const { onScreenChange } = renderShell();
    screen.getByRole("button", { name: "Cleanup" }).focus();
    await user.keyboard("{ArrowUp}");
    expect(onScreenChange).toHaveBeenCalledWith("settings");
    expect(screen.getByRole("button", { name: "Settings" })).toHaveFocus();
  });

  it("jumps to the first and the last entry with Home and End", async () => {
    const user = userEvent.setup();
    const { onScreenChange } = renderShell({ screen: "startup" });
    screen.getByRole("button", { name: "Startup" }).focus();
    await user.keyboard("{End}");
    expect(onScreenChange).toHaveBeenCalledWith("settings");
    await user.keyboard("{Home}");
    expect(onScreenChange).toHaveBeenCalledWith("clean");
  });

  /// The label says what the press does; `aria-pressed` says what is on now.
  it("announces the theme toggle state", () => {
    renderShell();
    expect(screen.getByTestId("theme-toggle")).toHaveAttribute("aria-pressed", "false");
    renderShell({ dark: true });
    expect(screen.getAllByTestId("theme-toggle")[1]).toHaveAttribute("aria-pressed", "true");
  });

  it("shows no sandbox banner while the engine runs against the real profile", () => {
    renderShell();
    expect(screen.queryByTestId("sandbox-banner")).not.toBeInTheDocument();
  });

  it("names the sandbox root in a banner, on every screen, while one is active", () => {
    renderShell({ sandbox: SANDBOX, screen: "startup" });
    const banner = screen.getByTestId("sandbox-banner");
    expect(banner).toHaveTextContent("Sandbox mode");
    expect(banner).toHaveTextContent(SANDBOX.root);
  });

  /// Entering the sandbox moves the engine off the real profile: the banner
  /// has to reach a screen reader on its own.
  it("announces the sandbox banner", () => {
    renderShell({ sandbox: SANDBOX });
    expect(screen.getByTestId("sandbox-banner")).toHaveAttribute("role", "status");
  });

  it("leaves the sandbox from the banner", async () => {
    const user = userEvent.setup();
    const onLeaveSandbox = vi.fn();
    renderShell({ sandbox: SANDBOX, onLeaveSandbox });
    await user.click(screen.getByRole("button", { name: "Leave" }));
    expect(onLeaveSandbox).toHaveBeenCalled();
  });
});
