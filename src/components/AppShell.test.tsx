import { describe, it, expect, vi } from "vitest";
import { render, screen } from "@testing-library/react";
import userEvent from "@testing-library/user-event";
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
  it("lists the three screens", () => {
    renderShell();
    expect(screen.getByRole("button", { name: "Cleanup" })).toBeInTheDocument();
    expect(screen.getByRole("button", { name: "Startup" })).toBeInTheDocument();
    expect(screen.getByRole("button", { name: "Settings" })).toBeInTheDocument();
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

  it("leaves the sandbox from the banner", async () => {
    const user = userEvent.setup();
    const onLeaveSandbox = vi.fn();
    renderShell({ sandbox: SANDBOX, onLeaveSandbox });
    await user.click(screen.getByRole("button", { name: "Leave" }));
    expect(onLeaveSandbox).toHaveBeenCalled();
  });
});
