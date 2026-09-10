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
  groupByCategory,
  listRules,
  runningBrowsers,
  scan,
  type CleanMode,
  type CleanReport,
  type RuleSummary,
  type ScanResult,
} from "@/lib/api";

const LIBELLE_MODE: Record<CleanMode, string> = {
  auto: "Auto",
  trash: "Corbeille",
  permanent: "Définitif",
};

/// Ce que ce mode détruira sans retour possible pour cette règle. La règle
/// Corbeille l'est toujours : le mode de suppression ne s'y applique pas.
export function estIrreversible(rule: RuleSummary, mode: CleanMode): boolean {
  if (rule.kind === "recycle-bin") return true;
  if (mode === "permanent") return true;
  return mode === "auto" && rule.risk === "low";
}

function Screen({ children }: { children: React.ReactNode }) {
  return (
    <>
      <header className="shrink-0 px-8 pt-7 pb-5">
        <h1 className="screen-title">Nettoyage</h1>
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
  }, [reloadKey]);

  const grouped = useMemo(() => groupByCategory(rules), [rules]);
  const total = useMemo(
    () => (results ?? []).reduce((sum, r) => sum + r.total_bytes, 0),
    [results]
  );
  const scannedIds = useMemo(() => (results ?? []).map((r) => r.rule_id), [results]);
  /// Les règles analysées que le mode courant détruira sans retour possible.
  const irreversibles = useMemo(
    () =>
      rules.filter((r) => scannedIds.includes(r.id) && estIrreversible(r, mode)),
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
      toast.success(`Nettoyé : ${formatBytes(done.freed_bytes)} libérés`);
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
            <p className="font-medium">Impossible de charger les règles.</p>
            <p className="font-mono text-xs text-muted-foreground">{rulesError}</p>
            <Button
              variant="outline"
              onClick={() => {
                setResults(null);
                setReport(null);
                setReloadKey((k) => k + 1);
              }}
            >
              Réessayer
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
              <span className="font-mono">{browsers.join(", ")}</span> est ouvert :
              ses fichiers en cours d'utilisation seront ignorés.
            </p>
          </div>
        )}

        <section className="flex flex-col gap-5 rounded-lg border bg-card p-5">
          <div className="flex items-start justify-between gap-6">
            <div className="min-w-0">
              <p className="eyebrow text-muted-foreground">Récupérable</p>
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
            <Button size="lg" onClick={onScan} disabled={busy || selected.size === 0}>
              Analyser
            </Button>
          </div>
          {results ? (
            <ReclaimGauge rules={rules} results={results} />
          ) : (
            <p className="text-sm text-muted-foreground">
              Analysez pour mesurer ce qui peut être libéré.
            </p>
          )}
        </section>

        {grouped.map(([category, catRules]) => {
          const catBytes = (results ?? [])
            .filter((r) => catRules.some((rule) => rule.id === r.rule_id))
            .reduce((sum, r) => sum + r.total_bytes, 0);
          return (
            <section key={category} className="flex flex-col gap-1.5">
              <div className="flex items-baseline justify-between px-1">
                <h2 className="eyebrow text-muted-foreground">{category}</h2>
                <p className="text-xs text-muted-foreground">
                  {catRules.length} règles
                  {results && (
                    <>
                      {" · "}
                      <span className="font-mono tnum">{formatBytes(catBytes)}</span>
                    </>
                  )}
                </p>
              </div>
              <ul className="overflow-hidden rounded-lg border bg-card">
                {catRules.map((rule, index) => {
                  const result = (results ?? []).find((r) => r.rule_id === rule.id);
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
                          onCheckedChange={() => toggleRule(rule.id)}
                        />
                        <span
                          className="min-w-0 flex-1 cursor-pointer truncate text-sm"
                          onClick={() => toggleRule(rule.id)}
                        >
                          {rule.label}
                        </span>
                        {rule.risk === "medium" && (
                          <Badge
                            variant="outline"
                            className="border-warning/40 bg-warning/12 text-warning-foreground"
                          >
                            risque moyen
                          </Badge>
                        )}
                        {rule.kind === "recycle-bin" && (
                          <Badge
                            data-testid={`note-${rule.id}`}
                            variant="outline"
                            className="border-destructive/40 text-destructive"
                          >
                            tous les volumes
                          </Badge>
                        )}
                        {result && (
                          <div
                            data-testid={`result-${rule.id}`}
                            className="flex shrink-0 items-baseline gap-3"
                          >
                            {result.skipped > 0 && (
                              <span className="font-mono tnum text-xs text-muted-foreground">
                                {formatCount(result.skipped)} ignoré
                                {result.skipped > 1 ? "s" : ""}
                              </span>
                            )}
                            <span className="w-24 text-right font-mono tnum text-sm">
                              {formatBytes(result.total_bytes)}
                            </span>
                            <span className="w-20 text-right font-mono tnum text-sm text-muted-foreground">
                              {formatCount(result.file_count)}
                              <span className="sr-only"> fichiers</span>
                            </span>
                          </div>
                        )}
                      </div>
                      {rule.kind === "recycle-bin" && (
                        <p className="px-4 pb-3 pl-11 text-xs text-muted-foreground">
                          Vide la corbeille de tous les volumes du poste, y compris
                          hors du profil utilisateur. Suppression définitive et
                          irréversible : le mode de suppression ne s'y applique pas.
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
                            {openPaths.has(rule.id) ? "Masquer" : "Afficher"} les chemins
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
            </section>
          );
        })}

        {report && (
          <section
            data-testid="clean-report"
            className="rounded-lg border bg-card p-5"
          >
            <h2 className="eyebrow text-muted-foreground">Dernier nettoyage</h2>
            <p className="mt-2 text-sm">
              <span className="font-mono tnum">{formatBytes(report.freed_bytes)}</span>{" "}
              libérés ·{" "}
              <span className="font-mono tnum">{formatCount(report.deleted)}</span>{" "}
              fichiers
              supprimés
            </p>
            {report.skipped.length > 0 && (
              <>
                <h3 className="mt-4 text-xs font-medium text-muted-foreground">
                  Ignorés (<span className="font-mono tnum">{formatCount(report.skipped.length)}</span>)
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
                Nettoyer{" "}
                <span className="font-mono tnum">{formatBytes(total)}</span> en
                mode {LIBELLE_MODE[mode]} ?
              </p>
              <p className="mt-0.5 text-xs text-muted-foreground">
                {irreversibles.length > 0 ? (
                  <>
                    Sans retour possible :{" "}
                    {irreversibles.map((r) => r.label).join(", ")}.
                  </>
                ) : (
                  <>Tout part à la corbeille et reste récupérable.</>
                )}
              </p>
            </div>
            <Button variant="outline" onClick={() => setConfirming(false)}>
              Annuler
            </Button>
            <Button size="lg" variant="destructive" onClick={onClean} disabled={busy}>
              Confirmer le nettoyage
            </Button>
          </>
        ) : (
          <>
            <div className="flex min-w-0 flex-1 items-center gap-3">
              <select
                id="clean-mode"
                aria-label="Mode de suppression"
                className="h-8 shrink-0 rounded-md border bg-card px-2 text-sm outline-none focus-visible:ring-3 focus-visible:ring-ring/50"
                value={mode}
                onChange={(e) => setMode(e.target.value as CleanMode)}
              >
                <option value="auto">Auto</option>
                <option value="trash">Corbeille</option>
                <option value="permanent">Définitif</option>
              </select>
              <p data-testid="mode-help" className="min-w-0 text-xs text-muted-foreground">
                Auto : suppression définitive pour les éléments à faible risque,
                corbeille pour les autres.
              </p>
            </div>
            <Button
              size="lg"
              variant="destructive"
              onClick={() => setConfirming(true)}
              disabled={busy || !results || scannedIds.length === 0}
            >
              Nettoyer
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
