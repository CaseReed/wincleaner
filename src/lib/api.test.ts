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

  it("scan sends ruleIds in camelCase", async () => {
    invoke.mockResolvedValue([]);
    await scan(["windows.temp"]);
    expect(invoke).toHaveBeenCalledWith("scan", { ruleIds: ["windows.temp"] });
  });

  it("clean sends ruleIds and mode", async () => {
    invoke.mockResolvedValue({ freed_bytes: 0, deleted: 0, skipped: [] });
    await clean(["edge.cache"], "auto");
    expect(invoke).toHaveBeenCalledWith("clean", {
      ruleIds: ["edge.cache"],
      mode: "auto",
    });
  });

  it("listRules, listStartup and runningBrowsers take no argument", async () => {
    invoke.mockResolvedValue([]);
    await listRules();
    expect(invoke).toHaveBeenCalledWith("list_rules");
    await listStartup();
    expect(invoke).toHaveBeenCalledWith("list_startup");
    await runningBrowsers();
    expect(invoke).toHaveBeenCalledWith("running_browsers");
  });

  it("setStartupEnabled sends id and enabled", async () => {
    invoke.mockResolvedValue(undefined);
    await setStartupEnabled("run:OneDrive", false);
    expect(invoke).toHaveBeenCalledWith("set_startup_enabled", {
      id: "run:OneDrive",
      enabled: false,
    });
  });

  it("groupByCategory preserves the order the categories appear in", () => {
    const rules: RuleSummary[] = [
      { id: "a", category: "System", label: "A", risk: "low", kind: "files", default_checked: true, unavailable_reason: null },
      { id: "b", category: "Browsers", label: "B", risk: "low", kind: "files", default_checked: true, unavailable_reason: null },
      { id: "c", category: "System", label: "C", risk: "medium", kind: "files", default_checked: false, unavailable_reason: null },
    ];
    const grouped = groupByCategory(rules);
    expect(grouped.map(([cat]) => cat)).toEqual(["System", "Browsers"]);
    expect(grouped[0][1].map((r) => r.id)).toEqual(["a", "c"]);
  });
});
