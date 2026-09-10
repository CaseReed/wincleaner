#!/usr/bin/env node
// Builds a GitHub release body: the raw markdown CHANGELOG.md section for the
// given version, followed by the unsigned-installers note and a link to the
// full changelog. Used by `.github/workflows/release.yml`; the CHANGELOG
// section itself is reused verbatim (`extractSection`'s `body` is already
// raw markdown, not the plain-text conversion `extract-whats-new.mjs` writes
// for the app). Node only, no dependencies.

import { readFileSync, writeFileSync } from "node:fs";
import { fileURLToPath, pathToFileURL } from "node:url";
import { dirname, join } from "node:path";
import { extractSection } from "./extract-whats-new.mjs";

const root = join(dirname(fileURLToPath(import.meta.url)), "..");

const UNSIGNED_NOTE =
  "Installers are unsigned until SignPath signing is configured in the repository secrets — see docs/code-signing.md.";
const CHANGELOG_LINK =
  "Full changelog: https://github.com/CaseReed/wincleaner/blob/main/CHANGELOG.md";

/// Returns the full release body markdown for `version`: the raw CHANGELOG
/// section, then a blank line, then the unsigned note and the changelog link.
export function buildReleaseNotes(changelog, version) {
  const { body } = extractSection(changelog, version);
  return `${body}\n\n${UNSIGNED_NOTE}\n${CHANGELOG_LINK}\n`;
}

function main() {
  const version = process.argv[2];
  if (!version) {
    console.error("Usage: node scripts/release-notes.mjs <version> [out-file]");
    process.exit(1);
  }
  const outFile = process.argv[3];

  const changelog = readFileSync(join(root, "CHANGELOG.md"), "utf8");

  let notes;
  try {
    notes = buildReleaseNotes(changelog, version);
  } catch (err) {
    console.error(err.message);
    process.exit(1);
  }

  if (outFile) {
    writeFileSync(outFile, notes, "utf8");
  } else {
    process.stdout.write(notes);
  }
}

// Only when run as a script: importing this file (tests) must not write
// anything or read from disk.
if (process.argv[1] && import.meta.url === pathToFileURL(process.argv[1]).href) {
  main();
}
