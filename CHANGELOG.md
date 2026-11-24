# Changelog

All notable changes to Posthaze Download Manager are documented in this file.
When a `vX.Y.Z` tag is pushed, `.github/workflows/release.yml` automatically uses the matching `## [X.Y.Z]` section below as the GitHub Release notes (falling back to GitHub's auto-generated notes if no section is present).

## [0.2.2] - 2026-11-24

### What's New
- **Standalone Settings Window & 6-Tab Cleanup:** Upgraded Settings into its own resizable OS window (`Ctrl+,`, tray menu, or TitleBar update chip) and reorganized options into 6 clean tabs (`General`, `Downloads`, `Network`, `Appearance`, `Integrations`, `System`).
- **In-Process Windows Toast Notifications:** Replaced external PowerShell notification spawns with native WinRT toasts (`Posthaze.DownloadManager` AUMID) that open/select transfers on click, offer **Open File**, **Show in Folder**, and **Retry** action buttons, display cleaner 3-line details, and include a new opt-in **Download Started** toggle.
- **File Integrity Verification & Recycle Bin Safety:**
  - Single-pass **Verify Checksum / File Integrity** modal computing `BLAKE3`, `SHA-256`, `SHA-512`, `SHA-1`, `MD5`, and `CRC32` with paste-to-verify (`PASTE` / raw / Base64 / `Repr-Digest`) and server digest matching.
  - Configurable **Windows Recycle Bin** deletion mode (`Keep on Disk`, `Recycle Bin`, `Permanent` via `Shift+Delete`).
  - Persisted destination **Folder History** quick-picks in the Add Download window.
- **Browser Extension Wave 2:** Added CHIPS partitioned cookie capture, active-tab `Referer` fallback, Chrome `DownloadItem` byte-size forwarding with signed-URL single-stream hints, domain/extension/min-size skip filters, and pre-filled Extension ID registration using `posthaze-cli.exe` directly without `.cmd` shims.
- **Refreshed App Icon & Installer Branding:** Updated desktop app icons, tray icons, Chrome extension icons, and Windows 11 Inno Setup wizard assets.

### Fixes
- **Duplicate & Collision Handling:** Normalized `(n)` auto-renamed suffixes in duplicate-URL/file detection and reserved active/paused output paths to prevent concurrent overwrites.
- **Zero Console Flashes & Modal Polish:** Added `CREATE_NO_WINDOW` to post-download external command hooks, eliminated silent fast-completion notification drops, and aligned all in-window modal action footers and copy buttons flush against their card frames.
- **Transfer & Probe Resilience:** Added disk-space and FAT32 `4 GiB - 1` pre-flight validation, persisted HTTP redirect target URLs, and improved `429 Retry-After` and slow-worker shedding recovery.

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
