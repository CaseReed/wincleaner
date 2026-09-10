import { describe, it, expect, beforeEach, afterEach, vi } from "vitest";
import { render, screen, waitFor } from "@testing-library/react";
import userEvent from "@testing-library/user-event";

const { checkForUpdates } = vi.hoisted(() => ({
  checkForUpdates: vi.fn(),
}));
vi.mock("@/lib/api", async () => {
  const actual = await vi.importActual<typeof import("@/lib/api")>("@/lib/api");
  return {
    ...actual,
    listRules: () => Promise.resolve([]),
    runningBrowsers: () => Promise.resolve([]),
    listStartup: () => Promise.resolve([]),
    checkForUpdates,
  };
});

const { setTheme } = vi.hoisted(() => ({
  setTheme: vi.fn(() => Promise.resolve()),
}));
vi.mock("@tauri-apps/api/window", () => ({
  getCurrentWindow: () => ({ setTheme }),
}));

vi.mock("sonner", () => ({
  toast: { error: vi.fn(), success: vi.fn(), info: vi.fn() },
  Toaster: () => null,
}));

import App from "./App";
import { toast } from "sonner";
import { AUTO_CHECK_KEY, LAST_NOTIFIED_KEY } from "@/lib/updates";

function stubPrefersDark(dark: boolean) {
  Object.defineProperty(window, "matchMedia", {
    configurable: true,
    writable: true,
    value: vi.fn((query: string) => ({
      matches: dark && query.includes("prefers-color-scheme: dark"),
      media: query,
      addEventListener() {},
      removeEventListener() {},
    })),
  });
}

beforeEach(() => {
  localStorage.clear();
  setTheme.mockClear();
  checkForUpdates.mockReset();
  vi.mocked(toast.info).mockClear();
  document.documentElement.classList.remove("dark");
});

afterEach(() => {
  Reflect.deleteProperty(window, "matchMedia");
});

describe("theme", () => {
  it("follows prefers-color-scheme on first launch", () => {
    stubPrefersDark(true);
    render(<App />);
    expect(document.documentElement).toHaveClass("dark");
  });

  it("the remembered choice wins over the system", () => {
    stubPrefersDark(true);
    localStorage.setItem("wincleaner.theme", "light");
    render(<App />);
    expect(document.documentElement).not.toHaveClass("dark");
  });

  it("the toggle persists the choice", async () => {
    const user = userEvent.setup();
    stubPrefersDark(false);
    render(<App />);
    await user.click(screen.getByTestId("theme-toggle"));
    expect(document.documentElement).toHaveClass("dark");
    expect(localStorage.getItem("wincleaner.theme")).toBe("dark");
  });

  it("also switches the window theme (Windows title bar)", async () => {
    const user = userEvent.setup();
    stubPrefersDark(false);
    render(<App />);
    expect(setTheme).toHaveBeenLastCalledWith("light");
    await user.click(screen.getByTestId("theme-toggle"));
    expect(setTheme).toHaveBeenLastCalledWith("dark");
  });
});

describe("the startup update consent gate", () => {
  it("never checks for updates when the switch has not been armed", async () => {
    stubPrefersDark(false);
    render(<App />);
    // Nothing to await on directly: give any stray microtask a chance to run,
    // then assert the network path was never touched.
    await Promise.resolve();
    expect(checkForUpdates).not.toHaveBeenCalled();
  });

  it("checks exactly once at startup when armed, and toasts once on a newer version", async () => {
    stubPrefersDark(false);
    localStorage.setItem(AUTO_CHECK_KEY, "true");
    checkForUpdates.mockResolvedValue({
      current: "0.1.0",
      latest: "0.2.0",
      is_newer: true,
      notes: null,
      url: null,
      published_at: null,
    });

    render(<App />);

    await waitFor(() => expect(checkForUpdates).toHaveBeenCalledTimes(1));
    await waitFor(() => expect(toast.info).toHaveBeenCalledTimes(1));
    expect(toast.info).toHaveBeenCalledWith(
      "WinCleaner 0.2.0 is available",
      expect.anything(),
    );
    expect(localStorage.getItem(LAST_NOTIFIED_KEY)).toBe("0.2.0");
  });
});
