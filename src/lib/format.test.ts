import { describe, it, expect } from "vitest";
import { formatBytes, formatCount } from "./format";

describe("formatBytes", () => {
  it("shows zero bytes", () => {
    expect(formatBytes(0)).toBe("0 B");
  });

  it("shows raw bytes below 1 KB", () => {
    expect(formatBytes(512)).toBe("512 B");
  });

  it("shows kilobytes with one decimal", () => {
    expect(formatBytes(1536)).toBe("1.5 KB");
  });

  it("shows megabytes", () => {
    expect(formatBytes(5 * 1024 * 1024)).toBe("5 MB");
  });

  it("shows gigabytes", () => {
    expect(formatBytes(3 * 1024 * 1024 * 1024)).toBe("3 GB");
  });
});

describe("formatCount", () => {
  it("groups thousands with a comma", () => {
    expect(formatCount(1234567)).toBe("1,234,567");
  });
});
