#!/usr/bin/env node
// Fails if package.json, src-tauri/Cargo.toml, src-tauri/Cargo.lock (the
// wincleaner package entry) and src-tauri/tauri.conf.json don't all declare
// the same version, or if CHANGELOG.md has no section for it. Node only, no
// dependencies.

import { readFileSync } from "node:fs";
import { fileURLToPath } from "node:url";
import { dirname, join } from "node:path";
import { extractSection } from "./extract-whats-new.mjs";

const root = join(dirname(fileURLToPath(import.meta.url)), "..");

function readJson(path) {
  return JSON.parse(readFileSync(join(root, path), "utf8"));
}

function readText(path) {
  return readFileSync(join(root, path), "utf8");
}

function firstMatch(text, re, label, source) {
  const m = text.match(re);
  if (!m) {
    console.error(`Could not find ${label} in ${source}`);
    process.exit(1);
  }
  return m[1];
}

const packageJsonVersion = readJson("package.json").version;
const tauriConfVersion = readJson("src-tauri/tauri.conf.json").version;

const cargoToml = readText("src-tauri/Cargo.toml");
const cargoTomlVersion = firstMatch(
  cargoToml,
  /^\[package\][\s\S]*?^version\s*=\s*"([^"]+)"/m,
  "package version",
  "src-tauri/Cargo.toml",
);

const cargoLock = readText("src-tauri/Cargo.lock");
const cargoLockVersion = firstMatch(
  cargoLock,
  /name = "wincleaner"\r?\nversion = "([^"]+)"/,
  "wincleaner package entry",
  "src-tauri/Cargo.lock",
);

const versions = {
  "package.json": packageJsonVersion,
  "src-tauri/tauri.conf.json": tauriConfVersion,
  "src-tauri/Cargo.toml": cargoTomlVersion,
  "src-tauri/Cargo.lock": cargoLockVersion,
};

const unique = new Set(Object.values(versions));

if (unique.size > 1) {
  console.error("Version mismatch across packaging files:");
  for (const [file, version] of Object.entries(versions)) {
    console.error(`  ${file}: ${version}`);
  }
  process.exit(1);
}

// The Settings screen shows this section: a release with no CHANGELOG entry
// would ship an empty "What's new" — and `npm run build` would fail late.
try {
  extractSection(readText("CHANGELOG.md"), packageJsonVersion);
} catch (err) {
  console.error(err.message);
  process.exit(1);
}

console.log(`Version OK: ${packageJsonVersion} (CHANGELOG section present)`);
