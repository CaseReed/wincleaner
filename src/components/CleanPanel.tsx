import { useEffect, useMemo, useState } from "react";
import { Button } from "@/components/ui/button";
import { Checkbox } from "@/components/ui/checkbox";
import { Badge } from "@/components/ui/badge";
import {
  Collapsible,
  CollapsibleContent,
  CollapsibleTrigger,
} from "@/components/ui/collapsible";
import { formatBytes } from "@/lib/format";
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

  const [reloadKey, setReloadKey] = useState(0);

  useEffect(() => {
    setRulesError(null);
    listRules()
      .then((loaded) => {
        setRules(loaded);
        setSelected(new Set(loaded.map((r) => r.id)));
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

  function toggleRule(id: string) {
    setSelected((prev) => {
      const next = new Set(prev);
      if (next.has(id)) next.delete(id);
      else next.add(id);
      return next;
    });
    setResults(null);
    setReport(null);
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
    try {
      setResults(await scan(rules.filter((r) => selected.has(r.id)).map((r) => r.id)));
    } catch (err) {
      setRulesError(String(err));
    } finally {
      setBusy(false);
    }
  }

  async function onClean() {
    setBusy(true);
    try {
      setReport(await clean(scannedIds, mode));
      setResults(null);
    } catch (err) {
      setRulesError(String(err));
    } finally {
      setBusy(false);
    }
  }

  if (rulesError) {
    return (
      <div
        data-testid="rules-error"
        className="flex flex-col items-start gap-3 rounded-md border border-red-500 bg-red-50 p-4 text-red-900 dark:bg-red-950 dark:text-red-100"
      >
        <p>{rulesError}</p>
        <Button
          variant="secondary"
          onClick={() => {
            setResults(null);
            setReport(null);
            setReloadKey((k) => k + 1);
          }}
        >
          Réessayer
        </Button>
      </div>
    );
  }

  return (
    <div className="flex flex-col gap-6">
      {browsers.length > 0 && (
        <div
          data-testid="browser-warning"
          className="rounded-md border border-amber-500 bg-amber-50 p-3 text-sm text-amber-900 dark:bg-amber-950 dark:text-amber-100"
        >
          Navigateur ouvert : {browsers.join(", ")}. Les fichiers en cours
          d'utilisation seront ignorés. Fermez-le pour un nettoyage complet.
        </div>
      )}

      {results && (
        <div className="flex items-baseline gap-2">
          <span className="text-sm text-muted-foreground">Total récupérable</span>
          <span data-testid="total-bytes" className="text-2xl font-semibold">
            {formatBytes(total)}
          </span>
        </div>
      )}

      {grouped.map(([category, catRules]) => (
        <section key={category} className="flex flex-col gap-2">
          <h2 className="text-sm font-semibold uppercase tracking-wide text-muted-foreground">
            {category}
          </h2>
          <ul className="flex flex-col gap-1">
            {catRules.map((rule) => {
              const result = (results ?? []).find((r) => r.rule_id === rule.id);
              return (
                <li key={rule.id} className="rounded-md border p-3">
                  <div className="flex items-center gap-3">
                    <Checkbox
                      id={rule.id}
                      aria-label={rule.label}
                      checked={selected.has(rule.id)}
                      onCheckedChange={() => toggleRule(rule.id)}
                    />
                    <span
                      className="flex-1 cursor-pointer"
                      onClick={() => toggleRule(rule.id)}
                    >
                      {rule.label}
                    </span>
                    {rule.risk === "medium" && <Badge variant="secondary">risque moyen</Badge>}
                    {rule.kind === "recycle-bin" && (
                      <Badge data-testid={`note-${rule.id}`} variant="destructive">
                        tous les volumes · définitif
                      </Badge>
                    )}
                    {result && (
                      <span data-testid={`result-${rule.id}`} className="text-sm">
                        {formatBytes(result.total_bytes)} · {result.file_count} fichiers
                        {result.skipped > 0 && ` · ${result.skipped} ignorés`}
                      </span>
                    )}
                  </div>
                  {rule.kind === "recycle-bin" && (
                    <p className="mt-2 text-xs text-muted-foreground">
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
                        className="mt-2 text-xs underline text-muted-foreground"
                      >
                        {openPaths.has(rule.id) ? "Masquer" : "Afficher"} les chemins
                      </CollapsibleTrigger>
                      <CollapsibleContent>
                        <ul className="mt-2 max-h-48 overflow-auto text-xs font-mono">
                          {result.paths.map((p) => (
                            <li key={p}>{p}</li>
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
      ))}

      <div className="flex flex-col gap-2">
        <div className="flex items-center gap-3">
          <Button onClick={onScan} disabled={busy || selected.size === 0}>
            Analyser
          </Button>
          <label htmlFor="clean-mode" className="text-sm">
            Mode de suppression
          </label>
          <select
            id="clean-mode"
            aria-label="Mode de suppression"
            className="rounded-md border bg-transparent px-2 py-1 text-sm"
            value={mode}
            onChange={(e) => setMode(e.target.value as CleanMode)}
          >
            <option value="auto">Auto</option>
            <option value="trash">Corbeille</option>
            <option value="permanent">Définitif</option>
          </select>
          <Button
            variant="destructive"
            onClick={onClean}
            disabled={busy || !results || scannedIds.length === 0}
          >
            Nettoyer
          </Button>
        </div>
        <p data-testid="mode-help" className="text-xs text-muted-foreground">
          Auto : suppression définitive pour les éléments à faible risque,
          corbeille pour les autres.
        </p>
      </div>

      {report && (
        <div data-testid="clean-report" className="rounded-md border p-4">
          <h3 className="mb-2 font-semibold">Rapport</h3>
          <p>
            {formatBytes(report.freed_bytes)} libérés · {report.deleted} fichiers supprimés
          </p>
          {report.skipped.length > 0 && (
            <>
              <h4 className="mt-3 text-sm font-semibold">
                Ignorés ({report.skipped.length})
              </h4>
              <ul className="mt-1 max-h-48 overflow-auto text-xs font-mono">
                {report.skipped.map((s) => (
                  <li key={s.path}>
                    {s.path} — {s.reason}
                  </li>
                ))}
              </ul>
            </>
          )}
        </div>
      )}
    </div>
  );
}
