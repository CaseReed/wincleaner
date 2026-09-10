import { invoke } from "@tauri-apps/api/core";
import { listen } from "@tauri-apps/api/event";

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

/// Mirrors `src-tauri/src/commands.rs::ScanProgress`, emitted once per rule
/// while `scan` runs. `total_bytes` is the running total measured since the
/// start of that scan, not the size of the rule that has just been measured.
export interface ScanProgress {
  done: number;
  total: number;
  rule_id: string;
  label: string;
  total_bytes: number;
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

/// Mirrors `src-tauri/src/update.rs::UpdateCheck`. `latest` is null when the
/// endpoint has no public release to offer, which is the normal answer while
/// the repository is private.
export interface UpdateCheck {
  current: string;
  latest: string | null;
  is_newer: boolean;
  notes: string | null;
  url: string | null;
  published_at: string | null;
}

/// Mirrors `src-tauri/src/sandbox.rs::SandboxSummary`. What the synthetic
/// profile holds, and where it lives: the whole engine points at `root` while
/// a sandbox is active.
export interface SandboxSummary {
  root: string;
  sentinels: number;
  junk: number;
  winapp2_rules: number;
}

/// Mirrors `src-tauri/src/sandbox.rs::SandboxVerdict`: the disk read back
/// against the manifest the sandbox promised, after a real clean.
export interface SandboxVerdict {
  sentinels_total: number;
  sentinels_intact: number;
  sentinels_damaged: string[];
  /// Junk of the rules that were cleaned, and nothing else.
  junk_total: number;
  junk_removed: number;
  junk_remaining: string[];
  /// How many rules the three counts above are scoped to.
  rules_cleaned: number;
  /// The junction baits: files a rule's glob matches, reachable only by
  /// crossing a junction the sandbox plants. There are two of them.
  outside_total: number;
  outside_intact: number;
  junctions_refused: boolean;
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

/// Subscribes to the progress of the running `scan`. Resolves with the
/// function that stops listening — call it when the subscriber goes away, or
/// the listener outlives it inside the webview. Needs `core:event:allow-listen`,
/// which `core:event:default` already grants
/// (https://v2.tauri.app/reference/acl/core-permissions/).
export function onScanProgress(
  cb: (progress: ScanProgress) => void,
): Promise<() => void> {
  return listen<ScanProgress>("scan-progress", (event) => cb(event.payload));
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

/// The only call in this application that reaches the network, and it does so
/// from Rust: one GET on the GitHub REST API (src-tauri/src/update.rs). The
/// rejection value is a stable code, not a sentence — see
/// `updateErrorMessage` in `src/lib/updates.ts`.
export function checkForUpdates(): Promise<UpdateCheck> {
  return invoke<UpdateCheck>("check_for_updates");
}

/// Builds the synthetic profile under `%TEMP%` and switches the whole engine
/// onto it: `listRules`, `scan` and `clean` then work against it and nothing
/// else. Rejects when a sandbox is already active.
export function sandboxEnter(): Promise<SandboxSummary> {
  return invoke<SandboxSummary>("sandbox_enter");
}

/// Removes the synthetic profile and hands the real catalogue back.
export function sandboxLeave(): Promise<void> {
  return invoke<void>("sandbox_leave");
}

/// The active sandbox, or null when the engine runs against the real profile.
export function sandboxStatus(): Promise<SandboxSummary | null> {
  return invoke<SandboxSummary | null>("sandbox_status");
}

/// Reads the sandbox back off the disk and compares it with what it promised.
/// `ruleIds` are the rules that were just cleaned: the junk counts are scoped
/// to them, so a partial selection is not judged against the whole catalogue.
export function sandboxVerify(ruleIds: string[]): Promise<SandboxVerdict> {
  return invoke<SandboxVerdict>("sandbox_verify", { ruleIds });
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
