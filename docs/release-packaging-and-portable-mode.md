# Release Packaging & Portable Mode

## Release Build Profile

The workspace `Cargo.toml` defines `[profile.release]` with `lto = true`, `codegen-units = 1`, and `strip = "symbols"`. On Windows, `posthaze-ui` enables `#![cfg_attr(not(debug_assertions), windows_subsystem = "windows")]` so release launches do not open a background console window while debug and test runs retain console output.

## Portable Data Directory (`posthaze.portable`)

`posthaze-engine` exposes `resolve_data_dir`, `default_data_dir`, `default_logs_dir`, and `default_db_path`:

- **Standard mode (default):** Stores `state.db` and `logs/` under `%LOCALAPPDATA%\posthaze\` (or `$HOME/.local/share/posthaze/`).
- **Portable mode:** When a `posthaze.portable` marker file exists in the same directory as the executable, both `posthaze-ui` and `posthaze-cli` automatically store `state.db` and `logs/` under `<exe_dir>\data\`.
