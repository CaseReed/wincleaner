import { describe, it, expect } from "vitest";
import { readFileSync, statSync } from "node:fs";
import { join } from "node:path";

const root = join(import.meta.dirname, "..", "..");

describe("third-party assets", () => {
  it("embeds the Winapp2 rule base with its licence and notice", () => {
    const dir = join(root, "src-tauri", "third_party", "winapp2");
    // A few hundred KB at the very least: a truncated download would silently
    // produce an empty rule catalogue instead of failing.
    expect(statSync(join(dir, "Winapp2.ini")).size).toBeGreaterThan(500_000);
    // Upstream now ships a comment preamble before the first section, so this
    // checks for a section header rather than requiring it at offset 0.
    expect(readFileSync(join(dir, "Winapp2.ini"), "utf8")).toContain("[");
    expect(readFileSync(join(dir, "LICENSE"), "utf8")).toContain("Attribution-ShareAlike 4.0");
    const notice = readFileSync(join(dir, "NOTICE"), "utf8");
    expect(notice).toContain("https://github.com/MoscaDotTo/Winapp2");
    expect(notice).toMatch(/Downloaded: \d{4}-\d{2}-\d{2}/);
  });

  it("credits Winapp2 in the repository notices", () => {
    const notices = readFileSync(join(root, "THIRD_PARTY_NOTICES.md"), "utf8");
    expect(notices).toContain("Winapp2");
    expect(notices).toContain("CC-BY-SA-4.0");
    expect(readFileSync(join(root, "README.md"), "utf8")).toContain("Winapp2");
  });
});
