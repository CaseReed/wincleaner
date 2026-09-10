import { describe, it, expect, beforeEach, afterEach, vi } from "vitest";
import { render, screen } from "@testing-library/react";
import userEvent from "@testing-library/user-event";

vi.mock("@/lib/api", async () => {
  const actual = await vi.importActual<typeof import("@/lib/api")>("@/lib/api");
  return {
    ...actual,
    listRules: () => Promise.resolve([]),
    runningBrowsers: () => Promise.resolve([]),
    listStartup: () => Promise.resolve([]),
  };
});

const { setTheme } = vi.hoisted(() => ({
  setTheme: vi.fn(() => Promise.resolve()),
}));
vi.mock("@tauri-apps/api/window", () => ({
  getCurrentWindow: () => ({ setTheme }),
}));

vi.mock("sonner", () => ({
  toast: { error: vi.fn(), success: vi.fn() },
  Toaster: () => null,
}));

import App from "./App";

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
