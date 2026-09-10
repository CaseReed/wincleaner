import { describe, it, expect, beforeEach } from "vitest";
import {
  AUTO_CHECK_KEY,
  LAST_NOTIFIED_KEY,
  markNotified,
  readAutoCheck,
  readLastNotified,
  shouldNotify,
  toPlainText,
  updateErrorMessage,
  writeAutoCheck,
} from "./updates";

describe("toPlainText", () => {
  it("leaves ordinary prose and bullets untouched", () => {
    expect(toPlainText("- One thing\n- Another thing")).toBe("- One thing\n- Another thing");
  });

  it("drops the hashes of a markdown heading", () => {
    expect(toPlainText("### Added\n- A thing")).toBe("Added\n- A thing");
    expect(toPlainText("# 0.3.0")).toBe("0.3.0");
  });

  it("keeps the label of a link and drops its target", () => {
    expect(toPlainText("See [the docs](https://example.com/x) for more")).toBe(
      "See the docs for more",
    );
  });

  /// The reason notes are never rendered as markdown: a spoofed release body
  /// must not be able to smuggle a clickable target past the reader. The
  /// nested parenthesis leaves a stray ")" behind — deliberately not worth a
  /// markdown parser, since what matters is that the target is gone and that
  /// the result is text either way.
  it("neutralises a javascript: link into its label", () => {
    const flattened = toPlainText("[Click me](javascript:alert(1))");
    expect(flattened).toBe("Click me)");
    expect(flattened).not.toContain("javascript:");
  });

  it("removes backticks around inline code", () => {
    expect(toPlainText("Run `npm test` first")).toBe("Run npm test first");
    expect(toPlainText("```\ncode\n```")).toBe("\ncode\n");
  });

  it("leaves markup characters as literal text, never as HTML", () => {
    // Nothing here is parsed: the string comes out as text, and the component
    // renders it as text.
    expect(toPlainText("<script>alert(1)</script>")).toBe("<script>alert(1)</script>");
  });

  it("handles an empty body", () => {
    expect(toPlainText("")).toBe("");
  });
});

describe("shouldNotify", () => {
  it("announces a version never announced before", () => {
    expect(shouldNotify(null, "0.3.0")).toBe(true);
  });

  it("stays quiet for a version already announced", () => {
    expect(shouldNotify("0.3.0", "0.3.0")).toBe(false);
  });

  it("announces again when a newer version appears", () => {
    expect(shouldNotify("0.3.0", "0.4.0")).toBe(true);
  });

  it("has nothing to announce without a version", () => {
    expect(shouldNotify(null, null)).toBe(false);
    expect(shouldNotify("0.3.0", null)).toBe(false);
  });
});

describe("updateErrorMessage", () => {
  it("gives each backend code its sentence", () => {
    expect(updateErrorMessage("offline")).toBe("Could not reach GitHub — check your connection");
    expect(updateErrorMessage("not-available")).toBe("No public release is available yet");
    expect(updateErrorMessage("rate-limited")).toBe("GitHub rate limit reached, try again later");
    expect(updateErrorMessage("malformed")).toBe("Could not read GitHub's answer");
  });

  it("falls back to the offline sentence for an unknown code", () => {
    expect(updateErrorMessage("something else")).toBe(
      "Could not reach GitHub — check your connection",
    );
  });
});

describe("localStorage accessors", () => {
  beforeEach(() => {
    localStorage.clear();
  });

  it("defaults automatic checking to off", () => {
    expect(readAutoCheck()).toBe(false);
  });

  it("round-trips the automatic-check choice", () => {
    writeAutoCheck(true);
    expect(localStorage.getItem(AUTO_CHECK_KEY)).toBe("true");
    expect(readAutoCheck()).toBe(true);
    writeAutoCheck(false);
    expect(readAutoCheck()).toBe(false);
  });

  it("reads anything but \"true\" as off", () => {
    localStorage.setItem(AUTO_CHECK_KEY, "yes");
    expect(readAutoCheck()).toBe(false);
  });

  it("round-trips the last announced version", () => {
    expect(readLastNotified()).toBeNull();
    markNotified("0.3.0");
    expect(localStorage.getItem(LAST_NOTIFIED_KEY)).toBe("0.3.0");
    expect(readLastNotified()).toBe("0.3.0");
  });
});
