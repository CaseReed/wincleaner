#!/usr/bin/env node
// Downloads the upstream Winapp2 rule base into src-tauri/third_party/winapp2/.
// Developer tool, run by hand: the application itself never touches the
// network — the ini is embedded in the binary with include_str!.
// Node only, no dependencies.

import { existsSync, mkdirSync, writeFileSync } from "node:fs";
import { fileURLToPath } from "node:url";
import { dirname, join } from "node:path";

const INI_URL =
  "https://raw.githubusercontent.com/MoscaDotTo/Winapp2/master/Non-CCleaner/Winapp2.ini";
const LICENSE_URL = "https://creativecommons.org/licenses/by-sa/4.0/legalcode.txt";

const root = join(dirname(fileURLToPath(import.meta.url)), "..");
const dir = join(root, "src-tauri", "third_party", "winapp2");

async function download(url) {
  const response = await fetch(url);
  if (!response.ok) {
    console.error(`GET ${url} failed: ${response.status} ${response.statusText}`);
    process.exit(1);
  }
  return await response.text();
}

mkdirSync(dir, { recursive: true });

// A UTF-8 BOM would end up as the first character of the include_str! literal
// and break the first section header: strip it here, once.
const ini = (await download(INI_URL)).replace(/^\uFEFF/, "");
if (ini.length < 500_000 || !ini.includes("[")) {
  console.error(
    `Refusing to write: ${INI_URL} returned ${ini.length} bytes with no section header.`,
  );
  process.exit(1);
}
writeFileSync(join(dir, "Winapp2.ini"), ini);

// The licence text never changes: fetched once, then left alone.
const licensePath = join(dir, "LICENSE");
if (!existsSync(licensePath)) {
  writeFileSync(licensePath, await download(LICENSE_URL));
}

const date = new Date().toISOString().slice(0, 10);
writeFileSync(
  join(dir, "NOTICE"),
  `Winapp2.ini — community-maintained cleaning rule base

Source: https://github.com/MoscaDotTo/Winapp2 (Non-CCleaner/Winapp2.ini)
Downloaded: ${date}
License: Creative Commons Attribution-ShareAlike 4.0 International (CC-BY-SA-4.0)
         Full text in the LICENSE file next to this notice, also at
         https://creativecommons.org/licenses/by-sa/4.0/legalcode.txt

WinCleaner converts these entries into its own cleaning rules at startup. The
converted rules are data under CC-BY-SA-4.0; the WinCleaner application code
stays MIT.
`,
);

console.log(`Winapp2.ini updated: ${ini.length} bytes, snapshot ${date}.`);
console.log("Now add a line under [Unreleased] in CHANGELOG.md recording that snapshot date.");
