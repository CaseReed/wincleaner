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

const ENTREES = [
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
    api.listStartup.mockReset().mockResolvedValue(ENTREES);
    api.setStartupEnabled.mockReset().mockResolvedValue(undefined);
  });

  it("affiche une ligne par entrée avec nom, commande et source", async () => {
    render(<StartupPanel />);
    expect(await screen.findByText("OneDrive")).toBeInTheDocument();
    const ligne = screen.getByTestId("startup-row-run:OneDrive");
    expect(ligne).toHaveTextContent("OneDrive.exe /background");
    expect(ligne).toHaveTextContent("Registre (Run)");
    expect(screen.getByTestId("startup-row-folder:Notes.lnk")).toHaveTextContent(
      "Dossier Démarrage"
    );
    expect(screen.getByTestId("startup-row-run-once:Patch")).toHaveTextContent(
      "Registre (RunOnce)"
    );
  });

  it("reflète l'état activé de chaque entrée", async () => {
    render(<StartupPanel />);
    expect(await screen.findByLabelText("Activer OneDrive")).toBeChecked();
    expect(screen.getByLabelText("Activer Notes.lnk")).not.toBeChecked();
  });

  it("désactive une entrée et rafraîchit la liste", async () => {
    const user = userEvent.setup();
    render(<StartupPanel />);
    const bascule = await screen.findByLabelText("Activer OneDrive");
    api.listStartup.mockResolvedValue([
      { ...ENTREES[0], enabled: false },
      ENTREES[1],
      ENTREES[2],
    ]);
    await user.click(bascule);
    await waitFor(() =>
      expect(api.setStartupEnabled).toHaveBeenCalledWith("run:OneDrive", false)
    );
    await waitFor(() => expect(screen.getByLabelText("Activer OneDrive")).not.toBeChecked());
  });

  it("l'interrupteur d'une entrée RunOnce est désactivé", async () => {
    render(<StartupPanel />);
    expect(await screen.findByLabelText("Activer Patch")).toHaveAttribute(
      "aria-disabled",
      "true"
    );
  });

  it("affiche un message quand la liste est vide", async () => {
    api.listStartup.mockResolvedValue([]);
    render(<StartupPanel />);
    expect(await screen.findByTestId("startup-empty")).toBeInTheDocument();
  });

  it("affiche l'erreur si la lecture échoue", async () => {
    api.listStartup.mockRejectedValue("accès au démarrage impossible : refusé");
    render(<StartupPanel />);
    expect(await screen.findByTestId("startup-error")).toHaveTextContent("refusé");
  });

  it("remet la bascule dans son état si l'écriture échoue", async () => {
    const user = userEvent.setup();
    api.setStartupEnabled.mockRejectedValue("accès refusé");
    render(<StartupPanel />);
    const bascule = await screen.findByLabelText("Activer OneDrive");
    await user.click(bascule);
    await waitFor(() => expect(screen.getByLabelText("Activer OneDrive")).toBeChecked());
  });

  it("annonce que seule la session courante est listée", async () => {
    render(<StartupPanel />);
    expect(await screen.findByTestId("startup-scope")).toHaveTextContent(
      /HKCU.*dossier Démarrage.*élévation/s
    );
  });
});
