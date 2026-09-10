import { describe, it, expect, vi, beforeEach } from "vitest";

const invoke = vi.fn();
vi.mock("@tauri-apps/api/core", () => ({ invoke: (...args: unknown[]) => invoke(...args) }));

import {
  scan,
  clean,
  listRules,
  listStartup,
  setStartupEnabled,
  runningBrowsers,
  groupByCategory,
  type RuleSummary,
} from "./api";

describe("api", () => {
  beforeEach(() => invoke.mockReset());

  it("scan envoie ruleIds en camelCase", async () => {
    invoke.mockResolvedValue([]);
    await scan(["windows.temp"]);
    expect(invoke).toHaveBeenCalledWith("scan", { ruleIds: ["windows.temp"] });
  });

  it("clean envoie ruleIds et mode", async () => {
    invoke.mockResolvedValue({ freed_bytes: 0, deleted: 0, skipped: [] });
    await clean(["edge.cache"], "auto");
    expect(invoke).toHaveBeenCalledWith("clean", {
      ruleIds: ["edge.cache"],
      mode: "auto",
    });
  });

  it("listRules, listStartup et runningBrowsers n'ont pas d'argument", async () => {
    invoke.mockResolvedValue([]);
    await listRules();
    expect(invoke).toHaveBeenCalledWith("list_rules");
    await listStartup();
    expect(invoke).toHaveBeenCalledWith("list_startup");
    await runningBrowsers();
    expect(invoke).toHaveBeenCalledWith("running_browsers");
  });

  it("setStartupEnabled envoie id et enabled", async () => {
    invoke.mockResolvedValue(undefined);
    await setStartupEnabled("run:OneDrive", false);
    expect(invoke).toHaveBeenCalledWith("set_startup_enabled", {
      id: "run:OneDrive",
      enabled: false,
    });
  });

  it("groupByCategory conserve l'ordre d'apparition des catégories", () => {
    const rules: RuleSummary[] = [
      { id: "a", category: "Système", label: "A", risk: "low", kind: "files" },
      { id: "b", category: "Navigateurs", label: "B", risk: "low", kind: "files" },
      { id: "c", category: "Système", label: "C", risk: "medium", kind: "files" },
    ];
    const grouped = groupByCategory(rules);
    expect(grouped.map(([cat]) => cat)).toEqual(["Système", "Navigateurs"]);
    expect(grouped[0][1].map((r) => r.id)).toEqual(["a", "c"]);
  });
});
