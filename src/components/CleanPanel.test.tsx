import { describe, it, expect, vi, beforeEach } from "vitest";
import { render, screen, waitFor } from "@testing-library/react";
import userEvent from "@testing-library/user-event";

const api = {
  listRules: vi.fn(),
  rulesSummary: vi.fn(),
  scan: vi.fn(),
  clean: vi.fn(),
  runningBrowsers: vi.fn(),
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
  };
});

import { CleanPanel } from "./CleanPanel";

const RULES = [
  { id: "windows.temp", category: "System", label: "Temporary files", risk: "low", kind: "files", default_checked: true },
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

describe("CleanPanel", () => {
  beforeEach(() => {
    api.listRules.mockReset().mockResolvedValue(RULES);
    api.scan.mockReset().mockResolvedValue([]);
    api.clean.mockReset().mockResolvedValue({ freed_bytes: 0, deleted: 0, skipped: [] });
    api.runningBrowsers.mockReset().mockResolvedValue([]);
    api.rulesSummary.mockReset().mockResolvedValue({
      native: 10,
      winapp2_retained: 1200,
      winapp2_detected: 42,
      winapp2_dropped: 900,
    });
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
      skipped: [{ path: String.raw`C:\Users\T\AppData\Local\Temp\lock.tmp`, reason: "file in use" }],
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
    expect(report).toHaveTextContent("file in use");
  });

  it("defaults to the auto mode", async () => {
    render(<CleanPanel />);
    await screen.findByLabelText("Temporary files");
    expect(screen.getByLabelText("Deletion mode")).toHaveValue("auto");
  });

  it("shows the banner when a targeted browser is open", async () => {
    api.runningBrowsers.mockResolvedValue(["msedge.exe"]);
    render(<CleanPanel />);
    expect(await screen.findByTestId("browser-warning")).toHaveTextContent("msedge.exe");
  });

  it("does not show the banner when no browser is open", async () => {
    render(<CleanPanel />);
    await screen.findByLabelText("Temporary files");
    expect(screen.queryByTestId("browser-warning")).toBeNull();
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
});
