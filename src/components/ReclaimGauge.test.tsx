import { describe, it, expect, afterEach, vi } from "vitest";
import { render, screen } from "@testing-library/react";
import { ReclaimGauge } from "./ReclaimGauge";
import type { RuleSummary, ScanResult } from "@/lib/api";

const RULES: RuleSummary[] = [
  { id: "windows.temp", category: "System", label: "Temporary files", risk: "low", kind: "files", default_checked: true, note: null, unavailable_reason: null },
  { id: "windows.recycle-bin", category: "System", label: "Recycle Bin", risk: "low", kind: "recycle-bin", default_checked: true, note: null, unavailable_reason: null },
  { id: "edge.cache", category: "Browsers", label: "Microsoft Edge cache", risk: "low", kind: "files", default_checked: true, note: null, unavailable_reason: null },
  { id: "chrome.cache", category: "Browsers", label: "Google Chrome cache", risk: "low", kind: "files", default_checked: true, note: null, unavailable_reason: null },
];

function result(rule_id: string, total_bytes: number): ScanResult {
  return { rule_id, file_count: 1, total_bytes, paths: [], skipped: 0 };
}

/// `matchMedia` does not exist in jsdom: we install it for the length of a test.
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
  it("renders nothing while no byte is reclaimable", () => {
    render(<ReclaimGauge rules={RULES} results={[result("windows.temp", 0)]} />);
    expect(screen.queryByTestId("reclaim-gauge")).toBeNull();
  });

  it("one segment per non-empty rule, width proportional to the bytes", () => {
    render(
      <ReclaimGauge
        rules={RULES}
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
    expect(segments[0]).toHaveAttribute("title", "Temporary files — 750 B");
  });

  it("the legend keeps the three largest contributors", () => {
    render(
      <ReclaimGauge
        rules={RULES}
        results={[
          result("windows.temp", 100),
          result("windows.recycle-bin", 400),
          result("edge.cache", 300),
          result("chrome.cache", 200),
        ]}
      />
    );
    const legend = screen.getByTestId("reclaim-gauge").querySelectorAll("li");
    expect(legend).toHaveLength(3);
    expect(legend[0]).toHaveTextContent("Recycle Bin");
    expect(legend[1]).toHaveTextContent("Microsoft Edge cache");
    expect(legend[2]).toHaveTextContent("Google Chrome cache");
  });

  /// A stack of coloured widths is the one thing a screen reader gets nothing
  /// from: the bar says in words what it draws.
  it("reads the bar out as its total and its largest contributors", () => {
    render(
      <ReclaimGauge
        rules={RULES}
        results={[
          result("windows.temp", 100),
          result("windows.recycle-bin", 400),
          result("edge.cache", 300),
          result("chrome.cache", 200),
        ]}
      />
    );
    expect(screen.getByRole("img")).toHaveAccessibleName(
      "Reclaimable 1000 B: Recycle Bin 400 B, Microsoft Edge cache 300 B, Google Chrome cache 200 B, and 1 smaller rule"
    );
  });

  it("names every segment when there are no more than three", () => {
    render(
      <ReclaimGauge
        rules={RULES}
        results={[result("windows.temp", 750), result("edge.cache", 250)]}
      />
    );
    expect(screen.getByRole("img")).toHaveAccessibleName(
      "Reclaimable 1000 B: Temporary files 750 B, Microsoft Edge cache 250 B"
    );
  });

  it("animates the width by default", () => {
    stubReducedMotion(false);
    render(<ReclaimGauge rules={RULES} results={[result("windows.temp", 1024)]} />);
    expect(screen.getByTestId("gauge-segment").style.transition).toBe(
      "width 500ms ease-out"
    );
  });

  it("does not animate under prefers-reduced-motion", () => {
    stubReducedMotion(true);
    render(<ReclaimGauge rules={RULES} results={[result("windows.temp", 1024)]} />);
    const segment = screen.getByTestId("gauge-segment");
    expect(segment.style.transition).toBe("none");
    expect(segment.style.width).toBe("100%");
  });
});
