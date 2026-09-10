import { describe, it, expect } from "vitest";
import { readFileSync } from "node:fs";
import { fileURLToPath } from "node:url";
import { dirname, join } from "node:path";
import { extractSection, toPlainText } from "./extract-whats-new.mjs";

const CHANGELOG = `# Changelog

All notable changes to this project are documented in this file.

## [Unreleased]

### Added

- Something not released yet.

## [0.2.0] - 2026-09-10

### Added

- A first entry that wraps
  over two lines.
- A second entry.

### Changed

- A changed entry.

## [0.1.0] - 2026-09-01

Initial MVP release.

### Added

- The very first entry.

[Unreleased]: https://example.invalid/compare/v0.2.0...HEAD
[0.2.0]: https://example.invalid/compare/v0.1.0...v0.2.0
[0.1.0]: https://example.invalid/releases/tag/v0.1.0
`;

describe("extractSection", () => {
  it("returns the version, the date and the body without the heading", () => {
    const section = extractSection(CHANGELOG, "0.2.0");
    expect(section.version).toBe("0.2.0");
    expect(section.date).toBe("2026-09-10");
    expect(section.body).toBe(
      [
        "### Added",
        "",
        "- A first entry that wraps",
        "  over two lines.",
        "- A second entry.",
        "",
        "### Changed",
        "",
        "- A changed entry.",
      ].join("\n"),
    );
  });

  it("stops at the next version heading", () => {
    expect(extractSection(CHANGELOG, "0.2.0").body).not.toContain("0.1.0");
    expect(extractSection(CHANGELOG, "0.2.0").body).not.toContain("very first");
  });

  it("drops the trailing link definitions of the last section", () => {
    const section = extractSection(CHANGELOG, "0.1.0");
    expect(section.body).toBe(
      ["Initial MVP release.", "", "### Added", "", "- The very first entry."].join("\n"),
    );
    expect(section.body).not.toContain("https://example.invalid");
  });

  it("reads a CRLF changelog the same way", () => {
    const section = extractSection(CHANGELOG.replace(/\n/g, "\r\n"), "0.2.0");
    expect(section.date).toBe("2026-09-10");
    expect(section.body).not.toContain("\r");
    expect(section.body.startsWith("### Added")).toBe(true);
  });

  it("accepts a section with no date", () => {
    const section = extractSection("## [9.9.9]\n\n- Dateless.\n", "9.9.9");
    expect(section.date).toBe("");
    expect(section.body).toBe("- Dateless.");
  });

  it("throws when the section is missing", () => {
    expect(() => extractSection(CHANGELOG, "0.3.0")).toThrow(/0\.3\.0/);
  });

  it("throws when the section is empty", () => {
    expect(() => extractSection("## [0.2.0] - 2026-09-10\n\n## [0.1.0]\n", "0.2.0")).toThrow(
      /empty/i,
    );
  });

  it("does not treat the version as a regular expression", () => {
    expect(() => extractSection(CHANGELOG, "0.2.0|0.1.0")).toThrow();
    expect(() => extractSection(CHANGELOG, "0x2x0")).toThrow();
  });
});

describe("toPlainText", () => {
  it("upper-cases a heading and adds a blank line before it unless it opens the text", () => {
    expect(toPlainText("### Added")).toBe("ADDED");
    expect(toPlainText("#### Added")).toBe("ADDED");
    expect(toPlainText("- one\n### Changed")).toBe("• one\n\nCHANGED");
  });

  it("drops the URL from a markdown link but keeps a bare URL untouched", () => {
    expect(toPlainText("[Winapp2](https://example.invalid/x)")).toBe("Winapp2");
    expect(toPlainText("See https://example.invalid/x directly.")).toBe(
      "See https://example.invalid/x directly.",
    );
  });

  it("removes inline code backticks, keeping the content", () => {
    expect(toPlainText("Reads `rules.toml` at startup.")).toBe("Reads rules.toml at startup.");
  });

  it("removes bold and italic markers, keeping the text", () => {
    expect(toPlainText("A **bold** and *italic* word.")).toBe("A bold and italic word.");
  });

  it("turns list markers into a bullet and preserves continuation indentation", () => {
    expect(toPlainText("- one\n  wraps here\n* two")).toBe("• one\n  wraps here\n• two");
  });

  it("collapses 3+ consecutive blank lines to 2", () => {
    expect(toPlainText("one\n\n\n\ntwo")).toBe("one\n\ntwo");
  });

  it("renders the real [0.2.0] section as clean plain text", () => {
    const changelog = readFileSync(join(dirname(fileURLToPath(import.meta.url)), "..", "CHANGELOG.md"), "utf8");
    const section = extractSection(changelog, "0.2.0");
    const body = toPlainText(section.body);
    expect(body).not.toContain("###");
    expect(body).not.toContain("](");
    expect(body).not.toContain("`");
    expect(body).toContain("ADDED");
  });
});
