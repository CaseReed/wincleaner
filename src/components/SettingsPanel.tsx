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

export function SettingsPanel() {
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
              Open source, MIT. No network access, no telemetry.
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
            <p data-testid="updates-placeholder" className="text-muted-foreground">
              Automatic update checks are not available yet. WinCleaner never
              contacts the network.
            </p>
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
