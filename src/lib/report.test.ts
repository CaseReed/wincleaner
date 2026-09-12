import { describe, it, expect } from "vitest";
import { renderHook } from "@testing-library/react";
import { useI18n, type I18n } from "@/i18n";
import type { CleanReport, RuleSummary, ScanResult } from "@/lib/api";
import { buildReportData, buildReportJson, buildReportText } from "./report";

function i18n(): I18n {
  return renderHook(() => useI18n()).result.current;
}

const RULES: RuleSummary[] = [
  {
    id: "windows.temp",
    category: "System",
    label: "Temporary files",
    risk: "low",
    kind: "files",
    default_checked: true,
    note: null,
    unavailable_reason: null,
  },
  {
    id: "windows.recycle-bin",
    category: "System",
    label: "Recycle Bin",
    risk: "medium",
    kind: "recycle-bin",
    default_checked: false,
    note: null,
    unavailable_reason: null,
  },
];

const SCANNED: ScanResult[] = [
  { rule_id: "windows.temp", file_count: 12, total_bytes: 1_288_490_188, paths: [], skipped: 0 },
  { rule_id: "windows.recycle-bin", file_count: 40, total_bytes: 134_217_728, paths: [], skipped: 2 },
];

const CLEAN_REPORT: CleanReport = {
  freed_bytes: 1_422_707_916,
  deleted: 52,
  skipped: [{ path: String.raw`C:\Users\T\AppData\Local\Temp\lock.tmp`, reason: "file in use" }],
};

const GENERATED_AT = new Date("2026-09-12T14:32:00.000Z");

describe("buildReportData", () => {
  it("pairs scanned rules with their labels and carries the clean totals", () => {
    const data = buildReportData(RULES, SCANNED, CLEAN_REPORT, "trash", "0.7.0", GENERATED_AT);
    expect(data.rules).toEqual([
      { id: "windows.temp", label: "Temporary files", files: 12, bytes: 1_288_490_188, skipped: 0 },
      { id: "windows.recycle-bin", label: "Recycle Bin", files: 40, bytes: 134_217_728, skipped: 2 },
    ]);
    expect(data.totalFiles).toBe(52);
    expect(data.totalBytes).toBe(1_422_707_916);
    expect(data.skipped).toEqual(CLEAN_REPORT.skipped);
    expect(data.mode).toBe("trash");
    expect(data.version).toBe("0.7.0");
  });

  it("falls back to the rule id when the rule is no longer in the catalogue", () => {
    const data = buildReportData([], SCANNED, CLEAN_REPORT, "auto", "0.7.0", GENERATED_AT);
    expect(data.rules[0].label).toBe("windows.temp");
  });
});

describe("buildReportText", () => {
  it("renders a readable report with a total and the skipped paths", () => {
    const data = buildReportData(RULES, SCANNED, CLEAN_REPORT, "trash", "0.7.0", GENERATED_AT);
    const text = buildReportText(data, i18n());
    expect(text).toContain("WinCleaner 0.7.0");
    expect(text).toContain("Mode: Recycle Bin");
    expect(text).toContain("Per rule: measured by the last Analyze. Totals: actually freed.");
    expect(text).toContain("Temporary files — 12 files measured, 1.2 GB");
    expect(text).toContain("Recycle Bin — 40 files measured, 128 MB (2 skipped)");
    expect(text).toContain("Total: 52 files, 1.3 GB freed");
    expect(text).toContain("Skipped:");
    expect(text).toContain(String.raw`C:\Users\T\AppData\Local\Temp\lock.tmp — file in use`);
  });

  it("omits the skipped section when nothing was skipped", () => {
    const data = buildReportData(
      RULES,
      SCANNED,
      { ...CLEAN_REPORT, skipped: [] },
      "auto",
      "0.7.0",
      GENERATED_AT,
    );
    expect(buildReportText(data, i18n())).not.toContain("Skipped:");
  });
});

describe("buildReportJson", () => {
  it("is a stable, language-neutral, snake_case shape", () => {
    const data = buildReportData(RULES, SCANNED, CLEAN_REPORT, "permanent", "0.7.0", GENERATED_AT);
    expect(buildReportJson(data)).toEqual({
      version: "0.7.0",
      generated_at: "2026-09-12T14:32:00.000Z",
      mode: "permanent",
      note: expect.stringContaining("last Analyze"),
      rules: [
        {
          id: "windows.temp",
          label: "Temporary files",
          files_measured: 12,
          bytes_measured: 1_288_490_188,
          skipped: 0,
        },
        {
          id: "windows.recycle-bin",
          label: "Recycle Bin",
          files_measured: 40,
          bytes_measured: 134_217_728,
          skipped: 2,
        },
      ],
      totals: { files_deleted: 52, bytes_freed: 1_422_707_916 },
      skipped: [{ path: String.raw`C:\Users\T\AppData\Local\Temp\lock.tmp`, reason: "file in use" }],
    });
  });

  it("keeps files_measured/bytes_measured distinct from totals when a rule skips files", () => {
    // The rule that skipped 2 files "measured" 40 at Analyze time; the
    // aggregate `totals` (from the real CleanReport) is what Clean actually
    // freed across every rule, and the two are not required to agree.
    const data = buildReportData(RULES, SCANNED, CLEAN_REPORT, "trash", "0.7.0", GENERATED_AT);
    const json = buildReportJson(data);
    expect(json.rules[1].files_measured).toBe(40);
    expect(json.totals.files_deleted).toBe(52);
  });
});
