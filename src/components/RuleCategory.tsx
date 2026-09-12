import { useId } from "react";
import { ChevronRight } from "lucide-react";
import { cn } from "@/lib/utils";
import { Badge } from "@/components/ui/badge";
import { Checkbox } from "@/components/ui/checkbox";
import {
  Collapsible,
  CollapsibleContent,
  CollapsibleTrigger,
} from "@/components/ui/collapsible";
import { useI18n } from "@/i18n";
import type { RuleSummary, ScanResult } from "@/lib/api";
import { ruleCategory, ruleDescription, ruleLabel } from "@/lib/rule-i18n";

/// Community rules carry a `winapp2.` id. The attribution is rendered only when
/// the category actually holds one, so it never reads as covering the native
/// rules sitting next to them.
function hasWinapp2Rule(rules: RuleSummary[]): boolean {
  return rules.some((r) => r.id.startsWith("winapp2."));
}

const WINAPP2_URL = "https://github.com/MoscaDotTo/Winapp2";

/// One category: its header — a real toggle, chevron included — and, when it is
/// open, the rows of the rules it holds.
export function RuleCategory({
  category,
  rules: catRules,
  results,
  catBytes,
  open,
  selected,
  openPaths,
  onToggleCategory,
  onToggleRule,
  onTogglePaths,
}: {
  category: string;
  rules: RuleSummary[];
  results: ScanResult[] | null;
  catBytes: number;
  open: boolean;
  selected: Set<string>;
  openPaths: Set<string>;
  onToggleCategory: () => void;
  onToggleRule: (id: string) => void;
  onTogglePaths: (id: string) => void;
}) {
  const { language, t, tn, tx, formatBytes, formatCount } = useI18n();
  const headingId = useId();
  const listId = useId();
  /// `category` is the grouping key rules.toml order relies on; the French
  /// spelling (when the interface is French) comes off the first rule in the
  /// group that carries one, every native rule in a category agreeing.
  const categoryRule = catRules.find((r) => r.category_fr);
  const categoryLabel = categoryRule ? ruleCategory(categoryRule, language) : category;
  return (
    <section className="flex flex-col gap-1.5" aria-labelledby={headingId}>
      <div
        className="flex cursor-pointer items-center justify-between gap-6 rounded px-1 py-1 outline-none hover:bg-accent"
        onClick={onToggleCategory}
      >
        <h2 className="contents" id={headingId}>
          <button
            type="button"
            data-testid={`toggle-category-${category}`}
            aria-expanded={open}
            /// Names the rows the chevron folds. The list stays mounted
            /// (hidden, not unmounted) while folded, so the id this points at
            /// always resolves.
            aria-controls={listId}
            className="flex min-w-0 items-center gap-1.5 rounded text-left outline-none focus-visible:ring-3 focus-visible:ring-ring"
          >
            <ChevronRight
              aria-hidden="true"
              className={cn(
                "size-3.5 shrink-0 text-muted-foreground transition-transform duration-200 motion-reduce:transition-none",
                open && "rotate-90"
              )}
            />
            <span className="eyebrow truncate text-muted-foreground">{categoryLabel}</span>
          </button>
        </h2>
        <p className="shrink-0 text-xs text-muted-foreground">
          {tn("rules.count", catRules.length)}
          {results && (
            <>
              {" · "}
              <span className="font-mono tnum">{formatBytes(catBytes)}</span>
            </>
          )}
        </p>
      </div>
      {/* Kept mounted (hidden, not unmounted) while folded, so the header's
          `aria-controls={listId}` always resolves to an element. The rows
          themselves render only while open: rendering all of them just to
          hide them would be up to 80 nodes of dead weight per category. */}
      <ul id={listId} hidden={!open} className="overflow-hidden rounded-lg border bg-card">
        {open && catRules.map((rule, index) => {
              const result = (results ?? []).find((r) => r.rule_id === rule.id);
              const unavailable = rule.unavailable_reason;
              const label = ruleLabel(rule, language);
              const description = ruleDescription(rule, language);
              return (
                <li
                  key={rule.id}
                  className={index > 0 ? "border-t" : undefined}
                >
                  <div className="flex h-11 items-center gap-3 px-4">
                    <Checkbox
                      id={rule.id}
                      aria-label={label}
                      checked={selected.has(rule.id)}
                      disabled={!!unavailable}
                      onCheckedChange={() => onToggleRule(rule.id)}
                    />
                    <span
                      className={
                        unavailable
                          ? "min-w-0 flex-1 truncate text-sm text-muted-foreground"
                          : "min-w-0 flex-1 cursor-pointer truncate text-sm select-none"
                      }
                      onClick={() => !unavailable && onToggleRule(rule.id)}
                    >
                      {label}
                    </span>
                    {rule.risk === "medium" && (
                      <Badge
                        variant="outline"
                        className="border-warning/40 bg-warning/12 text-warning-foreground"
                      >
                        {t("rules.mediumRisk")}
                      </Badge>
                    )}
                    {rule.kind === "recycle-bin" && (
                      <Badge
                        data-testid={`note-${rule.id}`}
                        variant="outline"
                        className="border-destructive/40 text-destructive"
                      >
                        {t("rules.allVolumes")}
                      </Badge>
                    )}
                    {result && (
                      <div
                        data-testid={`result-${rule.id}`}
                        className="flex shrink-0 items-baseline gap-3"
                      >
                        {result.skipped > 0 && (
                          <span className="font-mono tnum text-xs text-muted-foreground">
                            {t("rules.skipped", { count: formatCount(result.skipped) })}
                          </span>
                        )}
                        <span className="w-24 text-right font-mono tnum text-sm">
                          {formatBytes(result.total_bytes)}
                        </span>
                        <span className="w-20 text-right font-mono tnum text-sm text-muted-foreground">
                          {formatCount(result.file_count)}
                          <span className="sr-only">{t("rules.filesSr")}</span>
                        </span>
                      </div>
                    )}
                  </div>
                  {unavailable && (
                    <p
                      data-testid={`unavailable-${rule.id}`}
                      className="px-4 pb-3 pl-11 text-xs text-muted-foreground"
                    >
                      {t("rules.unavailable", { reason: unavailable })}
                    </p>
                  )}
                  {rule.kind === "recycle-bin" && (
                    <p className="px-4 pb-3 pl-11 text-xs text-muted-foreground">
                      {t("rules.recycleBinNote")}
                    </p>
                  )}
                  {rule.id === "windows.temp" && (
                    <p className="px-4 pb-3 pl-11 text-xs text-muted-foreground">
                      {t("rules.tempNote")}
                    </p>
                  )}
                  {description && (
                    <p
                      data-testid={`warning-${rule.id}`}
                      className="px-4 pb-3 pl-11 text-xs text-warning-foreground"
                    >
                      {description}
                    </p>
                  )}
                  {result && result.paths.length > 0 && (
                    <Collapsible
                      open={openPaths.has(rule.id)}
                      onOpenChange={() => onTogglePaths(rule.id)}
                    >
                      <CollapsibleTrigger
                        data-testid={`toggle-paths-${rule.id}`}
                        /* The trigger says "the paths"; which rule's paths is
                           only obvious from the row it sits under. The visible
                           words stay in the name, so speaking them still
                           works. */
                        aria-label={t(
                          openPaths.has(rule.id)
                            ? "rules.hidePathsOf"
                            : "rules.showPathsOf",
                          { label },
                        )}
                        className="mb-3 ml-[38px] inline-flex cursor-pointer items-center gap-1 rounded px-1 py-0.5 text-xs text-muted-foreground outline-none hover:bg-accent hover:text-foreground focus-visible:ring-3 focus-visible:ring-ring"
                      >
                        <ChevronRight
                          aria-hidden="true"
                          className={cn(
                            "size-3 shrink-0 transition-transform duration-200 motion-reduce:transition-none",
                            openPaths.has(rule.id) && "rotate-90"
                          )}
                        />
                        <span className="underline underline-offset-2">
                          {t(openPaths.has(rule.id) ? "rules.hidePaths" : "rules.showPaths")}
                        </span>
                      </CollapsibleTrigger>
                      {/* A landmark of its own: the list is long, and a screen
                          reader user needs a way out of it that is not
                          arrowing to the end. */}
                      <CollapsibleContent
                        role="region"
                        aria-label={t("rules.pathsOf", { label })}
                      >
                        <ul className="mx-4 mb-3 ml-11 max-h-48 overflow-auto rounded-[6px] bg-muted p-3 font-mono text-xs text-muted-foreground">
                          {result.paths.map((p) => (
                            <li key={p} className="truncate">
                              {p}
                            </li>
                          ))}
                        </ul>
                      </CollapsibleContent>
                    </Collapsible>
                  )}
                </li>
              );
            })}
      </ul>
      {open && hasWinapp2Rule(catRules) && (
        <p
          data-testid="winapp2-attribution"
          className="px-1 text-xs text-muted-foreground"
        >
          {tx("rules.winapp2Attribution", {
            url: <span className="font-mono">{WINAPP2_URL}</span>,
          })}
        </p>
      )}
    </section>
  );
}
