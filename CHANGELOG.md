# Changelog

All notable changes to Posthaze Download Manager are documented in this file.
When a `vX.Y.Z` tag is pushed, `.github/workflows/release.yml` automatically uses the matching `## [X.Y.Z]` section below as the GitHub Release notes (falling back to GitHub's auto-generated notes if no section is present).

## [0.2.1] - 2026-11-23

### What's New
- **Drag & Drop Files Out of Posthaze UI:** Drag single or multi-selected completed downloads directly from the download table into Windows Explorer, the Desktop, browsers, or chat apps via native Windows OLE (`CF_HDROP`).
- **Windows Platform Performance:**
  - Enabled Windows 11 EcoQoS (`ProcessPowerThrottling`) while hidden in the System Tray with zero active downloads.
  - Replaced `reg.exe` subprocesses with in-process Win32 Registry APIs for browser Native Messaging registration.
  - Added `FILE_FLAG_SEQUENTIAL_SCAN` to `compute_file_blake3` integrity checks.

### Fixes
- **Shell Open Crash Fix:** Offloaded `ShellExecuteW` / `explorer.exe` launches off the GPUI main thread with COM STA initialization to prevent `RefCell already borrowed` panics when opening downloaded files (including `.zip` archives).
- **Windows Known Downloads Folder:** `default_downloads_dir()` now queries `SHGetKnownFolderPath(FOLDERID_Downloads)` so custom or relocated Windows Downloads folders are honored.
- **Chrome Extension History & Offline Fallback:** Ignored historical/paused Chrome downloads on `onCreated` and added a native-host `ping` check so Chrome keeps downloads when Posthaze is not running.
- **cURL & Probe Reliability:** Fixed Windows `cmd` Copy-as-cURL `^\^"` inner-quote unescaping and stripped `If-Modified-Since` / `If-None-Match` headers during probes.

## [0.2.0] - 2026-11-22

### What's New
- **Windows 11 Mica Themes:** Added **Mica (System)**, **Mica Dark**, and **Mica Light** themes with automatic light/dark switching and clean fallback on Windows 10.
- **Lower RAM & Idle CPU (Same Full Speed):**
  - Cut transfer memory usage by up to 4× during heavy or multi-file downloads while improving sustained disk write speeds.
  - Paused background UI rendering and removed tray polling when minimized to the System Tray (near-0% idle CPU).
  - Made large download lists smoother with less memory churn and faster SQLite checkpoints.
- **Built-In Auto-Updater:** Automatically checks GitHub Releases for updates with Ed25519 + SHA-256 verification and one-click silent upgrades (update alerts only in portable mode).
- **Dynamic Top Toolbar:** Shows quick, labelled actions whenever you select one or more downloads.
- **Redesigned Windows 11 Installer:** Follows Windows Light/Dark mode with new Posthaze branding, adds `posthaze-cli` to your user `PATH` by default, safely closes tray instances during upgrades, and cleans up browser integration on uninstall.

## [0.1.1] - 2026-11-20

- Initial public Windows release with multi-connection HTTP/HTTPS engine, SQLite crash recovery, GPUI desktop UI, System Tray support, Browser Capture Native Messaging Host, and portable mode.
