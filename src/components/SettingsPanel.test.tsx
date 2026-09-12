import { describe, it, expect, vi, beforeEach, afterEach } from "vitest";
import { render, screen, waitFor, within } from "@testing-library/react";
import userEvent from "@testing-library/user-event";
import whatsNew from "@/generated/whats-new.json";
import { AUTO_CHECK_KEY } from "@/lib/updates";
import { I18nProvider, LANGUAGE_KEY } from "@/i18n";
import { SettingsPanel } from "./SettingsPanel";

vi.mock("@/lib/api", () => ({
  appMode: vi.fn(),
  checkForUpdates: vi.fn(),
  listExclusions: vi.fn(),
  listRules: vi.fn(),
  removeExclusion: vi.fn(),
  sandboxOrphans: vi.fn(),
  sandboxRemoveOrphans: vi.fn(),
}));
vi.mock("sonner", () => ({
  toast: { success: vi.fn(), error: vi.fn() },
}));

import {
  appMode,
  checkForUpdates,
  listExclusions,
  listRules,
  removeExclusion,
  sandboxOrphans,
  sandboxRemoveOrphans,
} from "@/lib/api";

const mockedAppMode = vi.mocked(appMode);
const mockedCheck = vi.mocked(checkForUpdates);
const mockedOrphans = vi.mocked(sandboxOrphans);
const mockedRemoveOrphans = vi.mocked(sandboxRemoveOrphans);
const mockedExclusions = vi.mocked(listExclusions);
const mockedRules = vi.mocked(listRules);
const mockedRemoveExclusion = vi.mocked(removeExclusion);

const SANDBOX = {
  root: String.raw`C:\Users\T\AppData\Local\Temp\wincleaner-sandbox-1a2b`,
  sentinels: 52,
  junk: 119,
  winapp2_rules: 14,
};

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
  // An installed build is the normal case; the portable line stays hidden.
  mockedAppMode.mockReset().mockResolvedValue(false);
  // Nothing left behind is the normal case, and the one every other test here
  // renders against.
  mockedOrphans.mockReset().mockResolvedValue([]);
  mockedRemoveOrphans.mockReset().mockResolvedValue(0);
  // No exclusion is the normal case, and the one every other test here renders
  // against.
  mockedExclusions.mockReset().mockResolvedValue([]);
  mockedRules.mockReset().mockResolvedValue([]);
  mockedRemoveExclusion.mockReset().mockResolvedValue(undefined);
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

  it("says nothing about portable mode in an installed build", async () => {
    render(<SettingsPanel />);
    await waitFor(() => expect(mockedAppMode).toHaveBeenCalled());
    expect(screen.queryByTestId("app-portable")).toBeNull();
  });

  it("says where the stores live when the build is portable", async () => {
    mockedAppMode.mockResolvedValue(true);
    render(<SettingsPanel />);
    expect(await screen.findByTestId("app-portable")).toHaveTextContent(
      "stored next to the executable",
    );
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

  /// "Checking…" on a disabled button is a spinner for the eye only: the state
  /// has to be in the markup for anything else to read it.
  it("marks the update check busy while it runs and announces its answer", async () => {
    let release!: (value: ReturnType<typeof answer>) => void;
    mockedCheck.mockReturnValue(new Promise((resolve) => (release = resolve)));
    render(<SettingsPanel />);

    await userEvent.click(screen.getByTestId("check-updates"));
    expect(screen.getByTestId("check-updates")).toHaveAttribute("aria-busy", "true");

    release(answer({ current: "0.2.0", latest: "0.2.0", is_newer: false }));
    const status = await screen.findByTestId("update-status");
    expect(status).toHaveAttribute("role", "status");
    expect(screen.getByTestId("check-updates")).toHaveAttribute("aria-busy", "false");
  });

  it("names each settings card by its own heading", () => {
    render(<SettingsPanel />);
    expect(screen.getByRole("region", { name: "About" })).toBeInTheDocument();
    expect(screen.getByRole("region", { name: "Sandbox" })).toBeInTheDocument();
    expect(screen.getByRole("region", { name: "Notices" })).toBeInTheDocument();
  });

  it("marks the sandbox buttons busy while the profile is being built", () => {
    render(<SettingsPanel sandboxBusy />);
    expect(screen.getByTestId("create-sandbox")).toHaveAttribute("aria-busy", "true");
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

  describe("Sandbox", () => {
    it("explains what a sandbox is and offers to create one", async () => {
      const user = userEvent.setup();
      const onEnterSandbox = vi.fn();
      render(<SettingsPanel onEnterSandbox={onEnterSandbox} />);

      const section = screen.getByTestId("sandbox");
      expect(section).toHaveTextContent(/synthetic Windows profile/);
      expect(section).toHaveTextContent(/nothing in your real profile is touched/i);
      expect(screen.queryByRole("button", { name: "Leave the sandbox" })).not.toBeInTheDocument();

      await user.click(screen.getByRole("button", { name: "Create a sandbox profile" }));
      expect(onEnterSandbox).toHaveBeenCalled();
    });

    it("shows what the active sandbox holds, and the way out", async () => {
      const user = userEvent.setup();
      const onLeaveSandbox = vi.fn();
      render(<SettingsPanel sandbox={SANDBOX} onLeaveSandbox={onLeaveSandbox} />);

      const section = screen.getByTestId("sandbox");
      expect(section).toHaveTextContent(SANDBOX.root);
      expect(section).toHaveTextContent("52");
      expect(section).toHaveTextContent("119");
      expect(section).toHaveTextContent("14");
      expect(
        screen.queryByRole("button", { name: "Create a sandbox profile" }),
      ).not.toBeInTheDocument();

      await user.click(screen.getByRole("button", { name: "Leave the sandbox" }));
      expect(onLeaveSandbox).toHaveBeenCalled();
    });

    /// A sandbox whose process died is not the user's problem to find: the
    /// line names it, and the button is the way out without a restart.
    it("offers to remove the sandbox folders a previous run left behind", async () => {
      const user = userEvent.setup();
      mockedOrphans.mockResolvedValueOnce([
        { path: String.raw`C:\Temp\wincleaner-sandbox-a-1-0`, size_bytes: 2 * 1024 * 1024, files: 40 },
        { path: String.raw`C:\Temp\wincleaner-sandbox-b-2-0`, size_bytes: 1024 * 1024, files: 20 },
      ]);
      mockedRemoveOrphans.mockResolvedValue(2);
      render(<SettingsPanel />);

      const line = await screen.findByTestId("sandbox-orphans");
      expect(line).toHaveTextContent("2 old sandbox folders (3 MB)");

      await user.click(within(line).getByRole("button", { name: "Remove" }));
      expect(mockedRemoveOrphans).toHaveBeenCalled();
      // The back end is asked again, and now answers nothing: the line goes.
      await waitFor(() => expect(screen.queryByTestId("sandbox-orphans")).toBeNull());
    });

    it("says nothing at all when no sandbox folder was left behind", async () => {
      render(<SettingsPanel />);
      await waitFor(() => expect(mockedOrphans).toHaveBeenCalled());
      expect(screen.queryByTestId("sandbox-orphans")).toBeNull();
    });

    /// Building the profile writes a few hundred files: without a busy state
    /// the button looks inert and a second click races the first.
    it("shows the work in progress and refuses a second click", () => {
      const onEnterSandbox = vi.fn();
      const { rerender } = render(
        <SettingsPanel sandboxBusy onEnterSandbox={onEnterSandbox} />,
      );
      expect(screen.getByRole("button", { name: /Creating/ })).toBeDisabled();

      rerender(<SettingsPanel sandbox={SANDBOX} sandboxBusy onLeaveSandbox={vi.fn()} />);
      expect(screen.getByRole("button", { name: /Removing/ })).toBeDisabled();
    });
  });

  describe("exclusions", () => {
    const EXCLUSION = {
      rule_id: "windows.temp",
      rule_label: "Temporary files",
      pattern: String.raw`%TEMP%\keep\**`,
      added: "2026-09-12",
    };

    it("tells the user where exclusions come from when there are none", async () => {
      render(<SettingsPanel />);
      expect(await screen.findByTestId("exclusions-empty")).toHaveTextContent(
        /Show the paths/,
      );
    });

    /// The stored pattern is what is shown: a resolved absolute path here
    /// would put back on screen exactly what the storage format avoids.
    it("lists the pattern, the rule and a localized date", async () => {
      mockedExclusions.mockResolvedValue([EXCLUSION]);
      render(<SettingsPanel />);

      const section = await screen.findByTestId("exclusions");
      expect(within(section).getByText(EXCLUSION.pattern)).toBeInTheDocument();
      expect(within(section).getByText(/Temporary files/)).toBeInTheDocument();
      // Localized, not the raw ISO string: `2026-09-12` reads as "Sep 12, 2026"
      // in English, parsed as a local date so the day never shifts.
      expect(within(section).getByText(/Sep 12, 2026/)).toBeInTheDocument();
      expect(within(section).queryByText(/2026-09-12/)).not.toBeInTheDocument();
      expect(within(section).queryByText(/C:/)).not.toBeInTheDocument();
    });

    /// The label comes from the loaded rule catalogue (`listRules`), localized
    /// through `ruleLabel` like every other rule label — not the back end's
    /// own (English) `rule_label` on the exclusion itself.
    it("shows the rule's French label when the interface is French", async () => {
      window.localStorage.setItem(LANGUAGE_KEY, "fr");
      mockedExclusions.mockResolvedValue([EXCLUSION]);
      mockedRules.mockResolvedValue([
        {
          id: "windows.temp",
          category: "System",
          label: "Temporary files",
          label_fr: "Fichiers temporaires",
          description_fr: null,
          category_fr: null,
          risk: "low",
          kind: "files",
          default_checked: true,
          note: null,
          unavailable_reason: null,
        },
      ]);
      render(
        <I18nProvider>
          <SettingsPanel />
        </I18nProvider>,
      );

      const section = await screen.findByTestId("exclusions");
      expect(await within(section).findByText(/Fichiers temporaires/)).toBeInTheDocument();
      expect(within(section).queryByText(/^Temporary files/)).not.toBeInTheDocument();
      // "12 sept. 2026" (fr-FR medium date style).
      expect(within(section).getByText(/12 sept\. 2026/)).toBeInTheDocument();
    });

    it("removes an exclusion and re-reads the store rather than assuming", async () => {
      mockedExclusions.mockResolvedValueOnce([EXCLUSION]).mockResolvedValueOnce([]);
      render(<SettingsPanel />);

      const remove = await screen.findByRole("button", {
        name: new RegExp("Stop excluding"),
      });
      await userEvent.click(remove);

      expect(mockedRemoveExclusion).toHaveBeenCalledWith(
        EXCLUSION.rule_id,
        EXCLUSION.pattern,
      );
      expect(await screen.findByTestId("exclusions-empty")).toBeInTheDocument();
    });

    it("says so when the store cannot be read, instead of showing an empty list", async () => {
      mockedExclusions.mockRejectedValue("exclusions.toml is not a valid exclusions file");
      render(<SettingsPanel />);

      expect(
        await screen.findByText(/exclusions could not be read/i),
      ).toBeInTheDocument();
      expect(screen.queryByTestId("exclusions-empty")).not.toBeInTheDocument();
    });
  });
});
