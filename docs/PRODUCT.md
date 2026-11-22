# Product

<!-- impeccable:product-schema 1 -->

## Platform

windows

## Stack

Rust + GPUI & GPUI Kit (`gpui.rs` / `gpui-kit` pure-Rust GPU-accelerated UI, Tokio async runtime, `reqwest` HTTP/HTTPS client, SQLite via `sqlx`, BLAKE3 hashing, `clap` headless CLI).

## Users

Windows 10/11 x64 power users and heavy downloaders who regularly fetch large files and need IDM-class multi-connection throughput, intelligent concurrency, and reliable pause/resume/recovery without bloat.

## Product Purpose

Posthaze Download Manager is a Windows-native download manager built to maximize sustainable HTTP/HTTPS download throughput, minimize CPU and RAM usage, and deliver reliable pause, resume, and crash recovery in a polished native desktop application.

## Positioning

Unlike browser-built-in downloaders or legacy bloated download managers, Posthaze pairs a headless, benchmarkable Rust multi-connection download engine (HTTP Range segmentation, adaptive worker pool, retry/resume management, SQLite persistence) with a lightweight native GPUI + GPUI Kit desktop interface.

## Operating Context

- **Desktop surfaces:** Dashboard, Download Queue, and Settings views rendered in GPUI + GPUI Kit on Windows 10/11.
- **Headless / CLI usage:** The core Rust download engine runs independently of the GUI for automated tests, benchmarks, and headless CLI downloads (`clap`).
- **Persistence & OS integration:** SQLite state persistence, Windows filesystem writes, and OS notifications.

## Capabilities and Constraints

- **v1 Scope:** Windows 10/11 x64, HTTP/HTTPS only, segmented downloading via HTTP Range requests, async multi-connection worker pool, pause/resume/recovery, SQLite persistence, and GPUI + GPUI Kit desktop UI.
- **Strict architectural boundary:** The GPUI + GPUI Kit UI (`posthaze-ui`) must remain completely decoupled from the core download engine and must never directly manage HTTP connections or file segments.
- **v1 Non-goals:** BitTorrent, video extraction, FTP/SFTP, browser extensions, and cloud synchronization are deferred until the core HTTP engine is stable.

## Brand Commitments

- **Product name:** Posthaze Download Manager (working name in early architecture doc: Velocity Download Manager / VDM).

## Evidence on Hand

- Architecture and technical specification in [`docs/Architecture Proposal - Windows Download Manager.md`](docs/Architecture%20Proposal%20-%20Windows%20Download%20Manager.md).
- No UI implementation, screenshots, or visual assets exist yet.

## Product Principles

1. **Throughput without bloat:** Maximize sustainable bandwidth utilization while keeping CPU and RAM overhead minimal.
2. **Engine independence:** Every core download capability must work and be testable headlessly, independent of the GPUI desktop UI.
3. **Resilient by default:** Pause, resume, retry, and crash recovery are first-class invariants backed by SQLite state.
4. **Native Windows craft:** Respect Windows 10/11 desktop expectations for clarity, scanability, and responsiveness.
