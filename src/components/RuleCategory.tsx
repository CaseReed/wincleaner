import { ChevronRight } from "lucide-react";
import { cn } from "@/lib/utils";
import { Badge } from "@/components/ui/badge";
import { Checkbox } from "@/components/ui/checkbox";
import {
  Collapsible,
  CollapsibleContent,
  CollapsibleTrigger,
} from "@/components/ui/collapsible";
import { formatBytes, formatCount } from "@/lib/format";
import type { RuleSummary, ScanResult } from "@/lib/api";

const RULE_COUNT = new Intl.NumberFormat("en-US");

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
  return (
    <section className="flex flex-col gap-1.5">
      <div
        className="flex cursor-pointer items-center justify-between gap-6 rounded px-1 py-1 outline-none hover:bg-accent"
        onClick={onToggleCategory}
      >
        <h2 className="contents">
          <button
            type="button"
            data-testid={`toggle-category-${category}`}
            aria-expanded={open}
            className="flex min-w-0 items-center gap-1.5 rounded text-left outline-none focus-visible:ring-3 focus-visible:ring-ring/50"
          >
            <ChevronRight
              aria-hidden="true"
              className={cn(
                "size-3.5 shrink-0 text-muted-foreground transition-transform duration-200 motion-reduce:transition-none",
                open && "rotate-90"
              )}
            />
            <span className="eyebrow truncate text-muted-foreground">{category}</span>
          </button>
        </h2>
        <p className="shrink-0 text-xs text-muted-foreground">
          {RULE_COUNT.format(catRules.length)} rules
          {results && (
            <>
              {" · "}
              <span className="font-mono tnum">{formatBytes(catBytes)}</span>
            </>
          )}
        </p>
      </div>
      {open && (
        <>
          <ul className="overflow-hidden rounded-lg border bg-card">
            {catRules.map((rule, index) => {
              const result = (results ?? []).find((r) => r.rule_id === rule.id);
              const unavailable = rule.unavailable_reason;
              return (
                <li
                  key={rule.id}
                  className={index > 0 ? "border-t" : undefined}
                >
                  <div className="flex h-11 items-center gap-3 px-4">
                    <Checkbox
                      id={rule.id}
                      aria-label={rule.label}
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
                      {rule.label}
                    </span>
                    {rule.risk === "medium" && (
                      <Badge
                        variant="outline"
                        className="border-warning/40 bg-warning/12 text-warning-foreground"
                      >
                        medium risk
                      </Badge>
                    )}
                    {rule.kind === "recycle-bin" && (
                      <Badge
                        data-testid={`note-${rule.id}`}
                        variant="outline"
                        className="border-destructive/40 text-destructive"
                      >
                        all volumes
                      </Badge>
                    )}
                    {result && (
                      <div
                        data-testid={`result-${rule.id}`}
                        className="flex shrink-0 items-baseline gap-3"
                      >
                        {result.skipped > 0 && (
                          <span className="font-mono tnum text-xs text-muted-foreground">
                            {formatCount(result.skipped)} skipped
                          </span>
                        )}
                        <span className="w-24 text-right font-mono tnum text-sm">
                          {formatBytes(result.total_bytes)}
                        </span>
                        <span className="w-20 text-right font-mono tnum text-sm text-muted-foreground">
                          {formatCount(result.file_count)}
                          <span className="sr-only"> files</span>
                        </span>
                      </div>
                    )}
                  </div>
                  {unavailable && (
                    <p
                      data-testid={`unavailable-${rule.id}`}
                      className="px-4 pb-3 pl-11 text-xs text-muted-foreground"
                    >
                      Unavailable on this machine: {unavailable}
                    </p>
                  )}
                  {rule.kind === "recycle-bin" && (
                    <p className="px-4 pb-3 pl-11 text-xs text-muted-foreground">
                      Empties the recycle bin of every volume on this machine,
                      including outside the user profile. Permanent and
                      irreversible: the deletion mode does not apply to it.
                    </p>
                  )}
                  {rule.id === "windows.temp" && (
                    <p className="px-4 pb-3 pl-11 text-xs text-muted-foreground">
                      Close any running installers before cleaning.
                    </p>
                  )}
                  {rule.note && (
                    <p
                      data-testid={`warning-${rule.id}`}
                      className="px-4 pb-3 pl-11 text-xs text-warning-foreground"
                    >
                      {rule.note}
                    </p>
                  )}
                  {result && result.paths.length > 0 && (
                    <Collapsible
                      open={openPaths.has(rule.id)}
                      onOpenChange={() => onTogglePaths(rule.id)}
                    >
                      <CollapsibleTrigger
                        data-testid={`toggle-paths-${rule.id}`}
                        className="mb-3 ml-[38px] inline-flex cursor-pointer items-center gap-1 rounded px-1 py-0.5 text-xs text-muted-foreground outline-none hover:bg-accent hover:text-foreground focus-visible:ring-3 focus-visible:ring-ring/50"
                      >
                        <ChevronRight
                          aria-hidden="true"
                          className={cn(
                            "size-3 shrink-0 transition-transform duration-200 motion-reduce:transition-none",
                            openPaths.has(rule.id) && "rotate-90"
                          )}
                        />
                        <span className="underline underline-offset-2">
                          {openPaths.has(rule.id) ? "Hide" : "Show"} the paths
                        </span>
                      </CollapsibleTrigger>
                      <CollapsibleContent>
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
          {hasWinapp2Rule(catRules) && (
            <p
              data-testid="winapp2-attribution"
              className="px-1 text-xs text-muted-foreground"
            >
              Some of the rules in this category are community rules
              from Winapp2 (CC-BY-SA 4.0) —{" "}
              <span className="font-mono">{WINAPP2_URL}</span>
            </p>
          )}
        </>
      )}
    </section>
  );
}
