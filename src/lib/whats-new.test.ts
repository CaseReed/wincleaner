import { describe, it, expect, beforeEach, vi, afterEach } from "vitest";
import { LAST_SEEN_KEY, markSeen, readLastSeen, shouldAnnounce } from "./whats-new";

describe("shouldAnnounce", () => {
  it("announces when a previous version was stored and differs", () => {
    expect(shouldAnnounce("0.1.0", "0.2.0")).toBe(true);
  });

  it("stays quiet on a first install, when nothing was stored", () => {
    expect(shouldAnnounce(null, "0.2.0")).toBe(false);
  });

  it("stays quiet on an empty stored value", () => {
    expect(shouldAnnounce("", "0.2.0")).toBe(false);
  });

  it("stays quiet when the version has not moved", () => {
    expect(shouldAnnounce("0.2.0", "0.2.0")).toBe(false);
  });
});

describe("readLastSeen / markSeen", () => {
  beforeEach(() => {
    localStorage.clear();
  });

  afterEach(() => {
    vi.restoreAllMocks();
  });

  it("reads back what was stored", () => {
    markSeen("0.2.0");
    expect(localStorage.getItem(LAST_SEEN_KEY)).toBe("0.2.0");
    expect(readLastSeen()).toBe("0.2.0");
  });

  it("reads null when nothing was stored", () => {
    expect(readLastSeen()).toBeNull();
  });

  it("survives a localStorage that throws", () => {
    vi.spyOn(Storage.prototype, "getItem").mockImplementation(() => {
      throw new Error("denied");
    });
    vi.spyOn(Storage.prototype, "setItem").mockImplementation(() => {
      throw new Error("denied");
    });
    expect(readLastSeen()).toBeNull();
    expect(() => markSeen("0.2.0")).not.toThrow();
  });
});
