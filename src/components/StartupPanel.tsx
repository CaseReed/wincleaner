import { useCallback, useEffect, useState } from "react";
import { toast } from "sonner";
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
      <div
        data-testid="startup-error"
        className="rounded-md border border-red-500 bg-red-50 p-4 text-red-900 dark:bg-red-950 dark:text-red-100"
      >
        {error}
      </div>
    );
  }

  if (entries.length === 0) {
    return (
      <p data-testid="startup-empty" className="text-sm text-muted-foreground">
        Aucun programme n'est configuré pour démarrer avec votre session.
      </p>
    );
  }

  return (
    <Table>
      <TableHeader>
        <TableRow>
          <TableHead>Nom</TableHead>
          <TableHead>Commande</TableHead>
          <TableHead>Source</TableHead>
          <TableHead className="w-24 text-right">Activé</TableHead>
        </TableRow>
      </TableHeader>
      <TableBody>
        {entries.map((entry) => (
          <TableRow key={entry.id} data-testid={`startup-row-${entry.id}`}>
            <TableCell className="font-medium">{entry.name}</TableCell>
            <TableCell className="max-w-md truncate font-mono text-xs" title={entry.command}>
              {entry.command}
            </TableCell>
            <TableCell className="text-sm">{SOURCE_LABEL[entry.source]}</TableCell>
            <TableCell className="text-right">
              <Switch
                aria-label={`Activer ${entry.name}`}
                checked={entry.enabled}
                disabled={entry.source === "run-once" || pending === entry.id}
                onCheckedChange={(next) => void onToggle(entry, next)}
              />
            </TableCell>
          </TableRow>
        ))}
      </TableBody>
    </Table>
  );
}
