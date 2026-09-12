import { useEffect, useId, useState } from "react";
import { Check, Copy, Loader2, X } from "lucide-react";
import { toast } from "sonner";
import { Button } from "@/components/ui/button";
import { Switch } from "@/components/ui/switch";
import {
  checkForUpdates,
  listExclusions,
  removeExclusion,
  sandboxOrphans,
  type Exclusion,
  sandboxRemoveOrphans,
  type SandboxOrphan,
  type SandboxSummary,
  type UpdateCheck,
} from "@/lib/api";
import { useI18n, type LanguagePreference, type TranslationKey } from "@/i18n";
import {
  readAutoCheck,
  toPlainText,
  updateErrorKey,
  writeAutoCheck,
} from "@/lib/updates";
import whatsNew from "@/generated/whats-new.json";

const GITHUB_URL = "https://github.com/CaseReed/wincleaner";
const WINAPP2_URL = "https://github.com/MoscaDotTo/Winapp2";

function Section({
  title,
  testId,
  children,
}: {
  title: string;
  testId?: string;
  children: React.ReactNode;
}) {
  const titleId = useId();
  return (
    <section
      data-testid={testId}
      /// Five stacked cards with no names is five anonymous regions to a
      /// screen reader: each takes the name of its own heading.
      aria-labelledby={titleId}
      className="max-w-3xl rounded-lg border bg-card p-5"
    >
      <h2 id={titleId} className="eyebrow text-muted-foreground">{title}</h2>
      <div className="mt-3 flex flex-col gap-2 text-sm">{children}</div>
    </section>
  );
}

/// What the Check for updates button is doing right now. The `result` and
/// `error` states are what the last check returned, kept until the next one.
type UpdateState =
  | { kind: "idle" }
  | { kind: "checking" }
  | { kind: "result"; check: UpdateCheck }
  | { kind: "error"; code: string };

/// The one screen in WinCleaner that can reach the network, and only on a
/// click: the switch below arms a check for the *next* start, it never fires
/// one here.
function UpdatesSection() {
  const { t } = useI18n();
  const [state, setState] = useState<UpdateState>({ kind: "idle" });
  const [autoCheck, setAutoCheck] = useState(readAutoCheck);
  const [copied, setCopied] = useState(false);

  async function onCheck() {
    setState({ kind: "checking" });
    setCopied(false);
    try {
      setState({ kind: "result", check: await checkForUpdates() });
    } catch (err) {
      // The backend rejects with a stable code; anything else is a broken IPC
      // call, which reads as "could not reach GitHub" just as truthfully.
      setState({ kind: "error", code: typeof err === "string" ? err : "offline" });
    }
  }

  async function onCopy(url: string) {
    try {
      await navigator.clipboard.writeText(url);
      setCopied(true);
      toast.success(t("updates.copied"));
    } catch {
      // No clipboard permission, or no clipboard at all: the URL is on screen
      // as text and can be selected by hand.
      toast.error(t("updates.copyFailed"));
    }
  }

  function onToggleAuto(next: boolean) {
    setAutoCheck(next);
    writeAutoCheck(next);
  }

  const checking = state.kind === "checking";

  return (
    <>
      <p className="font-medium">
        WinCleaner <span data-testid="updates-current">{whatsNew.version}</span>
      </p>

      <div className="mt-1 flex items-center gap-3">
        <Button
          data-testid="check-updates"
          onClick={() => void onCheck()}
          aria-busy={checking}
          disabled={checking}
        >
          {checking ? (
            <>
              <Loader2 className="size-4 animate-spin" />
              {t("updates.checking")}
            </>
          ) : (
            t("updates.check")
          )}
        </Button>
      </div>

      {state.kind === "error" && (
        <p data-testid="update-status" role="status" className="text-muted-foreground">
          {t(updateErrorKey(state.code))}
        </p>
      )}

      {state.kind === "result" && !state.check.is_newer && (
        <p data-testid="update-status" role="status" className="text-muted-foreground">
          {t("updates.upToDate", { version: state.check.current })}
        </p>
      )}

      {state.kind === "result" && state.check.is_newer && (
        <div className="flex flex-col gap-2">
          <p data-testid="update-status" role="status" className="font-medium">
            {t("updates.available", { version: state.check.latest ?? "" })}
          </p>
          {state.check.published_at && (
            // Display only, and deliberately not localised: the first ten
            // characters of the ISO timestamp GitHub returns.
            <p data-testid="update-published" className="text-muted-foreground">
              {t("updates.published", { date: state.check.published_at.slice(0, 10) })}
            </p>
          )}
          {state.check.notes && (
            // Plain text, as everywhere else here: no markdown renderer and no
            // HTML, so a spoofed release body has nothing to inject into
            // (docs/design-updater.md §3).
            <p
              data-testid="update-notes"
              className="text-sm whitespace-pre-wrap text-muted-foreground"
            >
              {toPlainText(state.check.notes)}
            </p>
          )}
          {state.check.url && (
            <div className="flex items-center gap-2">
              <span data-testid="update-url" className="font-mono text-xs text-muted-foreground">
                {state.check.url}
              </span>
              <Button
                data-testid="copy-update-url"
                variant="outline"
                size="sm"
                onClick={() => void onCopy(state.check.url!)}
              >
                {copied ? <Check className="size-3.5" /> : <Copy className="size-3.5" />}
                {t("updates.copyLink")}
              </Button>
            </div>
          )}
        </div>
      )}

      <label className="mt-2 flex items-center gap-3">
        <Switch
          data-testid="auto-check"
          aria-label={t("updates.autoCheck")}
          checked={autoCheck}
          onCheckedChange={onToggleAuto}
        />
        <span>{t("updates.autoCheck")}</span>
      </label>
      <p data-testid="auto-check-privacy" className="text-xs text-muted-foreground">
        {t("updates.autoCheckPrivacy")}
      </p>
    </>
  );
}

/// The one screen that can switch the whole engine off the user's own profile.
/// Fully controlled: entering and leaving are the parent's business, because
/// the sandbox is application state — the banner and the Startup screen read
/// it too.
function SandboxSection({
  sandbox,
  busy = false,
  onEnterSandbox,
  onLeaveSandbox,
}: {
  sandbox: SandboxSummary | null;
  /// Creating or removing the profile is in flight: both write or delete a few
  /// hundred files, and a second click would race the first.
  busy?: boolean;
  onEnterSandbox?: () => void;
  onLeaveSandbox?: () => void;
}) {
  const { t, tx, formatCount } = useI18n();
  return (
    <>
      <p className="text-muted-foreground">{t("sandboxSection.what")}</p>
      <p className="text-muted-foreground">{t("sandboxSection.active")}</p>

      {sandbox ? (
        <>
          <p data-testid="sandbox-root" className="font-mono text-xs break-all">
            {sandbox.root}
          </p>
          <p className="text-muted-foreground">
            {tx("sandboxSection.counts", {
              junk: <span className="font-mono tnum">{formatCount(sandbox.junk)}</span>,
              sentinels: (
                <span className="font-mono tnum">{formatCount(sandbox.sentinels)}</span>
              ),
              rules: (
                <span className="font-mono tnum">
                  {formatCount(sandbox.winapp2_rules)}
                </span>
              ),
            })}
          </p>
          <div className="mt-1 flex items-center gap-3">
            <Button variant="outline" onClick={onLeaveSandbox} aria-busy={busy} disabled={busy}>
              {busy ? (
                <>
                  <Loader2 className="size-4 animate-spin" />
                  {t("sandboxSection.removing")}
                </>
              ) : (
                t("sandboxSection.leave")
              )}
            </Button>
          </div>
        </>
      ) : (
        <div className="mt-1 flex items-center gap-3">
          <Button data-testid="create-sandbox" onClick={onEnterSandbox} aria-busy={busy} disabled={busy}>
            {busy ? (
              <>
                <Loader2 className="size-4 animate-spin" />
                {t("sandboxSection.creating")}
              </>
            ) : (
              t("sandboxSection.create")
            )}
          </Button>
        </div>
      )}
    </>
  );
}

/// Sandbox directories a previous run left in %TEMP% without removing them: a
/// crash, a kill, a close the back end could not finish in time. Startup
/// sweeps them on its own — this is the line that lets a user who has just
/// watched it happen be rid of them without restarting.
///
/// Absent, not empty, when there is nothing to clean up: a permanent "0 old
/// sandbox folders" would be a scab on a screen most users open once.
/// The exclusions the user built from "Show the paths", and the only way to
/// undo one. Patterns are shown verbatim, in their `%VAR%\…` form: that is
/// what is actually stored, and showing a resolved absolute path here would
/// put back on screen the very thing the storage format avoids.
function ExclusionsSection() {
  const { t } = useI18n();
  const [exclusions, setExclusions] = useState<Exclusion[] | null>(null);
  const [failed, setFailed] = useState(false);
  const [busy, setBusy] = useState<string | null>(null);

  useEffect(() => {
    void listExclusions()
      .then(setExclusions)
      .catch(() => setFailed(true));
  }, []);

  async function onRemove(exclusion: Exclusion) {
    setBusy(exclusion.pattern);
    try {
      await removeExclusion(exclusion.rule_id, exclusion.pattern);
      // Re-read rather than splice: the store on disk is the truth, and a
      // removal the back end refused must stay on screen.
      setExclusions(await listExclusions());
      toast.success(t("exclusions.removed"));
    } catch (err) {
      toast.error(String(err));
    } finally {
      setBusy(null);
    }
  }

  if (failed) {
    return <p className="text-muted-foreground">{t("exclusions.loadFailed")}</p>;
  }
  if (exclusions === null) return null;
  if (exclusions.length === 0) {
    return (
      <p data-testid="exclusions-empty" className="text-muted-foreground">
        {t("exclusions.empty")}
      </p>
    );
  }

  return (
    <ul className="flex flex-col gap-2">
      {exclusions.map((exclusion) => (
        <li
          key={`${exclusion.rule_id} ${exclusion.pattern}`}
          className="flex items-center gap-3"
        >
          <div className="min-w-0 flex-1">
            <p className="truncate font-mono text-xs">{exclusion.pattern}</p>
            <p className="text-xs text-muted-foreground">
              {exclusion.rule_label} ·{" "}
              {t("exclusions.addedOn", { date: exclusion.added })}
            </p>
          </div>
          <Button
            variant="ghost"
            size="sm"
            aria-label={t("exclusions.remove", { pattern: exclusion.pattern })}
            aria-busy={busy === exclusion.pattern}
            disabled={busy !== null}
            onClick={() => void onRemove(exclusion)}
          >
            <X aria-hidden="true" className="size-4" />
          </Button>
        </li>
      ))}
    </ul>
  );
}

function SandboxOrphansLine() {
  const { t, tn, txn, formatBytes, formatCount } = useI18n();
  const [orphans, setOrphans] = useState<SandboxOrphan[]>([]);
  const [busy, setBusy] = useState(false);

  useEffect(() => {
    // A failure here is not worth a toast: the startup sweep is the guarantee,
    // this line is the convenience.
    void sandboxOrphans()
      .then(setOrphans)
      .catch(() => {});
  }, []);

  async function onRemove() {
    setBusy(true);
    try {
      const removed = await sandboxRemoveOrphans();
      // The back end is asked again rather than assumed empty: a directory it
      // could not remove must stay on screen.
      setOrphans(await sandboxOrphans());
      toast.success(tn("sandboxSection.orphansRemoved", removed));
    } catch (err) {
      toast.error(String(err));
    } finally {
      setBusy(false);
    }
  }

  if (orphans.length === 0) return null;

  const bytes = orphans.reduce((sum, orphan) => sum + orphan.size_bytes, 0);

  return (
    <div data-testid="sandbox-orphans" className="mt-1 flex items-center gap-3">
      <p className="text-muted-foreground">
        {txn("sandboxSection.orphans", orphans.length, {
          count: <span className="font-mono tnum">{formatCount(orphans.length)}</span>,
          bytes: <span className="font-mono tnum">{formatBytes(bytes)}</span>,
        })}
      </p>
      <Button variant="outline" size="sm" aria-busy={busy} disabled={busy} onClick={() => void onRemove()}>
        {busy ? (
          <>
            <Loader2 className="size-4 animate-spin" />
            {t("sandboxSection.removing")}
          </>
        ) : (
          t("common.remove")
        )}
      </Button>
    </div>
  );
}

const LANGUAGE_OPTIONS: { value: LanguagePreference; label: TranslationKey }[] = [
  { value: "system", label: "settings.languageSystem" },
  { value: "en", label: "settings.languageEn" },
  { value: "fr", label: "settings.languageFr" },
];

/// The interface language. It covers the UI chrome and, since v0.8.0, the
/// native rules' labels and descriptions (optional `label_fr`/
/// `description_fr` in `src-tauri/rules.toml`, `src/lib/rule-i18n.ts` picks
/// them). Winapp2 (community) rules, the "What's new" body extracted from
/// CHANGELOG.md at build time, and the release notes from GitHub stay in
/// English whatever is picked here.
function LanguageSection() {
  const { t, preference, setPreference } = useI18n();
  return (
    <>
      <select
        data-testid="language"
        aria-label={t("settings.language")}
        className="h-8 w-48 rounded-md border bg-card px-2 text-sm outline-none focus-visible:ring-3 focus-visible:ring-ring"
        value={preference}
        onChange={(e) => setPreference(e.target.value as LanguagePreference)}
      >
        {LANGUAGE_OPTIONS.map(({ value, label }) => (
          <option key={value} value={value}>
            {t(label)}
          </option>
        ))}
      </select>
      <p className="text-xs text-muted-foreground">{t("settings.languageNote")}</p>
    </>
  );
}

export function SettingsPanel({
  sandbox = null,
  sandboxBusy = false,
  onEnterSandbox,
  onLeaveSandbox,
}: {
  sandbox?: SandboxSummary | null;
  sandboxBusy?: boolean;
  onEnterSandbox?: () => void;
  onLeaveSandbox?: () => void;
} = {}) {
  const { t, tx } = useI18n();
  return (
    <>
      <header className="shrink-0 px-8 pt-7 pb-5">
        <h1 className="screen-title">{t("settings.title")}</h1>
        <p className="mt-1 text-sm text-muted-foreground">{t("settings.subtitle")}</p>
      </header>
      <div className="min-h-0 flex-1 overflow-auto px-8 pb-8">
        <div className="flex flex-col gap-4">
          <Section title={t("settings.about")} testId="about">
            <p className="font-medium">
              WinCleaner <span data-testid="app-version">{whatsNew.version}</span>
            </p>
            <p className="text-muted-foreground">{t("settings.aboutBody")}</p>
            <p className="font-mono text-xs text-muted-foreground">{GITHUB_URL}</p>
          </Section>

          <Section title={t("settings.whatsNew", { version: whatsNew.version })}>
            {/* Plain text, deliberately: no markdown renderer and no HTML, so
                release notes can never become an injection surface (see
                docs/design-updater.md §3). Bullets stay as "- " text. */}
            <p
              data-testid="whats-new"
              className="text-sm whitespace-pre-wrap text-muted-foreground"
            >
              {whatsNew.body}
            </p>
          </Section>

          <Section title={t("settings.language")}>
            <LanguageSection />
          </Section>

          <Section title={t("settings.updates")}>
            <UpdatesSection />
          </Section>

          <Section title={t("settings.sandbox")} testId="sandbox">
            <SandboxSection
              sandbox={sandbox}
              busy={sandboxBusy}
              onEnterSandbox={onEnterSandbox}
              onLeaveSandbox={onLeaveSandbox}
            />
            <SandboxOrphansLine />
          </Section>

          <Section title={t("settings.exclusions")} testId="exclusions">
            <ExclusionsSection />
          </Section>

          <Section title={t("settings.notices")} testId="notices">
            <p className="text-muted-foreground">{t("notices.mit")}</p>
            <p className="text-muted-foreground">
              {tx("notices.winapp2", {
                url: <span className="font-mono text-xs">{WINAPP2_URL}</span>,
              })}
            </p>
            <p className="text-muted-foreground">{t("notices.bundled")}</p>
          </Section>
        </div>
      </div>
    </>
  );
}
