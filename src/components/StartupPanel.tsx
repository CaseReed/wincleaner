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
  run: "Registry (Run)",
  "run-once": "Registry (RunOnce)",
  folder: "Startup folder",
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
        <h1 className="screen-title">Startup</h1>
        <p className="mt-1 text-sm text-muted-foreground">{subtitle}</p>
        {/* The MVP scope is a subset of the Task Manager Startup tab: saying
            so avoids sending the user hunting for an entry that cannot be
            there. */}
        <p data-testid="startup-scope" className="mt-2 text-xs text-muted-foreground">
          Only the entries of your own session are listed: the HKCU registry
          (Run, RunOnce) and your Startup folder. Entries shared by all users
          and scheduled tasks require elevation and stay out of scope.
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
    // Optimistic: put back by the refresh, or by the catch on failure.
    setEntries((prev) =>
      prev.map((e) => (e.id === entry.id ? { ...e, enabled: next } : e))
    );
    try {
      await setStartupEnabled(entry.id, next);
      await refresh();
      toast.success(
        next ? `${entry.name} enabled at startup` : `${entry.name} disabled at startup`
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
      <Screen subtitle="Decide what starts with your session.">
        <div
          data-testid="startup-error"
          className="flex max-w-xl flex-col gap-2 rounded-lg border border-destructive/40 bg-destructive/8 p-5"
        >
          <p className="font-medium">Could not read the startup programs.</p>
          <p className="font-mono text-xs text-muted-foreground">{error}</p>
        </div>
      </Screen>
    );
  }

  if (entries.length === 0) {
    return (
      <Screen subtitle="Decide what starts with your session.">
        <p data-testid="startup-empty" className="text-sm text-muted-foreground">
          No program starts with your session.
        </p>
      </Screen>
    );
  }

  const enabled = entries.filter((e) => e.enabled).length;

  return (
    <Screen
      subtitle={
        entries.length > 1
          ? `${entries.length} programs, ${enabled} enabled`
          : `1 program, ${enabled} enabled`
      }
    >
      <div className="overflow-hidden rounded-lg border bg-card">
        {/* table-fixed: Windows commands are long, and without this they push
            the source and the switch out of the window. */}
        <Table className="table-fixed">
          <TableHeader>
            <TableRow className="hover:bg-transparent">
              <TableHead className="eyebrow w-[30%] px-4 text-muted-foreground">
                Name
              </TableHead>
              <TableHead className="eyebrow px-4 text-muted-foreground">Command</TableHead>
              <TableHead className="eyebrow w-[9.5rem] px-4 text-muted-foreground">
                Source
              </TableHead>
              <TableHead className="eyebrow w-20 px-4 text-right text-muted-foreground">
                Enabled
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
                  title={readOnly ? "Read-only" : undefined}
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
                      aria-label={`Enable ${entry.name}`}
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
