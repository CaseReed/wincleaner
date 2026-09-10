#!/usr/bin/env node
// Extracts the CHANGELOG.md section of the version declared in package.json
// into `src/generated/whats-new.json`, which the Settings screen imports.
// Build-time only: the application itself never reads CHANGELOG.md, and never
// reaches the network. Node only, no dependencies.

import { mkdirSync, readFileSync, writeFileSync } from "node:fs";
import { fileURLToPath, pathToFileURL } from "node:url";
import { dirname, join } from "node:path";

const root = join(dirname(fileURLToPath(import.meta.url)), "..");

/// Escapes what would otherwise be regular-expression syntax: a version string
/// carrying `.` or `|` must match literally, not as a pattern.
function escapeRegExp(text) {
  return text.replace(/[.*+?^${}()|[\]\\]/g, "\\$&");
}

/// Returns `{ version, date, body }` for `## [<version>] - <date>` in the given
/// changelog text. `body` is the section without its heading, trimmed, with the
/// trailing link definitions of the last section removed. Throws with a clear
/// message when the section is missing or empty.
export function extractSection(changelog, version) {
  // A checkout with `core.autocrlf` on carries \r\n: normalise once, so the
  // generated JSON never depends on how the repository was cloned.
  const text = changelog.replace(/\r\n/g, "\n");
  const heading = new RegExp(
    String.raw`^## \[${escapeRegExp(version)}\][ \t]*(?:-[ \t]*(\S+))?[ \t]*$`,
    "m",
  );
  const match = text.match(heading);
  if (!match) {
    throw new Error(
      `CHANGELOG.md has no "## [${version}]" section. Add one before releasing ${version}.`,
    );
  }

  const start = match.index + match[0].length;
  const rest = text.slice(start);
  const next = rest.search(/^## /m);
  let body = next === -1 ? rest : rest.slice(0, next);
  // The link definitions closing the file belong to no section: without this
  // they would land in the body of the oldest version.
  body = body
    .split("\n")
    .filter((line) => !/^\[[^\]]+\]:\s/.test(line))
    .join("\n")
    .trim();

  if (body === "") {
    throw new Error(`The "## [${version}]" section of CHANGELOG.md is empty.`);
  }

  return { version, date: match[1] ?? "", body };
}

function main() {
  const version = JSON.parse(readFileSync(join(root, "package.json"), "utf8")).version;
  const changelog = readFileSync(join(root, "CHANGELOG.md"), "utf8");

  let section;
  try {
    section = extractSection(changelog, version);
  } catch (err) {
    console.error(err.message);
    process.exit(1);
  }

  const out = join(root, "src", "generated", "whats-new.json");
  mkdirSync(dirname(out), { recursive: true });
  writeFileSync(out, `${JSON.stringify(section, null, 2)}\n`, "utf8");
  console.log(`What's new written for ${version}: src/generated/whats-new.json`);
}

// Only when run as a script: importing this file (tests, check-version.mjs)
// must not write anything.
if (process.argv[1] && import.meta.url === pathToFileURL(process.argv[1]).href) {
  main();
}
