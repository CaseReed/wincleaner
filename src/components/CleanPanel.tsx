import { useEffect, useId, useMemo, useRef, useState } from "react";
import { Info, Loader2, ShieldCheck, ShieldX, TriangleAlert } from "lucide-react";
import { toast } from "sonner";
import { cn } from "@/lib/utils";
import { Button } from "@/components/ui/button";
import { Checkbox } from "@/components/ui/checkbox";
import { ReclaimGauge, prefersReducedMotion } from "@/components/ReclaimGauge";
import { RuleCategory } from "@/components/RuleCategory";
import { formatBytes, formatCount } from "@/lib/format";
import {
  clean,
  filterRules,
  groupByCategory,
  listRules,
  onCleanProgress,
  onScanProgress,
  rulesSummary,
  runningBrowsers,
  sandboxVerify,
  scan,
  sortGrouped,
  type CleanMode,
  type CleanProgress,
  type CleanReport,
  type RuleSummary,
  type RulesSummary,
  type SandboxSummary,
  type SandboxVerdict,
  type ScanProgress,
  type ScanResult,
} from "@/lib/api";

const MODE_LABEL: Record<CleanMode, string> = {
  auto: "Auto",
  trash: "Recycle Bin",
  permanent: "Permanent",
};

/// Categories that start folded: `Applications` is hundreds of rows, almost all
/// of them converted Winapp2 rules that are unchecked by default, and unfolding
/// them is a deliberate act. It is NOT a "nothing selected in here" category:
/// native rules live there too (`npm.cache`, checked by default). Folding is
/// presentation only — Analyze sends every selected rule, folded or filtered
/// out of view.
const COLLAPSED_BY_DEFAULT = ["Applications"];

const SORT_KEY = "wincleaner.sortBySize";
const HINT_KEY = "wincleaner.hintDismissed";

/// `localStorage` throws in a webview with site data disabled: an unreadable
/// preference is simply "off", never a crash on the first render.
function readFlag(key: string): boolean {
  try {
    return window.localStorage.getItem(key) === "1";
  } catch {
    return false;
  }
}

function writeFlag(key: string, value: boolean) {
  try {
    window.localStorage.setItem(key, value ? "1" : "0");
  } catch {
    // A preference we cannot persist is still a preference for this session:
    // nothing to report to the user.
  }
}

const RULE_COUNT = new Intl.NumberFormat("en-US");

/// How often the live region is allowed to speak while a walk is still going,
/// besides its final event. `done % 10` never fires at all below ten rules —
/// which is the common case with a handful of rules selected — and is silent
/// for the whole run either way, so time is what actually bounds how long a
/// screen-reader user goes without an update.
const ANNOUNCE_INTERVAL_MS = 5000;

/// Whether this progress event should be spoken: the last one always is, and
/// otherwise at most one announcement per `ANNOUNCE_INTERVAL_MS`. Shared by
/// scan and clean, whose progress shapes differ only in field names.
function shouldAnnounce(lastAnnouncedAt: number, now: number, done: number, total: number): boolean {
  return done >= total || now - lastAnnouncedAt >= ANNOUNCE_INTERVAL_MS;
}

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

/// Whether the sandbox came through untouched on every count. One `false`
/// here is the only interesting outcome: it means a guard let something past.
export function verdictPasses(v: SandboxVerdict): boolean {
  return (
    v.sentinels_damaged.length === 0 &&
    v.junk_remaining.length === 0 &&
    v.outside_intact === v.outside_total &&
    v.junctions_refused
  );
}

/// One line of the verdict: a claim, the count that backs it, and optionally
/// what the count is scoped to.
function VerdictLine({
  label,
  got,
  of,
  scope,
}: {
  label: string;
  got: number;
  of: number;
  scope?: string;
}) {
  return (
    <li>
      {label}{" "}
      <span className="font-mono tnum">
        {formatCount(got)} / {formatCount(of)}
      </span>
      {scope && <span className="text-muted-foreground"> {scope}</span>}
    </li>
  );
}

/// What the sandbox looked like after the clean, read back off the disk. Shown
/// only in sandbox mode: outside it there is no manifest to compare against.
function SandboxVerdictCard({ verdict }: { verdict: SandboxVerdict }) {
  const passed = verdictPasses(verdict);
  const titleId = useId();
  return (
    <section
      data-testid="sandbox-verdict"
      data-verdict={passed ? "pass" : "fail"}
      /// The answer to "did a guard let something past". It appears after the
      /// clean, well below the button that started it: announced, not left to
      /// be found.
      role="status"
      aria-labelledby={titleId}
      className={cn(
        "rounded-lg border p-5",
        passed
          ? "border-success/40 bg-success/8"
          : "border-destructive/40 bg-destructive/8"
      )}
    >
      <h2 id={titleId} className="eyebrow flex items-center gap-2 text-muted-foreground">
        {passed ? (
          <ShieldCheck className="size-4 text-success" aria-hidden="true" />
        ) : (
          <ShieldX className="size-4 text-destructive" aria-hidden="true" />
        )}
        Sandbox verdict
        {/* The colour of the card is the whole verdict for a sighted user;
            without this it is the one thing a screen reader cannot read. */}
        <span className="sr-only">: {passed ? "passed" : "failed"}</span>
      </h2>
      <ul className="mt-2.5 flex flex-col gap-1 text-sm">
        <VerdictLine
          label="Sentinels intact"
          got={verdict.sentinels_intact}
          of={verdict.sentinels_total}
        />
        <VerdictLine
          label="Junk removed"
          got={verdict.junk_removed}
          of={verdict.junk_total}
          scope={`for the ${formatCount(verdict.rules_cleaned)} ${
            verdict.rules_cleaned === 1 ? "rule" : "rules"
          } cleaned`}
        />
        <VerdictLine
          label="Junction baits untouched"
          got={verdict.outside_intact}
          of={verdict.outside_total}
        />
        {!verdict.junctions_refused && (
          <li className="text-destructive-foreground">
            A junction the sandbox planted no longer stands.
          </li>
        )}
      </ul>
      {verdict.sentinels_damaged.length > 0 && (
        <>
          <h3 className="mt-4 text-xs font-medium text-muted-foreground">
            Deleted or rewritten, and should not have been
          </h3>
          <ul className="mt-1.5 max-h-40 overflow-auto rounded-[6px] bg-muted p-3 font-mono text-xs text-muted-foreground">
            {verdict.sentinels_damaged.map((p) => (
              <li key={p} className="truncate">
                {p}
              </li>
            ))}
          </ul>
        </>
      )}
      {verdict.junk_remaining.length > 0 && (
        <>
          <h3 className="mt-4 text-xs font-medium text-muted-foreground">
            Junk still on disk
          </h3>
          <ul className="mt-1.5 max-h-40 overflow-auto rounded-[6px] bg-muted p-3 font-mono text-xs text-muted-foreground">
            {verdict.junk_remaining.map((p) => (
              <li key={p} className="truncate">
                {p}
              </li>
            ))}
          </ul>
        </>
      )}
    </section>
  );
}

export function CleanPanel({
  sandbox = null,
}: {
  /// Non-null while a sandbox is active: every rule below then describes the
  /// synthetic profile, and a Clean is followed by a verdict read back off its
  /// disk.
  sandbox?: SandboxSummary | null;
} = {}) {
  const [rules, setRules] = useState<RuleSummary[]>([]);
  const [rulesError, setRulesError] = useState<string | null>(null);
  const [selected, setSelected] = useState<Set<string>>(new Set());
  const [results, setResults] = useState<ScanResult[] | null>(null);
  const [report, setReport] = useState<CleanReport | null>(null);
  const [verdict, setVerdict] = useState<SandboxVerdict | null>(null);
  /// The verify call failed. Shown inside the verdict card as a line of its
  /// own: it says nothing about the clean, which has already been reported.
  const [verdictError, setVerdictError] = useState<string | null>(null);
  const [mode, setMode] = useState<CleanMode>("auto");
  const [browsers, setBrowsers] = useState<string[]>([]);
  const [busyAction, setBusyAction] = useState<"scan" | "clean" | null>(null);
  /// The last `scan-progress` event of the running scan, or null before the
  /// first one arrives. Read only while a scan is pending.
  const [progress, setProgress] = useState<ScanProgress | null>(null);
  /// A ref, not the state: the subscription is made once, and a stale closure
  /// over `busyAction` would let a late event from a finished scan repaint a
  /// hero that is showing results.
  const scanPending = useRef(false);
  /// The last `clean-progress` event of the running clean, or null before the
  /// first one arrives. Read only while a clean is pending.
  const [cleanProgress, setCleanProgress] = useState<CleanProgress | null>(null);
  const cleanPending = useRef(false);
  /// When the live region last spoke, for the scan and the clean walks
  /// respectively: `shouldAnnounce` throttles on this instead of a rule count
  /// that can stay below ten for a whole run.
  const lastScanAnnounce = useRef(0);
  const lastCleanAnnounce = useRef(0);
  const [openPaths, setOpenPaths] = useState<Set<string>>(new Set());
  const [confirming, setConfirming] = useState(false);
  const [query, setQuery] = useState("");
  /// Categories the user folded or unfolded by hand, overriding the default
  /// (and, after a scan, the automatic unfold). Cleared by every new scan.
  const [folds, setFolds] = useState<Map<string, boolean>>(new Map());
  const [summary, setSummary] = useState<RulesSummary | null>(null);
  const [sortBySize, setSortBySize] = useState<boolean>(() => readFlag(SORT_KEY));
  const [hintDismissed, setHintDismissed] = useState<boolean>(() => readFlag(HINT_KEY));
  /// What a screen reader is told about the walk. Deliberately not the text on
  /// screen: that one repaints on every rule, which would be hundreds of
  /// interruptions for a scan nobody can hurry.
  const [announcement, setAnnouncement] = useState("");
  const heroTitleId = useId();
  const reportTitleId = useId();
  const confirmTitleId = useId();
  const confirmDetailId = useId();
  const confirmRef = useRef<HTMLDivElement | null>(null);
  const cleanButtonRef = useRef<HTMLButtonElement | null>(null);
  /// Set when the confirmation is dismissed by the user rather than carried
  /// out: the focus then belongs back on the button that raised it.
  const returnFocus = useRef(false);

  const busy = busyAction !== null;
  /// Zero until the first event: a bar that starts full would be a lie.
  const scanPercent =
    progress && progress.total > 0 ? (progress.done / progress.total) * 100 : 0;
  const cleanPercent =
    cleanProgress && cleanProgress.total_rules > 0
      ? (cleanProgress.done_rules / cleanProgress.total_rules) * 100
      : 0;
  const stillProgress = prefersReducedMotion();

  const [reloadKey, setReloadKey] = useState(0);

  useEffect(() => {
    setRulesError(null);
    listRules()
      .then((loaded) => {
        setRules(loaded);
        // In the sandbox every rule that applies starts checked: the point of
        // the profile is to exercise the whole catalogue against it, and the
        // usual caution behind `default_checked` — irreversible deletions in a
        // profile the user cares about — has no subject here.
        setSelected(
          new Set(
            loaded
              .filter((r) => (sandbox ? !r.unavailable_reason : r.default_checked))
              .map((r) => r.id)
          )
        );
      })
      .catch((err) => setRulesError(String(err)));
    runningBrowsers()
      .then(setBrowsers)
      .catch(() => setBrowsers([]));
    rulesSummary()
      .then(setSummary)
      .catch(() => setSummary(null));
  }, [reloadKey]);

  /// Subscribed once, for the life of the panel: `scan` emits while it walks,
  /// so the listener has to be in place before the command is invoked.
  useEffect(() => {
    let stop: (() => void) | null = null;
    let gone = false;
    onScanProgress((step) => {
      if (!scanPending.current) return;
      setProgress(step);
      // At most every five seconds, and the last one: enough to know the walk
      // is moving, rare enough to leave room for anything else being read.
      const now = Date.now();
      if (shouldAnnounce(lastScanAnnounce.current, now, step.done, step.total)) {
        lastScanAnnounce.current = now;
        setAnnouncement(
          `Analyzing: ${RULE_COUNT.format(step.done)} of ${RULE_COUNT.format(
            step.total
          )} rules, ${formatBytes(step.total_bytes)} so far`
        );
      }
    })
      .then((unlisten) => {
        // Unmounted while the subscription was still resolving: drop it at
        // once instead of leaving a listener behind.
        if (gone) unlisten();
        else stop = unlisten;
      })
      .catch(() => {
        // No progress feedback, but Analyze itself still works: the hero falls
        // back to the rule count.
      });
    return () => {
      gone = true;
      stop?.();
    };
  }, []);

  /// Same contract as the scan subscription above: in place before `clean` is
  /// invoked, because `clean` emits while it deletes.
  useEffect(() => {
    let stop: (() => void) | null = null;
    let gone = false;
    onCleanProgress((step) => {
      if (!cleanPending.current) return;
      setCleanProgress(step);
      // A rule that deletes 59,000 files reports every 500 of them, and
      // `done_rules` never moves in between: gating on time, not on the rule
      // count, is what keeps a single long rule from going silent for its
      // whole run.
      const now = Date.now();
      if (shouldAnnounce(lastCleanAnnounce.current, now, step.done_rules, step.total_rules)) {
        lastCleanAnnounce.current = now;
        setAnnouncement(
          `Cleaning: ${RULE_COUNT.format(step.done_rules)} of ${RULE_COUNT.format(
            step.total_rules
          )} rules, ${formatBytes(step.bytes_freed)} freed so far`
        );
      }
    })
      .then((unlisten) => {
        if (gone) unlisten();
        else stop = unlisten;
      })
      .catch(() => {
        // No progress feedback, but Clean itself still works: the hero falls
        // back to the rule count.
      });
    return () => {
      gone = true;
      stop?.();
    };
  }, []);

  /// The confirmation is the last stop before an irreversible deletion: the
  /// focus goes to it, so what is about to happen is read out, and `Escape`
  /// steps back out of it from anywhere.
  useEffect(() => {
    if (confirming) {
      confirmRef.current?.focus();
      return;
    }
    if (returnFocus.current) {
      returnFocus.current = false;
      cleanButtonRef.current?.focus();
    }
  }, [confirming]);

  useEffect(() => {
    if (!confirming) return;
    function onKeyDown(event: KeyboardEvent) {
      if (event.key !== "Escape") return;
      event.preventDefault();
      cancelConfirm();
    }
    document.addEventListener("keydown", onKeyDown);
    return () => document.removeEventListener("keydown", onKeyDown);
  }, [confirming]);

  function cancelConfirm() {
    returnFocus.current = true;
    setConfirming(false);
  }

  const searching = query.trim().length > 0;
  const grouped = useMemo(() => {
    const base = groupByCategory(filterRules(rules, query));
    return sortBySize ? sortGrouped(base, results) : base;
  }, [rules, query, sortBySize, results]);
  /// Analyze measures every rule that applies to this machine — walking a
  /// directory reads it, it never touches it — so an unchecked rule can still
  /// tell the user what checking it would free.
  const availableRules = useMemo(
    () => rules.filter((r) => !r.unavailable_reason),
    [rules]
  );
  /// The checkbox decides what Clean deletes, and nothing else. The hero total,
  /// the gauge and the Clean label answer "what will Clean free": the checked
  /// rules that were measured.
  const checkedResults = useMemo(
    () => (results ?? []).filter((r) => selected.has(r.rule_id)),
    [results, selected]
  );
  const total = useMemo(
    () => checkedResults.reduce((sum, r) => sum + r.total_bytes, 0),
    [checkedResults]
  );
  const uncheckedTotal = useMemo(
    () =>
      (results ?? [])
        .filter((r) => !selected.has(r.rule_id))
        .reduce((sum, r) => sum + r.total_bytes, 0),
    [results, selected]
  );
  const cleanIds = useMemo(() => checkedResults.map((r) => r.rule_id), [checkedResults]);
  const recycleBinChecked = useMemo(
    () => rules.some((r) => r.kind === "recycle-bin" && selected.has(r.id)),
    [rules, selected]
  );
  /// The rules Clean is about to delete that the current mode will destroy
  /// with no way back.
  const irreversibleRules = useMemo(
    () => rules.filter((r) => cleanIds.includes(r.id) && isIrreversible(r, mode)),
    [rules, cleanIds, mode]
  );

  /// Toggling changes what Clean will delete, not what has been measured: the
  /// results stand until the next Analyze. A confirmation raised on the old
  /// total no longer describes the new one, so it steps back.
  function toggleRule(id: string) {
    setSelected((prev) => {
      const next = new Set(prev);
      if (next.has(id)) next.delete(id);
      else next.add(id);
      return next;
    });
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
  /// would make the search field lie. Once a scan has run, the categories that
  /// hold reclaimable bytes open themselves and the empty ones step aside —
  /// biggest wins first, without the user hunting for them.
  function isOpen(category: string, catBytes: number): boolean {
    if (searching) return true;
    const manual = folds.get(category);
    if (manual !== undefined) return manual;
    if (results) return catBytes > 0;
    return !COLLAPSED_BY_DEFAULT.includes(category);
  }

  function toggleCategory(category: string, open: boolean) {
    setFolds((prev) => new Map(prev).set(category, !open));
  }

  function toggleSortBySize() {
    setSortBySize((prev) => {
      writeFlag(SORT_KEY, !prev);
      return !prev;
    });
  }

  function dismissHint() {
    writeFlag(HINT_KEY, true);
    setHintDismissed(true);
  }

  async function onScan() {
    setBusyAction("scan");
    setReport(null);
    setVerdict(null);
    setVerdictError(null);
    setConfirming(false);
    setProgress(null);
    setAnnouncement("Analyzing…");
    lastScanAnnounce.current = 0;
    scanPending.current = true;
    // A new scan re-decides which categories are worth showing: the previous
    // hand folds no longer describe these results.
    setFolds(new Map());
    try {
      const measured = await scan(availableRules.map((r) => r.id));
      setResults(measured);
      const checked = measured.filter((r) => selected.has(r.rule_id));
      setAnnouncement(
        `Analysis complete: ${formatBytes(
          checked.reduce((sum, r) => sum + r.total_bytes, 0)
        )} reclaimable in ${RULE_COUNT.format(checked.length)} selected ${
          checked.length === 1 ? "rule" : "rules"
        }`
      );
    } catch (err) {
      setRulesError(String(err));
    } finally {
      scanPending.current = false;
      setBusyAction(null);
    }
  }

  async function onClean() {
    setConfirming(false);
    setBusyAction("clean");
    setVerdict(null);
    setVerdictError(null);
    setCleanProgress(null);
    setAnnouncement("Cleaning…");
    lastCleanAnnounce.current = 0;
    cleanPending.current = true;
    // What was cleaned, captured before `results` is dropped: the verdict is
    // scoped to these rules, so it must be the list Clean was actually given.
    const cleaned = cleanIds;
    try {
      const done = await clean(cleaned, mode);
      setReport(done);
      setResults(null);
      setAnnouncement(
        `Cleanup complete: ${formatBytes(done.freed_bytes)} freed, ${formatCount(
          done.deleted
        )} files deleted`
      );
      toast.success(`Cleaned: ${formatBytes(done.freed_bytes)} freed`);
      // The whole point of the sandbox: read the disk back against what it
      // promised, instead of trusting the report the cleaner just wrote. In
      // its own try: a verify that fails says nothing about the clean that
      // succeeded, and must not replace the report with an error screen.
      if (sandbox) {
        try {
          setVerdict(await sandboxVerify(cleaned));
        } catch (err) {
          setVerdictError(String(err));
        }
      }
    } catch (err) {
      setRulesError(String(err));
    } finally {
      cleanPending.current = false;
      setBusyAction(null);
    }
  }

  if (rulesError) {
    return (
      <Screen>
        <div className="min-h-0 flex-1 overflow-auto px-8 pb-8">
          <div
            data-testid="rules-error"
            /// The screen has nothing else on it: the failure is the content,
            /// and it interrupts.
            role="alert"
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
        {!hintDismissed && (
          <div
            data-testid="first-launch-hint"
            className="flex items-center gap-2.5 rounded-lg border bg-card px-4 py-2.5 text-sm text-muted-foreground"
          >
            <Info className="size-4 shrink-0 text-muted-foreground" />
            <p className="min-w-0 flex-1">
              Auto mode deletes low-risk items permanently and sends the rest to
              the Recycle Bin. Change the mode in the bottom bar before cleaning.
            </p>
            <Button
              variant="ghost"
              size="sm"
              className="shrink-0 cursor-pointer"
              onClick={dismissHint}
            >
              Got it
            </Button>
          </div>
        )}

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

        {/* One live region for the whole screen, off to the side of the
            painting: the visible progress text repaints on every rule, and
            making that one live would read the same sentence hundreds of
            times. */}
        <p
          data-testid="scan-announcement"
          className="sr-only"
          role="status"
          aria-live="polite"
        >
          {announcement}
        </p>

        <section
          aria-labelledby={heroTitleId}
          className="flex flex-col gap-5 rounded-lg border bg-card p-5"
        >
          <div className="flex items-start justify-between gap-6">
            <div className="min-w-0">
              {/* Names the section without becoming a heading: the h2 level
                  belongs to the rule categories, and an extra one here would
                  put "Reclaimable" in the document outline above them. */}
              <p id={heroTitleId} className="eyebrow text-muted-foreground">
                {busyAction === "clean" ? "Freed" : "Reclaimable"}
              </p>
              {busyAction === "clean" ? (
                // What the clean has actually freed so far. A Recycle Bin
                // holding tens of thousands of files is minutes of work: the
                // number ticks up instead of standing at an em dash.
                <>
                  <p
                    data-testid="clean-progress-bytes"
                    className="mt-1.5 font-mono tnum text-[2rem] leading-none font-semibold"
                  >
                    {formatBytes(cleanProgress?.bytes_freed ?? 0)}
                  </p>
                  <p
                    data-testid="clean-progress-files"
                    className="mt-1.5 text-sm text-muted-foreground"
                  >
                    <span className="font-mono tnum">
                      {formatCount(cleanProgress?.files_deleted ?? 0)}
                    </span>{" "}
                    files deleted
                  </p>
                </>
              ) : busyAction === "scan" ? (
                // What has been measured so far, ticking up as the walk
                // progresses instead of an em dash held for thirty seconds.
                <p
                  data-testid="scan-progress-bytes"
                  className="mt-1.5 font-mono tnum text-[2rem] leading-none font-semibold"
                >
                  {formatBytes(progress?.total_bytes ?? 0)}
                </p>
              ) : results ? (
                <>
                  <p
                    data-testid="total-bytes"
                    className="mt-1.5 font-mono tnum text-[2rem] leading-none font-semibold"
                  >
                    {formatBytes(total)}
                  </p>
                  {uncheckedTotal > 0 && (
                    <p
                      data-testid="unchecked-bytes"
                      className="mt-1.5 text-sm text-muted-foreground"
                    >
                      <span className="font-mono tnum">
                        {formatBytes(uncheckedTotal)}
                      </span>{" "}
                      more in unchecked rules
                    </p>
                  )}
                </>
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
              <Button
                size="lg"
                onClick={onScan}
                aria-busy={busyAction === "scan"}
                disabled={busy || availableRules.length === 0}
              >
                {busyAction === "scan" ? (
                  <>
                    <Loader2 className="size-4 animate-spin" />
                    Analyzing…
                  </>
                ) : (
                  "Analyze"
                )}
              </Button>
            </div>
          </div>
          {busyAction ? (
            <div data-testid="hero-status" className="flex flex-col gap-2.5">
              <div className="h-2.5 w-full overflow-hidden rounded-[5px] bg-muted">
                <div
                  data-testid={
                    busyAction === "clean" ? "clean-progress-bar" : "scan-progress-bar"
                  }
                  className="h-full rounded-[5px]"
                  style={{
                    width: `${busyAction === "clean" ? cleanPercent : scanPercent}%`,
                    background: "var(--primary)",
                    transition: stillProgress ? "none" : "width 200ms linear",
                  }}
                />
              </div>
              <p className="text-sm text-muted-foreground">
                {busyAction === "clean" ? (
                  cleanProgress ? (
                    <span data-testid="clean-progress">
                      Cleaning{" "}
                      <span className="font-mono tnum">
                        {RULE_COUNT.format(cleanProgress.done_rules)} /{" "}
                        {RULE_COUNT.format(cleanProgress.total_rules)}
                      </span>{" "}
                      · {cleanProgress.label}
                    </span>
                  ) : (
                    // Between the click and the first event: the recycle bin
                    // goes first, and nothing has been deleted yet.
                    `Cleaning ${RULE_COUNT.format(cleanIds.length)} rules…`
                  )
                ) : progress ? (
                  <span data-testid="scan-progress">
                    Analyzing{" "}
                    <span className="font-mono tnum">
                      {RULE_COUNT.format(progress.done)} /{" "}
                      {RULE_COUNT.format(progress.total)}
                    </span>{" "}
                    · {progress.label}
                  </span>
                ) : (
                  // Between the click and the first event: the total is only
                  // known once Rust has counted the rules it was sent.
                  `Analyzing ${RULE_COUNT.format(availableRules.length)} rules…`
                )}
              </p>
            </div>
          ) : results ? (
            <ReclaimGauge rules={rules} results={checkedResults} />
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
            className="h-9 w-full rounded-md border bg-card px-3 text-sm outline-none focus-visible:ring-3 focus-visible:ring-ring"
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

        <div
          data-testid="rule-list"
          className={cn(
            "flex flex-col gap-6",
            busy && "pointer-events-none opacity-60"
          )}
        >
        {grouped.map(([category, catRules]) => {
          const catBytes = (results ?? [])
            .filter((r) => catRules.some((rule) => rule.id === r.rule_id))
            .reduce((sum, r) => sum + r.total_bytes, 0);
          const open = isOpen(category, catBytes);
          return (
            <RuleCategory
              key={category}
              category={category}
              rules={catRules}
              results={results}
              catBytes={catBytes}
              open={open}
              selected={selected}
              openPaths={openPaths}
              onToggleCategory={() => toggleCategory(category, open)}
              onToggleRule={toggleRule}
              onTogglePaths={togglePaths}
            />
          );
        })}
        </div>

        {verdict && <SandboxVerdictCard verdict={verdict} />}

        {verdictError && (
          <section
            data-testid="sandbox-verdict-error"
            role="status"
            className="rounded-lg border border-warning/40 bg-warning/12 p-5 text-sm"
          >
            <h2 className="eyebrow text-muted-foreground">Sandbox verdict</h2>
            <p className="mt-2">
              The cleanup ran; reading the sandbox back failed, so there is no
              verdict this time.
            </p>
            <p className="mt-1 font-mono text-xs text-muted-foreground">
              {verdictError}
            </p>
          </section>
        )}

        {report && (
          <section
            data-testid="clean-report"
            /// What the cleanup actually did, reported where the button was
            /// not: it is the answer to the click, so it is announced.
            role="status"
            aria-labelledby={reportTitleId}
            className="rounded-lg border bg-card p-5"
          >
            <h2 id={reportTitleId} className="eyebrow text-muted-foreground">
              Last cleanup
            </h2>
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
            <div
              data-testid="confirm-clean"
              ref={confirmRef}
              /// Focused as it appears: the last chance to step back is worth
              /// nothing if it is read out only to whoever happens to look at
              /// the bottom bar. `Escape` and Cancel both send the focus back.
              tabIndex={-1}
              role="group"
              aria-labelledby={confirmTitleId}
              aria-describedby={confirmDetailId}
              className="min-w-0 flex-1 outline-none focus-visible:ring-3 focus-visible:ring-ring"
            >
              <p id={confirmTitleId} className="text-sm font-medium">
                Clean{" "}
                <span className="font-mono tnum">{formatBytes(total)}</span> in{" "}
                {MODE_LABEL[mode]} mode?
              </p>
              <p id={confirmDetailId} className="mt-0.5 text-xs text-muted-foreground">
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
            <Button variant="outline" onClick={cancelConfirm}>
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
                className="h-8 shrink-0 rounded-md border bg-card px-2 text-sm outline-none focus-visible:ring-3 focus-visible:ring-ring"
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
              ref={cleanButtonRef}
              onClick={() => setConfirming(true)}
              aria-busy={busyAction === "clean"}
              disabled={busy || !results || cleanIds.length === 0}
            >
              {busyAction === "clean" ? (
                <>
                  <Loader2 className="size-4 animate-spin" />
                  Cleaning…
                </>
              ) : (
                <>
                  Clean
                  {results && total > 0 && (
                    <span className="font-mono tnum">{formatBytes(total)}</span>
                  )}
                </>
              )}
            </Button>
          </>
        )}
      </footer>
    </Screen>
  );
}
