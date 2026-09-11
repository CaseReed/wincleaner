import { describe, it, expect } from "vitest";
import { readFileSync } from "node:fs";
import { resolve } from "node:path";

/// The palette is written in oklch, which says nothing about legibility on its
/// own: a token can be nudged half a point and quietly drop a paragraph under
/// the 4.5:1 that WCAG asks of body text. This reads the real stylesheet and
/// recomputes the ratios, so a future edit to a colour fails here rather than
/// on a user's screen.

const CSS = readFileSync(resolve(process.cwd(), "src/index.css"), "utf8");

/// oklch -> linear sRGB (the space WCAG's relative luminance is defined in),
/// clamped into gamut the way a browser does when a colour falls outside it.
function oklch(l: number, c: number, hDeg: number): [number, number, number] {
  const h = (hDeg * Math.PI) / 180;
  const a = c * Math.cos(h);
  const b = c * Math.sin(h);
  const lc = (l + 0.3963377774 * a + 0.2158037573 * b) ** 3;
  const mc = (l - 0.1055613458 * a - 0.0638541728 * b) ** 3;
  const sc = (l - 0.0894841775 * a - 1.291485548 * b) ** 3;
  return [
    4.0767416621 * lc - 3.3077115913 * mc + 0.2309699292 * sc,
    -1.2684380046 * lc + 2.6097574011 * mc - 0.3413193965 * sc,
    -0.0041960863 * lc - 0.7034186147 * mc + 1.707614701 * sc,
  ].map((v) => Math.min(Math.max(v, 0), 1)) as [number, number, number];
}

function luminance([r, g, b]: [number, number, number]): number {
  return 0.2126 * r + 0.7152 * g + 0.0722 * b;
}

function contrast(fg: [number, number, number], bg: [number, number, number]): number {
  const [hi, lo] = [luminance(fg), luminance(bg)].sort((a, b) => b - a);
  return (hi + 0.05) / (lo + 0.05);
}

/// A colour painted at `alpha` over another, the way `bg-destructive/8` is.
function over(
  fg: [number, number, number],
  bg: [number, number, number],
  alpha: number,
): [number, number, number] {
  return fg.map((c, i) => c * alpha + bg[i] * (1 - alpha)) as [number, number, number];
}

/// The `:root` block is the light theme, `.dark` the dark one. Only the plain
/// `oklch(l c h)` tokens are read: the two `oklch(1 0 0 / 10%)` borders are not
/// text colours and have no ratio to check.
function tokens(selector: string): Record<string, [number, number, number]> {
  const block = CSS.split(selector + " {")[1].split("\n}")[0];
  const found: Record<string, [number, number, number]> = {};
  for (const [, name, l, c, h] of block.matchAll(
    /--([a-z0-9-]+):\s*oklch\(([\d.]+) ([\d.]+) ([\d.]+)\)/g,
  )) {
    found[name] = oklch(Number(l), Number(c), Number(h));
  }
  return found;
}

const THEMES = {
  light: tokens(":root"),
  dark: tokens(".dark"),
};

describe("theme contrast", () => {
  /// Body text, the muted paragraphs included: 4.5:1 is the floor.
  const TEXT_PAIRS: [string, string][] = [
    ["foreground", "background"],
    ["foreground", "card"],
    ["muted-foreground", "background"],
    ["muted-foreground", "card"],
    ["muted-foreground", "muted"],
    ["muted-foreground", "sidebar"],
    ["warning-foreground", "card"],
    ["success-foreground", "card"],
    ["primary-foreground", "primary"],
  ];

  for (const theme of ["light", "dark"] as const) {
    for (const [fg, bg] of TEXT_PAIRS) {
      it(`${theme}: ${fg} on ${bg} is legible`, () => {
        expect(contrast(THEMES[theme][fg], THEMES[theme][bg])).toBeGreaterThanOrEqual(4.5);
      });
    }

    /// The Clean and Confirm buttons: `--destructive` itself only reached
    /// 4.11:1 (light) and 3.75:1 (dark) on its own wash, which is why
    /// `--destructive-foreground` exists. The hover tint is the worst case.
    it(`${theme}: the destructive button text is legible on its own wash`, () => {
      const t = THEMES[theme];
      const alpha = theme === "dark" ? 0.3 : 0.2;
      expect(
        contrast(t["destructive-foreground"], over(t.destructive, t.background, alpha)),
      ).toBeGreaterThanOrEqual(4.5);
    });

    /// The focus ring is drawn in `--ring`: a non-text indicator, so 3:1.
    it(`${theme}: the focus ring stands out from what it surrounds`, () => {
      const t = THEMES[theme];
      expect(contrast(t.ring, t.background)).toBeGreaterThanOrEqual(3);
      expect(contrast(t.ring, t.card)).toBeGreaterThanOrEqual(3);
      expect(contrast(t.ring, t.sidebar)).toBeGreaterThanOrEqual(3);
    });
  }
});
