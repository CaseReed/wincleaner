import { describe, it, expect, vi, beforeEach, afterEach } from "vitest";
import { render, screen, waitFor } from "@testing-library/react";
import userEvent from "@testing-library/user-event";
import whatsNew from "@/generated/whats-new.json";
import { AUTO_CHECK_KEY } from "@/lib/updates";
import { SettingsPanel } from "./SettingsPanel";

vi.mock("@/lib/api", () => ({ checkForUpdates: vi.fn() }));
vi.mock("sonner", () => ({
  toast: { success: vi.fn(), error: vi.fn() },
}));

import { checkForUpdates } from "@/lib/api";

const mockedCheck = vi.mocked(checkForUpdates);

/// A check that answered, with only the fields a given state needs.
function answer(over: Partial<Awaited<ReturnType<typeof checkForUpdates>>> = {}) {
  return {
    current: "0.2.0",
    latest: "0.2.0",
    is_newer: false,
    notes: null,
    url: null,
    published_at: null,
    ...over,
  };
}

beforeEach(() => {
  localStorage.clear();
  mockedCheck.mockReset();
});

afterEach(() => {
  vi.unstubAllGlobals();
});

describe("SettingsPanel", () => {
  it("shows the application name, version and stance", () => {
    render(<SettingsPanel />);
    expect(screen.getByTestId("app-version")).toHaveTextContent(whatsNew.version);
    expect(screen.getByTestId("about")).toHaveTextContent(
      "Open source, MIT. No telemetry, and no network access except one request to GitHub when you click Check for updates or enable automatic checks (off by default).",
    );
    expect(screen.getByTestId("about")).toHaveTextContent("github.com/CaseReed/wincleaner");
  });

  it("renders the changelog body as plain text under the version heading", () => {
    render(<SettingsPanel />);
    expect(
      screen.getByRole("heading", { name: `What's new in ${whatsNew.version}` }),
    ).toBeInTheDocument();
    const body = screen.getByTestId("whats-new");
    expect(whatsNew.body.length).toBeGreaterThan(0);
    // Plain text only: the bullets are literal "- ", and no markup was parsed.
    expect(body.textContent).toBe(whatsNew.body);
    expect(body.querySelector("a")).toBeNull();
    expect(body.querySelector("ul")).toBeNull();
  });

  it("credits the MIT licence, Winapp2 and the bundled assets", () => {
    render(<SettingsPanel />);
    const notices = screen.getByTestId("notices");
    expect(notices).toHaveTextContent("MIT");
    expect(notices).toHaveTextContent("Community rules from Winapp2 (CC-BY-SA 4.0)");
    expect(notices).toHaveTextContent("github.com/MoscaDotTo/Winapp2");
    expect(notices).toHaveTextContent(/Fonts and icons are bundled/);
  });
});

describe("SettingsPanel — updates", () => {
  it("contacts nothing until the button is clicked", () => {
    render(<SettingsPanel />);
    expect(screen.getByTestId("updates-current")).toHaveTextContent(whatsNew.version);
    expect(mockedCheck).not.toHaveBeenCalled();
    expect(screen.queryByTestId("update-status")).toBeNull();
  });

  it("spins while the check is running, then reports being up to date", async () => {
    let release!: (value: ReturnType<typeof answer>) => void;
    mockedCheck.mockReturnValue(new Promise((resolve) => (release = resolve)));
    render(<SettingsPanel />);

    await userEvent.click(screen.getByTestId("check-updates"));
    expect(screen.getByTestId("check-updates")).toBeDisabled();
    expect(screen.getByTestId("check-updates")).toHaveTextContent("Checking…");

    release(answer({ current: "0.2.0", latest: "0.2.0", is_newer: false }));
    await waitFor(() =>
      expect(screen.getByTestId("update-status")).toHaveTextContent("You’re up to date (0.2.0)"),
    );
    expect(screen.getByTestId("check-updates")).toBeEnabled();
  });

  it("reports the local version, not the release tag, when not newer", async () => {
    // A local build ahead of the last published tag (or a hand-pushed old
    // tag) must still read as "you're up to date" against the version that
    // is actually installed, never the older tag GitHub happened to answer.
    mockedCheck.mockResolvedValue(answer({ current: "0.2.0", latest: "0.1.0", is_newer: false }));
    render(<SettingsPanel />);
    await userEvent.click(screen.getByTestId("check-updates"));
    await waitFor(() =>
      expect(screen.getByTestId("update-status")).toHaveTextContent("You’re up to date (0.2.0)"),
    );
  });

  it("announces a newer version with its date, notes and link", async () => {
    mockedCheck.mockResolvedValue(
      answer({
        latest: "0.4.0",
        is_newer: true,
        notes: "### Added\n- A thing",
        url: "https://github.com/CaseReed/wincleaner/releases/tag/v0.4.0",
        published_at: "2026-09-10T12:00:00Z",
      }),
    );
    render(<SettingsPanel />);
    await userEvent.click(screen.getByTestId("check-updates"));

    await waitFor(() =>
      expect(screen.getByTestId("update-status")).toHaveTextContent(
        "WinCleaner 0.4.0 is available",
      ),
    );
    expect(screen.getByTestId("update-published")).toHaveTextContent("Published 2026-09-10");
    // Notes are flattened text: the heading hashes are gone and no anchor or
    // list element was ever created.
    const notes = screen.getByTestId("update-notes");
    expect(notes.textContent).toBe("Added\n- A thing");
    expect(notes.querySelector("a")).toBeNull();
    expect(screen.getByTestId("update-url")).toHaveTextContent(
      "https://github.com/CaseReed/wincleaner/releases/tag/v0.4.0",
    );
  });

  it("copies the release link to the clipboard", async () => {
    const writeText = vi.fn().mockResolvedValue(undefined);
    vi.stubGlobal("navigator", { ...navigator, clipboard: { writeText } });
    mockedCheck.mockResolvedValue(
      answer({
        latest: "0.4.0",
        is_newer: true,
        url: "https://github.com/CaseReed/wincleaner/releases/tag/v0.4.0",
      }),
    );
    render(<SettingsPanel />);
    await userEvent.click(screen.getByTestId("check-updates"));
    await waitFor(() => expect(screen.getByTestId("copy-update-url")).toBeInTheDocument());

    await userEvent.click(screen.getByTestId("copy-update-url"));
    expect(writeText).toHaveBeenCalledWith(
      "https://github.com/CaseReed/wincleaner/releases/tag/v0.4.0",
    );
  });

  it("survives a clipboard that refuses", async () => {
    vi.stubGlobal("navigator", {
      ...navigator,
      clipboard: { writeText: vi.fn().mockRejectedValue(new Error("denied")) },
    });
    mockedCheck.mockResolvedValue(
      answer({ latest: "0.4.0", is_newer: true, url: "https://example.com/r" }),
    );
    render(<SettingsPanel />);
    await userEvent.click(screen.getByTestId("check-updates"));
    await waitFor(() => expect(screen.getByTestId("copy-update-url")).toBeInTheDocument());

    await userEvent.click(screen.getByTestId("copy-update-url"));
    // The URL is still on screen: nothing was lost by the failure.
    expect(screen.getByTestId("update-url")).toHaveTextContent("https://example.com/r");
  });

  it.each([
    ["offline", "Could not reach GitHub — check your connection"],
    ["not-available", "No public release is available yet"],
    ["rate-limited", "GitHub rate limit reached, try again later"],
  ])("turns the %s code into its sentence", async (code, sentence) => {
    mockedCheck.mockRejectedValue(code);
    render(<SettingsPanel />);
    await userEvent.click(screen.getByTestId("check-updates"));
    await waitFor(() => expect(screen.getByTestId("update-status")).toHaveTextContent(sentence));
  });

  it("offers automatic checking off, and persists the choice", async () => {
    render(<SettingsPanel />);
    const auto = screen.getByTestId("auto-check");
    expect(auto).toHaveAttribute("aria-checked", "false");
    expect(localStorage.getItem(AUTO_CHECK_KEY)).toBeNull();

    await userEvent.click(auto);
    expect(auto).toHaveAttribute("aria-checked", "true");
    expect(localStorage.getItem(AUTO_CHECK_KEY)).toBe("true");
    // Arming the switch does not itself fire a check: it applies at next start.
    expect(mockedCheck).not.toHaveBeenCalled();

    await userEvent.click(auto);
    expect(localStorage.getItem(AUTO_CHECK_KEY)).toBe("false");
  });

  it("starts from the persisted choice", () => {
    localStorage.setItem(AUTO_CHECK_KEY, "true");
    render(<SettingsPanel />);
    expect(screen.getByTestId("auto-check")).toHaveAttribute("aria-checked", "true");
  });

  it("says exactly what the automatic check sends", () => {
    render(<SettingsPanel />);
    expect(screen.getByTestId("auto-check-privacy")).toHaveTextContent(
      "When enabled, WinCleaner sends one request to api.github.com at startup with no identifiers other than the app version in the User-Agent.",
    );
  });
});
