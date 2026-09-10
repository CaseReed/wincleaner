import { useEffect, useState } from "react";
import { formatBytes } from "@/lib/format";
import type { RuleSummary, ScanResult } from "@/lib/api";

/// Trois luminosités de `--primary` pour les catégories (la première est la plus
/// sombre), plus une couleur propre à la corbeille : elle ne libère pas de
/// l'espace comme les autres, elle détruit.
const CATEGORY_TONES = ["var(--chart-1)", "var(--chart-3)", "var(--chart-2)"];
const RECYCLE_TONE = "color-mix(in oklch, var(--destructive) 70%, transparent)";

export interface GaugeSegment {
  id: string;
  label: string;
  bytes: number;
  color: string;
}

/// Joint les résultats de scan aux règles, dans l'ordre de rules.toml.
export function buildSegments(
  rules: RuleSummary[],
  results: ScanResult[],
): GaugeSegment[] {
  const categories: string[] = [];
  const segments: GaugeSegment[] = [];
  for (const rule of rules) {
    if (!categories.includes(rule.category)) categories.push(rule.category);
    const result = results.find((r) => r.rule_id === rule.id);
    if (!result || result.total_bytes <= 0) continue;
    segments.push({
      id: rule.id,
      label: rule.label,
      bytes: result.total_bytes,
      color:
        rule.kind === "recycle-bin"
          ? RECYCLE_TONE
          : CATEGORY_TONES[categories.indexOf(rule.category) % CATEGORY_TONES.length],
    });
  }
  return segments;
}

function prefersReducedMotion(): boolean {
  return (
    typeof window !== "undefined" &&
    typeof window.matchMedia === "function" &&
    window.matchMedia("(prefers-reduced-motion: reduce)").matches
  );
}

export function ReclaimGauge({
  rules,
  results,
}: {
  rules: RuleSummary[];
  results: ScanResult[];
}) {
  const segments = buildSegments(rules, results);
  const total = segments.reduce((sum, s) => sum + s.bytes, 0);
  const still = prefersReducedMotion();
  // Le premier rendu pose les segments à zéro ; l'effet passif s'exécute après
  // la peinture, la transition CSS fait le reste.
  const [grown, setGrown] = useState(still);

  useEffect(() => {
    if (!still) setGrown(true);
  }, [still]);

  if (total <= 0) return null;

  return (
    <div data-testid="reclaim-gauge" className="flex flex-col gap-3">
      <div className="flex h-2.5 w-full gap-[2px] overflow-hidden rounded-[5px] bg-muted">
        {segments.map((segment) => (
          <div
            key={segment.id}
            data-testid="gauge-segment"
            title={`${segment.label} — ${formatBytes(segment.bytes)}`}
            style={{
              width: grown ? `${(segment.bytes / total) * 100}%` : "0%",
              background: segment.color,
              transition: still ? "none" : "width 500ms ease-out",
            }}
          />
        ))}
      </div>
      <ul className="flex flex-wrap items-center gap-x-5 gap-y-1.5">
        {[...segments]
          .sort((a, b) => b.bytes - a.bytes)
          .slice(0, 3)
          .map((segment) => (
            <li key={segment.id} className="flex items-center gap-2 text-sm">
              <span
                aria-hidden="true"
                className="size-2 shrink-0 rounded-full"
                style={{ background: segment.color }}
              />
              <span className="text-muted-foreground">{segment.label}</span>
              <span className="font-mono tnum text-foreground">
                {formatBytes(segment.bytes)}
              </span>
            </li>
          ))}
      </ul>
    </div>
  );
}
