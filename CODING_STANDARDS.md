# Coding Standards

Read during review. Mechanical rules live in `scripts/check-repo.ps1`, CI, and clippy; this file holds judgement calls only.

## Windows integration

- A GUI process spawned from a console or native-host binary gets `Stdio::null()` on all three streams plus `DETACHED_PROCESS`, so it never inherits the browser's pipes (`crates/posthaze-cli/src/native_host.rs`).
- Opening a file or URL uses the user's default app (`ShellExecuteW`); never choose the handler.

## UI module size

Add new UI sections of `crates/posthaze-ui/src/main.rs` as sibling modules; keep `main.rs` for wiring.

## Releases

Bump the version in `Cargo.toml`, `extensions/chrome/manifest.json`, and `packaging/windows/posthaze.iss` together; `scripts/check-repo.ps1` fails on drift.
