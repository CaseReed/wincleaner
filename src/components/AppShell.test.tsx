import { describe, it, expect, vi } from "vitest";
import { render, screen } from "@testing-library/react";
import userEvent from "@testing-library/user-event";
import { AppShell } from "./AppShell";

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
});
