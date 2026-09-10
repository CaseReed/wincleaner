import { useCallback, useEffect, useState } from "react";
import { toast } from "sonner";
import { Badge } from "@/components/ui/badge";
import { Switch } from "@/components/ui/switch";
import {
  Table,
  TableBody,
  TableCell,
  TableHead,
  TableHeader,
  TableRow,
} from "@/components/ui/table";
import {
  listStartup,
  setStartupEnabled,
  type StartupEntry,
  type StartupSource,
} from "@/lib/api";

const SOURCE_LABEL: Record<StartupSource, string> = {
  run: "Registre (Run)",
  "run-once": "Registre (RunOnce)",
  folder: "Dossier Démarrage",
};

function Screen({
  subtitle,
  children,
}: {
  subtitle: string;
  children: React.ReactNode;
}) {
  return (
    <>
      <header className="shrink-0 px-8 pt-7 pb-5">
        <h1 className="screen-title">Démarrage</h1>
        <p className="mt-1 text-sm text-muted-foreground">{subtitle}</p>
        {/* Le périmètre MVP est un sous-ensemble de l'onglet Démarrage du
            Gestionnaire des tâches : le dire évite de faire chercher une
            entrée qui ne peut pas y être. */}
        <p data-testid="startup-scope" className="mt-2 text-xs text-muted-foreground">
          Seules les entrées de votre session sont listées : registre HKCU (Run,
          RunOnce) et votre dossier Démarrage. Les entrées communes à tous les
          utilisateurs et les tâches planifiées demandent une élévation et
          restent hors périmètre.
        </p>
      </header>
      <div className="min-h-0 flex-1 overflow-auto px-8 pb-8">{children}</div>
    </>
  );
}

export function StartupPanel() {
  const [entries, setEntries] = useState<StartupEntry[]>([]);
  const [error, setError] = useState<string | null>(null);
  const [pending, setPending] = useState<string | null>(null);

  const refresh = useCallback(async () => {
    try {
      setEntries(await listStartup());
      setError(null);
    } catch (err) {
      setError(String(err));
    }
  }, []);

  useEffect(() => {
    void refresh();
  }, [refresh]);

  async function onToggle(entry: StartupEntry, next: boolean) {
    setPending(entry.id);
    // Optimiste : remis en place par le refresh, ou par le catch si échec.
    setEntries((prev) =>
      prev.map((e) => (e.id === entry.id ? { ...e, enabled: next } : e))
    );
    try {
      await setStartupEnabled(entry.id, next);
      await refresh();
      toast.success(
        next ? `${entry.name} activé au démarrage` : `${entry.name} désactivé au démarrage`
      );
    } catch (err) {
      setEntries((prev) =>
        prev.map((e) => (e.id === entry.id ? { ...e, enabled: entry.enabled } : e))
      );
      toast.error(String(err));
    } finally {
      setPending(null);
    }
  }

  if (error) {
    return (
      <Screen subtitle="Décidez ce qui se lance avec votre session.">
        <div
          data-testid="startup-error"
          className="flex max-w-xl flex-col gap-2 rounded-lg border border-destructive/40 bg-destructive/8 p-5"
        >
          <p className="font-medium">Impossible de lire les programmes au démarrage.</p>
          <p className="font-mono text-xs text-muted-foreground">{error}</p>
        </div>
      </Screen>
    );
  }

  if (entries.length === 0) {
    return (
      <Screen subtitle="Décidez ce qui se lance avec votre session.">
        <p data-testid="startup-empty" className="text-sm text-muted-foreground">
          Aucun programme ne démarre avec votre session.
        </p>
      </Screen>
    );
  }

  const enabled = entries.filter((e) => e.enabled).length;

  return (
    <Screen
      subtitle={
        entries.length > 1
          ? `${entries.length} programmes, ${enabled} activés`
          : `1 programme, ${enabled} activé`
      }
    >
      <div className="overflow-hidden rounded-lg border bg-card">
        {/* table-fixed : les commandes Windows sont longues, sans quoi elles
            poussent la source et l'interrupteur hors de la fenêtre. */}
        <Table className="table-fixed">
          <TableHeader>
            <TableRow className="hover:bg-transparent">
              <TableHead className="eyebrow w-[30%] px-4 text-muted-foreground">
                Nom
              </TableHead>
              <TableHead className="eyebrow px-4 text-muted-foreground">Commande</TableHead>
              <TableHead className="eyebrow w-[9.5rem] px-4 text-muted-foreground">
                Source
              </TableHead>
              <TableHead className="eyebrow w-20 px-4 text-right text-muted-foreground">
                Activé
              </TableHead>
            </TableRow>
          </TableHeader>
          <TableBody>
            {entries.map((entry) => {
              const readOnly = entry.source === "run-once";
              return (
                <TableRow
                  key={entry.id}
                  data-testid={`startup-row-${entry.id}`}
                  title={readOnly ? "Lecture seule" : undefined}
                  className={readOnly ? "text-muted-foreground" : undefined}
                >
                  <TableCell className="h-11 px-4 text-sm font-medium">
                    <div className="truncate" title={entry.name}>
                      {entry.name}
                    </div>
                  </TableCell>
                  <TableCell className="h-11 px-4 font-mono text-xs text-muted-foreground">
                    <div className="truncate" title={entry.command}>
                      {entry.command}
                    </div>
                  </TableCell>
                  <TableCell className="h-11 px-4">
                    <Badge variant="outline" className="max-w-full text-muted-foreground">
                      <span className="truncate">{SOURCE_LABEL[entry.source]}</span>
                    </Badge>
                  </TableCell>
                  <TableCell className="h-11 px-4 text-right">
                    <Switch
                      aria-label={`Activer ${entry.name}`}
                      checked={entry.enabled}
                      disabled={readOnly || pending === entry.id}
                      onCheckedChange={(next) => void onToggle(entry, next)}
                    />
                  </TableCell>
                </TableRow>
              );
            })}
          </TableBody>
        </Table>
      </div>
    </Screen>
  );
}
