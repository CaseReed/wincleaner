import { useState } from "react";
import { Check, Copy, Loader2 } from "lucide-react";
import { toast } from "sonner";
import { Button } from "@/components/ui/button";
import { Switch } from "@/components/ui/switch";
import { checkForUpdates, type SandboxSummary, type UpdateCheck } from "@/lib/api";
import {
  readAutoCheck,
  toPlainText,
  updateErrorMessage,
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
  return (
    <section
      data-testid={testId}
      className="max-w-3xl rounded-lg border bg-card p-5"
    >
      <h2 className="eyebrow text-muted-foreground">{title}</h2>
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
      toast.success("Release link copied");
    } catch {
      // No clipboard permission, or no clipboard at all: the URL is on screen
      // as text and can be selected by hand.
      toast.error("Could not copy the link");
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
        <Button data-testid="check-updates" onClick={() => void onCheck()} disabled={checking}>
          {checking ? (
            <>
              <Loader2 className="size-4 animate-spin" />
              Checking…
            </>
          ) : (
            "Check for updates"
          )}
        </Button>
      </div>

      {state.kind === "error" && (
        <p data-testid="update-status" className="text-muted-foreground">
          {updateErrorMessage(state.code)}
        </p>
      )}

      {state.kind === "result" && !state.check.is_newer && (
        <p data-testid="update-status" className="text-muted-foreground">
          You&rsquo;re up to date ({state.check.current})
        </p>
      )}

      {state.kind === "result" && state.check.is_newer && (
        <div className="flex flex-col gap-2">
          <p data-testid="update-status" className="font-medium">
            WinCleaner {state.check.latest} is available
          </p>
          {state.check.published_at && (
            // Display only, and deliberately not localised: the first ten
            // characters of the ISO timestamp GitHub returns.
            <p data-testid="update-published" className="text-muted-foreground">
              Published {state.check.published_at.slice(0, 10)}
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
                Copy link
              </Button>
            </div>
          )}
        </div>
      )}

      <label className="mt-2 flex items-center gap-3">
        <Switch
          data-testid="auto-check"
          aria-label="Check automatically at startup"
          checked={autoCheck}
          onCheckedChange={onToggleAuto}
        />
        <span>Check automatically at startup</span>
      </label>
      <p data-testid="auto-check-privacy" className="text-xs text-muted-foreground">
        When enabled, WinCleaner sends one request to api.github.com at startup
        with no identifiers other than the app version in the User-Agent.
      </p>
    </>
  );
}

const RULE_COUNT = new Intl.NumberFormat("en-US");

/// The one screen that can switch the whole engine off the user's own profile.
/// Fully controlled: entering and leaving are the parent's business, because
/// the sandbox is application state — the banner and the Startup screen read
/// it too.
function SandboxSection({
  sandbox,
  onEnterSandbox,
  onLeaveSandbox,
}: {
  sandbox: SandboxSummary | null;
  onEnterSandbox?: () => void;
  onLeaveSandbox?: () => void;
}) {
  return (
    <>
      <p className="text-muted-foreground">
        A sandbox is a synthetic Windows profile WinCleaner builds under your
        temporary directory: junk files every rule is meant to remove, plus
        decoy documents, keys and caches that must survive.
      </p>
      <p className="text-muted-foreground">
        While it is active, Analyze and Clean run for real against that profile
        and nothing else — nothing in your real profile is touched, and the
        Recycle Bin and the startup registry keys stay out of reach.
      </p>

      {sandbox ? (
        <>
          <p data-testid="sandbox-root" className="font-mono text-xs break-all">
            {sandbox.root}
          </p>
          <p className="text-muted-foreground">
            <span className="font-mono tnum">
              {RULE_COUNT.format(sandbox.junk)}
            </span>{" "}
            junk files ·{" "}
            <span className="font-mono tnum">
              {RULE_COUNT.format(sandbox.sentinels)}
            </span>{" "}
            files that must survive ·{" "}
            <span className="font-mono tnum">
              {RULE_COUNT.format(sandbox.winapp2_rules)}
            </span>{" "}
            Winapp2 rules detected
          </p>
          <div className="mt-1 flex items-center gap-3">
            <Button variant="outline" onClick={onLeaveSandbox}>
              Leave the sandbox
            </Button>
          </div>
        </>
      ) : (
        <div className="mt-1 flex items-center gap-3">
          <Button data-testid="create-sandbox" onClick={onEnterSandbox}>
            Create a sandbox profile
          </Button>
        </div>
      )}
    </>
  );
}

export function SettingsPanel({
  sandbox = null,
  onEnterSandbox,
  onLeaveSandbox,
}: {
  sandbox?: SandboxSummary | null;
  onEnterSandbox?: () => void;
  onLeaveSandbox?: () => void;
} = {}) {
  return (
    <>
      <header className="shrink-0 px-8 pt-7 pb-5">
        <h1 className="screen-title">Settings</h1>
        <p className="mt-1 text-sm text-muted-foreground">
          What this build is, what changed, and what it is made of.
        </p>
      </header>
      <div className="min-h-0 flex-1 overflow-auto px-8 pb-8">
        <div className="flex flex-col gap-4">
          <Section title="About" testId="about">
            <p className="font-medium">
              WinCleaner <span data-testid="app-version">{whatsNew.version}</span>
            </p>
            <p className="text-muted-foreground">
              Open source, MIT. No telemetry, and no network access except one
              request to GitHub when you click Check for updates or enable
              automatic checks (off by default).
            </p>
            <p className="font-mono text-xs text-muted-foreground">{GITHUB_URL}</p>
          </Section>

          <Section title={`What's new in ${whatsNew.version}`}>
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

          <Section title="Updates">
            <UpdatesSection />
          </Section>

          <Section title="Sandbox" testId="sandbox">
            <SandboxSection
              sandbox={sandbox}
              onEnterSandbox={onEnterSandbox}
              onLeaveSandbox={onLeaveSandbox}
            />
          </Section>

          <Section title="Notices" testId="notices">
            <p className="text-muted-foreground">WinCleaner is released under the MIT licence.</p>
            <p className="text-muted-foreground">
              Community rules from Winapp2 (CC-BY-SA 4.0) —{" "}
              <span className="font-mono text-xs">{WINAPP2_URL}</span>
            </p>
            <p className="text-muted-foreground">
              Fonts and icons are bundled with the application; nothing is fetched at runtime.
            </p>
          </Section>
        </div>
      </div>
    </>
  );
}
