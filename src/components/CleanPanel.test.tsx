import { describe, it, expect, vi, beforeEach, afterEach } from "vitest";
import { act, render, screen, waitFor } from "@testing-library/react";
import userEvent from "@testing-library/user-event";

const api = {
  listRules: vi.fn(),
  rulesSummary: vi.fn(),
  scan: vi.fn(),
  clean: vi.fn(),
  runningBrowsers: vi.fn(),
  quitBrowser: vi.fn(),
  onScanProgress: vi.fn(),
  onCleanProgress: vi.fn(),
  sandboxVerify: vi.fn(),
  addExclusion: vi.fn(),
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
    quitBrowser: (process: string) => api.quitBrowser(process),
    onScanProgress: (cb: (p: unknown) => void) => api.onScanProgress(cb),
    onCleanProgress: (cb: (p: unknown) => void) => api.onCleanProgress(cb),
    sandboxVerify: (ids: string[]) => api.sandboxVerify(ids),
    addExclusion: (ruleId: string, index: number, scope: string) =>
      api.addExclusion(ruleId, index, scope),
  };
});

vi.mock("sonner", () => ({
  toast: { success: vi.fn(), error: vi.fn() },
}));

import { toast } from "sonner";
import { CleanPanel } from "./CleanPanel";
import { I18nProvider, LANGUAGE_KEY } from "@/i18n";

const RULES = [
  {
    id: "windows.temp",
    category: "System",
    label: "Temporary files",
    label_fr: "Fichiers temporaires",
    risk: "low",
    kind: "files",
    default_checked: true,
  },
  { id: "edge.cache", category: "Browsers", label: "Microsoft Edge cache", risk: "low", kind: "files", default_checked: true },
];

const RECYCLE_BIN = {
  id: "windows.recycle-bin",
  category: "System",
  label: "Recycle Bin",
  risk: "low",
  kind: "recycle-bin",
  default_checked: false,
};

type Progress = {
  done: number;
  total: number;
  rule_id: string;
  label: string;
  total_bytes: number;
  running?: { rule_id: string; label: string }[];
};

type CleanStep = {
  done_rules: number;
  total_rules: number;
  rule_id: string;
  label: string;
  files_deleted: number;
  bytes_freed: number;
};

/// Set by the `onScanProgress` mock: lets a test play the events the Rust side
/// would emit, in the middle of a scan that has not resolved yet.
let emitProgress: (p: Progress) => void = () => {};
let unlisten = vi.fn();
/// The same, for the `clean-progress` events of a clean still running.
let emitClean: (p: CleanStep) => void = () => {};
let cleanUnlisten = vi.fn();

describe("CleanPanel", () => {
  beforeEach(() => {
    emitProgress = () => {};
    unlisten = vi.fn();
    api.onScanProgress.mockReset().mockImplementation((cb: (p: Progress) => void) => {
      emitProgress = cb;
      return Promise.resolve(unlisten);
    });
    emitClean = () => {};
    cleanUnlisten = vi.fn();
    api.onCleanProgress.mockReset().mockImplementation((cb: (p: CleanStep) => void) => {
      emitClean = cb;
      return Promise.resolve(cleanUnlisten);
    });
    api.listRules.mockReset().mockResolvedValue(RULES);
    api.scan.mockReset().mockResolvedValue([]);
    api.clean.mockReset().mockResolvedValue({ freed_bytes: 0, deleted: 0, skipped: [] });
    api.runningBrowsers.mockReset().mockResolvedValue([]);
    api.quitBrowser.mockReset().mockResolvedValue(9);
    api.sandboxVerify.mockReset();
    vi.mocked(toast.error).mockReset();
    vi.mocked(toast.success).mockReset();
    api.addExclusion.mockReset().mockResolvedValue({
      rule_id: "windows.temp",
      rule_label: "Temporary files",
      pattern: String.raw`%TEMP%\a.txt`,
      added: "2026-09-12",
    });
    api.rulesSummary.mockReset().mockResolvedValue({
      native: 10,
      winapp2_retained: 1200,
      winapp2_detected: 42,
      winapp2_dropped: 900,
    });
  });

  // A test that switches to fake timers and fails before its own `finally`
  // would otherwise leave every later test hanging on a timer that never
  // fires.
  afterEach(() => {
    vi.useRealTimers();
  });

  it("shows the rules grouped by category", async () => {
    render(<CleanPanel />);
    expect(await screen.findByText("System")).toBeInTheDocument();
    expect(screen.getByText("Browsers")).toBeInTheDocument();
    expect(screen.getByLabelText("Temporary files")).toBeInTheDocument();
    expect(screen.getByLabelText("Microsoft Edge cache")).toBeInTheDocument();
  });

  /// Scanning is read-only: unchecking a rule must not cost the user the
  /// knowledge of what it holds. The checkbox decides what Clean deletes, not
  /// what Analyze measures.
  it("measures every available rule, checked or not", async () => {
    const user = userEvent.setup();
    render(<CleanPanel />);
    await screen.findByLabelText("Temporary files");
    await user.click(screen.getByLabelText("Microsoft Edge cache"));
    await user.click(screen.getByRole("button", { name: /Analyze/ }));
    await waitFor(() =>
      expect(api.scan).toHaveBeenCalledWith(["windows.temp", "edge.cache"])
    );
  });

  it("shows the size of an unchecked rule after a scan", async () => {
    const user = userEvent.setup();
    api.scan.mockResolvedValue([
      { rule_id: "windows.temp", file_count: 2, total_bytes: 2048, paths: [], skipped: 0 },
      { rule_id: "edge.cache", file_count: 1, total_bytes: 1024, paths: [], skipped: 0 },
    ]);
    render(<CleanPanel />);
    await screen.findByLabelText("Temporary files");
    await user.click(screen.getByLabelText("Microsoft Edge cache"));
    await user.click(screen.getByRole("button", { name: /Analyze/ }));

    expect(await screen.findByTestId("result-edge.cache")).toHaveTextContent("1 KB");
    expect(screen.getByLabelText("Microsoft Edge cache")).not.toBeChecked();
  });

  /// The hero answers "what will Clean free", not "what is lying around":
  /// the unchecked bytes are stated separately instead of being folded in.
  it("counts only the checked rules in the total, the gauge and the Clean label", async () => {
    const user = userEvent.setup();
    api.scan.mockResolvedValue([
      { rule_id: "windows.temp", file_count: 2, total_bytes: 2048, paths: [], skipped: 0 },
      { rule_id: "edge.cache", file_count: 1, total_bytes: 1024, paths: [], skipped: 0 },
    ]);
    render(<CleanPanel />);
    await screen.findByLabelText("Temporary files");
    await user.click(screen.getByLabelText("Microsoft Edge cache"));
    await user.click(screen.getByRole("button", { name: /Analyze/ }));

    expect(await screen.findByTestId("total-bytes")).toHaveTextContent("2 KB");
    expect(screen.getByTestId("unchecked-bytes")).toHaveTextContent("1 KB more in unchecked rules");
    expect(screen.getByRole("button", { name: /^Clean/ })).toHaveTextContent("2 KB");
    // The gauge draws the checked rules only.
    expect(screen.getAllByTestId("gauge-segment")).toHaveLength(1);
  });

  it("hides the unchecked line when every measured rule is checked", async () => {
    const user = userEvent.setup();
    api.scan.mockResolvedValue([
      { rule_id: "windows.temp", file_count: 2, total_bytes: 2048, paths: [], skipped: 0 },
      { rule_id: "edge.cache", file_count: 1, total_bytes: 1024, paths: [], skipped: 0 },
    ]);
    render(<CleanPanel />);
    await screen.findByLabelText("Temporary files");
    await user.click(screen.getByRole("button", { name: /Analyze/ }));

    expect(await screen.findByTestId("total-bytes")).toHaveTextContent("3 KB");
    expect(screen.queryByTestId("unchecked-bytes")).toBeNull();
  });

  /// Re-deciding what to delete is not a reason to re-walk the disk: the
  /// measurements stand until the next Analyze.
  it("keeps the results and updates the total when a rule is toggled after a scan", async () => {
    const user = userEvent.setup();
    api.scan.mockResolvedValue([
      { rule_id: "windows.temp", file_count: 2, total_bytes: 2048, paths: [], skipped: 0 },
      { rule_id: "edge.cache", file_count: 1, total_bytes: 1024, paths: [], skipped: 0 },
    ]);
    render(<CleanPanel />);
    await screen.findByLabelText("Temporary files");
    await user.click(screen.getByRole("button", { name: /Analyze/ }));
    expect(await screen.findByTestId("total-bytes")).toHaveTextContent("3 KB");

    await user.click(screen.getByLabelText("Microsoft Edge cache"));

    expect(api.scan).toHaveBeenCalledTimes(1);
    expect(screen.getByTestId("total-bytes")).toHaveTextContent("2 KB");
    expect(screen.getByTestId("unchecked-bytes")).toHaveTextContent("1 KB");
    // The row keeps the size it was measured at.
    expect(screen.getByTestId("result-edge.cache")).toHaveTextContent("1 KB");
  });

  it("cleans the checked rules only, never the ones merely measured", async () => {
    const user = userEvent.setup();
    api.scan.mockResolvedValue([
      { rule_id: "windows.temp", file_count: 2, total_bytes: 2048, paths: [], skipped: 0 },
      { rule_id: "edge.cache", file_count: 1, total_bytes: 1024, paths: [], skipped: 0 },
    ]);
    render(<CleanPanel />);
    await screen.findByLabelText("Temporary files");
    await user.click(screen.getByRole("button", { name: /Analyze/ }));
    await screen.findByTestId("total-bytes");

    await user.click(screen.getByLabelText("Microsoft Edge cache"));
    await user.click(screen.getByRole("button", { name: /^Clean/ }));

    const confirmation = await screen.findByTestId("confirm-clean");
    expect(confirmation).toHaveTextContent("2 KB");
    expect(confirmation).not.toHaveTextContent("Microsoft Edge cache");

    await user.click(screen.getByRole("button", { name: /Confirm cleanup/ }));
    await waitFor(() =>
      expect(api.clean).toHaveBeenCalledWith(["windows.temp"], "auto")
    );
  });

  it("shows the size per rule, the total and the expandable paths", async () => {
    const user = userEvent.setup();
    api.scan.mockResolvedValue([
      {
        rule_id: "windows.temp",
        file_count: 2,
        total_bytes: 2048,
        paths: [String.raw`C:\Users\T\AppData\Local\Temp\a.txt`],
        skipped: 0,
      },
      { rule_id: "edge.cache", file_count: 1, total_bytes: 1024, paths: [], skipped: 3 },
    ]);
    render(<CleanPanel />);
    await screen.findByLabelText("Temporary files");
    await user.click(screen.getByRole("button", { name: /Analyze/ }));

    expect(await screen.findByTestId("total-bytes")).toHaveTextContent("3 KB");
    expect(screen.getByTestId("result-windows.temp")).toHaveTextContent("2 KB");
    expect(screen.getByTestId("result-windows.temp")).toHaveTextContent("2 files");
    expect(screen.getByTestId("result-edge.cache")).toHaveTextContent("3 skipped");

    expect(screen.queryByText(String.raw`C:\Users\T\AppData\Local\Temp\a.txt`)).toBeNull();
    await user.click(screen.getByTestId("toggle-paths-windows.temp"));
    expect(
      await screen.findByText(String.raw`C:\Users\T\AppData\Local\Temp\a.txt`)
    ).toBeInTheDocument();
  });

  /// `aria-expanded` alone says something folds; `aria-controls` says what.
  it("points each category header at the rows it folds", async () => {
    render(<CleanPanel />);
    const header = await screen.findByTestId("toggle-category-System");
    const listId = header.getAttribute("aria-controls");
    expect(listId).toBeTruthy();
    expect(document.getElementById(listId!)).toContainElement(
      screen.getByLabelText("Temporary files")
    );
  });

  it("makes the path list a named region behind a labelled toggle", async () => {
    const user = userEvent.setup();
    api.scan.mockResolvedValue([
      {
        rule_id: "windows.temp",
        file_count: 1,
        total_bytes: 2048,
        paths: [String.raw`C:\Users\T\AppData\Local\Temp\a.txt`],
        skipped: 0,
      },
    ]);
    render(<CleanPanel />);
    await screen.findByLabelText("Temporary files");
    await user.click(screen.getByRole("button", { name: /Analyze/ }));

    const trigger = await screen.findByTestId("toggle-paths-windows.temp");
    expect(trigger).toHaveAccessibleName("Show the paths of Temporary files");
    expect(trigger).toHaveAttribute("aria-expanded", "false");

    await user.click(trigger);
    expect(trigger).toHaveAttribute("aria-expanded", "true");
    expect(trigger).toHaveAttribute("aria-controls");
    expect(
      await screen.findByRole("region", { name: "Paths of Temporary files" })
    ).toHaveTextContent(String.raw`C:\Users\T\AppData\Local\Temp\a.txt`);
  });

  describe("exclusions", () => {
    const TWO_PATHS = [
      {
        rule_id: "windows.temp",
        file_count: 2,
        total_bytes: 2048,
        paths: [
          String.raw`C:\Users\T\AppData\Local\Temp\a.txt`,
          String.raw`C:\Users\T\AppData\Local\Temp\b.txt`,
        ],
        skipped: 0,
      },
    ];

    async function openPaths() {
      const user = userEvent.setup();
      api.scan.mockResolvedValue(TWO_PATHS);
      render(<CleanPanel />);
      await screen.findByLabelText("Temporary files");
      await user.click(screen.getByRole("button", { name: /Analyze/ }));
      await user.click(await screen.findByTestId("toggle-paths-windows.temp"));
      return user;
    }

    /// The two actions carry the path and the rule in their accessible name:
    /// the icons repeat on every row, so "Exclude this file" alone would name
    /// nothing in particular.
    it("offers a file and a folder action on each path, both named", async () => {
      await openPaths();
      expect(screen.getByTestId("exclude-file-windows.temp-0")).toHaveAccessibleName(
        String.raw`Exclude C:\Users\T\AppData\Local\Temp\a.txt from Temporary files`,
      );
      expect(screen.getByTestId("exclude-folder-windows.temp-1")).toHaveAccessibleName(
        String.raw`Exclude the folder holding C:\Users\T\AppData\Local\Temp\b.txt from Temporary files`,
      );
    });

    /// The command takes an index, never a path: that is the invariant the
    /// whole feature is built around (CLAUDE.md).
    it("sends the index and the scope, never the path", async () => {
      const user = await openPaths();
      await user.click(screen.getByTestId("exclude-file-windows.temp-1"));

      expect(api.addExclusion).toHaveBeenCalledWith("windows.temp", 1, "file");
      const sent = api.addExclusion.mock.calls[0];
      expect(sent.some((arg: unknown) => String(arg).includes("C:"))).toBe(false);
    });

    it("passes the folder scope when the folder action is used", async () => {
      const user = await openPaths();
      await user.click(screen.getByTestId("exclude-folder-windows.temp-0"));
      expect(api.addExclusion).toHaveBeenCalledWith("windows.temp", 0, "folder");
    });

    /// Hiding the row must not renumber the ones after it: the index IS the
    /// handle the back end resolves to a path, so a shifted index would
    /// exclude the wrong file on the next click.
    it("hides the excluded row while keeping the other indices stable", async () => {
      const user = await openPaths();
      await user.click(screen.getByTestId("exclude-file-windows.temp-0"));

      await waitFor(() =>
        expect(screen.queryByTestId("exclude-file-windows.temp-0")).toBeNull(),
      );
      expect(
        screen.queryByText(String.raw`C:\Users\T\AppData\Local\Temp\a.txt`),
      ).toBeNull();

      // The surviving row kept its original index.
      expect(screen.getByTestId("exclude-file-windows.temp-1")).toBeInTheDocument();
      await user.click(screen.getByTestId("exclude-file-windows.temp-1"));
      expect(api.addExclusion).toHaveBeenLastCalledWith("windows.temp", 1, "file");
    });

    /// A scan result carries no per-file size, so the byte figure cannot be
    /// corrected. Saying it is stale beats inventing a number.
    it("drops the file count by one and flags the figures as stale", async () => {
      const user = await openPaths();
      expect(screen.getByTestId("result-windows.temp")).toHaveTextContent("2 files");

      await user.click(screen.getByTestId("exclude-file-windows.temp-0"));

      // The accessible count is pluralised: down to 1 remaining file, the
      // sr-only text reads the English singular rather than always " files".
      await waitFor(() =>
        expect(screen.getByTestId("result-windows.temp")).toHaveTextContent("1 file"),
      );
      expect(screen.getByTestId("stale-windows.temp")).toHaveTextContent(
        "Analyze again to refresh the figures",
      );
      // The byte total is left exactly as measured.
      expect(screen.getByTestId("result-windows.temp")).toHaveTextContent("2 KB");
    });

    /// A folder exclusion stores `%VAR%\folder\**`: everything under that
    /// folder is out. Hiding only the clicked row left its siblings on screen,
    /// each still offering an Exclude button for a file already excluded.
    it("hides every row under the folder, not just the one clicked", async () => {
      const user = userEvent.setup();
      api.scan.mockResolvedValue([
        {
          rule_id: "windows.temp",
          file_count: 3,
          total_bytes: 3072,
          paths: [
            String.raw`C:\Users\T\AppData\Local\Temp\cache\a.txt`,
            String.raw`C:\Users\T\AppData\Local\Temp\CACHE\b.txt`,
            String.raw`C:\Users\T\AppData\Local\Temp\other\c.txt`,
          ],
          skipped: 0,
        },
      ]);
      render(<CleanPanel />);
      await screen.findByLabelText("Temporary files");
      await user.click(screen.getByRole("button", { name: /Analyze/ }));
      await user.click(await screen.findByTestId("toggle-paths-windows.temp"));
      expect(screen.getByTestId("result-windows.temp")).toHaveTextContent("3 files");

      await user.click(screen.getByTestId("exclude-folder-windows.temp-0"));

      // Both rows of that folder go, whatever their casing; the file in the
      // sibling folder stays, and keeps its own index.
      await waitFor(() =>
        expect(screen.queryByTestId("exclude-file-windows.temp-0")).toBeNull(),
      );
      expect(screen.queryByTestId("exclude-file-windows.temp-1")).toBeNull();
      expect(screen.getByTestId("exclude-file-windows.temp-2")).toBeInTheDocument();
      // The count drops by the two rows actually hidden, and the figures are
      // flagged stale rather than recomputed.
      expect(screen.getByTestId("result-windows.temp")).toHaveTextContent("1 file");
      expect(screen.getByTestId("stale-windows.temp")).toBeInTheDocument();
    });

    it("keeps the row when the back end refuses", async () => {
      api.addExclusion.mockRejectedValue("not under %TEMP%");
      const user = await openPaths();
      await user.click(screen.getByTestId("exclude-file-windows.temp-0"));

      await waitFor(() => expect(api.addExclusion).toHaveBeenCalled());
      expect(screen.getByTestId("exclude-file-windows.temp-0")).toBeInTheDocument();
      expect(screen.queryByTestId("stale-windows.temp")).toBeNull();
    });

    /// Excluding a rule's own root would empty the rule while its checkbox
    /// still read as on. The back end refuses with a stable code; the window
    /// is what turns that into advice in the user's language.
    it("turns the rule-root refusal into its own message, not the raw code", async () => {
      api.addExclusion.mockRejectedValue("exclusion-is-rule-root");
      const user = await openPaths();
      await user.click(screen.getByTestId("exclude-folder-windows.temp-0"));

      await waitFor(() =>
        expect(toast.error).toHaveBeenCalledWith(
          "This folder is the rule\u2019s own root: uncheck the rule instead of excluding it.",
        ),
      );
      // The row stays: nothing was excluded.
      expect(screen.getByTestId("exclude-folder-windows.temp-0")).toBeInTheDocument();
    });

    /// A fresh analysis already has the exclusions applied, and the old
    /// indices point into a list that no longer exists.
    it("forgets the hidden rows on the next analysis", async () => {
      const user = await openPaths();
      await user.click(screen.getByTestId("exclude-file-windows.temp-0"));
      await waitFor(() =>
        expect(screen.queryByTestId("exclude-file-windows.temp-0")).toBeNull(),
      );

      await user.click(screen.getByRole("button", { name: /Analyze/ }));
      await waitFor(() => expect(screen.queryByTestId("stale-windows.temp")).toBeNull());
      expect(screen.getByTestId("result-windows.temp")).toHaveTextContent("2 files");
    });
  });

  it("disables the Clean button before any scan", async () => {
    render(<CleanPanel />);
    await screen.findByLabelText("Temporary files");
    expect(screen.getByRole("button", { name: /^Clean/ })).toBeDisabled();
  });

  it("cleans with the chosen mode and shows the report", async () => {
    const user = userEvent.setup();
    api.scan.mockResolvedValue([
      { rule_id: "windows.temp", file_count: 2, total_bytes: 2048, paths: [], skipped: 0 },
      { rule_id: "edge.cache", file_count: 0, total_bytes: 0, paths: [], skipped: 0 },
    ]);
    api.clean.mockResolvedValue({
      freed_bytes: 2048,
      deleted: 2,
      skipped: [
        {
          path: String.raw`C:\Users\T\AppData\Local\Temp\lock.tmp`,
          reason: "file in use",
          code: "in-use",
        },
      ],
    });
    render(<CleanPanel />);
    await screen.findByLabelText("Temporary files");
    await user.click(screen.getByRole("button", { name: /Analyze/ }));
    await screen.findByTestId("total-bytes");

    await user.selectOptions(screen.getByLabelText("Deletion mode"), "permanent");
    await user.click(screen.getByRole("button", { name: /^Clean/ }));
    await user.click(await screen.findByRole("button", { name: /Confirm cleanup/ }));

    await waitFor(() =>
      expect(api.clean).toHaveBeenCalledWith(["windows.temp", "edge.cache"], "permanent")
    );
    const report = await screen.findByTestId("clean-report");
    expect(report).toHaveTextContent("2 KB");
    expect(report).toHaveTextContent("2");
    // The code translated, never the raw message.
    expect(report).toHaveTextContent("File in use or locked");
  });

  it("copies the report as text to the clipboard", async () => {
    const user = userEvent.setup();
    const writeText = vi.fn().mockResolvedValue(undefined);
    vi.stubGlobal("navigator", { ...navigator, clipboard: { writeText } });
    api.scan.mockResolvedValue([
      { rule_id: "windows.temp", file_count: 2, total_bytes: 2048, paths: [], skipped: 0 },
      { rule_id: "edge.cache", file_count: 0, total_bytes: 0, paths: [], skipped: 0 },
    ]);
    api.clean.mockResolvedValue({
      freed_bytes: 2048,
      deleted: 2,
      skipped: [
        {
          path: String.raw`C:\Users\T\AppData\Local\Temp\lock.tmp`,
          reason: "file in use",
          code: "in-use",
        },
      ],
    });
    render(<CleanPanel />);
    await screen.findByLabelText("Temporary files");
    await user.click(screen.getByRole("button", { name: /Analyze/ }));
    await screen.findByTestId("total-bytes");
    await user.click(screen.getByRole("button", { name: /^Clean/ }));
    await user.click(await screen.findByRole("button", { name: /Confirm cleanup/ }));
    await screen.findByTestId("clean-report");

    await user.click(screen.getByTestId("copy-report"));

    await waitFor(() => expect(writeText).toHaveBeenCalledTimes(1));
    const text = writeText.mock.calls[0][0] as string;
    expect(text).toContain("WinCleaner");
    expect(text).toContain("Temporary files — 2 files measured, 2 KB");
    expect(text).toContain("Total: 2 files, 2 KB freed");
    expect(text).toContain("File in use or locked");
  });

  /// The footer used to render `mode.autoHelp` whatever was selected, so
  /// picking Permanent still read "recycle bin for the rest".
  it("shows the help text of the selected deletion mode", async () => {
    const user = userEvent.setup();
    render(<CleanPanel />);
    await screen.findByLabelText("Temporary files");
    const help = screen.getByTestId("mode-help");
    expect(help).toHaveTextContent("Auto: permanent deletion for low-risk items");

    await user.selectOptions(screen.getByLabelText("Deletion mode"), "trash");
    expect(help).toHaveTextContent("everything goes to the bin and stays recoverable");

    await user.selectOptions(screen.getByLabelText("Deletion mode"), "permanent");
    expect(help).toHaveTextContent("Nothing is recoverable");
    expect(help).not.toHaveTextContent("Auto:");
  });

  it("defaults to the auto mode", async () => {
    render(<CleanPanel />);
    await screen.findByLabelText("Temporary files");
    expect(screen.getByLabelText("Deletion mode")).toHaveValue("auto");
  });

  it("shows the banner when a targeted browser has a window open", async () => {
    api.runningBrowsers.mockResolvedValue([
      { process: "msedge.exe", name: "Microsoft Edge", processes: 3, has_window: true },
    ]);
    render(<CleanPanel />);
    expect(await screen.findByTestId("browser-warning")).toHaveTextContent(
      "Microsoft Edge is open: its cache files in use will be skipped. Close it for a complete cleanup."
    );
  });

  it("warns about background processes when the browser has no window left", async () => {
    api.runningBrowsers.mockResolvedValue([
      { process: "chrome.exe", name: "Google Chrome", processes: 9, has_window: false },
    ]);
    render(<CleanPanel />);
    expect(await screen.findByTestId("browser-warning")).toHaveTextContent(
      "Google Chrome is still running in the background (9 processes): quit it from the notification area, or its cache files in use will be skipped."
    );
  });

  it("offers no Quit button for a browser that has a window open", async () => {
    api.runningBrowsers.mockResolvedValue([
      { process: "msedge.exe", name: "Microsoft Edge", processes: 3, has_window: true },
    ]);
    render(<CleanPanel />);
    await screen.findByTestId("browser-warning");
    expect(screen.queryByRole("button", { name: "Quit Microsoft Edge" })).toBeNull();
  });

  it("names what force-closing costs before quitting a background browser", async () => {
    const user = userEvent.setup();
    api.runningBrowsers.mockResolvedValue([
      { process: "chrome.exe", name: "Google Chrome", processes: 9, has_window: false },
    ]);
    render(<CleanPanel />);
    await user.click(await screen.findByRole("button", { name: "Quit Google Chrome" }));

    expect(screen.getByTestId("confirm-quit")).toHaveTextContent(
      "Google Chrome will be force-closed. Nothing is open on screen, but Google Chrome may offer to restore its session next time it starts."
    );
    expect(api.quitBrowser).not.toHaveBeenCalled();

    await user.keyboard("{Escape}");
    expect(screen.queryByTestId("confirm-quit")).toBeNull();
  });

  it("stops the browser by process name and re-reads the banner", async () => {
    const user = userEvent.setup();
    api.runningBrowsers
      .mockResolvedValueOnce([
        { process: "chrome.exe", name: "Google Chrome", processes: 9, has_window: false },
      ])
      .mockResolvedValue([]);
    render(<CleanPanel />);
    await user.click(await screen.findByRole("button", { name: "Quit Google Chrome" }));
    await user.click(screen.getByRole("button", { name: "Quit Google Chrome" }));

    expect(api.quitBrowser).toHaveBeenCalledWith("chrome.exe");
    expect(toast.success).toHaveBeenCalledWith("Google Chrome stopped (9 processes)");
    await waitFor(() => expect(screen.queryByTestId("browser-warning")).toBeNull());
  });

  it("announces the quit confirmation as a live region", async () => {
    const user = userEvent.setup();
    api.runningBrowsers.mockResolvedValue([
      { process: "chrome.exe", name: "Google Chrome", processes: 9, has_window: false },
    ]);
    render(<CleanPanel />);
    await user.click(await screen.findByRole("button", { name: "Quit Google Chrome" }));

    const confirm = screen.getByTestId("confirm-quit");
    expect(confirm).toHaveAttribute("role", "status");
    expect(confirm).toHaveFocus();
  });

  it("moves the focus to Analyze once the browser is gone", async () => {
    const user = userEvent.setup();
    api.runningBrowsers
      .mockResolvedValueOnce([
        { process: "chrome.exe", name: "Google Chrome", processes: 9, has_window: false },
      ])
      .mockResolvedValue([]);
    render(<CleanPanel />);
    await user.click(await screen.findByRole("button", { name: "Quit Google Chrome" }));
    await user.click(screen.getByRole("button", { name: "Quit Google Chrome" }));

    await waitFor(() =>
      expect(screen.getByRole("button", { name: "Analyze" })).toHaveFocus()
    );
  });

  it("puts the focus back on the Quit button when the quit failed", async () => {
    const user = userEvent.setup();
    api.runningBrowsers.mockResolvedValue([
      { process: "chrome.exe", name: "Google Chrome", processes: 9, has_window: false },
    ]);
    api.quitBrowser.mockRejectedValue("browser-has-window");
    render(<CleanPanel />);
    await user.click(await screen.findByRole("button", { name: "Quit Google Chrome" }));
    await user.click(screen.getByRole("button", { name: "Quit Google Chrome" }));

    await waitFor(() =>
      expect(screen.getByRole("button", { name: "Quit Google Chrome" })).toHaveFocus()
    );
  });

  it("says so when a window appeared between the banner and the click", async () => {
    const user = userEvent.setup();
    api.runningBrowsers.mockResolvedValue([
      { process: "chrome.exe", name: "Google Chrome", processes: 9, has_window: false },
    ]);
    api.quitBrowser.mockRejectedValue("browser-has-window");
    render(<CleanPanel />);
    await user.click(await screen.findByRole("button", { name: "Quit Google Chrome" }));
    await user.click(screen.getByRole("button", { name: "Quit Google Chrome" }));

    await waitFor(() =>
      expect(toast.error).toHaveBeenCalledWith(
        "Google Chrome has just opened a window: close it yourself instead."
      )
    );
  });

  it("does not show the banner when no browser is open", async () => {
    render(<CleanPanel />);
    await screen.findByLabelText("Temporary files");
    expect(screen.queryByTestId("browser-warning")).toBeNull();
  });

  it("re-checks running browsers when Analyze is clicked", async () => {
    const user = userEvent.setup();
    api.runningBrowsers
      .mockResolvedValueOnce([])
      .mockResolvedValueOnce([
        { process: "chrome.exe", name: "Google Chrome", processes: 9, has_window: false },
      ]);
    render(<CleanPanel />);
    await screen.findByLabelText("Temporary files");
    expect(screen.queryByTestId("browser-warning")).toBeNull();

    await user.click(screen.getByRole("button", { name: /Analyze/ }));

    expect(await screen.findByTestId("browser-warning")).toHaveTextContent(
      "Google Chrome is still running in the background"
    );
    expect(api.runningBrowsers).toHaveBeenCalledTimes(2);
  });

  it("shows the rule loading error", async () => {
    api.listRules.mockRejectedValue("rules.toml is invalid: bad risk");
    render(<CleanPanel />);
    expect(await screen.findByTestId("rules-error")).toHaveTextContent("rules.toml is invalid");
  });

  it("the Retry button reloads the rules", async () => {
    const user = userEvent.setup();
    api.listRules.mockRejectedValueOnce("rules.toml is invalid: bad risk");
    render(<CleanPanel />);
    await screen.findByTestId("rules-error");

    api.listRules.mockResolvedValue(RULES);
    await user.click(screen.getByRole("button", { name: /Retry/ }));

    expect(await screen.findByLabelText("Temporary files")).toBeInTheDocument();
    expect(screen.queryByTestId("rules-error")).toBeNull();
  });

  it("explains the auto mode under the selector", async () => {
    render(<CleanPanel />);
    await screen.findByLabelText("Temporary files");
    expect(screen.getByTestId("mode-help")).toHaveTextContent(
      /Auto.*permanent deletion.*low-risk.*recycle bin/s
    );
  });

  it("warns that emptied directories go in every mode", async () => {
    render(<CleanPanel />);
    await screen.findByLabelText("Temporary files");
    expect(screen.getByTestId("empty-dirs-note")).toHaveTextContent(/whatever the mode/);
  });

  it("invites closing installers before cleaning the temporary folder", async () => {
    render(<CleanPanel />);
    await screen.findByLabelText("Temporary files");
    expect(
      screen.getByText(/Close any running installers before cleaning/)
    ).toBeInTheDocument();
  });

  it("flags that the Recycle Bin rule acts on every volume", async () => {
    api.listRules.mockResolvedValue([...RULES, RECYCLE_BIN]);
    render(<CleanPanel />);
    expect(await screen.findByTestId("note-windows.recycle-bin")).toHaveTextContent(
      "all volumes"
    );
    expect(
      screen.getByText(/Empties the recycle bin of every volume/)
    ).toBeInTheDocument();
    // The "files" rules do not carry that note.
    expect(screen.queryByTestId("note-windows.temp")).toBeNull();
  });

  it("does not check the Recycle Bin rule by default", async () => {
    api.listRules.mockResolvedValue([...RULES, RECYCLE_BIN]);
    render(<CleanPanel />);
    expect(await screen.findByLabelText("Recycle Bin")).not.toBeChecked();
    expect(screen.getByLabelText("Temporary files")).toBeChecked();
  });

  /// Its "scan" is the read-only `SHQueryRecycleBinW`: measuring it costs
  /// nothing and it stays unchecked, so Clean will not empty it.
  it("measures the rules unchecked by default, including the Recycle Bin", async () => {
    const user = userEvent.setup();
    api.listRules.mockResolvedValue([...RULES, RECYCLE_BIN]);
    render(<CleanPanel />);
    expect(await screen.findByLabelText("Recycle Bin")).not.toBeChecked();
    await user.click(screen.getByRole("button", { name: /Analyze/ }));
    await waitFor(() =>
      expect(api.scan).toHaveBeenCalledWith([
        "windows.temp",
        "edge.cache",
        "windows.recycle-bin",
      ])
    );
  });

  it("announces that the Recycle Bin is emptied first when it is checked", async () => {
    const user = userEvent.setup();
    api.listRules.mockResolvedValue([...RULES, RECYCLE_BIN]);
    render(<CleanPanel />);
    await screen.findByLabelText("Recycle Bin");
    expect(screen.queryByTestId("recycle-order-note")).toBeNull();
    await user.click(screen.getByLabelText("Recycle Bin"));
    expect(screen.getByTestId("recycle-order-note")).toHaveTextContent(/emptied first/);
  });

  it("greys out an unavailable rule and shows the reason", async () => {
    const user = userEvent.setup();
    api.listRules.mockResolvedValue([
      {
        ...RULES[0],
        default_checked: false,
        unavailable_reason: String.raw`path "%TEMP%\**\*" is outside the user profile`,
      },
      RULES[1],
    ]);
    render(<CleanPanel />);
    const checkbox = await screen.findByLabelText("Temporary files");
    expect(checkbox).toHaveAttribute("aria-disabled", "true");
    expect(checkbox).not.toBeChecked();
    expect(screen.getByTestId("unavailable-windows.temp")).toHaveTextContent(
      /outside the user profile/
    );

    // It is never sent to the back end, even by clicking the row.
    await user.click(screen.getByRole("button", { name: /Analyze/ }));
    await waitFor(() => expect(api.scan).toHaveBeenCalledWith(["edge.cache"]));
  });

  it("asks for confirmation before cleaning", async () => {
    const user = userEvent.setup();
    api.scan.mockResolvedValue([
      { rule_id: "windows.temp", file_count: 2, total_bytes: 2048, paths: [], skipped: 0 },
    ]);
    render(<CleanPanel />);
    await screen.findByLabelText("Temporary files");
    await user.click(screen.getByRole("button", { name: /Analyze/ }));
    await screen.findByTestId("total-bytes");
    await user.click(screen.getByRole("button", { name: /^Clean/ }));

    // Nothing is gone until the confirmation is given.
    expect(api.clean).not.toHaveBeenCalled();
    const confirmation = await screen.findByTestId("confirm-clean");
    expect(confirmation).toHaveTextContent("2 KB");
    expect(confirmation).toHaveTextContent(/Auto/);
    expect(confirmation).toHaveTextContent(/Temporary files/);

    await user.click(screen.getByRole("button", { name: /Confirm cleanup/ }));
    await waitFor(() => expect(api.clean).toHaveBeenCalled());
  });

  it("cancelling the confirmation cleans nothing", async () => {
    const user = userEvent.setup();
    api.scan.mockResolvedValue([
      { rule_id: "windows.temp", file_count: 2, total_bytes: 2048, paths: [], skipped: 0 },
    ]);
    render(<CleanPanel />);
    await screen.findByLabelText("Temporary files");
    await user.click(screen.getByRole("button", { name: /Analyze/ }));
    await screen.findByTestId("total-bytes");
    await user.click(screen.getByRole("button", { name: /^Clean/ }));
    await user.click(await screen.findByRole("button", { name: /Cancel/ }));

    expect(api.clean).not.toHaveBeenCalled();
    expect(screen.queryByTestId("confirm-clean")).toBeNull();
    expect(screen.getByRole("button", { name: /^Clean/ })).toBeInTheDocument();
  });

  /// The confirmation appears at the bottom of a screen the user is not
  /// looking at: without the focus move, a keyboard user has to Tab through
  /// every rule to reach it, and a screen reader is told nothing at all.
  it("moves the focus to the confirmation when it appears", async () => {
    const user = userEvent.setup();
    api.scan.mockResolvedValue([
      { rule_id: "windows.temp", file_count: 2, total_bytes: 2048, paths: [], skipped: 0 },
    ]);
    render(<CleanPanel />);
    await screen.findByLabelText("Temporary files");
    await user.click(screen.getByRole("button", { name: /Analyze/ }));
    await screen.findByTestId("total-bytes");
    await user.click(screen.getByRole("button", { name: /^Clean/ }));

    const confirmation = await screen.findByTestId("confirm-clean");
    expect(confirmation).toHaveFocus();
    // Cancel and Confirm are the next two tab stops, in that order.
    await user.tab();
    expect(screen.getByRole("button", { name: /Cancel/ })).toHaveFocus();
    await user.tab();
    expect(screen.getByRole("button", { name: /Confirm cleanup/ })).toHaveFocus();
  });

  it("Escape cancels the confirmation and gives the Clean button its focus back", async () => {
    const user = userEvent.setup();
    api.scan.mockResolvedValue([
      { rule_id: "windows.temp", file_count: 2, total_bytes: 2048, paths: [], skipped: 0 },
    ]);
    render(<CleanPanel />);
    await screen.findByLabelText("Temporary files");
    await user.click(screen.getByRole("button", { name: /Analyze/ }));
    await screen.findByTestId("total-bytes");
    await user.click(screen.getByRole("button", { name: /^Clean/ }));
    await screen.findByTestId("confirm-clean");

    await user.keyboard("{Escape}");

    expect(api.clean).not.toHaveBeenCalled();
    expect(screen.queryByTestId("confirm-clean")).toBeNull();
    expect(screen.getByRole("button", { name: /^Clean/ })).toHaveFocus();
  });

  it("gives the Clean button its focus back when Cancel is used", async () => {
    const user = userEvent.setup();
    api.scan.mockResolvedValue([
      { rule_id: "windows.temp", file_count: 2, total_bytes: 2048, paths: [], skipped: 0 },
    ]);
    render(<CleanPanel />);
    await screen.findByLabelText("Temporary files");
    await user.click(screen.getByRole("button", { name: /Analyze/ }));
    await screen.findByTestId("total-bytes");
    await user.click(screen.getByRole("button", { name: /^Clean/ }));
    await user.click(await screen.findByRole("button", { name: /Cancel/ }));

    expect(screen.getByRole("button", { name: /^Clean/ })).toHaveFocus();
  });

  it("names what the confirmation is about to do", async () => {
    const user = userEvent.setup();
    api.scan.mockResolvedValue([
      { rule_id: "windows.temp", file_count: 2, total_bytes: 2048, paths: [], skipped: 0 },
    ]);
    render(<CleanPanel />);
    await screen.findByLabelText("Temporary files");
    await user.click(screen.getByRole("button", { name: /Analyze/ }));
    await screen.findByTestId("total-bytes");
    await user.click(screen.getByRole("button", { name: /^Clean/ }));

    const confirmation = await screen.findByTestId("confirm-clean");
    expect(confirmation).toHaveAccessibleName("Clean 2 KB in Auto mode?");
    expect(confirmation).toHaveAccessibleDescription(/No way back/);
  });

  it("the confirmation lists the recycle bin as irreversible even in Recycle Bin mode", async () => {
    const user = userEvent.setup();
    api.listRules.mockResolvedValue([...RULES, RECYCLE_BIN]);
    api.scan.mockResolvedValue([
      { rule_id: "windows.temp", file_count: 2, total_bytes: 2048, paths: [], skipped: 0 },
      { rule_id: "windows.recycle-bin", file_count: 1, total_bytes: 10, paths: [], skipped: 0 },
    ]);
    render(<CleanPanel />);
    await user.click(await screen.findByLabelText("Recycle Bin"));
    await user.selectOptions(screen.getByLabelText("Deletion mode"), "trash");
    await user.click(screen.getByRole("button", { name: /Analyze/ }));
    await screen.findByTestId("total-bytes");
    await user.click(screen.getByRole("button", { name: /^Clean/ }));

    const confirmation = await screen.findByTestId("confirm-clean");
    expect(confirmation).toHaveTextContent(/no way back/i);
    expect(confirmation).toHaveTextContent("Recycle Bin");
    // In Recycle Bin mode, temporary files stay recoverable.
    expect(confirmation).not.toHaveTextContent("Temporary files");
  });

  const APP_RULE = {
    id: "winapp2.7-zip",
    category: "Applications",
    label: "7-Zip",
    risk: "medium",
    kind: "files",
    default_checked: false,
    note: "This deletes the saved archive history.",
    unavailable_reason: null,
  };

  it("collapses the Applications category by default and expands it on demand", async () => {
    const user = userEvent.setup();
    api.listRules.mockResolvedValue([...RULES, APP_RULE]);
    render(<CleanPanel />);
    await screen.findByLabelText("Temporary files");

    // The category header is there with its count, the rows are not.
    expect(screen.getByText("Applications")).toBeInTheDocument();
    expect(screen.queryByLabelText("7-Zip")).toBeNull();

    await user.click(screen.getByTestId("toggle-category-Applications"));
    expect(await screen.findByLabelText("7-Zip")).toBeInTheDocument();
    // The categories that were never collapsed stay visible.
    expect(screen.getByLabelText("Temporary files")).toBeInTheDocument();
  });

  it("filters the rules by label across every category", async () => {
    const user = userEvent.setup();
    api.listRules.mockResolvedValue([...RULES, APP_RULE]);
    render(<CleanPanel />);
    await screen.findByLabelText("Temporary files");

    await user.type(screen.getByTestId("rule-search"), "zip");

    // A search reaches into the collapsed category: hiding a match would make
    // the search field lie.
    expect(await screen.findByLabelText("7-Zip")).toBeInTheDocument();
    expect(screen.queryByLabelText("Temporary files")).toBeNull();
    expect(screen.queryByText("System")).toBeNull();
  });

  it("shows the rule counts and credits Winapp2", async () => {
    const user = userEvent.setup();
    api.listRules.mockResolvedValue([...RULES, APP_RULE]);
    render(<CleanPanel />);

    const summary = await screen.findByTestId("rules-summary");
    expect(summary).toHaveTextContent("10 built-in");
    expect(summary).toHaveTextContent("42");
    expect(summary).toHaveTextContent("1,200");

    await user.click(screen.getByTestId("toggle-category-Applications"));
    expect(await screen.findByTestId("winapp2-attribution")).toHaveTextContent(
      "Winapp2 (CC-BY-SA 4.0)"
    );
  });

  /// The credit must not stretch over the native rules of the category: a
  /// category holding only built-in rules carries no Winapp2 line.
  it("does not credit Winapp2 in a category holding no community rule", async () => {
    const user = userEvent.setup();
    const NATIVE_APP_RULE = {
      ...APP_RULE,
      id: "npm.cache",
      label: "npm cache",
      risk: "low",
      default_checked: true,
      note: null,
    };
    api.listRules.mockResolvedValue([...RULES, NATIVE_APP_RULE]);
    render(<CleanPanel />);
    await screen.findByLabelText("Temporary files");

    await user.click(screen.getByTestId("toggle-category-Applications"));
    expect(await screen.findByLabelText("npm cache")).toBeInTheDocument();
    expect(screen.queryByTestId("winapp2-attribution")).toBeNull();
  });

  /// Folding and filtering are presentation: a rule the user ticked and then
  /// hid behind a search must still be analysed, otherwise the search field
  /// silently unselects rules.
  it("analyses a selected rule that the search filter hides", async () => {
    const user = userEvent.setup();
    render(<CleanPanel />);
    await screen.findByLabelText("Temporary files");

    await user.type(screen.getByTestId("rule-search"), "Edge");
    expect(screen.queryByLabelText("Temporary files")).toBeNull();

    await user.click(screen.getByRole("button", { name: /Analyze/ }));
    await waitFor(() =>
      expect(api.scan).toHaveBeenCalledWith(["windows.temp", "edge.cache"])
    );
  });

  it("shows the note carried by a rule", async () => {
    const user = userEvent.setup();
    api.listRules.mockResolvedValue([...RULES, APP_RULE]);
    render(<CleanPanel />);
    await screen.findByLabelText("Temporary files");
    await user.click(screen.getByTestId("toggle-category-Applications"));
    expect(await screen.findByTestId("warning-winapp2.7-zip")).toHaveTextContent(
      "saved archive history"
    );
  });

  it("does not check the Winapp2 rules by default", async () => {
    const user = userEvent.setup();
    api.listRules.mockResolvedValue([...RULES, APP_RULE]);
    render(<CleanPanel />);
    await screen.findByLabelText("Temporary files");
    await user.click(screen.getByTestId("toggle-category-Applications"));
    expect(await screen.findByLabelText("7-Zip")).not.toBeChecked();

    // Measured all the same, so the user can see what checking it would free.
    await user.click(screen.getByRole("button", { name: /Analyze/ }));
    await waitFor(() =>
      expect(api.scan).toHaveBeenCalledWith([
        "windows.temp",
        "edge.cache",
        "winapp2.7-zip",
      ])
    );
  });

  it("shows a message when the search matches nothing", async () => {
    const user = userEvent.setup();
    render(<CleanPanel />);
    await screen.findByLabelText("Temporary files");

    await user.type(screen.getByTestId("rule-search"), "nothing matches this");

    expect(await screen.findByTestId("search-empty")).toHaveTextContent(
      "No rules match your search."
    );
  });

  it("sorts the rules by size when the toggle is on, and remembers it", async () => {
    const user = userEvent.setup();
    window.localStorage.clear();
    api.scan.mockResolvedValue([
      { rule_id: "windows.temp", file_count: 1, total_bytes: 1024, paths: [], skipped: 0 },
      { rule_id: "edge.cache", file_count: 1, total_bytes: 4096, paths: [], skipped: 0 },
    ]);
    const { unmount } = render(<CleanPanel />);
    await screen.findByLabelText("Temporary files");

    // Off by default, and only usable once there is something to sort.
    expect(screen.getByLabelText("Sort by size")).not.toBeChecked();
    await user.click(screen.getByRole("button", { name: /Analyze/ }));
    await screen.findByTestId("total-bytes");

    await user.click(screen.getByLabelText("Sort by size"));
    // Browsers (4 KB) now comes before System (1 KB).
    const headings = screen.getAllByRole("heading", { level: 2 }).map((h) => h.textContent);
    expect(headings).toEqual(["Browsers", "System"]);
    expect(window.localStorage.getItem("wincleaner.sortBySize")).toBe("1");

    unmount();
    render(<CleanPanel />);
    expect(await screen.findByLabelText("Sort by size")).toBeChecked();
  });

  it("keeps the rules.toml order when the toggle is off", async () => {
    const user = userEvent.setup();
    window.localStorage.clear();
    api.scan.mockResolvedValue([
      { rule_id: "windows.temp", file_count: 1, total_bytes: 1024, paths: [], skipped: 0 },
      { rule_id: "edge.cache", file_count: 1, total_bytes: 4096, paths: [], skipped: 0 },
    ]);
    render(<CleanPanel />);
    await screen.findByLabelText("Temporary files");
    await user.click(screen.getByRole("button", { name: /Analyze/ }));
    await screen.findByTestId("total-bytes");

    const headings = screen.getAllByRole("heading", { level: 2 }).map((h) => h.textContent);
    expect(headings).toEqual(["System", "Browsers"]);
  });

  /// Every category header is a toggle, not only the ones folded by default:
  /// after a scan any of them may be folded, so the affordance must be there
  /// from the first render.
  it("makes every category header a real toggle button", async () => {
    const user = userEvent.setup();
    api.listRules.mockResolvedValue([...RULES, APP_RULE]);
    render(<CleanPanel />);
    await screen.findByLabelText("Temporary files");

    const system = screen.getByTestId("toggle-category-System");
    expect(system.tagName).toBe("BUTTON");
    expect(system).toHaveAttribute("aria-expanded", "true");
    expect(screen.getByTestId("toggle-category-Applications")).toHaveAttribute(
      "aria-expanded",
      "false"
    );

    await user.click(system);
    expect(screen.getByTestId("toggle-category-System")).toHaveAttribute(
      "aria-expanded",
      "false"
    );
    expect(screen.queryByLabelText("Temporary files")).toBeNull();
  });

  it("shows a loading state while analysing", async () => {
    const user = userEvent.setup();
    let release: (results: unknown[]) => void = () => {};
    api.scan.mockImplementation(
      () => new Promise((resolve) => { release = resolve as typeof release; })
    );
    render(<CleanPanel />);
    await screen.findByLabelText("Temporary files");
    await user.click(screen.getByRole("button", { name: /Analyze/ }));

    expect(await screen.findByRole("button", { name: /Analyzing/ })).toBeDisabled();
    expect(screen.getByTestId("hero-status")).toHaveTextContent("Analyzing 2 rules");
    const list = screen.getByTestId("rule-list");
    expect(list.className).toContain("opacity-60");
    expect(list.className).toContain("pointer-events-none");

    release([]);
    await waitFor(() => expect(screen.queryByTestId("hero-status")).toBeNull());
  });

  /// The spinner said nothing about a walk that lasts ten to thirty seconds.
  /// The bar and the counter come from the events, not from a guess.
  it("draws the progress of the scan rule by rule", async () => {
    const user = userEvent.setup();
    let release: (results: unknown[]) => void = () => {};
    api.scan.mockImplementation(
      () => new Promise((resolve) => { release = resolve as typeof release; })
    );
    render(<CleanPanel />);
    await screen.findByLabelText("Temporary files");
    await user.click(screen.getByRole("button", { name: /Analyze/ }));
    await waitFor(() => expect(api.onScanProgress).toHaveBeenCalled());

    act(() =>
      emitProgress({ done: 1, total: 4, rule_id: "windows.temp", label: "Temporary files", total_bytes: 1024 })
    );
    expect(screen.getByTestId("scan-progress")).toHaveTextContent(
      "Analyzing 1 / 4 · Temporary files"
    );
    expect(screen.getByTestId("scan-progress-bar")).toHaveStyle({ width: "25%" });
    expect(screen.getByTestId("scan-progress-bytes")).toHaveTextContent("1 KB");

    act(() =>
      emitProgress({ done: 2, total: 4, rule_id: "edge.cache", label: "Microsoft Edge cache", total_bytes: 3072 })
    );
    expect(screen.getByTestId("scan-progress")).toHaveTextContent(
      "Analyzing 2 / 4 · Microsoft Edge cache"
    );
    expect(screen.getByTestId("scan-progress-bar")).toHaveStyle({ width: "50%" });
    expect(screen.getByTestId("scan-progress-bytes")).toHaveTextContent("3 KB");

    act(() =>
      emitProgress({ done: 4, total: 4, rule_id: "winapp2.7-zip", label: "7-Zip", total_bytes: 4096 })
    );
    expect(screen.getByTestId("scan-progress-bar")).toHaveStyle({ width: "100%" });

    // Once the scan returns, the normal post-scan hero takes the slot back.
    release([
      { rule_id: "windows.temp", file_count: 1, total_bytes: 4096, paths: [], skipped: 0 },
    ]);
    await waitFor(() => expect(screen.queryByTestId("scan-progress")).toBeNull());
    expect(screen.getByTestId("total-bytes")).toHaveTextContent("4 KB");
  });

  /// The counter used to name the rule that had just *finished*: with a full
  /// Recycle Bin measured last, the hero read "84 / 85 · AMD" for four minutes
  /// while the bin was the one everything was waiting on.
  it("names the rules still being measured, not the one that just finished", async () => {
    const user = userEvent.setup();
    api.listRules.mockResolvedValue([...RULES, RECYCLE_BIN]);
    let release: (results: unknown[]) => void = () => {};
    api.scan.mockImplementation(
      () => new Promise((resolve) => { release = resolve as typeof release; })
    );
    render(<CleanPanel />);
    await screen.findByLabelText("Temporary files");
    await user.click(screen.getByRole("button", { name: /Analyze/ }));
    await waitFor(() => expect(api.onScanProgress).toHaveBeenCalled());

    act(() =>
      emitProgress({
        done: 2,
        total: 3,
        rule_id: "edge.cache",
        label: "Microsoft Edge cache",
        total_bytes: 1024,
        running: [{ rule_id: "windows.recycle-bin", label: "Recycle Bin" }],
      })
    );
    expect(screen.getByTestId("scan-progress")).toHaveTextContent(
      "Analyzing 2 / 3 · still measuring: Recycle Bin"
    );

    // The last event has nothing left in flight: the counter goes back to
    // naming what it just closed.
    act(() =>
      emitProgress({
        done: 3,
        total: 3,
        rule_id: "windows.recycle-bin",
        label: "Recycle Bin",
        total_bytes: 2048,
        running: [],
      })
    );
    expect(screen.getByTestId("scan-progress")).toHaveTextContent(
      "Analyzing 3 / 3 · Recycle Bin"
    );
    release([]);
  });

  /// A live region fed by every event would read the same sentence hundreds of
  /// times over a thirty-second walk. `done % 10` used to gate this, but never
  /// fires at all below ten rules; time is what actually bounds the silence.
  it("announces the scan at most once every five seconds, and on completion", async () => {
    const user = userEvent.setup();
    let release: (results: unknown[]) => void = () => {};
    api.scan.mockImplementation(
      () => new Promise((resolve) => { release = resolve as typeof release; })
    );
    render(<CleanPanel />);
    await screen.findByLabelText("Temporary files");
    await user.click(screen.getByRole("button", { name: /Analyze/ }));
    await waitFor(() => expect(api.onScanProgress).toHaveBeenCalled());

    const live = screen.getByTestId("scan-announcement");
    expect(live).toHaveAttribute("aria-live", "polite");

    // Fake timers only from here: rendering and `userEvent` above need real
    // ones to resolve their own internal waits.
    vi.useFakeTimers();
    try {

      // Three events inside one second: only the first is announced.
      act(() =>
        emitProgress({ done: 1, total: 25, rule_id: "a", label: "A", total_bytes: 1024 })
      );
      expect(live).toHaveTextContent("Analyzing: 1 of 25 rules, 1 KB so far");
      act(() => {
        vi.advanceTimersByTime(400);
        emitProgress({ done: 2, total: 25, rule_id: "b", label: "B", total_bytes: 2048 });
      });
      expect(live).toHaveTextContent("Analyzing: 1 of 25 rules, 1 KB so far");
      act(() => {
        vi.advanceTimersByTime(400);
        emitProgress({ done: 3, total: 25, rule_id: "c", label: "C", total_bytes: 3072 });
      });
      expect(live).toHaveTextContent("Analyzing: 1 of 25 rules, 1 KB so far");

      // Five seconds after the last announcement, the next event is spoken.
      act(() => {
        vi.advanceTimersByTime(4200);
        emitProgress({ done: 4, total: 25, rule_id: "d", label: "D", total_bytes: 4096 });
      });
      expect(live).toHaveTextContent("Analyzing: 4 of 25 rules, 4 KB so far");

      // The final event is always announced, throttle or not.
      act(() =>
        emitProgress({ done: 25, total: 25, rule_id: "z", label: "Z", total_bytes: 5120 })
      );
      expect(live).toHaveTextContent("Analyzing: 25 of 25 rules, 5 KB so far");
    } finally {
      vi.useRealTimers();
    }

    release([
      { rule_id: "windows.temp", file_count: 1, total_bytes: 4096, paths: [], skipped: 0 },
    ]);
    await waitFor(() =>
      expect(live).toHaveTextContent("Analysis complete: 4 KB reclaimable in 1 selected rule")
    );
  });

  it("announces the cleanup report and the rule loading error", async () => {
    const user = userEvent.setup();
    api.scan.mockResolvedValue([
      { rule_id: "windows.temp", file_count: 2, total_bytes: 2048, paths: [], skipped: 0 },
    ]);
    api.clean.mockResolvedValue({ freed_bytes: 2048, deleted: 2, skipped: [] });
    render(<CleanPanel />);
    await screen.findByLabelText("Temporary files");
    await user.click(screen.getByRole("button", { name: /Analyze/ }));
    await screen.findByTestId("total-bytes");
    await user.click(screen.getByRole("button", { name: /^Clean/ }));
    await user.click(await screen.findByRole("button", { name: /Confirm cleanup/ }));

    const report = await screen.findByTestId("clean-report");
    expect(report).toHaveAttribute("role", "status");
    expect(report).toHaveAccessibleName("Last cleanup");
  });

  /// Analyzes, then starts a clean that never resolves: the hero is left on
  /// the state the `clean-progress` events paint.
  async function startPendingClean() {
    const user = userEvent.setup();
    api.scan.mockResolvedValue([
      { rule_id: "windows.temp", file_count: 2, total_bytes: 2048, paths: [], skipped: 0 },
    ]);
    api.clean.mockImplementation(() => new Promise(() => {}));
    render(<CleanPanel />);
    await screen.findByLabelText("Temporary files");
    await user.click(screen.getByRole("button", { name: /Analyze/ }));
    await screen.findByTestId("total-bytes");
    await user.click(screen.getByRole("button", { name: /^Clean/ }));
    await user.click(await screen.findByRole("button", { name: /Confirm cleanup/ }));
    await waitFor(() => expect(api.onCleanProgress).toHaveBeenCalled());
  }

  /// A disabled "Cleaning…" button said nothing about thirteen minutes of
  /// work. The bar and the running total come from the events.
  it("draws the progress of the clean rule by rule", async () => {
    await startPendingClean();

    expect(screen.getByTestId("hero-status")).toHaveTextContent("Cleaning 1 rule");

    act(() =>
      emitClean({
        done_rules: 1,
        total_rules: 4,
        rule_id: "windows.recycle-bin",
        label: "Recycle Bin",
        files_deleted: 500,
        bytes_freed: 1024,
      })
    );
    expect(screen.getByTestId("clean-progress")).toHaveTextContent(
      "Cleaning 1 / 4 · Recycle Bin"
    );
    expect(screen.getByTestId("clean-progress-bar")).toHaveStyle({ width: "25%" });
    expect(screen.getByTestId("clean-progress-bytes")).toHaveTextContent("1 KB");
    expect(screen.getByTestId("clean-progress-files")).toHaveTextContent("500 files deleted");

    act(() =>
      emitClean({
        done_rules: 3,
        total_rules: 4,
        rule_id: "windows.temp",
        label: "Temporary files",
        files_deleted: 1200,
        bytes_freed: 3072,
      })
    );
    expect(screen.getByTestId("clean-progress")).toHaveTextContent(
      "Cleaning 3 / 4 · Temporary files"
    );
    expect(screen.getByTestId("clean-progress-bar")).toHaveStyle({ width: "75%" });
    expect(screen.getByTestId("clean-progress-bytes")).toHaveTextContent("3 KB");
    expect(screen.getByTestId("clean-progress-files")).toHaveTextContent("1,200 files deleted");

    // The action bar keeps its disabled button while all this happens.
    expect(screen.getByRole("button", { name: /Cleaning/ })).toBeDisabled();
  });

  /// `done_rules % 10` never fires below ten rules, and a rule reporting every
  /// 500 files never moves `done_rules` at all in between: time is what
  /// actually bounds how long the live region stays silent.
  it("announces the clean at most once every five seconds, and on completion", async () => {
    await startPendingClean();
    // Fake timers only from here: `startPendingClean` drives `userEvent`,
    // which needs real ones to resolve its own internal delays.
    vi.useFakeTimers();
    try {
      const live = screen.getByTestId("scan-announcement");

      // Three events inside one second, same rule still running: only the
      // first is announced.
      act(() =>
        emitClean({ done_rules: 1, total_rules: 25, rule_id: "a", label: "A", files_deleted: 500, bytes_freed: 1024 })
      );
      expect(live).toHaveTextContent("Cleaning: 1 of 25 rules, 1 KB freed so far");
      act(() => {
        vi.advanceTimersByTime(400);
        emitClean({ done_rules: 1, total_rules: 25, rule_id: "a", label: "A", files_deleted: 1000, bytes_freed: 2048 });
      });
      expect(live).toHaveTextContent("Cleaning: 1 of 25 rules, 1 KB freed so far");
      act(() => {
        vi.advanceTimersByTime(400);
        emitClean({ done_rules: 1, total_rules: 25, rule_id: "a", label: "A", files_deleted: 1500, bytes_freed: 3072 });
      });
      expect(live).toHaveTextContent("Cleaning: 1 of 25 rules, 1 KB freed so far");

      // Five seconds after the last announcement, the next event is spoken —
      // even though it is still the very same rule.
      act(() => {
        vi.advanceTimersByTime(4200);
        emitClean({ done_rules: 1, total_rules: 25, rule_id: "a", label: "A", files_deleted: 2000, bytes_freed: 4096 });
      });
      expect(live).toHaveTextContent("Cleaning: 1 of 25 rules, 4 KB freed so far");

      // The final event is always announced, throttle or not.
      act(() =>
        emitClean({ done_rules: 25, total_rules: 25, rule_id: "z", label: "Z", files_deleted: 3000, bytes_freed: 5120 })
      );
      expect(live).toHaveTextContent("Cleaning: 25 of 25 rules, 5 KB freed so far");
    } finally {
      vi.useRealTimers();
    }
  });

  /// An event arriving outside a clean (a previous run finishing late) must not
  /// repaint a hero that is showing results.
  it("ignores clean progress events when no clean is pending", async () => {
    render(<CleanPanel />);
    await screen.findByLabelText("Temporary files");
    await waitFor(() => expect(api.onCleanProgress).toHaveBeenCalled());

    act(() =>
      emitClean({
        done_rules: 1,
        total_rules: 4,
        rule_id: "windows.temp",
        label: "Temporary files",
        files_deleted: 10,
        bytes_freed: 1024,
      })
    );
    expect(screen.queryByTestId("clean-progress")).toBeNull();
    expect(screen.queryByTestId("clean-progress-bytes")).toBeNull();
  });

  it("raises the rule loading error as an alert", async () => {
    api.listRules.mockRejectedValue("rules.toml is invalid: bad risk");
    render(<CleanPanel />);
    expect(await screen.findByRole("alert")).toHaveTextContent("rules.toml is invalid");
  });

  /// An event arriving outside a scan (a previous run finishing late) must not
  /// repaint a hero that is showing results.
  it("ignores progress events when no scan is pending", async () => {
    render(<CleanPanel />);
    await screen.findByLabelText("Temporary files");
    await waitFor(() => expect(api.onScanProgress).toHaveBeenCalled());

    act(() =>
      emitProgress({ done: 1, total: 4, rule_id: "windows.temp", label: "Temporary files", total_bytes: 1024 })
    );
    expect(screen.queryByTestId("scan-progress")).toBeNull();
  });

  it("stops listening for progress when it unmounts", async () => {
    const { unmount } = render(<CleanPanel />);
    await screen.findByLabelText("Temporary files");
    await waitFor(() => expect(api.onScanProgress).toHaveBeenCalled());

    unmount();
    await waitFor(() => expect(unlisten).toHaveBeenCalled());
    await waitFor(() => expect(cleanUnlisten).toHaveBeenCalled());
  });

  /// Biggest wins first: the scan decides what is worth looking at.
  it("unfolds the categories holding bytes and folds the empty ones after a scan", async () => {
    const user = userEvent.setup();
    api.listRules.mockResolvedValue([...RULES, APP_RULE]);
    api.scan.mockResolvedValue([
      { rule_id: "windows.temp", file_count: 1, total_bytes: 1024, paths: [], skipped: 0 },
      { rule_id: "edge.cache", file_count: 0, total_bytes: 0, paths: [], skipped: 0 },
      { rule_id: "winapp2.7-zip", file_count: 1, total_bytes: 4096, paths: [], skipped: 0 },
    ]);
    render(<CleanPanel />);
    await screen.findByLabelText("Temporary files");
    await user.click(screen.getByTestId("toggle-category-Applications"));
    await user.click(await screen.findByLabelText("7-Zip"));
    await user.click(screen.getByRole("button", { name: /Analyze/ }));
    await screen.findByTestId("total-bytes");

    expect(screen.getByTestId("toggle-category-Applications")).toHaveAttribute(
      "aria-expanded",
      "true"
    );
    expect(screen.getByLabelText("7-Zip")).toBeInTheDocument();
    expect(screen.getByLabelText("Temporary files")).toBeInTheDocument();
    expect(screen.getByTestId("toggle-category-Browsers")).toHaveAttribute(
      "aria-expanded",
      "false"
    );
    expect(screen.queryByLabelText("Microsoft Edge cache")).toBeNull();

    // A fold the user asks for after the scan is respected.
    await user.click(screen.getByTestId("toggle-category-System"));
    expect(screen.queryByLabelText("Temporary files")).toBeNull();
  });

  it("shows the first-launch hint until it is dismissed, and remembers it", async () => {
    const user = userEvent.setup();
    window.localStorage.clear();
    const { unmount } = render(<CleanPanel />);

    const hint = await screen.findByTestId("first-launch-hint");
    expect(hint).toHaveTextContent(/Recycle Bin/);
    await user.click(screen.getByRole("button", { name: /Got it/ }));
    expect(screen.queryByTestId("first-launch-hint")).toBeNull();
    expect(window.localStorage.getItem("wincleaner.hintDismissed")).toBe("1");

    unmount();
    render(<CleanPanel />);
    await screen.findByLabelText("Temporary files");
    expect(screen.queryByTestId("first-launch-hint")).toBeNull();
  });

  describe("sandbox verdict", () => {
    const SANDBOX = {
      root: String.raw`C:\Users\T\AppData\Local\Temp\wincleaner-sandbox-1a2b`,
      sentinels: 52,
      junk: 119,
      winapp2_rules: 14,
    };

    const PASS = {
      sentinels_total: 52,
      sentinels_intact: 52,
      sentinels_damaged: [],
      junk_total: 119,
      junk_removed: 119,
      junk_remaining: [],
      rules_cleaned: 9,
      outside_total: 2,
      outside_intact: 2,
      junctions_refused: true,
    };

    beforeEach(() => {
      api.scan.mockResolvedValue([
        { rule_id: "windows.temp", file_count: 2, total_bytes: 2048, paths: [], skipped: 0 },
      ]);
    });

    async function cleanOnce() {
      const user = userEvent.setup();
      render(<CleanPanel sandbox={SANDBOX} />);
      await screen.findByLabelText("Temporary files");
      await user.click(screen.getByRole("button", { name: /Analyze/ }));
      await screen.findByTestId("total-bytes");
      await user.click(screen.getByRole("button", { name: /^Clean/ }));
      await user.click(await screen.findByRole("button", { name: /Confirm cleanup/ }));
    }

    it("is not asked for outside the sandbox", async () => {
      const user = userEvent.setup();
      render(<CleanPanel />);
      await screen.findByLabelText("Temporary files");
      await user.click(screen.getByRole("button", { name: /Analyze/ }));
      await screen.findByTestId("total-bytes");
      await user.click(screen.getByRole("button", { name: /^Clean/ }));
      await user.click(await screen.findByRole("button", { name: /Confirm cleanup/ }));
      await screen.findByTestId("clean-report");
      expect(api.sandboxVerify).not.toHaveBeenCalled();
      expect(screen.queryByTestId("sandbox-verdict")).toBeNull();
    });

    it("reads the disk back after a sandbox clean and passes", async () => {
      api.sandboxVerify.mockResolvedValue(PASS);
      await cleanOnce();

      const verdict = await screen.findByTestId("sandbox-verdict");
      expect(verdict).toHaveAttribute("data-verdict", "pass");
      expect(verdict).toHaveTextContent("Sentinels intact 52 / 52");
      expect(verdict).toHaveTextContent("Junk removed 119 / 119 for the 9 rules cleaned");
      expect(verdict).toHaveTextContent("Junction baits untouched 2 / 2");
      expect(api.sandboxVerify).toHaveBeenCalledWith(["windows.temp"]);
    });

    it("names what was damaged or survived when it fails", async () => {
      api.sandboxVerify.mockResolvedValue({
        ...PASS,
        sentinels_intact: 51,
        sentinels_damaged: [String.raw`C:\sandbox\profile\Documents\thesis.docx`],
        junk_removed: 118,
        junk_remaining: [String.raw`C:\sandbox\profile\AppData\Local\Temp\stray.tmp`],
        outside_intact: 1,
        junctions_refused: false,
      });
      await cleanOnce();

      const verdict = await screen.findByTestId("sandbox-verdict");
      expect(verdict).toHaveAttribute("data-verdict", "fail");
      expect(verdict).toHaveTextContent("Sentinels intact 51 / 52");
      expect(verdict).toHaveTextContent("thesis.docx");
      expect(verdict).toHaveTextContent("stray.tmp");
      expect(verdict).toHaveTextContent("Junction baits untouched 1 / 2");
      expect(verdict).toHaveTextContent("A junction the sandbox planted no longer stands.");
    });

    /// A verify that fails says nothing about the clean that succeeded: the
    /// report used to be thrown away and replaced by the rules-error screen.
    it("keeps the cleanup report when reading the sandbox back fails", async () => {
      api.sandboxVerify.mockRejectedValue("No sandbox is active.");
      await cleanOnce();

      expect(await screen.findByTestId("clean-report")).toBeInTheDocument();
      expect(screen.queryByTestId("rules-error")).toBeNull();
      expect(screen.queryByTestId("sandbox-verdict")).toBeNull();
      expect(await screen.findByTestId("sandbox-verdict-error")).toHaveTextContent(
        "No sandbox is active.",
      );
    });

    /// The sandbox exists to exercise everything: holding back the rules a
    /// real profile would leave unchecked is the whole point of not being a
    /// real profile.
    it("starts with every available rule checked", async () => {
      api.listRules.mockResolvedValue([...RULES, { ...RECYCLE_BIN, default_checked: false }]);
      render(<CleanPanel sandbox={SANDBOX} />);
      await screen.findByLabelText("Temporary files");
      expect(screen.getByLabelText("Recycle Bin")).toBeChecked();
    });

    it("leaves the default selection alone outside the sandbox", async () => {
      api.listRules.mockResolvedValue([...RULES, { ...RECYCLE_BIN, default_checked: false }]);
      render(<CleanPanel />);
      await screen.findByLabelText("Temporary files");
      expect(screen.getByLabelText("Recycle Bin")).not.toBeChecked();
    });
  });

  /// The native rules' French label (`label_fr` in rules.toml) shows up only
  /// when the interface is French; Winapp2 rules have none and always fall
  /// back to English. `CleanPanel` has no provider of its own in the tests
  /// above (default context: English), so these wrap it in `I18nProvider`.
  describe("native rule labels in French", () => {
    afterEach(() => {
      window.localStorage.removeItem(LANGUAGE_KEY);
    });

    it("shows the French label when the interface is French", async () => {
      window.localStorage.setItem(LANGUAGE_KEY, "fr");
      render(
        <I18nProvider>
          <CleanPanel />
        </I18nProvider>,
      );
      expect(await screen.findByText("Fichiers temporaires")).toBeInTheDocument();
      // A Winapp2-style rule with no label_fr still reads in English.
      expect(screen.getByText("Microsoft Edge cache")).toBeInTheDocument();
    });

    it("shows the English label when the interface is English", async () => {
      render(<CleanPanel />);
      expect(await screen.findByText("Temporary files")).toBeInTheDocument();
    });

    it("uses the French label in the Analyze progress line", async () => {
      const user = userEvent.setup();
      let release: (results: unknown[]) => void = () => {};
      api.scan.mockImplementation(
        () => new Promise((resolve) => { release = resolve as typeof release; })
      );
      window.localStorage.setItem(LANGUAGE_KEY, "fr");
      render(
        <I18nProvider>
          <CleanPanel />
        </I18nProvider>,
      );
      await screen.findByText("Fichiers temporaires");
      await user.click(screen.getByRole("button", { name: /Analyser/ }));
      await waitFor(() => expect(api.onScanProgress).toHaveBeenCalled());

      // Rust never localises the event's own `label` (see CLAUDE.md): the
      // front end looks the French label up by `rule_id` from the loaded
      // rule summaries instead of trusting the event's English text.
      act(() =>
        emitProgress({
          done: 1,
          total: 1,
          rule_id: "windows.temp",
          label: "Temporary files",
          total_bytes: 1024,
        })
      );
      expect(screen.getByTestId("scan-progress")).toHaveTextContent("Fichiers temporaires");

      release([]);
    });
  });
});
