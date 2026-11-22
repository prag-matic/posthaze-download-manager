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

## System Tray, Close-to-Tray & Launch on Startup (`0008_tray_and_startup.sql`)

- **Windows System Tray (`tray.rs`):** `posthaze-ui` creates a persistent System Tray icon with **Show / Hide Posthaze**, **Pause All Downloads**, and **Quit Posthaze** menu actions (plus left-click toggle).
- **Close to System Tray (`ui_close_to_tray`, default `true`):** Closing the main window minimizes to the tray so `Active` `Download`s and **Browser Capture** stay alive; **Tray → Quit** or `Ctrl+Q` exits the process cleanly.
- **Launch on Windows Startup (`launch_on_startup`, default `false`):** Saving **Settings → General → Launch on Windows Startup** writes or removes `HKCU\Software\Microsoft\Windows\CurrentVersion\Run\Posthaze` with `"<exe>" --minimized` using the Win32 Registry API (`RegCreateKeyW` / `RegSetValueExW` / `RegDeleteValueW`).

## Built-In Browser Native Messaging Registration (`Settings → Integration`)

- **One-Click Registration (`browser_integration.rs`):** **Settings → Integration** displays live HKCU registration status for **Google Chrome**, **Microsoft Edge**, and **Brave Browser**, accepts the 32-character Extension ID, writes `posthaze-native-host.cmd` + `com.posthaze.host.json` into `default_data_dir()`, and updates the `HKCU\Software\...\NativeMessagingHosts\com.posthaze.host` registry keys.

## Windows Installer, Portable ZIP & Release Pipeline

- **Inno Setup (`packaging/windows/posthaze.iss`, Inno Setup 6.7+):** Per-user Windows installer (`PrivilegesRequired=lowest`, `MinVersion=10.0.17763`, `UsePreviousAppDir=yes`, `UsePreviousTasks=yes`) featuring:
  - **Windows 11 Dynamic Light/Dark Styling:** Uses `WizardStyle=modern dynamic windows11 hidebevels` with warm paper (`#F7F5F0`) / Chronophotographic Graphite (`#141311`) backgrounds and HiDPI (200%) light/dark PNG sidebar and header assets under `packaging/windows/assets/` (regenerated via `python scripts/generate-installer-assets.py`).
  - **Bundled Components & Tasks:** Installs `posthaze-ui.exe`, `posthaze-cli.exe`, and `extensions/chrome/` into `%LOCALAPPDATA%\Programs\Posthaze`, with Start Menu shortcuts, **Add `posthaze-cli` to user PATH** (checked by default, cleanly added/removed from `HKCU\Environment\Path`), optional Desktop / Windows Startup (`--minimized`) tasks, and `HKCU\...\App Paths` entries for `Win+R` launching.
  - **Tray-Safe Upgrades & Silent Auto-Update Compatibility:** Stops any tray-minimized `posthaze-ui.exe` before file replacement (`PrepareToInstall` / `InitializeUninstall`) while preserving `CloseApplications=yes`, `RestartApplications=yes`, and the silent-relaunch `[Run]` entry used by `updater.rs`.
  - **Clean Uninstall:** Removes Chrome/Edge/Brave `NativeMessagingHosts\com.posthaze.host` keys (`dontcreatekey uninsdeletekey`), removes `{app}` from user `PATH`, and on interactive uninstall offers an opt-in prompt (defaulting to **No**) to remove `%LOCALAPPDATA%\posthaze`.
- **Local Release Helper (`scripts/release.ps1`):** Run `./scripts/release.ps1 -Version X.Y.Z [-BuildLocal] [-Push]` to bump `Cargo.toml`, `extensions/chrome/manifest.json`, and `packaging/windows/posthaze.iss` together, refresh `Cargo.lock`, run all CI gates (`check-repo.ps1`, `cargo doc`, `cargo clippy`, `cargo test`), optionally build the local `dist\posthaze-X.Y.Z-windows-x64-setup.exe`, and optionally commit, tag, and push.
- **GitHub Actions (`.github/workflows/release.yml` & `.github/release.yml`):** Tag-triggered (`v*`) or manual (`workflow_dispatch`) workflow that enforces Inno Setup `6.7+`, runs all repo/doc/clippy/test gates, builds release binaries, packages `posthaze-<version>-windows-x64-portable.zip` and `posthaze-<version>-windows-x64-setup.exe`, signs the installer with `posthaze-keytool sign` (`UPDATER_SIGNING_KEY` secret) and cryptographically verifies `.sig` against `ACCEPTED_PUBLIC_KEYS` in `updater.rs` via `posthaze-keytool verify`, verifies `SHA256SUMS.txt`, extracts matching `## [<version>]` notes from `CHANGELOG.md` (falling back to GitHub auto-generated notes), automatically flags SemVer prerelease tags (`-rc`, `-beta`), uploads dry-run artifacts on `workflow_dispatch`, and publishes the GitHub Release immediately on `v*` tags. See [Auto Updater](auto-updater.md) for verification and key-rotation details.
