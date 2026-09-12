import type { I18n, TranslationKey } from "@/i18n";
import type { CleanMode, CleanReport, RuleSummary, ScanResult, SkippedItem } from "@/lib/api";

/// Shared with `CleanPanel`'s deletion-mode `<select>`: both need the same
/// label for the mode a report is about.
export const MODE_LABEL: Record<CleanMode, TranslationKey> = {
  auto: "mode.auto",
  trash: "mode.trash",
  permanent: "mode.permanent",
};

export interface ReportRuleLine {
  id: string;
  label: string;
  files: number;
  bytes: number;
  skipped: number;
}

/// What the "Copy report" / "Copy as JSON" buttons both build from. `Clean`
/// only ever returns aggregate totals (`CleanReport`, see
/// `src-tauri/src/clean.rs`) — never a path, per the "no Tauri command takes
/// a path" invariant — so the per-rule breakdown here is the last Analyze
/// measurement for the rules Clean was asked to act on: exactly the numbers
/// the confirmation and the hero total already showed before the click.
export interface ReportData {
  version: string;
  generatedAt: Date;
  mode: CleanMode;
  rules: ReportRuleLine[];
  totalFiles: number;
  totalBytes: number;
  skipped: SkippedItem[];
}

/// Pairs the rules a clean was scoped to with what Analyze last measured for
/// them, and the mode and totals the clean itself reported. Pure: no
/// clipboard, no formatting, no i18n — `buildReportText`/`buildReportJson`
/// own that.
export function buildReportData(
  rules: RuleSummary[],
  scanned: ScanResult[],
  cleanReport: CleanReport,
  mode: CleanMode,
  version: string,
  generatedAt: Date,
): ReportData {
  const labels = new Map(rules.map((r) => [r.id, r.label]));
  return {
    version,
    generatedAt,
    mode,
    rules: scanned.map((r) => ({
      id: r.rule_id,
      label: labels.get(r.rule_id) ?? r.rule_id,
      files: r.file_count,
      bytes: r.total_bytes,
      skipped: r.skipped,
    })),
    totalFiles: cleanReport.deleted,
    totalBytes: cleanReport.freed_bytes,
    skipped: cleanReport.skipped,
  };
}

/// The readable report, in the current UI language. Rule ids are data
/// (`rules.toml`/Winapp2) and stay untranslated, same as everywhere else the
/// application shows a rule label.
export function buildReportText(data: ReportData, i18n: I18n): string {
  const { t, formatBytes, formatCount, locale } = i18n;
  const date = new Intl.DateTimeFormat(locale, {
    dateStyle: "medium",
    timeStyle: "short",
  }).format(data.generatedAt);

  const lines: string[] = [
    t("report.text.header", { version: data.version, date }),
    t("report.text.mode", { mode: t(MODE_LABEL[data.mode]) }),
    "",
  ];

  for (const rule of data.rules) {
    const key: TranslationKey =
      rule.skipped > 0 ? "report.text.ruleSkipped" : "report.text.rule";
    lines.push(
      t(key, {
        label: rule.label,
        files: formatCount(rule.files),
        bytes: formatBytes(rule.bytes),
        skipped: formatCount(rule.skipped),
      }),
    );
  }

  lines.push(
    "",
    t("report.text.total", {
      files: formatCount(data.totalFiles),
      bytes: formatBytes(data.totalBytes),
    }),
  );

  if (data.skipped.length > 0) {
    lines.push("", t("report.text.skippedHeader"));
    for (const s of data.skipped) {
      lines.push(`${s.path} — ${s.reason}`);
    }
  }

  return lines.join("\n");
}

/// The `version`/`generated_at`/`mode`/`rules`/`totals`/`skipped` shape a
/// script parses. Language-neutral: rule ids and labels are the same data
/// `buildReportText` uses, never routed through the dictionary.
export interface ReportJson {
  version: string;
  generated_at: string;
  mode: CleanMode;
  rules: {
    id: string;
    label: string;
    files_deleted: number;
    bytes_freed: number;
    skipped: number;
  }[];
  totals: { files_deleted: number; bytes_freed: number };
  skipped: { path: string; reason: string }[];
}

export function buildReportJson(data: ReportData): ReportJson {
  return {
    version: data.version,
    generated_at: data.generatedAt.toISOString(),
    mode: data.mode,
    rules: data.rules.map((r) => ({
      id: r.id,
      label: r.label,
      files_deleted: r.files,
      bytes_freed: r.bytes,
      skipped: r.skipped,
    })),
    totals: { files_deleted: data.totalFiles, bytes_freed: data.totalBytes },
    skipped: data.skipped.map((s) => ({ path: s.path, reason: s.reason })),
  };
}
