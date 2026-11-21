# Comprehensive Settings, Custom Headers & Context Menu

This document summarizes the configurable `Settings`, custom HTTP header / cURL support, and download row context menu implemented across `posthaze-engine` and `posthaze-ui`.

## Comprehensive Settings (`SettingsModalState`)

The Settings modal uses a 9-tab left-navigation layout (`General`, `Interface`, `Downloads`, `Network`, `Notifications`, `Automation`, `Integration`, `Updates`, `Reset`) backed by SQLite (`0004_extended_settings.sql` through `0009_auto_check_updates.sql`):

- **General:** Default download folder, optional file-type subfolder auto-categorization (`Archives`, `Video`, `Audio`, `Documents`, `Programs` via `categorize_subfolder`), close-to-tray (`ui_close_to_tray`), and launch on Windows startup (`launch_on_startup`).
- **Interface:** 8 built-in theme presets (5 opaque themes plus Windows 11 22H2+ `Mica (System)`, `Mica Dark`, and `Mica Light` with automatic opaque fallback on Windows 10 / High Contrast / Transparency Effects off), accent override, row height density, visible columns, toolbar labels, scrollbar minimap, titlebar telemetry, `ui_click_row_opens_inspector`, `ui_show_add_paused`, `ui_lock_file_extension`, and `ui_zoom_percent` (`90%`, `100%`, `110%`, `125%`).
- **Downloads:** File-exists collision policy (`CollisionPolicy::{Rename, Overwrite, Ask}`), row delete button default (`DeleteDefaultAction::{Ask, Remove, DeleteFile}`), and automatic retry count for transient failures (`auto_retry_count`).
- **Network:** Proxy routing (`ProxyMode::{System, None, Manual}`), global download speed limit (`BandwidthLimiter` with `Low` / `Medium` / `High` / `Unlimited` presets or custom KiB/s), `max_workers` (`1..32`), `max_concurrent_downloads`, and `sync_on_complete`.
- **Notifications:** Windows Toast notifications (`notify_on_added`, `notify_on_completed`, `notify_on_failed`, `notify_only_inactive`).
- **Automation:** Post-completion external command hook (`%path%` substitution) with a 1-click Windows Defender (`MpCmdRun.exe`) preset.
- **Integration:** One-click Browser Capture Native Messaging Host registration (`com.posthaze.host`) for Chrome, Edge, and Brave.
- **Updates:** Current version and install mode (`INSTALLED` / `PORTABLE`), automatic update check toggle (`auto_check_updates`, startup + 24h), **Check Now**, signed silent install (`INSTALL v{x}` on installed builds), and GitHub release notes link.
- **Reset:** Copy Config JSON, Open Logs Folder, and one-click `Reset All Settings to Defaults` restoring `Settings::default()`.

## Custom Headers & Paste as cURL

- `DownloadEngine::probe_with_headers` and `DownloadRequest.headers` persist custom request headers (`Cookie`, `Referer`, `User-Agent`) in SQLite (`0003_request_headers.sql`) so pause and resume preserve session context.
- `posthaze-ui` (`curl_parser.rs`) parses both plain URLs and Browser DevTools *Copy as cURL* commands (Bash and Windows `cmd` `^"` syntax) in the URL bar and **Paste & Probe**.

## Download Row Context Menu & File Operations

Right-clicking a download row opens `context_menu.rs` with state-aware transfer controls (`Pause`, `Resume`, `Restart Fresh` / `Download Again`, `Cancel Transfer`), file/link actions (`Open File`, `Show in Folder`, `Copy URL`, `Inspect Byte-Map`), `Completed`-file operations (`File Integrity (BLAKE3)`, `Rename File...`, `Move To...` backed by `DownloadEngine::compute_file_blake3`, `rename_completed_file`, and `move_completed_file`), and explicit `Remove from List` vs `Delete File & Entry` actions.
