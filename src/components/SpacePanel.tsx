import { useEffect, useRef, useState } from "react";
import { FolderOpen, Loader2 } from "lucide-react";
import { toast } from "sonner";
import { Button } from "@/components/ui/button";
import {
  Table,
  TableBody,
  TableCaption,
  TableCell,
  TableHead,
  TableHeader,
  TableRow,
} from "@/components/ui/table";
import { prefersReducedMotion } from "@/components/ReclaimGauge";
import { useI18n, type TranslationKey } from "@/i18n";
import { formatDate } from "@/lib/format";
import {
  onSpaceProgress,
  spaceReveal,
  spaceScan,
  type RevealKind,
  type SpaceProgress,
  type SpaceResult,
  type SpaceRoot,
  type SandboxSummary,
} from "@/lib/api";

/// The stable root ids the back end sends (`src-tauri/src/space.rs`), in the
/// order it measures them. A root missing from this map would render its id,
/// which is why the mapping lives here and not in a template string.
const ROOT_LABEL: Record<string, TranslationKey> = {
  downloads: "space.root.downloads",
  desktop: "space.root.desktop",
  documents: "space.root.documents",
  pictures: "space.root.pictures",
  videos: "space.root.videos",
  music: "space.root.music",
};

/// The last segment of a path: what the reveal button's accessible name says,
/// because "Reveal in Explorer" repeated a hundred times names nothing.
export function baseName(path: string): string {
  const trimmed = path.replace(/[\\/]+$/, "");
  const cut = Math.max(trimmed.lastIndexOf("\\"), trimmed.lastIndexOf("/"));
  return cut === -1 ? trimmed : trimmed.slice(cut + 1);
}

function Screen({ children }: { children: React.ReactNode }) {
  const { t } = useI18n();
  return (
    <>
      <header className="shrink-0 px-8 pt-7 pb-5">
        <h1 className="screen-title">{t("space.title")}</h1>
        <p className="mt-1 text-sm text-muted-foreground">{t("space.subtitle")}</p>
      </header>
      {children}
    </>
  );
}

/// One known folder, as a bar of the breakdown. The width is relative to the
/// largest root, not to the total: six roots where one holds 95% would
/// otherwise be five invisible lines.
function RootBar({
  root,
  largest,
  still,
}: {
  root: SpaceRoot;
  largest: number;
  still: boolean;
}) {
  const { t, tn, formatBytes } = useI18n();
  const label = root.name in ROOT_LABEL ? t(ROOT_LABEL[root.name]) : root.name;
  return (
    <li data-testid={`space-root-${root.name}`} className="flex flex-col gap-1.5">
      <div className="flex items-baseline justify-between gap-4 text-sm">
        <span className="min-w-0 truncate" title={root.path}>
          {label}
        </span>
        <span className="shrink-0 text-muted-foreground">
          <span className="font-mono tnum text-foreground">{formatBytes(root.bytes)}</span>{" "}
          <span className="text-xs">{tn("space.rootFiles", root.files)}</span>
        </span>
      </div>
      <div className="h-2.5 w-full overflow-hidden rounded-[5px] bg-muted">
        <div
          data-testid="space-root-bar"
          className="h-full rounded-[5px]"
          style={{
            width: largest > 0 ? `${(root.bytes / largest) * 100}%` : "0%",
            background: "var(--chart-1)",
            transition: still ? "none" : "width 500ms ease-out",
          }}
        />
      </div>
    </li>
  );
}

function RevealButton({ kind, index, path }: { kind: RevealKind; index: number; path: string }) {
  const { t } = useI18n();
  return (
    <Button
      variant="ghost"
      size="sm"
      data-testid={`space-reveal-${kind}-${index}`}
      aria-label={t("space.revealNamed", { name: baseName(path) })}
      title={t("space.reveal")}
      onClick={() => {
        // The row is stale the moment the file moves: the back end says so,
        // and the toast is where that answer belongs.
        void spaceReveal(kind, index).catch((err) => toast.error(String(err)));
      }}
    >
      <FolderOpen className="size-4" aria-hidden="true" />
      <span className="sr-only">{t("space.reveal")}</span>
    </Button>
  );
}

export function SpacePanel({
  sandbox = null,
}: {
  /// Non-null while a sandbox is active. The back end refuses `space_scan`
  /// then — the known folders it measures are the user's real ones, which no
  /// sandbox stands in for — so the screen states that instead of asking.
  sandbox?: SandboxSummary | null;
} = {}) {
  const { t, tn, tx, formatBytes, formatCount, locale } = useI18n();
  const [result, setResult] = useState<SpaceResult | null>(null);
  const [progress, setProgress] = useState<SpaceProgress | null>(null);
  const [busy, setBusy] = useState(false);
  const [error, setError] = useState<string | null>(null);
  /// One live region for the screen: the visible progress text repaints on
  /// every root, and making that live would read it six times over.
  const [announcement, setAnnouncement] = useState("");
  /// Events keep arriving for a moment after the command resolves; only a
  /// measurement this panel started may move the bar.
  const pending = useRef(false);
  const inSandbox = sandbox !== null;

  useEffect(() => {
    let stop: (() => void) | null = null;
    let gone = false;
    onSpaceProgress((step) => {
      if (!pending.current) return;
      setProgress(step);
      setAnnouncement(
        t("announce.spaceProgress", {
          done: formatCount(step.done),
          total: formatCount(step.total),
          bytes: formatBytes(step.total_bytes),
        }),
      );
    })
      .then((unlisten) => {
        if (gone) unlisten();
        else stop = unlisten;
      })
      .catch(() => {
        // No progress feedback, but Measure itself still works.
      });
    return () => {
      gone = true;
      stop?.();
    };
  }, [t, formatCount, formatBytes]);

  if (inSandbox) {
    return (
      <Screen>
        <div className="min-h-0 flex-1 overflow-auto px-8 pb-8">
          <div
            data-testid="space-sandbox-notice"
            className="flex max-w-xl flex-col gap-2 rounded-lg border bg-card p-5"
          >
            <p className="font-medium">{t("space.sandboxTitle")}</p>
            <p className="text-sm text-muted-foreground">{t("space.sandboxBody")}</p>
          </div>
        </div>
      </Screen>
    );
  }

  async function onMeasure() {
    setBusy(true);
    setError(null);
    setProgress(null);
    pending.current = true;
    setAnnouncement(t("announce.measuring"));
    try {
      const measured = await spaceScan();
      setResult(measured);
      setAnnouncement(
        t("announce.spaceDone", {
          bytes: formatBytes(measured.roots.reduce((sum, r) => sum + r.bytes, 0)),
          count: formatCount(measured.roots.length),
        }),
      );
    } catch (err) {
      setError(String(err));
      setAnnouncement(t("space.error"));
    } finally {
      pending.current = false;
      setBusy(false);
    }
  }

  const total = result ? result.roots.reduce((sum, r) => sum + r.bytes, 0) : 0;
  const largest = result ? Math.max(0, ...result.roots.map((r) => r.bytes)) : 0;
  const percent = progress && progress.total > 0 ? (progress.done / progress.total) * 100 : 0;
  const still = prefersReducedMotion();

  return (
    <Screen>
      <div className="flex min-h-0 flex-1 flex-col gap-6 overflow-auto px-8 pb-8">
        <p data-testid="space-announcement" className="sr-only" role="status" aria-live="polite">
          {announcement}
        </p>

        <section className="flex flex-col gap-5 rounded-lg border bg-card p-5">
          <div className="flex items-start justify-between gap-6">
            <div className="min-w-0">
              <p className="eyebrow text-muted-foreground">{t("space.total")}</p>
              <p
                data-testid="space-total"
                className="mt-1 font-mono tnum text-3xl font-semibold tracking-tight"
              >
                {formatBytes(busy ? (progress?.total_bytes ?? 0) : total)}
              </p>
            </div>
            <Button size="lg" onClick={() => void onMeasure()} aria-busy={busy} disabled={busy}>
              {busy ? (
                <>
                  <Loader2 className="size-4 animate-spin" />
                  {t("space.measuring")}
                </>
              ) : (
                t("space.measure")
              )}
            </Button>
          </div>

          {busy ? (
            <div data-testid="space-status" className="flex flex-col gap-2.5">
              <div className="h-2.5 w-full overflow-hidden rounded-[5px] bg-muted">
                <div
                  data-testid="space-progress-bar"
                  className="h-full rounded-[5px]"
                  style={{
                    width: `${percent}%`,
                    background: "var(--primary)",
                    transition: still ? "none" : "width 200ms linear",
                  }}
                />
              </div>
              <p className="text-sm text-muted-foreground">
                {progress ? (
                  <span data-testid="space-progress">
                    {tx("space.progress", {
                      counter: (
                        <span className="font-mono tnum">
                          {formatCount(progress.done)} / {formatCount(progress.total)}
                        </span>
                      ),
                      label:
                        progress.root in ROOT_LABEL
                          ? t(ROOT_LABEL[progress.root])
                          : progress.root,
                    })}
                  </span>
                ) : (
                  // Between the click and the first event: the roots are only
                  // known once the shell has resolved them.
                  t("space.measuring")
                )}
              </p>
            </div>
          ) : result ? (
            <ul className="flex flex-col gap-3">
              {result.roots.map((root) => (
                <RootBar key={root.name} root={root} largest={largest} still={still} />
              ))}
            </ul>
          ) : (
            <p className="text-sm text-muted-foreground">{t("space.empty")}</p>
          )}
        </section>

        {error && (
          <div
            data-testid="space-error"
            role="alert"
            className="flex max-w-xl flex-col gap-2 rounded-lg border border-destructive/40 bg-destructive/8 p-5"
          >
            <p className="font-medium">{t("space.error")}</p>
            <p className="font-mono text-xs text-muted-foreground">{error}</p>
          </div>
        )}

        {result && result.skipped_roots.length > 0 && (
          <p data-testid="space-skipped-roots" className="text-sm text-warning-foreground">
            {t("space.skippedRoots", {
              names: result.skipped_roots
                .map((name) => (name in ROOT_LABEL ? t(ROOT_LABEL[name]) : name))
                .join(", "),
            })}
          </p>
        )}

        {result && result.skipped_files > 0 && (
          <p data-testid="space-skipped-files" className="text-xs text-muted-foreground">
            {tn("space.skippedFiles", result.skipped_files)}
          </p>
        )}

        {result && result.files.length > 0 && (
          <section data-testid="space-files" className="flex flex-col gap-3">
            <h2 className="text-base font-semibold tracking-tight">{t("space.files")}</h2>
            <div className="overflow-hidden rounded-lg border bg-card">
              <Table className="table-fixed">
                <TableCaption className="sr-only">{t("space.filesCaption")}</TableCaption>
                <TableHeader>
                  <TableRow className="hover:bg-transparent">
                    <TableHead className="eyebrow px-4 text-muted-foreground">
                      {t("space.colPath")}
                    </TableHead>
                    <TableHead className="eyebrow w-28 px-4 text-right text-muted-foreground">
                      {t("space.colSize")}
                    </TableHead>
                    <TableHead className="eyebrow w-32 px-4 text-muted-foreground">
                      {t("space.colModified")}
                    </TableHead>
                    <TableHead className="eyebrow w-20 px-4 text-right text-muted-foreground">
                      {t("space.colReveal")}
                    </TableHead>
                  </TableRow>
                </TableHeader>
                <TableBody>
                  {result.files.map((file) => (
                    <TableRow key={file.index} data-testid={`space-file-${file.index}`}>
                      <TableCell className="h-11 px-4 font-mono text-xs text-muted-foreground">
                        <div className="truncate" title={file.path}>
                          {file.path}
                        </div>
                      </TableCell>
                      <TableCell className="h-11 px-4 text-right font-mono tnum text-sm">
                        {formatBytes(file.bytes)}
                      </TableCell>
                      <TableCell className="h-11 px-4 text-sm text-muted-foreground">
                        {file.modified === null
                          ? t("space.unknownDate")
                          : formatDate(file.modified, locale)}
                      </TableCell>
                      <TableCell className="h-11 px-4 text-right">
                        <RevealButton kind="file" index={file.index} path={file.path} />
                      </TableCell>
                    </TableRow>
                  ))}
                </TableBody>
              </Table>
            </div>
          </section>
        )}

        {result && result.folders.length > 0 && (
          <section data-testid="space-folders" className="flex flex-col gap-3">
            <h2 className="text-base font-semibold tracking-tight">{t("space.folders")}</h2>
            <div className="overflow-hidden rounded-lg border bg-card">
              <Table className="table-fixed">
                <TableCaption className="sr-only">{t("space.foldersCaption")}</TableCaption>
                <TableHeader>
                  <TableRow className="hover:bg-transparent">
                    <TableHead className="eyebrow px-4 text-muted-foreground">
                      {t("space.colPath")}
                    </TableHead>
                    <TableHead className="eyebrow w-28 px-4 text-right text-muted-foreground">
                      {t("space.colSize")}
                    </TableHead>
                    <TableHead className="eyebrow w-32 px-4 text-muted-foreground">
                      {t("space.colContents")}
                    </TableHead>
                    <TableHead className="eyebrow w-20 px-4 text-right text-muted-foreground">
                      {t("space.colReveal")}
                    </TableHead>
                  </TableRow>
                </TableHeader>
                <TableBody>
                  {result.folders.map((folder) => (
                    <TableRow key={folder.index} data-testid={`space-folder-${folder.index}`}>
                      <TableCell className="h-11 px-4 font-mono text-xs text-muted-foreground">
                        <div className="truncate" title={folder.path}>
                          {folder.path}
                        </div>
                      </TableCell>
                      <TableCell className="h-11 px-4 text-right font-mono tnum text-sm">
                        {formatBytes(folder.bytes)}
                      </TableCell>
                      <TableCell className="h-11 px-4 text-sm text-muted-foreground">
                        {tn("space.rootFiles", folder.files)}
                      </TableCell>
                      <TableCell className="h-11 px-4 text-right">
                        <RevealButton kind="folder" index={folder.index} path={folder.path} />
                      </TableCell>
                    </TableRow>
                  ))}
                </TableBody>
              </Table>
            </div>
          </section>
        )}
      </div>
    </Screen>
  );
}
