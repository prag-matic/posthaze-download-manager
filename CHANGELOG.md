# Changelog

All notable changes to Posthaze Download Manager are documented in this file.
When a `vX.Y.Z` tag is pushed, `.github/workflows/release.yml` automatically uses the matching `## [X.Y.Z]` section below as the GitHub Release notes (falling back to GitHub's auto-generated notes if no section is present).

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
