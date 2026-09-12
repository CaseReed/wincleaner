import type { I18n, TranslationKey } from "@/i18n";
import type { CleanMode, CleanReport, RuleSummary, ScanResult, SkippedItem } from "@/lib/api";
import { ruleLabel } from "@/lib/rule-i18n";

/// Shared with `CleanPanel`'s deletion-mode `<select>`: both need the same
/// label for the mode a report is about.
export const MODE_LABEL: Record<CleanMode, TranslationKey> = {
  auto: "mode.auto",
  trash: "mode.trash",
  permanent: "mode.permanent",
};

/// `files`/`bytes` are what the last Analyze *measured* for this rule, not
/// what Clean actually freed: a rule with a skipped file (in use, access
/// denied…) frees less than it measured, and `CleanReport` never breaks its
/// totals down by rule to correct for that. Callers must say "measured", not
/// "freed", next to these two fields.
export interface ReportRuleLine {
  id: string;
  label: string;
  /// French label of the rule, for `buildReportText` to pick through
  /// `ruleLabel` (see CLAUDE.md: never `rule.label`/`label_fr` picked
  /// anywhere else). `buildReportJson` never reads it: the JSON stays
  /// English and language-neutral.
  label_fr: string | null;
  files: number;
  bytes: number;
  skipped: number;
}

/// What the "Copy report" / "Copy as JSON" buttons both build from. `Clean`
/// only ever returns aggregate totals (`CleanReport`, see
/// `src-tauri/src/clean.rs`) — never a path, per the "no Tauri command takes
/// a path" invariant — so `rules[].files`/`.bytes` below are the last Analyze
/// measurement for the rules Clean was asked to act on (what the confirmation
/// and the hero total already showed before the click), while `totalFiles`/
/// `totalBytes` are what Clean actually reported freeing. The two can differ
/// whenever a rule skips a file.
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
  const labels = new Map(rules.map((r) => [r.id, { label: r.label, label_fr: r.label_fr }]));
  return {
    version,
    generatedAt,
    mode,
    rules: scanned.map((r) => {
      const info = labels.get(r.rule_id);
      return {
        id: r.rule_id,
        label: info?.label ?? r.rule_id,
        label_fr: info?.label_fr ?? null,
        files: r.file_count,
        bytes: r.total_bytes,
        skipped: r.skipped,
      };
    }),
    totalFiles: cleanReport.deleted,
    totalBytes: cleanReport.freed_bytes,
    skipped: cleanReport.skipped,
  };
}

/// The readable report, in the current UI language. Rule ids are data
/// (`rules.toml`/Winapp2) and stay untranslated, same as everywhere else the
/// application shows a rule label.
export function buildReportText(data: ReportData, i18n: I18n): string {
  const { t, formatBytes, formatCount, locale, language } = i18n;
  const date = new Intl.DateTimeFormat(locale, {
    dateStyle: "medium",
    timeStyle: "short",
  }).format(data.generatedAt);

  const lines: string[] = [
    t("report.text.header", { version: data.version, date }),
    t("report.text.mode", { mode: t(MODE_LABEL[data.mode]) }),
    t("report.text.note"),
    "",
  ];

  for (const rule of data.rules) {
    const key: TranslationKey =
      rule.skipped > 0 ? "report.text.ruleSkipped" : "report.text.rule";
    lines.push(
      t(key, {
        label: ruleLabel(rule, language),
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

/// The `version`/`generated_at`/`mode`/`note`/`rules`/`totals`/`skipped`
/// shape a script parses. Language-neutral: rule ids and labels are the same
/// data `buildReportText` uses, never routed through the dictionary. Mirrors
/// the text report's distinction: `rules[].*_measured` is the last Analyze
/// measurement, `totals.*` is what Clean actually freed — `note` spells that
/// out for a reader who only has the JSON.
export interface ReportJson {
  version: string;
  generated_at: string;
  mode: CleanMode;
  note: string;
  rules: {
    id: string;
    label: string;
    files_measured: number;
    bytes_measured: number;
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
    note:
      "rules[].files_measured/bytes_measured come from the last Analyze; totals.files_deleted/bytes_freed are what Clean actually freed.",
    rules: data.rules.map((r) => ({
      id: r.id,
      label: r.label,
      files_measured: r.files,
      bytes_measured: r.bytes,
      skipped: r.skipped,
    })),
    totals: { files_deleted: data.totalFiles, bytes_freed: data.totalBytes },
    skipped: data.skipped.map((s) => ({ path: s.path, reason: s.reason })),
  };
}
