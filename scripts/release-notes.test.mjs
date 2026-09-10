import { describe, it, expect } from "vitest";
import { buildReleaseNotes } from "./release-notes.mjs";

const CHANGELOG = `# Changelog

## [Unreleased]

## [0.2.0] - 2026-09-10

### Added

- A first entry.
- A second entry.

## [0.1.0] - 2026-09-01

Initial MVP release.

### Added

- The very first entry.

[Unreleased]: https://example.invalid/compare/v0.2.0...HEAD
[0.2.0]: https://example.invalid/compare/v0.1.0...v0.2.0
[0.1.0]: https://example.invalid/releases/tag/v0.1.0
`;

describe("buildReleaseNotes", () => {
  it("carries the raw markdown CHANGELOG section, then the unsigned note and the changelog link", () => {
    const notes = buildReleaseNotes(CHANGELOG, "0.2.0");
    expect(notes).toBe(
      [
        "### Added",
        "",
        "- A first entry.",
        "- A second entry.",
        "",
        "Installers are unsigned until SignPath signing is configured in the repository secrets — see docs/code-signing.md.",
        "Full changelog: https://github.com/CaseReed/wincleaner/blob/main/CHANGELOG.md",
        "",
      ].join("\n"),
    );
  });

  it("keeps markdown syntax untouched, unlike the plain-text conversion", () => {
    const notes = buildReleaseNotes(
      "## [1.0.0]\n\n- Reads `rules.toml` and a [link](https://example.invalid).\n",
      "1.0.0",
    );
    expect(notes).toContain("`rules.toml`");
    expect(notes).toContain("[link](https://example.invalid)");
  });

  it("throws clearly when the section is missing", () => {
    expect(() => buildReleaseNotes(CHANGELOG, "9.9.9")).toThrow(/9\.9\.9/);
  });
});
