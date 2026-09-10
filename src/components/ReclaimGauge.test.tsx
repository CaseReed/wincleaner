import { describe, it, expect, afterEach, vi } from "vitest";
import { render, screen } from "@testing-library/react";
import { ReclaimGauge } from "./ReclaimGauge";
import type { RuleSummary, ScanResult } from "@/lib/api";

const REGLES: RuleSummary[] = [
  { id: "windows.temp", category: "Système", label: "Fichiers temporaires", risk: "low", kind: "files", default_checked: true, unavailable_reason: null },
  { id: "windows.recycle-bin", category: "Système", label: "Corbeille", risk: "low", kind: "recycle-bin", default_checked: true, unavailable_reason: null },
  { id: "edge.cache", category: "Navigateurs", label: "Cache Microsoft Edge", risk: "low", kind: "files", default_checked: true, unavailable_reason: null },
  { id: "chrome.cache", category: "Navigateurs", label: "Cache Google Chrome", risk: "low", kind: "files", default_checked: true, unavailable_reason: null },
];

function result(rule_id: string, total_bytes: number): ScanResult {
  return { rule_id, file_count: 1, total_bytes, paths: [], skipped: 0 };
}

/// `matchMedia` n'existe pas dans jsdom : on le pose pour la durée d'un test.
function stubReducedMotion(reduce: boolean) {
  Object.defineProperty(window, "matchMedia", {
    configurable: true,
    writable: true,
    value: vi.fn((query: string) => ({
      matches: reduce && query.includes("reduced-motion"),
      media: query,
      addEventListener() {},
      removeEventListener() {},
    })),
  });
}

afterEach(() => {
  Reflect.deleteProperty(window, "matchMedia");
});

describe("ReclaimGauge", () => {
  it("ne rend rien tant qu'aucun octet n'est récupérable", () => {
    render(<ReclaimGauge rules={REGLES} results={[result("windows.temp", 0)]} />);
    expect(screen.queryByTestId("reclaim-gauge")).toBeNull();
  });

  it("un segment par règle non vide, largeur proportionnelle aux octets", () => {
    render(
      <ReclaimGauge
        rules={REGLES}
        results={[
          result("windows.temp", 750),
          result("windows.recycle-bin", 0),
          result("edge.cache", 250),
        ]}
      />
    );
    const segments = screen.getAllByTestId("gauge-segment");
    expect(segments).toHaveLength(2);
    expect(segments[0].style.width).toBe("75%");
    expect(segments[1].style.width).toBe("25%");
    expect(segments[0]).toHaveAttribute("title", "Fichiers temporaires — 750 o");
  });

  it("la légende retient les trois plus gros contributeurs", () => {
    render(
      <ReclaimGauge
        rules={REGLES}
        results={[
          result("windows.temp", 100),
          result("windows.recycle-bin", 400),
          result("edge.cache", 300),
          result("chrome.cache", 200),
        ]}
      />
    );
    const legende = screen.getByTestId("reclaim-gauge").querySelectorAll("li");
    expect(legende).toHaveLength(3);
    expect(legende[0]).toHaveTextContent("Corbeille");
    expect(legende[1]).toHaveTextContent("Cache Microsoft Edge");
    expect(legende[2]).toHaveTextContent("Cache Google Chrome");
  });

  it("anime la largeur par défaut", () => {
    stubReducedMotion(false);
    render(<ReclaimGauge rules={REGLES} results={[result("windows.temp", 1024)]} />);
    expect(screen.getByTestId("gauge-segment").style.transition).toBe(
      "width 500ms ease-out"
    );
  });

  it("n'anime pas sous prefers-reduced-motion", () => {
    stubReducedMotion(true);
    render(<ReclaimGauge rules={REGLES} results={[result("windows.temp", 1024)]} />);
    const segment = screen.getByTestId("gauge-segment");
    expect(segment.style.transition).toBe("none");
    expect(segment.style.width).toBe("100%");
  });
});
