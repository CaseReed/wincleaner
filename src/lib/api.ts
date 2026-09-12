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
  /// French translation of `label`, null for a Winapp2 rule (community rules
  /// stay English) or a native rule that has none. See `ruleLabel` in
  /// `src/lib/rule-i18n.ts`.
  label_fr: string | null;
  /// French translation of `note`, on the same terms as `label_fr`.
  description_fr: string | null;
  /// French translation of `category`, on the same terms as `label_fr`.
  category_fr: string | null;
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
  /// True when the figures were reused from the Recycle Bin cache instead of
  /// measured (`src-tauri/src/recycle_cache.rs`). Only the Recycle Bin rule
  /// can ever set it. Optional: an older back end simply never sends it.
  cached?: boolean;
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
  /// The rules still being measured when the event went out, in catalogue
  /// order. Without it the counter names the rule that has just *finished*,
  /// which on a full Recycle Bin left the bar reading the wrong rule for four
  /// minutes. Optional so a dropped field never breaks the hero.
  running?: RunningRule[];
}

/// Mirrors `src-tauri/src/commands.rs::RunningRule`. Carries the id next to
/// the label for the same reason `ScanProgress` does: Rust never localises, so
/// the French label is looked up here, by id.
export interface RunningRule {
  rule_id: string;
  label: string;
}

/// Mirrors `src-tauri/src/commands.rs::CleanProgress`, emitted while `clean`
/// runs: once as each rule closes, and every 500 files inside a rule — a
/// Recycle Bin holding tens of thousands of files is minutes of work on its
/// own. `files_deleted` and `bytes_freed` are the running totals since the
/// start of that clean, not the counts of the rule named by the event.
export interface CleanProgress {
  done_rules: number;
  total_rules: number;
  rule_id: string;
  label: string;
  files_deleted: number;
  bytes_freed: number;
}

export interface SkippedItem {
  path: string;
  /// The raw message from the shell or from `std::io`, kept verbatim for the
  /// JSON report and for the tooltip. The screen shows `code` translated.
  reason: string;
  /// One of `src-tauri/src/clean.rs`'s `SKIP_*` codes: `in-use`,
  /// `access-denied`, `not-found`, `other`. Optional: an unknown or missing
  /// code falls back to `other`.
  code?: string;
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

/// Mirrors `src-tauri/src/sandbox.rs::Orphan`: a sandbox directory left in
/// `%TEMP%` by a process that is no longer running. The size never counts
/// anything behind a junction, because removing it never crosses one either.
export interface SandboxOrphan {
  path: string;
  size_bytes: number;
  files: number;
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

/// Subscribes to the progress of the running `clean`. Same contract as
/// `onScanProgress`: resolves with the function that stops listening.
export function onCleanProgress(
  cb: (progress: CleanProgress) => void,
): Promise<() => void> {
  return listen<CleanProgress>("clean-progress", (event) => cb(event.payload));
}

/// Mirrors `src-tauri/src/commands.rs::RunningBrowser`. `processes` is how
/// many instances of that browser's process are running; `has_window` says
/// whether any of them still owns a visible window — Chrome (and Edge) keep
/// several background processes alive after every window is closed
/// ("Continue running background apps"), which have no window at all.
export interface RunningBrowser {
  process: string;
  name: string;
  processes: number;
  has_window: boolean;
}

export function runningBrowsers(): Promise<RunningBrowser[]> {
  return invoke<RunningBrowser[]>("running_browsers");
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

/// Sandbox directories a previous run left behind in `%TEMP%` — a crash, a
/// kill, a close the back end could not finish in time. Never includes the
/// active sandbox, nor one another running WinCleaner is using.
export function sandboxOrphans(): Promise<SandboxOrphan[]> {
  return invoke<SandboxOrphan[]>("sandbox_orphans");
}

/// Removes those directories and resolves with how many went. Startup does the
/// same sweep on its own; this is the way to do it without restarting.
export function sandboxRemoveOrphans(): Promise<number> {
  return invoke<number>("sandbox_remove_orphans");
}

/// What the user pointed at in "Show the paths": the file itself, or the
/// directory holding it.
export type ExclusionScope = "file" | "folder";

/// `add_exclusion` rejects with this stable code — not a sentence — when the
/// folder asked for is one of the rule's own walk roots, which would empty the
/// rule rather than narrow it. The window turns it into its own localized
/// message; every other rejection is already a sentence.
/// Mirrors `src-tauri/src/exclusions.rs::RULE_ROOT_CODE`.
export const EXCLUSION_RULE_ROOT = "exclusion-is-rule-root";

/// Mirrors `src-tauri/src/commands.rs::ExclusionView`. `pattern` is always in
/// `%VAR%\…` form — the back end folds the resolved profile back into the
/// variable, so nothing here carries an account name.
export interface Exclusion {
  rule_id: string;
  rule_label: string;
  pattern: string;
  /// `YYYY-MM-DD`.
  added: string;
}

/// Excludes one entry of the last analysis of `ruleId` from that rule, for
/// good.
///
/// `index` is the position in the `paths` array the last `scan` returned — the
/// path itself is never sent. That is the same invariant `scan` and `clean`
/// obey (`CLAUDE.md`): no Tauri command takes a path, which is also why there
/// is no free-form glob editor.
export function addExclusion(
  ruleId: string,
  index: number,
  scope: ExclusionScope,
): Promise<Exclusion> {
  return invoke<Exclusion>("add_exclusion", { ruleId, index, scope });
}

export function listExclusions(): Promise<Exclusion[]> {
  return invoke<Exclusion[]>("list_exclusions");
}

export function removeExclusion(ruleId: string, pattern: string): Promise<void> {
  return invoke<void>("remove_exclusion", { ruleId, pattern });
}

/// Mirrors `src-tauri/src/space.rs::SpaceRoot`. `name` is a stable id
/// (`downloads`, `desktop`, …), not a label: the dictionary turns it into one.
export interface SpaceRoot {
  name: string;
  path: string;
  bytes: number;
  files: number;
}

/// Mirrors `src-tauri/src/space.rs::SpaceFile`. `index` is the row's position
/// in this result and the only handle `spaceReveal` takes — no path ever goes
/// back to Rust, the same invariant `scan` and `addExclusion` obey.
export interface SpaceFile {
  index: number;
  path: string;
  bytes: number;
  /// Milliseconds since the Unix epoch, or null when the filesystem reports
  /// no modification time.
  modified: number | null;
}

export interface SpaceFolder {
  index: number;
  path: string;
  bytes: number;
  files: number;
}

export interface SpaceResult {
  roots: SpaceRoot[];
  files: SpaceFile[];
  folders: SpaceFolder[];
  skipped_files: number;
  /// Known folders refused because they resolve outside the profile. Named so
  /// the user knows the total is short.
  skipped_roots: string[];
}

/// Mirrors `src-tauri/src/space.rs::SpaceProgress`, emitted as each root
/// finishes. `total_bytes` is the running total since the start.
export interface SpaceProgress {
  done: number;
  total: number;
  root: string;
  total_bytes: number;
}

/// Which of the two lists `spaceReveal` counts the index against.
export type RevealKind = "file" | "folder";

/// Measures the user's known folders. Read-only: this screen deletes nothing,
/// and the back end offers it no way to. Rejects while a sandbox is active.
export function spaceScan(): Promise<SpaceResult> {
  return invoke<SpaceResult>("space_scan");
}

/// Same contract as `onScanProgress`: resolves with the function that stops
/// listening.
export function onSpaceProgress(
  cb: (progress: SpaceProgress) => void,
): Promise<() => void> {
  return listen<SpaceProgress>("space-progress", (event) => cb(event.payload));
}

/// Shows the row at `index` in Explorer. Rejects when the measurement is stale
/// (no such index) or the path has since gone.
export function spaceReveal(kind: RevealKind, index: number): Promise<void> {
  return invoke<void>("space_reveal", { kind, index });
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
/// not searched: `winapp2.7-zip` is an implementation detail. Matches against
/// both `label` and `label_fr` regardless of the current UI language: a
/// French interface still shows the English label of a Winapp2 rule, and a
/// user typing from memory should find a rule by either name.
export function filterRules(rules: RuleSummary[], query: string): RuleSummary[] {
  const needle = query.trim().toLowerCase();
  if (!needle) return rules;
  return rules.filter(
    (rule) =>
      rule.label.toLowerCase().includes(needle) ||
      (rule.label_fr?.toLowerCase().includes(needle) ?? false),
  );
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
