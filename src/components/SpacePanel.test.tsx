import { describe, it, expect, vi, beforeEach } from "vitest";
import { render, screen, waitFor } from "@testing-library/react";
import userEvent from "@testing-library/user-event";

const api = {
  spaceScan: vi.fn(),
  spaceReveal: vi.fn(),
};

vi.mock("@/lib/api", async () => {
  const actual = await vi.importActual<typeof import("@/lib/api")>("@/lib/api");
  return {
    ...actual,
    spaceScan: () => api.spaceScan(),
    spaceReveal: (kind: string, index: number) => api.spaceReveal(kind, index),
    onSpaceProgress: () => Promise.resolve(() => {}),
  };
});

vi.mock("sonner", () => ({ toast: { error: vi.fn(), success: vi.fn() } }));

import { toast } from "sonner";
const toastError = vi.mocked(toast.error);

import { I18nProvider, LANGUAGE_KEY } from "@/i18n";
import { SpacePanel, baseName } from "./SpacePanel";

const SANDBOX = {
  root: String.raw`C:\Users\T\AppData\Local\Temp\wincleaner-sandbox-1a2b`,
  sentinels: 52,
  junk: 119,
  winapp2_rules: 14,
};

/// 2023-05-04, as epoch milliseconds: what Rust sends for `modified`.
const MODIFIED = Date.UTC(2023, 4, 4, 12);

const RESULT = {
  roots: [
    { name: "downloads", path: String.raw`C:\Users\T\Downloads`, bytes: 3_000_000, files: 40 },
    { name: "pictures", path: String.raw`C:\Users\T\Pictures`, bytes: 1_000_000, files: 12 },
  ],
  files: [
    {
      index: 0,
      path: String.raw`C:\Users\T\Downloads\installer.iso`,
      bytes: 2_500_000,
      modified: MODIFIED,
    },
    {
      index: 1,
      path: String.raw`C:\Users\T\Pictures\holiday.raw`,
      bytes: 900_000,
      modified: null,
    },
  ],
  folders: [
    { index: 0, path: String.raw`C:\Users\T\Downloads\archive`, bytes: 2_800_000, files: 31 },
  ],
  skipped_files: 0,
  skipped_roots: [] as string[],
};

async function measure() {
  await userEvent.click(screen.getByRole("button", { name: "Measure" }));
  await waitFor(() => expect(screen.getByTestId("space-files")).toBeInTheDocument());
}

describe("SpacePanel", () => {
  beforeEach(() => {
    localStorage.clear();
    toastError.mockReset();
    api.spaceScan.mockReset().mockResolvedValue(RESULT);
    api.spaceReveal.mockReset().mockResolvedValue(undefined);
  });

  it("measures nothing until asked", () => {
    render(<SpacePanel />);
    expect(screen.getByText("Measure to see where your space has gone.")).toBeInTheDocument();
    expect(api.spaceScan).not.toHaveBeenCalled();
  });

  it("shows the roots, the largest files and the largest folders", async () => {
    render(<SpacePanel />);
    await measure();

    // Header card: the total across the roots, then one bar per root.
    expect(screen.getByTestId("space-total")).toHaveTextContent("3.8 MB");
    expect(screen.getByTestId("space-root-downloads")).toHaveTextContent("Downloads");
    expect(screen.getByTestId("space-root-pictures")).toHaveTextContent("Pictures");

    const file = screen.getByTestId("space-file-0");
    expect(file).toHaveTextContent("installer.iso");
    expect(file).toHaveTextContent("2.4 MB");
    expect(file).toHaveTextContent("May 4, 2023");
    // No date is an em dash, not "Invalid Date".
    expect(screen.getByTestId("space-file-1")).toHaveTextContent("—");

    const folder = screen.getByTestId("space-folder-0");
    expect(folder).toHaveTextContent("archive");
    expect(folder).toHaveTextContent("31 files");
  });

  /// The row's index is the whole payload: no path ever travels back to Rust.
  it("reveals a row by its index, never by its path", async () => {
    render(<SpacePanel />);
    await measure();

    await userEvent.click(
      screen.getByRole("button", { name: "Reveal holiday.raw in Explorer" }),
    );
    expect(api.spaceReveal).toHaveBeenCalledWith("file", 1);

    await userEvent.click(screen.getByRole("button", { name: "Reveal archive in Explorer" }));
    expect(api.spaceReveal).toHaveBeenCalledWith("folder", 0);
  });

  it("reports a reveal the back end refused", async () => {
    api.spaceReveal.mockRejectedValue("\"C:\\Users\\T\\Downloads\\installer.iso\" no longer exists");
    render(<SpacePanel />);
    await measure();

    await userEvent.click(
      screen.getByRole("button", { name: "Reveal installer.iso in Explorer" }),
    );
    await waitFor(() => expect(toastError).toHaveBeenCalled());
    expect(String(toastError.mock.calls[0][0])).toContain("no longer exists");
  });

  it("names the roots it could not measure", async () => {
    api.spaceScan.mockResolvedValue({ ...RESULT, skipped_roots: ["videos"] });
    render(<SpacePanel />);
    await measure();
    expect(screen.getByTestId("space-skipped-roots")).toHaveTextContent("Videos");
  });

  it("says so when the measurement fails", async () => {
    api.spaceScan.mockRejectedValue("the profile could not be resolved");
    render(<SpacePanel />);
    await userEvent.click(screen.getByRole("button", { name: "Measure" }));
    await waitFor(() => expect(screen.getByTestId("space-error")).toBeInTheDocument());
    expect(screen.getByTestId("space-error")).toHaveTextContent("the profile could not be resolved");
  });

  /// The known folders are the user's real ones: there is nothing for the
  /// sandbox to stand in for, and the screen says so rather than offering a
  /// button that would be refused.
  it("refuses to measure while a sandbox is active", () => {
    render(<SpacePanel sandbox={SANDBOX} />);
    expect(screen.getByTestId("space-sandbox-notice")).toHaveTextContent(
      "Unavailable while the sandbox is active.",
    );
    expect(screen.queryByRole("button", { name: "Measure" })).not.toBeInTheDocument();
    expect(api.spaceScan).not.toHaveBeenCalled();
  });

  it("reads in French, dates and byte units included", async () => {
    localStorage.setItem(LANGUAGE_KEY, "fr");
    render(
      <I18nProvider>
        <SpacePanel />
      </I18nProvider>,
    );
    await userEvent.click(screen.getByRole("button", { name: "Mesurer" }));
    await waitFor(() => expect(screen.getByTestId("space-files")).toBeInTheDocument());

    expect(
      screen.getByRole("heading", { name: "Fichiers les plus volumineux" }),
    ).toBeInTheDocument();
    expect(
      screen.getByRole("heading", { name: "Dossiers les plus volumineux" }),
    ).toBeInTheDocument();
    expect(screen.getByTestId("space-root-downloads")).toHaveTextContent("Téléchargements");
    expect(screen.getByTestId("space-file-0")).toHaveTextContent("4 mai 2023");
    expect(screen.getByTestId("space-total")).toHaveTextContent("Mo");
    expect(
      screen.getByRole("button", { name: "Afficher installer.iso dans l’Explorateur" }),
    ).toBeInTheDocument();
  });
});

describe("baseName", () => {
  it("keeps the last segment, trailing separator or not", () => {
    expect(baseName(String.raw`C:\Users\T\Downloads\a.iso`)).toBe("a.iso");
    expect(baseName("C:\\Users\\T\\Downloads\\")).toBe("Downloads");
    expect(baseName("a.iso")).toBe("a.iso");
  });
});
