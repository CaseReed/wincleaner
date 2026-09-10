import { invoke } from "@tauri-apps/api/core";

export type Risk = "low" | "medium";
export type RuleKind = "files" | "recycle-bin";
export type CleanMode = "trash" | "permanent" | "auto";
export type StartupSource = "run" | "run-once" | "folder";

export interface RuleSummary {
  id: string;
  category: string;
  label: string;
  risk: Risk;
  kind: RuleKind;
  /// Checkbox ticked on first launch. False for anything we do not clean
  /// without having explicitly asked for it (see src-tauri/rules.toml).
  default_checked: boolean;
  /// Inline warning carried by the rule (Winapp2 `Warning=`). Shown under the
  /// row.
  note: string | null;
  /// Set when the rule does not apply on this machine (variable missing, or
  /// pointing outside the profile). The row is greyed out and inert.
  unavailable_reason: string | null;
}

export interface RulesSummary {
  native: number;
  winapp2_retained: number;
  winapp2_detected: number;
  winapp2_dropped: number;
}

export interface ScanResult {
  rule_id: string;
  file_count: number;
  total_bytes: number;
  paths: string[];
  skipped: number;
}

export interface SkippedItem {
  path: string;
  reason: string;
}

export interface CleanReport {
  freed_bytes: number;
  deleted: number;
  skipped: SkippedItem[];
}

export interface StartupEntry {
  id: string;
  name: string;
  command: string;
  source: StartupSource;
  enabled: boolean;
}

export function listRules(): Promise<RuleSummary[]> {
  return invoke<RuleSummary[]>("list_rules");
}

export function scan(ruleIds: string[]): Promise<ScanResult[]> {
  return invoke<ScanResult[]>("scan", { ruleIds });
}

export function clean(ruleIds: string[], mode: CleanMode): Promise<CleanReport> {
  return invoke<CleanReport>("clean", { ruleIds, mode });
}

export function runningBrowsers(): Promise<string[]> {
  return invoke<string[]>("running_browsers");
}

export function rulesSummary(): Promise<RulesSummary> {
  return invoke<RulesSummary>("rules_summary");
}

export function listStartup(): Promise<StartupEntry[]> {
  return invoke<StartupEntry[]>("list_startup");
}

export function setStartupEnabled(id: string, enabled: boolean): Promise<void> {
  return invoke<void>("set_startup_enabled", { id, enabled });
}

/// Groups rules by category, preserving the order of rules.toml.
export function groupByCategory(rules: RuleSummary[]): [string, RuleSummary[]][] {
  const order: string[] = [];
  const buckets = new Map<string, RuleSummary[]>();
  for (const rule of rules) {
    if (!buckets.has(rule.category)) {
      buckets.set(rule.category, []);
      order.push(rule.category);
    }
    buckets.get(rule.category)!.push(rule);
  }
  return order.map((category) => [category, buckets.get(category)!]);
}

/// Filters on the label, across every category. The rule id is deliberately
/// not searched: `winapp2.7-zip` is an implementation detail.
export function filterRules(rules: RuleSummary[], query: string): RuleSummary[] {
  const needle = query.trim().toLowerCase();
  if (!needle) return rules;
  return rules.filter((rule) => rule.label.toLowerCase().includes(needle));
}

/// Biggest wins first: rules by reclaimable size inside each category, then
/// categories by their total. A rule with no scan result counts as zero.
/// Copies before sorting: `groupByCategory` returns the arrays the caller
/// still renders in rules.toml order when the toggle is off.
export function sortGrouped(
  grouped: [string, RuleSummary[]][],
  results: ScanResult[] | null,
): [string, RuleSummary[]][] {
  if (!results) return grouped;
  const bytes = new Map(results.map((r) => [r.rule_id, r.total_bytes]));
  const size = (rule: RuleSummary) => bytes.get(rule.id) ?? 0;
  const total = (rules: RuleSummary[]) => rules.reduce((sum, r) => sum + size(r), 0);
  return grouped
    .map(
      ([category, rules]) =>
        [category, [...rules].sort((a, b) => size(b) - size(a))] as [string, RuleSummary[]],
    )
    .sort((a, b) => total(b[1]) - total(a[1]));
}
