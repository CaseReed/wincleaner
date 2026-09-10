import { describe, it, expect, vi, beforeEach } from "vitest";
import { render, screen, waitFor } from "@testing-library/react";
import userEvent from "@testing-library/user-event";

const api = {
  listStartup: vi.fn(),
  setStartupEnabled: vi.fn(),
};

vi.mock("@/lib/api", async () => {
  const actual = await vi.importActual<typeof import("@/lib/api")>("@/lib/api");
  return {
    ...actual,
    listStartup: () => api.listStartup(),
    setStartupEnabled: (id: string, enabled: boolean) => api.setStartupEnabled(id, enabled),
  };
});

vi.mock("sonner", () => ({ toast: { error: vi.fn(), success: vi.fn() } }));

import { StartupPanel } from "./StartupPanel";

const ENTRIES = [
  {
    id: "run:OneDrive",
    name: "OneDrive",
    command: String.raw`C:\Program Files\OneDrive\OneDrive.exe /background`,
    source: "run",
    enabled: true,
  },
  {
    id: "folder:Notes.lnk",
    name: "Notes.lnk",
    command: String.raw`C:\Users\T\AppData\Roaming\...\Startup\Notes.lnk`,
    source: "folder",
    enabled: false,
  },
  {
    id: "run-once:Patch",
    name: "Patch",
    command: String.raw`C:\Temp\patch.exe`,
    source: "run-once",
    enabled: true,
  },
];

describe("StartupPanel", () => {
  beforeEach(() => {
    api.listStartup.mockReset().mockResolvedValue(ENTRIES);
    api.setStartupEnabled.mockReset().mockResolvedValue(undefined);
  });

  it("shows one row per entry with name, command and source", async () => {
    render(<StartupPanel />);
    expect(await screen.findByText("OneDrive")).toBeInTheDocument();
    const row = screen.getByTestId("startup-row-run:OneDrive");
    expect(row).toHaveTextContent("OneDrive.exe /background");
    expect(row).toHaveTextContent("Registry (Run)");
    expect(screen.getByTestId("startup-row-folder:Notes.lnk")).toHaveTextContent(
      "Startup folder"
    );
    expect(screen.getByTestId("startup-row-run-once:Patch")).toHaveTextContent(
      "Registry (RunOnce)"
    );
  });

  it("reflects the enabled state of each entry", async () => {
    render(<StartupPanel />);
    expect(await screen.findByLabelText("Enable OneDrive")).toBeChecked();
    expect(screen.getByLabelText("Enable Notes.lnk")).not.toBeChecked();
  });

  it("disables an entry and refreshes the list", async () => {
    const user = userEvent.setup();
    render(<StartupPanel />);
    const toggle = await screen.findByLabelText("Enable OneDrive");
    api.listStartup.mockResolvedValue([
      { ...ENTRIES[0], enabled: false },
      ENTRIES[1],
      ENTRIES[2],
    ]);
    await user.click(toggle);
    await waitFor(() =>
      expect(api.setStartupEnabled).toHaveBeenCalledWith("run:OneDrive", false)
    );
    await waitFor(() => expect(screen.getByLabelText("Enable OneDrive")).not.toBeChecked());
  });

  it("the switch of a RunOnce entry is disabled", async () => {
    render(<StartupPanel />);
    expect(await screen.findByLabelText("Enable Patch")).toHaveAttribute(
      "aria-disabled",
      "true"
    );
  });

  it("shows a message when the list is empty", async () => {
    api.listStartup.mockResolvedValue([]);
    render(<StartupPanel />);
    expect(await screen.findByTestId("startup-empty")).toBeInTheDocument();
  });

  it("shows the error when reading fails", async () => {
    api.listStartup.mockRejectedValue("cannot access startup entries: denied");
    render(<StartupPanel />);
    expect(await screen.findByTestId("startup-error")).toHaveTextContent("denied");
  });

  it("puts the switch back when the write fails", async () => {
    const user = userEvent.setup();
    api.setStartupEnabled.mockRejectedValue("access denied");
    render(<StartupPanel />);
    const toggle = await screen.findByLabelText("Enable OneDrive");
    await user.click(toggle);
    await waitFor(() => expect(screen.getByLabelText("Enable OneDrive")).toBeChecked());
  });

  it("announces that only the current session is listed", async () => {
    render(<StartupPanel />);
    expect(await screen.findByTestId("startup-scope")).toHaveTextContent(
      /HKCU.*Startup folder.*elevation/s
    );
  });
});
