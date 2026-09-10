import { describe, it, expect, vi, beforeEach } from "vitest";
import { render, screen, waitFor } from "@testing-library/react";
import userEvent from "@testing-library/user-event";

const api = {
  listRules: vi.fn(),
  scan: vi.fn(),
  clean: vi.fn(),
  runningBrowsers: vi.fn(),
};

vi.mock("@/lib/api", async () => {
  const actual = await vi.importActual<typeof import("@/lib/api")>("@/lib/api");
  return {
    ...actual,
    listRules: () => api.listRules(),
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
  });

  it("shows the rules grouped by category", async () => {
    render(<CleanPanel />);
    expect(await screen.findByText("System")).toBeInTheDocument();
    expect(screen.getByText("Browsers")).toBeInTheDocument();
    expect(screen.getByLabelText("Temporary files")).toBeInTheDocument();
    expect(screen.getByLabelText("Microsoft Edge cache")).toBeInTheDocument();
  });

  it("only scans the checked rules", async () => {
    const user = userEvent.setup();
    render(<CleanPanel />);
    await screen.findByLabelText("Temporary files");
    await user.click(screen.getByLabelText("Microsoft Edge cache"));
    await user.click(screen.getByRole("button", { name: /Analyze/ }));
    await waitFor(() => expect(api.scan).toHaveBeenCalledWith(["windows.temp"]));
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

  it("does not scan the rules unchecked by default", async () => {
    const user = userEvent.setup();
    api.listRules.mockResolvedValue([...RULES, RECYCLE_BIN]);
    render(<CleanPanel />);
    await screen.findByLabelText("Recycle Bin");
    await user.click(screen.getByRole("button", { name: /Analyze/ }));
    await waitFor(() =>
      expect(api.scan).toHaveBeenCalledWith(["windows.temp", "edge.cache"])
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
});
