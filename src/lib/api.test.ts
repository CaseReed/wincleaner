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
  filterRules,
  sortGrouped,
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
      { id: "a", category: "System", label: "A", risk: "low", kind: "files", default_checked: true, note: null, unavailable_reason: null },
      { id: "b", category: "Browsers", label: "B", risk: "low", kind: "files", default_checked: true, note: null, unavailable_reason: null },
      { id: "c", category: "System", label: "C", risk: "medium", kind: "files", default_checked: false, note: null, unavailable_reason: null },
    ];
    const grouped = groupByCategory(rules);
    expect(grouped.map(([cat]) => cat)).toEqual(["System", "Browsers"]);
    expect(grouped[0][1].map((r) => r.id)).toEqual(["a", "c"]);
  });

  describe("filterRules", () => {
    const rules = [
      { id: "windows.temp", category: "System", label: "Temporary files", risk: "low", kind: "files", default_checked: true, note: null, unavailable_reason: null },
      { id: "winapp2.7-zip", category: "Applications", label: "7-Zip", risk: "medium", kind: "files", default_checked: false, note: null, unavailable_reason: null },
    ] as RuleSummary[];

    it("returns everything for an empty query", () => {
      expect(filterRules(rules, "   ")).toHaveLength(2);
    });

    it("matches the label, case-insensitively, across categories", () => {
      expect(filterRules(rules, "zip").map((r) => r.id)).toEqual(["winapp2.7-zip"]);
      expect(filterRules(rules, "TEMPORARY").map((r) => r.id)).toEqual(["windows.temp"]);
      expect(filterRules(rules, "nothing")).toEqual([]);
    });
  });

  describe("sortGrouped", () => {
    const grouped: [string, RuleSummary[]][] = [
      ["System", [{ id: "a" }, { id: "b" }] as RuleSummary[]],
      ["Applications", [{ id: "c" }] as RuleSummary[]],
    ];
    const results = [
      { rule_id: "a", file_count: 1, total_bytes: 10, paths: [], skipped: 0 },
      { rule_id: "b", file_count: 1, total_bytes: 300, paths: [], skipped: 0 },
      { rule_id: "c", file_count: 1, total_bytes: 500, paths: [], skipped: 0 },
    ];

    it("returns the input untouched when there is no scan yet", () => {
      expect(sortGrouped(grouped, null)).toEqual(grouped);
    });

    it("sorts rules inside a category and categories by their total, descending", () => {
      // Applications totals 500 (just "c"), System totals 310 (300 + 10):
      // Applications comes first, and within System "b" outranks "a".
      const sorted = sortGrouped(grouped, results);
      expect(sorted.map(([category]) => category)).toEqual(["Applications", "System"]);
      expect(sorted[1][1].map((r) => r.id)).toEqual(["b", "a"]);
    });

    it("treats a rule with no result as zero bytes", () => {
      const sorted = sortGrouped(grouped, [results[0]]);
      expect(sorted.map(([category]) => category)).toEqual(["System", "Applications"]);
      expect(sorted[0][1].map((r) => r.id)).toEqual(["a", "b"]);
    });
  });
});
