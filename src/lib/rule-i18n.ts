import type { Language } from "@/i18n";
import type { RuleSummary } from "@/lib/api";

/// The interface language covers the chrome only; a rule's label, note and
/// category come from `src-tauri/rules.toml` (native rules) or Winapp2
/// (community rules, always English). These three helpers are the single
/// place that picks the French field when one exists and the UI is French,
/// falling back to English otherwise — everywhere a rule's text reaches the
/// screen or a live region goes through them, so a Winapp2 rule (no French
/// fields) reads the same in both languages.

export function ruleLabel(
  rule: Pick<RuleSummary, "label" | "label_fr">,
  locale: Language,
): string {
  return (locale === "fr" && rule.label_fr) || rule.label;
}

export function ruleDescription(rule: RuleSummary, locale: Language): string | null {
  return (locale === "fr" && rule.description_fr) || rule.note;
}

export function ruleCategory(rule: RuleSummary, locale: Language): string {
  return (locale === "fr" && rule.category_fr) || rule.category;
}
