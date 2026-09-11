import { describe, it, expect } from "vitest";
import { readFileSync } from "node:fs";
import { resolve } from "node:path";

/// jsdom has no layout and no media queries, so the only honest proof that the
/// app honours `prefers-reduced-motion` is that the rule is in the stylesheet
/// and covers both kinds of movement. The two animations driven from
/// JavaScript — the gauge and the scan progress bar — are proved by their own
/// tests, which stub `matchMedia`.

const CSS = readFileSync(resolve(process.cwd(), "src/index.css"), "utf8");

describe("reduced motion", () => {
  const block = CSS.split("@media (prefers-reduced-motion: reduce)")[1];

  it("the stylesheet answers prefers-reduced-motion", () => {
    expect(block).toBeDefined();
  });

  it("stops both transitions and animations, not only one of them", () => {
    expect(block).toMatch(/transition-duration:\s*0\.01ms\s*!important/);
    expect(block).toMatch(/animation-duration:\s*0\.01ms\s*!important/);
    expect(block).toMatch(/animation-iteration-count:\s*1\s*!important/);
  });

  /// A zero-length transition never fires `transitionend`: anything waiting on
  /// one would wait forever.
  it("shortens the durations instead of setting them to zero", () => {
    expect(block).not.toMatch(/transition-duration:\s*0s/);
  });

  it("reaches pseudo-elements, where the sidebar draws its active marker", () => {
    expect(block).toMatch(/\*::before/);
    expect(block).toMatch(/\*::after/);
  });
});
