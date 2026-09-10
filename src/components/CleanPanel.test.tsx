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

const REGLES = [
  { id: "windows.temp", category: "Système", label: "Fichiers temporaires", risk: "low", kind: "files", default_checked: true },
  { id: "edge.cache", category: "Navigateurs", label: "Cache Microsoft Edge", risk: "low", kind: "files", default_checked: true },
];

const CORBEILLE = {
  id: "windows.recycle-bin",
  category: "Système",
  label: "Corbeille",
  risk: "low",
  kind: "recycle-bin",
  default_checked: false,
};

describe("CleanPanel", () => {
  beforeEach(() => {
    api.listRules.mockReset().mockResolvedValue(REGLES);
    api.scan.mockReset().mockResolvedValue([]);
    api.clean.mockReset().mockResolvedValue({ freed_bytes: 0, deleted: 0, skipped: [] });
    api.runningBrowsers.mockReset().mockResolvedValue([]);
  });

  it("affiche les règles groupées par catégorie", async () => {
    render(<CleanPanel />);
    expect(await screen.findByText("Système")).toBeInTheDocument();
    expect(screen.getByText("Navigateurs")).toBeInTheDocument();
    expect(screen.getByLabelText("Fichiers temporaires")).toBeInTheDocument();
    expect(screen.getByLabelText("Cache Microsoft Edge")).toBeInTheDocument();
  });

  it("n'analyse que les règles cochées", async () => {
    const user = userEvent.setup();
    render(<CleanPanel />);
    await screen.findByLabelText("Fichiers temporaires");
    await user.click(screen.getByLabelText("Cache Microsoft Edge"));
    await user.click(screen.getByRole("button", { name: /Analyser/ }));
    await waitFor(() => expect(api.scan).toHaveBeenCalledWith(["windows.temp"]));
  });

  it("affiche la taille par règle, le total et les chemins dépliables", async () => {
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
    await screen.findByLabelText("Fichiers temporaires");
    await user.click(screen.getByRole("button", { name: /Analyser/ }));

    expect(await screen.findByTestId("total-bytes")).toHaveTextContent("3 Ko");
    expect(screen.getByTestId("result-windows.temp")).toHaveTextContent("2 Ko");
    expect(screen.getByTestId("result-windows.temp")).toHaveTextContent("2 fichiers");
    expect(screen.getByTestId("result-edge.cache")).toHaveTextContent("3 ignorés");

    expect(screen.queryByText(String.raw`C:\Users\T\AppData\Local\Temp\a.txt`)).toBeNull();
    await user.click(screen.getByTestId("toggle-paths-windows.temp"));
    expect(
      await screen.findByText(String.raw`C:\Users\T\AppData\Local\Temp\a.txt`)
    ).toBeInTheDocument();
  });

  it("le bouton Nettoyer est désactivé avant toute analyse", async () => {
    render(<CleanPanel />);
    await screen.findByLabelText("Fichiers temporaires");
    expect(screen.getByRole("button", { name: /^Nettoyer/ })).toBeDisabled();
  });

  it("nettoie avec le mode choisi et affiche le rapport", async () => {
    const user = userEvent.setup();
    api.scan.mockResolvedValue([
      { rule_id: "windows.temp", file_count: 2, total_bytes: 2048, paths: [], skipped: 0 },
      { rule_id: "edge.cache", file_count: 0, total_bytes: 0, paths: [], skipped: 0 },
    ]);
    api.clean.mockResolvedValue({
      freed_bytes: 2048,
      deleted: 2,
      skipped: [{ path: String.raw`C:\Users\T\AppData\Local\Temp\lock.tmp`, reason: "fichier utilisé" }],
    });
    render(<CleanPanel />);
    await screen.findByLabelText("Fichiers temporaires");
    await user.click(screen.getByRole("button", { name: /Analyser/ }));
    await screen.findByTestId("total-bytes");

    await user.selectOptions(screen.getByLabelText("Mode de suppression"), "permanent");
    await user.click(screen.getByRole("button", { name: /^Nettoyer/ }));
    await user.click(await screen.findByRole("button", { name: /Confirmer le nettoyage/ }));

    await waitFor(() =>
      expect(api.clean).toHaveBeenCalledWith(["windows.temp", "edge.cache"], "permanent")
    );
    const rapport = await screen.findByTestId("clean-report");
    expect(rapport).toHaveTextContent("2 Ko");
    expect(rapport).toHaveTextContent("2");
    expect(rapport).toHaveTextContent("fichier utilisé");
  });

  it("le mode par défaut est auto", async () => {
    render(<CleanPanel />);
    await screen.findByLabelText("Fichiers temporaires");
    expect(screen.getByLabelText("Mode de suppression")).toHaveValue("auto");
  });

  it("affiche le bandeau si un navigateur ciblé est ouvert", async () => {
    api.runningBrowsers.mockResolvedValue(["msedge.exe"]);
    render(<CleanPanel />);
    expect(await screen.findByTestId("browser-warning")).toHaveTextContent("msedge.exe");
  });

  it("n'affiche pas le bandeau si aucun navigateur n'est ouvert", async () => {
    render(<CleanPanel />);
    await screen.findByLabelText("Fichiers temporaires");
    expect(screen.queryByTestId("browser-warning")).toBeNull();
  });

  it("affiche l'erreur de chargement des règles", async () => {
    api.listRules.mockRejectedValue("rules.toml est invalide : mauvais risk");
    render(<CleanPanel />);
    expect(await screen.findByTestId("rules-error")).toHaveTextContent("rules.toml est invalide");
  });

  it("le bouton Réessayer relance le chargement des règles", async () => {
    const user = userEvent.setup();
    api.listRules.mockRejectedValueOnce("rules.toml est invalide : mauvais risk");
    render(<CleanPanel />);
    await screen.findByTestId("rules-error");

    api.listRules.mockResolvedValue(REGLES);
    await user.click(screen.getByRole("button", { name: /Réessayer/ }));

    expect(await screen.findByLabelText("Fichiers temporaires")).toBeInTheDocument();
    expect(screen.queryByTestId("rules-error")).toBeNull();
  });

  it("explique le mode auto sous le sélecteur", async () => {
    render(<CleanPanel />);
    await screen.findByLabelText("Fichiers temporaires");
    expect(screen.getByTestId("mode-help")).toHaveTextContent(
      /Auto.*définitive.*faible risque.*corbeille/s
    );
  });

  it("signale que la règle Corbeille agit sur tous les volumes", async () => {
    api.listRules.mockResolvedValue([...REGLES, CORBEILLE]);
    render(<CleanPanel />);
    expect(await screen.findByTestId("note-windows.recycle-bin")).toHaveTextContent(
      "tous les volumes"
    );
    expect(screen.getByText(/Vide la corbeille de tous les volumes/)).toBeInTheDocument();
    // Les règles « files » ne portent pas cette note.
    expect(screen.queryByTestId("note-windows.temp")).toBeNull();
  });

  it("ne coche pas la règle Corbeille par défaut", async () => {
    api.listRules.mockResolvedValue([...REGLES, CORBEILLE]);
    render(<CleanPanel />);
    expect(await screen.findByLabelText("Corbeille")).not.toBeChecked();
    expect(screen.getByLabelText("Fichiers temporaires")).toBeChecked();
  });

  it("n'analyse pas les règles décochées par défaut", async () => {
    const user = userEvent.setup();
    api.listRules.mockResolvedValue([...REGLES, CORBEILLE]);
    render(<CleanPanel />);
    await screen.findByLabelText("Corbeille");
    await user.click(screen.getByRole("button", { name: /Analyser/ }));
    await waitFor(() =>
      expect(api.scan).toHaveBeenCalledWith(["windows.temp", "edge.cache"])
    );
  });

  it("annonce que la Corbeille est vidée en premier quand elle est cochée", async () => {
    const user = userEvent.setup();
    api.listRules.mockResolvedValue([...REGLES, CORBEILLE]);
    render(<CleanPanel />);
    await screen.findByLabelText("Corbeille");
    expect(screen.queryByTestId("recycle-order-note")).toBeNull();
    await user.click(screen.getByLabelText("Corbeille"));
    expect(screen.getByTestId("recycle-order-note")).toHaveTextContent(/vidée en premier/);
  });

  it("demande confirmation avant de nettoyer", async () => {
    const user = userEvent.setup();
    api.scan.mockResolvedValue([
      { rule_id: "windows.temp", file_count: 2, total_bytes: 2048, paths: [], skipped: 0 },
    ]);
    render(<CleanPanel />);
    await screen.findByLabelText("Fichiers temporaires");
    await user.click(screen.getByRole("button", { name: /Analyser/ }));
    await screen.findByTestId("total-bytes");
    await user.click(screen.getByRole("button", { name: /^Nettoyer/ }));

    // Rien n'est parti tant que la confirmation n'est pas donnée.
    expect(api.clean).not.toHaveBeenCalled();
    const confirmation = await screen.findByTestId("confirm-clean");
    expect(confirmation).toHaveTextContent("2 Ko");
    expect(confirmation).toHaveTextContent(/Auto/);
    expect(confirmation).toHaveTextContent(/Fichiers temporaires/);

    await user.click(screen.getByRole("button", { name: /Confirmer le nettoyage/ }));
    await waitFor(() => expect(api.clean).toHaveBeenCalled());
  });

  it("annuler la confirmation ne nettoie rien", async () => {
    const user = userEvent.setup();
    api.scan.mockResolvedValue([
      { rule_id: "windows.temp", file_count: 2, total_bytes: 2048, paths: [], skipped: 0 },
    ]);
    render(<CleanPanel />);
    await screen.findByLabelText("Fichiers temporaires");
    await user.click(screen.getByRole("button", { name: /Analyser/ }));
    await screen.findByTestId("total-bytes");
    await user.click(screen.getByRole("button", { name: /^Nettoyer/ }));
    await user.click(await screen.findByRole("button", { name: /Annuler/ }));

    expect(api.clean).not.toHaveBeenCalled();
    expect(screen.queryByTestId("confirm-clean")).toBeNull();
    expect(screen.getByRole("button", { name: /^Nettoyer/ })).toBeInTheDocument();
  });

  it("la confirmation énumère la corbeille comme irréversible même en mode Corbeille", async () => {
    const user = userEvent.setup();
    api.listRules.mockResolvedValue([...REGLES, CORBEILLE]);
    api.scan.mockResolvedValue([
      { rule_id: "windows.temp", file_count: 2, total_bytes: 2048, paths: [], skipped: 0 },
      { rule_id: "windows.recycle-bin", file_count: 1, total_bytes: 10, paths: [], skipped: 0 },
    ]);
    render(<CleanPanel />);
    await user.click(await screen.findByLabelText("Corbeille"));
    await user.selectOptions(screen.getByLabelText("Mode de suppression"), "trash");
    await user.click(screen.getByRole("button", { name: /Analyser/ }));
    await screen.findByTestId("total-bytes");
    await user.click(screen.getByRole("button", { name: /^Nettoyer/ }));

    const confirmation = await screen.findByTestId("confirm-clean");
    expect(confirmation).toHaveTextContent(/sans retour possible/i);
    expect(confirmation).toHaveTextContent("Corbeille");
    // En mode Corbeille, les fichiers temporaires sont récupérables.
    expect(confirmation).not.toHaveTextContent("Fichiers temporaires");
  });
});
