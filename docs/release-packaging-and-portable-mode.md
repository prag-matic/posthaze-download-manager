# Release Packaging & Portable Mode

## Release Build Profile

The workspace `Cargo.toml` defines `[profile.release]` with `lto = true`, `codegen-units = 1`, and `strip = "symbols"`. On Windows, `posthaze-ui` enables `#![cfg_attr(not(debug_assertions), windows_subsystem = "windows")]` so release launches do not open a background console window while debug and test runs retain console output.

## Portable Data Directory (`posthaze.portable`)

`posthaze-engine` exposes `resolve_data_dir`, `default_data_dir`, `default_logs_dir`, and `default_db_path`:

- **Standard mode (default):** Stores `state.db` and `logs/` under `%LOCALAPPDATA%\posthaze\` (or `$HOME/.local/share/posthaze/`).
- **Portable mode:** When a `posthaze.portable` marker file exists in the same directory as the executable, both `posthaze-ui` and `posthaze-cli` automatically store `state.db` and `logs/` under `<exe_dir>\data\`.

## Diagnostic Logs & Panic Hook

- **Rolling File Logs:** `posthaze-ui` initializes a `tracing` file subscriber at startup writing `posthaze.log` into `default_logs_dir()` (`<data_dir>\logs\`), rotating at `2 MiB` and retaining up to `5` historical log files (`posthaze.1.log` .. `posthaze.5.log`).
- **Panic Hook:** Unexpected panics record their payload and source location into `posthaze.log` before terminating.
- **Open Logs Folder:** **Settings → Reset → OPEN LOGS FOLDER** ensures the logs directory exists and opens it directly in Windows Explorer.

## Single-Instance Wake & `--minimized` Launch

- **Single-Instance Wake:** Launching `posthaze-ui` without `--minimized` checks if the named-pipe IPC server is already listening. If so, it sends `{"type":"show"}` (`PipeCommand::ShowMainWindow`) over the pipe to restore and focus the running main window and exits immediately.
- **`--minimized` Flag:** Launching `posthaze-ui --minimized` creates the main window minimized and unfocused so background **Browser Capture** stays active without interrupting the desktop.
