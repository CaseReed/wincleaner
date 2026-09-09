import { describe, it, expect } from "vitest";
import { formatBytes } from "./format";

describe("formatBytes", () => {
  it("affiche 0 octet", () => {
    expect(formatBytes(0)).toBe("0 o");
  });

  it("affiche les octets bruts sous 1 Ko", () => {
    expect(formatBytes(512)).toBe("512 o");
  });

  it("affiche les kilo-octets avec une décimale", () => {
    expect(formatBytes(1536)).toBe("1,5 Ko");
  });

  it("affiche les méga-octets", () => {
    expect(formatBytes(5 * 1024 * 1024)).toBe("5 Mo");
  });

  it("affiche les giga-octets", () => {
    expect(formatBytes(3 * 1024 * 1024 * 1024)).toBe("3 Go");
  });
});
