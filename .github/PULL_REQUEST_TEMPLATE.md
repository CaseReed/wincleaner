## What

<!-- What this change modifies, in one or two sentences. -->

## Why

<!-- The problem or the need that motivates this change. -->

## Checks run

- [ ] `npm test`
- [ ] `npm run build`
- [ ] `cargo test --manifest-path src-tauri/Cargo.toml -- --test-threads=1`
- [ ] `cargo clippy --manifest-path src-tauri/Cargo.toml -- -D warnings`
- [ ] The relevant manual check in `docs/manual-verification.md` (if applicable)
