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
  /// Case cochée au premier lancement. Faux pour ce qu'on ne nettoie pas
  /// sans l'avoir voulu explicitement (cf. src-tauri/rules.toml).
  default_checked: boolean;
  /// Renseigné quand la règle ne s'applique pas sur cette machine (variable
  /// absente, ou pointant hors du profil). La ligne est grisée et inerte.
  unavailable_reason: string | null;
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

export function listStartup(): Promise<StartupEntry[]> {
  return invoke<StartupEntry[]>("list_startup");
}

export function setStartupEnabled(id: string, enabled: boolean): Promise<void> {
  return invoke<void>("set_startup_enabled", { id, enabled });
}

/// Regroupe les règles par catégorie en conservant l'ordre de rules.toml.
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
