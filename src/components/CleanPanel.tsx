import { useEffect, useMemo, useState } from "react";
import { TriangleAlert } from "lucide-react";
import { toast } from "sonner";
import { Button } from "@/components/ui/button";
import { Checkbox } from "@/components/ui/checkbox";
import { Badge } from "@/components/ui/badge";
import {
  Collapsible,
  CollapsibleContent,
  CollapsibleTrigger,
} from "@/components/ui/collapsible";
import { ReclaimGauge } from "@/components/ReclaimGauge";
import { formatBytes, formatCount } from "@/lib/format";
import {
  clean,
  filterRules,
  groupByCategory,
  listRules,
  rulesSummary,
  runningBrowsers,
  scan,
  sortGrouped,
  type CleanMode,
  type CleanReport,
  type RuleSummary,
  type RulesSummary,
  type ScanResult,
} from "@/lib/api";

const MODE_LABEL: Record<CleanMode, string> = {
  auto: "Auto",
  trash: "Recycle Bin",
  permanent: "Permanent",
};

/// Categories that start folded: the converted Winapp2 rules are hundreds of
/// rows, none of them checked by default, and unfolding them is a deliberate
/// act.
const COLLAPSED_BY_DEFAULT = ["Applications"];

const WINAPP2_URL = "https://github.com/MoscaDotTo/Winapp2";

const SORT_KEY = "wincleaner.sortBySize";

/// `localStorage` throws in a webview with site data disabled: an unreadable
/// preference is simply "off", never a crash on the first render.
function readSortPreference(): boolean {
  try {
    return window.localStorage.getItem(SORT_KEY) === "1";
  } catch {
    return false;
  }
}

const RULE_COUNT = new Intl.NumberFormat("en-US");

/// Whether this mode will destroy this rule's content with no way back. The
/// Recycle Bin rule always is: the deletion mode does not apply to it.
export function isIrreversible(rule: RuleSummary, mode: CleanMode): boolean {
  if (rule.kind === "recycle-bin") return true;
  if (mode === "permanent") return true;
  return mode === "auto" && rule.risk === "low";
}

function Screen({ children }: { children: React.ReactNode }) {
  return (
    <>
      <header className="shrink-0 px-8 pt-7 pb-5">
        <h1 className="screen-title">Cleanup</h1>
      </header>
      {children}
    </>
  );
}

export function CleanPanel() {
  const [rules, setRules] = useState<RuleSummary[]>([]);
  const [rulesError, setRulesError] = useState<string | null>(null);
  const [selected, setSelected] = useState<Set<string>>(new Set());
  const [results, setResults] = useState<ScanResult[] | null>(null);
  const [report, setReport] = useState<CleanReport | null>(null);
  const [mode, setMode] = useState<CleanMode>("auto");
  const [browsers, setBrowsers] = useState<string[]>([]);
  const [busy, setBusy] = useState(false);
  const [openPaths, setOpenPaths] = useState<Set<string>>(new Set());
  const [confirming, setConfirming] = useState(false);
  const [query, setQuery] = useState("");
  const [expanded, setExpanded] = useState<Set<string>>(new Set());
  const [summary, setSummary] = useState<RulesSummary | null>(null);
  const [sortBySize, setSortBySize] = useState<boolean>(readSortPreference);

  const [reloadKey, setReloadKey] = useState(0);

  useEffect(() => {
    setRulesError(null);
    listRules()
      .then((loaded) => {
        setRules(loaded);
        setSelected(new Set(loaded.filter((r) => r.default_checked).map((r) => r.id)));
      })
      .catch((err) => setRulesError(String(err)));
    runningBrowsers()
      .then(setBrowsers)
      .catch(() => setBrowsers([]));
    rulesSummary()
      .then(setSummary)
      .catch(() => setSummary(null));
  }, [reloadKey]);

  const searching = query.trim().length > 0;
  const grouped = useMemo(() => {
    const base = groupByCategory(filterRules(rules, query));
    return sortBySize ? sortGrouped(base, results) : base;
  }, [rules, query, sortBySize, results]);
  const total = useMemo(
    () => (results ?? []).reduce((sum, r) => sum + r.total_bytes, 0),
    [results]
  );
  const scannedIds = useMemo(() => (results ?? []).map((r) => r.rule_id), [results]);
  const recycleBinChecked = useMemo(
    () => rules.some((r) => r.kind === "recycle-bin" && selected.has(r.id)),
    [rules, selected]
  );
  /// The scanned rules that the current mode will destroy with no way back.
  const irreversibleRules = useMemo(
    () =>
      rules.filter((r) => scannedIds.includes(r.id) && isIrreversible(r, mode)),
    [rules, scannedIds, mode]
  );

  function toggleRule(id: string) {
    setSelected((prev) => {
      const next = new Set(prev);
      if (next.has(id)) next.delete(id);
      else next.add(id);
      return next;
    });
    setResults(null);
    setReport(null);
    setConfirming(false);
  }

  function togglePaths(id: string) {
    setOpenPaths((prev) => {
      const next = new Set(prev);
      if (next.has(id)) next.delete(id);
      else next.add(id);
      return next;
    });
  }

  /// A search reaches into a folded category: a match the user cannot see
  /// would make the search field lie.
  function isOpen(category: string): boolean {
    if (searching) return true;
    return expanded.has(category) || !COLLAPSED_BY_DEFAULT.includes(category);
  }

  function toggleCategory(category: string) {
    setExpanded((prev) => {
      const next = new Set(prev);
      if (next.has(category)) next.delete(category);
      else next.add(category);
      return next;
    });
  }

  function toggleSortBySize() {
    setSortBySize((prev) => {
      const next = !prev;
      try {
        window.localStorage.setItem(SORT_KEY, next ? "1" : "0");
      } catch {
        // A preference we cannot persist is still a preference for this
        // session: nothing to report to the user.
      }
      return next;
    });
  }

  async function onScan() {
    setBusy(true);
    setReport(null);
    setConfirming(false);
    try {
      setResults(await scan(rules.filter((r) => selected.has(r.id)).map((r) => r.id)));
    } catch (err) {
      setRulesError(String(err));
    } finally {
      setBusy(false);
    }
  }

  async function onClean() {
    setConfirming(false);
    setBusy(true);
    try {
      const done = await clean(scannedIds, mode);
      setReport(done);
      setResults(null);
      toast.success(`Cleaned: ${formatBytes(done.freed_bytes)} freed`);
    } catch (err) {
      setRulesError(String(err));
    } finally {
      setBusy(false);
    }
  }

  if (rulesError) {
    return (
      <Screen>
        <div className="min-h-0 flex-1 overflow-auto px-8 pb-8">
          <div
            data-testid="rules-error"
            className="flex max-w-xl flex-col items-start gap-3 rounded-lg border border-destructive/40 bg-destructive/8 p-5"
          >
            <p className="font-medium">Could not load the rules.</p>
            <p className="font-mono text-xs text-muted-foreground">{rulesError}</p>
            <Button
              variant="outline"
              onClick={() => {
                setResults(null);
                setReport(null);
                setReloadKey((k) => k + 1);
              }}
            >
              Retry
            </Button>
          </div>
        </div>
      </Screen>
    );
  }

  return (
    <Screen>
      <div className="flex min-h-0 flex-1 flex-col gap-6 overflow-auto px-8 pb-8">
        {browsers.length > 0 && (
          <div
            data-testid="browser-warning"
            className="flex items-start gap-2.5 rounded-lg border border-warning/40 bg-warning/12 px-4 py-3 text-sm text-warning-foreground"
          >
            <TriangleAlert className="mt-px size-4 shrink-0 text-warning" />
            <p>
              <span className="font-mono">{browsers.join(", ")}</span> is open:
              its files that are currently in use will be skipped.
            </p>
          </div>
        )}

        <section className="flex flex-col gap-5 rounded-lg border bg-card p-5">
          <div className="flex items-start justify-between gap-6">
            <div className="min-w-0">
              <p className="eyebrow text-muted-foreground">Reclaimable</p>
              {results ? (
                <p
                  data-testid="total-bytes"
                  className="mt-1.5 font-mono tnum text-[2rem] leading-none font-semibold"
                >
                  {formatBytes(total)}
                </p>
              ) : (
                <p className="mt-1.5 text-[2rem] leading-none font-normal text-muted-foreground">
                  —
                </p>
              )}
            </div>
            <div className="flex shrink-0 items-center gap-4">
              <span className="flex items-center gap-2 text-sm text-muted-foreground">
                <Checkbox
                  data-testid="sort-by-size"
                  aria-label="Sort by size"
                  checked={sortBySize}
                  disabled={!results}
                  onCheckedChange={toggleSortBySize}
                />
                <span aria-hidden="true">Sort by size</span>
              </span>
              <Button size="lg" onClick={onScan} disabled={busy || selected.size === 0}>
                Analyze
              </Button>
            </div>
          </div>
          {results ? (
            <ReclaimGauge rules={rules} results={results} />
          ) : (
            <p className="text-sm text-muted-foreground">
              Analyze to measure what can be freed.
            </p>
          )}
        </section>

        <div className="flex flex-col gap-1.5">
          <input
            data-testid="rule-search"
            type="search"
            aria-label="Search rules"
            placeholder="Search rules"
            className="h-9 w-full rounded-md border bg-card px-3 text-sm outline-none focus-visible:ring-3 focus-visible:ring-ring/50"
            value={query}
            onChange={(e) => setQuery(e.target.value)}
          />
          {summary && (
            <p data-testid="rules-summary" className="px-1 text-xs text-muted-foreground">
              <span className="font-mono tnum">{RULE_COUNT.format(summary.native)}</span>{" "}
              built-in rules ·{" "}
              <span className="font-mono tnum">
                {RULE_COUNT.format(summary.winapp2_detected)}
              </span>{" "}
              Winapp2 rules detected out of{" "}
              <span className="font-mono tnum">
                {RULE_COUNT.format(summary.winapp2_retained)}
              </span>{" "}
              converted (
              <span className="font-mono tnum">
                {RULE_COUNT.format(summary.winapp2_dropped)}
              </span>{" "}
              entries not supported)
            </p>
          )}
        </div>

        {searching && grouped.length === 0 && (
          <p data-testid="search-empty" className="px-1 text-sm text-muted-foreground">
            No rules match your search.
          </p>
        )}

        {grouped.map(([category, catRules]) => {
          const catBytes = (results ?? [])
            .filter((r) => catRules.some((rule) => rule.id === r.rule_id))
            .reduce((sum, r) => sum + r.total_bytes, 0);
          const collapsible = COLLAPSED_BY_DEFAULT.includes(category);
          const open = isOpen(category);
          const heading = (
            <>
              <h2 className="eyebrow text-muted-foreground">{category}</h2>
              <p className="text-xs text-muted-foreground">
                {RULE_COUNT.format(catRules.length)} rules
                {results && (
                  <>
                    {" · "}
                    <span className="font-mono tnum">{formatBytes(catBytes)}</span>
                  </>
                )}
              </p>
            </>
          );
          return (
            <section key={category} className="flex flex-col gap-1.5">
              {collapsible ? (
                <button
                  type="button"
                  data-testid={`toggle-category-${category}`}
                  aria-expanded={open}
                  onClick={() => toggleCategory(category)}
                  className="flex items-baseline justify-between gap-6 rounded px-1 text-left outline-none focus-visible:ring-3 focus-visible:ring-ring/50"
                >
                  {heading}
                </button>
              ) : (
                <div className="flex items-baseline justify-between px-1">{heading}</div>
              )}
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
                              onCheckedChange={() => toggleRule(rule.id)}
                            />
                            <span
                              className={
                                unavailable
                                  ? "min-w-0 flex-1 truncate text-sm text-muted-foreground"
                                  : "min-w-0 flex-1 cursor-pointer truncate text-sm"
                              }
                              onClick={() => !unavailable && toggleRule(rule.id)}
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
                              onOpenChange={() => togglePaths(rule.id)}
                            >
                              <CollapsibleTrigger
                                data-testid={`toggle-paths-${rule.id}`}
                                className="mb-3 ml-11 rounded text-xs text-muted-foreground underline underline-offset-2 outline-none hover:text-foreground focus-visible:ring-3 focus-visible:ring-ring/50"
                              >
                                {openPaths.has(rule.id) ? "Hide" : "Show"} the paths
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
                  {collapsible && (
                    <p
                      data-testid="winapp2-attribution"
                      className="px-1 text-xs text-muted-foreground"
                    >
                      Community rules from Winapp2 (CC-BY-SA 4.0) —{" "}
                      <span className="font-mono">{WINAPP2_URL}</span>
                    </p>
                  )}
                </>
              )}
            </section>
          );
        })}

        {report && (
          <section
            data-testid="clean-report"
            className="rounded-lg border bg-card p-5"
          >
            <h2 className="eyebrow text-muted-foreground">Last cleanup</h2>
            <p className="mt-2 text-sm">
              <span className="font-mono tnum">{formatBytes(report.freed_bytes)}</span>{" "}
              freed ·{" "}
              <span className="font-mono tnum">{formatCount(report.deleted)}</span>{" "}
              files deleted
            </p>
            {report.skipped.length > 0 && (
              <>
                <h3 className="mt-4 text-xs font-medium text-muted-foreground">
                  Skipped (<span className="font-mono tnum">{formatCount(report.skipped.length)}</span>)
                </h3>
                <ul className="mt-1.5 max-h-48 overflow-auto rounded-[6px] bg-muted p-3 font-mono text-xs text-muted-foreground">
                  {report.skipped.map((s) => (
                    <li key={s.path} className="truncate">
                      {s.path} — {s.reason}
                    </li>
                  ))}
                </ul>
              </>
            )}
          </section>
        )}
      </div>

      <footer className="flex shrink-0 items-center gap-4 border-t bg-background px-8 py-3.5">
        {confirming ? (
          <>
            <div data-testid="confirm-clean" className="min-w-0 flex-1">
              <p className="text-sm font-medium">
                Clean{" "}
                <span className="font-mono tnum">{formatBytes(total)}</span> in{" "}
                {MODE_LABEL[mode]} mode?
              </p>
              <p className="mt-0.5 text-xs text-muted-foreground">
                {irreversibleRules.length > 0 ? (
                  <>
                    No way back:{" "}
                    {irreversibleRules.map((r) => r.label).join(", ")}.
                  </>
                ) : (
                  <>Everything goes to the recycle bin and stays recoverable.</>
                )}
              </p>
            </div>
            <Button variant="outline" onClick={() => setConfirming(false)}>
              Cancel
            </Button>
            <Button size="lg" variant="destructive" onClick={onClean} disabled={busy}>
              Confirm cleanup
            </Button>
          </>
        ) : (
          <>
            <div className="flex min-w-0 flex-1 items-center gap-3">
              <select
                id="clean-mode"
                aria-label="Deletion mode"
                className="h-8 shrink-0 rounded-md border bg-card px-2 text-sm outline-none focus-visible:ring-3 focus-visible:ring-ring/50"
                value={mode}
                onChange={(e) => setMode(e.target.value as CleanMode)}
              >
                <option value="auto">Auto</option>
                <option value="trash">Recycle Bin</option>
                <option value="permanent">Permanent</option>
              </select>
              <p data-testid="mode-help" className="min-w-0 text-xs text-muted-foreground">
                {recycleBinChecked ? (
                  <span data-testid="recycle-order-note">
                    The Recycle Bin is emptied first: whatever the other rules
                    drop into it during the same pass is not swept away.
                  </span>
                ) : (
                  <>
                    Auto: permanent deletion for low-risk items, recycle bin for
                    the rest.
                  </>
                )}{" "}
                <span data-testid="empty-dirs-note">
                  Directories a rule empties are removed whatever the mode: an
                  empty directory holds no data.
                </span>
              </p>
            </div>
            <Button
              size="lg"
              variant="destructive"
              onClick={() => setConfirming(true)}
              disabled={busy || !results || scannedIds.length === 0}
            >
              Clean
              {results && total > 0 && (
                <span className="font-mono tnum">{formatBytes(total)}</span>
              )}
            </Button>
          </>
        )}
      </footer>
    </Screen>
  );
}
