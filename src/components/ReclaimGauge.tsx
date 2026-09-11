import { useEffect, useState } from "react";
import { formatBytes } from "@/lib/format";
import type { RuleSummary, ScanResult } from "@/lib/api";

/// Three lightness levels of `--primary` for the categories (the first is the
/// darkest), plus a colour of its own for the recycle bin: it does not free
/// space like the others, it destroys.
const CATEGORY_TONES = ["var(--chart-1)", "var(--chart-3)", "var(--chart-2)"];
const RECYCLE_TONE = "color-mix(in oklch, var(--destructive) 70%, transparent)";

export interface GaugeSegment {
  id: string;
  label: string;
  bytes: number;
  color: string;
}

/// Joins the scan results to the rules, in the order of rules.toml.
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

/// Also read by the scan progress bar in `CleanPanel`: both grow a width, and
/// both must stop doing so when the user asked for no motion.
export function prefersReducedMotion(): boolean {
  return (
    typeof window !== "undefined" &&
    typeof window.matchMedia === "function" &&
    window.matchMedia("(prefers-reduced-motion: reduce)").matches
  );
}

/// What the bar says, in words: the total, then the segments that carry it,
/// biggest first. A stack of coloured widths is the one thing a screen reader
/// gets nothing from, so it is spelled out instead of described.
export function describeSegments(segments: GaugeSegment[], total: number): string {
  const ranked = [...segments].sort((a, b) => b.bytes - a.bytes);
  const named = ranked
    .slice(0, 3)
    .map((s) => `${s.label} ${formatBytes(s.bytes)}`)
    .join(", ");
  const rest = ranked.length - 3;
  const tail =
    rest > 0 ? `, and ${rest} smaller ${rest === 1 ? "rule" : "rules"}` : "";
  return `Reclaimable ${formatBytes(total)}: ${named}${tail}`;
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
  // The first render lays the segments out at zero width; the passive effect
  // runs after paint, and the CSS transition does the rest.
  const [grown, setGrown] = useState(still);

  useEffect(() => {
    if (!still) setGrown(true);
  }, [still]);

  if (total <= 0) return null;

  return (
    <div data-testid="reclaim-gauge" className="flex flex-col gap-3">
      <div
        role="img"
        aria-label={describeSegments(segments, total)}
        className="flex h-2.5 w-full gap-[2px] overflow-hidden rounded-[5px] bg-muted"
      >
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
