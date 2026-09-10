# Releasing

## Cutting a release

1. Bump the version in all three places so `npm run version:check` passes:
   - `package.json` (`version`)
   - `src-tauri/Cargo.toml` (`[package].version`) — then run
     `cd src-tauri && cargo build` (or `cargo check`) once to refresh
     `Cargo.lock`'s own `wincleaner` package entry, which the check also
     reads.
   - `src-tauri/tauri.conf.json` (`version`)
2. Move the `[Unreleased]` section of `CHANGELOG.md` into a new
   `[x.y.z] - YYYY-MM-DD` section (today's date), and add the compare/tag
   links at the bottom of the file.
3. Run the checks locally before committing: `npm run version:check`,
   `npm test`, `npm run build`, `cd src-tauri && cargo test -- --test-threads=1`,
   `npm run tauri build`.
4. Commit:
   ```
   git add package.json src-tauri/Cargo.toml src-tauri/Cargo.lock src-tauri/tauri.conf.json CHANGELOG.md
   git commit -m "chore(release): vx.y.z"
   ```
5. Tag and push:
   ```
   git tag -a vx.y.z -m "vx.y.z"
   git push origin main
   git push origin vx.y.z
   ```

## What the release workflow does

Pushing a tag matching `v*` triggers `.github/workflows/release.yml` on
`windows-latest`:

1. Checks out, installs Rust and Node, `npm ci`.
2. `npm run version:check` — fails if `package.json`, `src-tauri/Cargo.toml`,
   `src-tauri/Cargo.lock` and `src-tauri/tauri.conf.json` disagree.
3. Compares the pushed tag (minus the `v` prefix) against `package.json`'s
   version and fails the run if they don't match, so a release can never ship
   under the wrong version.
4. `tauri-apps/tauri-action@v0` builds the app and publishes a GitHub Release
   named `WinCleaner <tag>`, with the MSI and NSIS installers from
   `src-tauri/target/release/bundle/` attached as release assets. The release
   is published directly (not a draft); it is marked as a prerelease when the
   tag contains a hyphen (e.g. `v0.2.0-beta.1`).
5. If the four `SIGNPATH_*` repository secrets are set, the installers are
   also submitted to SignPath for signing (see `docs/code-signing.md`); while
   they are unset, that step is skipped and the release stays unsigned.

## Where the artifacts land

- Locally: `src-tauri/target/release/bundle/msi/*.msi` and
  `src-tauri/target/release/bundle/nsis/*-setup.exe`.
- On CI: attached to the GitHub Release at
  `https://github.com/CaseReed/wincleaner/releases/tag/vx.y.z`.

## Signing note

Installers are unsigned until the SignPath OSS programme secrets are
configured. See `docs/code-signing.md` for the sign-up procedure and the
exact secret names. Until then, Smart App Control blocks execution and
SmartScreen warns on first run.

## Yanking a bad release

1. Delete the GitHub Release: `gh release delete vx.y.z --yes` (add
   `--cleanup-tag` to also delete the remote tag in one step, or do it
   separately as below).
2. Delete the tag:
   ```
   git tag -d vx.y.z
   git push origin :refs/tags/vx.y.z
   ```
3. Fix the issue, bump to a new patch version (never reuse a version number),
   and cut a fresh release following the steps above.
