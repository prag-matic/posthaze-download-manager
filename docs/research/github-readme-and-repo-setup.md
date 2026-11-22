# Research: GitHub Repository Setup & `README.md` Blueprint

> **Date:** 2026-11-22
> **Repository:** [`prag-matic/posthaze-download-manager`](https://github.com/prag-matic/posthaze-download-manager) (v0.2.0)
> **Scope:** GitHub About metadata, Topics/Tags, Community Health Files, and a ready-to-use, plain-English `README.md` blueprint grounded exclusively in repository primary sources and official GitHub documentation.

---

## 1. Primary-Source Platform Constraints (GitHub Docs)

| Feature | Verified Rule / Limit | Primary Source |
|---|---|---|
| **About — Description** | Maximum **350 characters** (GitHub API/UI rejects longer strings). Keeping it under ~160 characters prevents truncation on repository search cards and social previews. | GitHub REST API & Repository Settings (`docs.github.com`) |
| **About — Topics** | Maximum **20 topics** per repository; **≤ 50 characters** per topic; **lowercase letters (`a-z`), numbers (`0-9`), and hyphens (`-`) only** (no spaces, uppercase, or underscores). | GitHub Docs — *Classifying your repository with topics* (`https://docs.github.com/en/repositories/managing-your-repositorys-settings-and-features/customizing-your-repository/classifying-your-repository-with-topics`) |
| **Theme-Aware Images** | Use standard HTML `<picture>` with `<source media="(prefers-color-scheme: dark)">` and `<source media="(prefers-color-scheme: light)">`. The older `#gh-dark-mode-only` / `#gh-light-mode-only` URL fragments are deprecated. | GitHub Docs — *Basic writing and formatting syntax: Specifying the theme an image is shown to* (`https://docs.github.com/en/get-started/writing-on-github/getting-started-with-writing-and-formatting-on-github/basic-writing-and-formatting-syntax#specifying-the-theme-an-image-is-shown-to`) |
| **Collapsible Sections** | Native HTML `<details>` and `<summary>` blocks render cleanly on GitHub and support full GitHub-Flavored Markdown inside them (keep a blank line after `</summary>`). Ideal for hiding dense technical specs or CLI tables from casual downloaders. | GitHub Docs — *Organizing information with collapsed sections* (`https://docs.github.com/en/get-started/writing-on-github/working-with-advanced-formatting/organizing-information-with-collapsed-sections`) |
| **Workflow Status Badges** | Official URL pattern: `https://github.com/<OWNER>/<REPO>/actions/workflows/<WORKFLOW_FILE>/badge.svg` (optional `?branch=main`). | GitHub Docs — *Adding a workflow status badge* (`https://docs.github.com/en/actions/monitoring-and-troubleshooting-workflows/monitoring-workflows/adding-a-workflow-status-badge`) |
| **Community Health Profile** | Evaluated under **Insights → Community Standards**. Checks for: `README.md`, `LICENSE` (or `LICENSE.md`), `CONTRIBUTING.md`, `CODE_OF_CONDUCT.md`, `SECURITY.md`, Issue templates (`.github/ISSUE_TEMPLATE/`), and Pull Request template (`.github/PULL_REQUEST_TEMPLATE.md`). | GitHub Docs — *About community profiles for public repositories* (`https://docs.github.com/en/communities/setting-up-your-project-for-healthy-contributions/about-community-profiles-for-public-repositories`) |

---

## 2. GitHub Repository **About** Section

Go to the repository homepage → click the gear icon (**⚙️**) next to **About**.

### Recommended Description (164 characters — well within GitHub's 350-character limit)

```text
Fast, lightweight Windows 10/11 download manager built in Rust & GPUI. Multi-connection acceleration, crash-safe resume, Mica themes, and Chrome/Edge/Brave capture.
```

*(Crisp, plain-English, scannable, zero buzzword fluff. Shorter 129-char alternative if you prefer single-line mobile fit:)*

```text
Fast, lightweight Windows download manager built in Rust & GPUI — multi-connection speed, crash-safe resume, and browser capture.
```

### Website URL
- Point to the **Latest Release** page until a dedicated landing page exists:
  `https://github.com/prag-matic/posthaze-download-manager/releases/latest`

### Sidebar Toggles
- **Releases:** Checked (displays the latest version badge & download link in the sidebar).
- **Packages / Deployments:** Unchecked (reduces visual noise).

---

## 3. GitHub Repository **Topics / Tags** (20 of 20)

All 20 tags below strictly follow GitHub's rules (lowercase, numbers, hyphens only; ≤ 50 chars) and balance high-volume discovery tags with specific Windows/Rust/GPUI ecosystem tags:

```text
download-manager
windows
windows-11
rust
gpui
desktop-app
native-app
http-downloader
multi-connection
chrome-extension
edge-extension
brave-extension
browser-extension
sqlite
tokio
mica
portable-app
cli
open-source
idm-alternative
```

*(20 tags total. `idm-alternative` and `download-manager` capture end-user Windows search intent; `rust`, `gpui`, `tokio`, `sqlite`, and `windows-11` capture developer discovery.)*

---

## 4. Verified Repository Facts & Benchmarks (Primary-Source Audit)

Every number, filename, and capability in the README draft below is verified directly from files in this repository:

### 4.1 Workspace & Release Metadata
- **Workspace crates** (`Cargo.toml`, `crates/*/Cargo.toml`): `posthaze-engine`, `posthaze-cli`, `posthaze-ui`, `posthaze-keytool` (`publish = false`).
- **Rust edition & license** (`Cargo.toml`): `edition = "2024"`, `license = "MIT"`, `version = "0.2.0"`.
- **CI & Release workflows** (`.github/workflows/ci.yml`, `.github/workflows/release.yml`):
  - `ci.yml` runs on `push`/`pull_request` to `main` on `windows-latest` (`check-repo.ps1`, Inno Setup 6.7+ check, `cargo test --workspace`, `cargo clippy -- -D warnings`, `cargo doc` with `-D warnings`).
  - `release.yml` publishes `posthaze-<ver>-windows-x64-setup.exe`, `posthaze-<ver>-windows-x64-setup.exe.sig`, `posthaze-<ver>-windows-x64-portable.zip`, and `SHA256SUMS.txt`.
- **Installer & Portable Mode** (`docs/release-packaging-and-portable-mode.md`, `.github/workflows/release.yml`):
  - Per-user Inno Setup 6.7+ installer (`PrivilegesRequired=lowest`, Windows 10 1809+ / Windows 11), bundles `posthaze-ui.exe`, `posthaze-cli.exe`, and `extensions/chrome/` into `%LOCALAPPDATA%\Programs\Posthaze`, adds `posthaze-cli` to user `PATH` by default.
  - Portable ZIP includes a `posthaze.portable` marker file so `state.db` and `logs/` stay in `<exe_dir>\data\`.
- **Auto-Updater** (`docs/auto-updater.md`): Checks GitHub Releases on startup and every 24 hours; verifies both **SHA-256** and **Ed25519** signatures before silent upgrade (portable builds show an update notice + GitHub link and never self-overwrite).
- **Themes** (`docs/creating-themes.md`): 8 built-in themes — 5 opaque (`Chronophotographic`, `Blueprint Cyanotype`, `Phosphor Amber`, `Nordic Frost`, `Archival Paper`) + 3 Windows 11 Mica themes (`Mica (System)`, `Mica Dark`, `Mica Light`) with automatic fallback on Windows 10.

### 4.2 Available Screenshots (`docs/screenshots/`)
All 5 files currently exist in `docs/screenshots/`:
1. `docs/screenshots/posthaze-chronophotographic.png` — Chronophotographic Dark (default dark theme)
2. `docs/screenshots/posthaze-archival-light.png` — Archival Light (light theme)
3. `docs/screenshots/posthaze-mica-dark.png` — Windows 11 Mica Dark translucent theme
4. `docs/screenshots/installer-welcome.png` — Windows 11 Installer (Welcome screen)
5. `docs/screenshots/installer-options.png` — Windows 11 Installer (Options screen)

### 4.3 Verified Benchmarks (`docs/benchmarks/`)
- **Dynamic Work-Stealing vs. Static Segmentation** (`docs/benchmarks/ticket-04-work-stealing.md`):
  - Real-world `115.46 MiB` GitHub Release MSI (`vectorcraft-0.24.1-windows-arm64.msi`, verified SHA-256).
  - At default **8 connections (`-c 8`)**: **3.17× faster** — `38.35 s` (`3.01 MiB/s`) down to **`12.08 s`** (`9.56 MiB/s` / `80.17 Mbps`).
  - At **16 connections (`-c 16`)**: **`11.12 s`** (`10.38 MiB/s` / `87.08 Mbps`); peak speed at `-c 32`: **`22.95 MiB/s`** (`192.51 Mbps`).
- **Sustained Multi-Gbps Throughput & SQLite Checkpoint Cost** (`docs/benchmarks/ram-and-performance-optimization.md`, `docs/benchmarks/milestone-02-sqlite-checkpoint-overhead.md`):
  - Sustained `1 GiB` loopback medians: **`1.8–4.2 Gbps`** (peaks up to **`7.0 Gbps`** on `100 MiB` loopback; **`7.36 Gbps`** in Milestone 02 `c=4` DB run) using a bounded `16 MiB` per-download writer buffer pool.
  - Background SQLite WAL checkpoints add **~0–5% overhead** (within loopback variance).
- **Windows Release-Build RAM & Idle CPU** (`docs/benchmarks/ram-and-performance-optimization.md`):
  - Measured on Windows 11 Pro (`i5-11320H`, 16 GiB RAM, NVMe SSD, portable release `posthaze-ui.exe`, 10 s window after 8 s warmup):
    - Cold Idle (Visible): **`97.13 MiB`** Working Set (`84.10 MiB` Private Bytes)
    - Cold Idle (Hidden in System Tray): **`85.76 MiB`** Working Set
    - **500-Download Queue (Hidden in System Tray):** **`86.73 MiB`** Working Set (**+0.97 MiB** for 500 persisted downloads) and **`0.0000 s` CPU over 10 seconds** (0% idle CPU)
    - **500-Download Queue (Visible):** **`102.29 MiB`** Working Set (**+5.16 MiB** for 500 downloads)

### 4.4 Browser Extension Setup (`extensions/chrome/README.md`, `extensions/chrome/manifest.json`)
- Manifest V3 extension for **Google Chrome**, **Microsoft Edge**, and **Brave**.
- Permissions: `downloads`, `nativeMessaging`, `cookies`, `contextMenus`, `storage`, `<all_urls>`.
- 4-step setup: Load unpacked `extensions/chrome/` → copy 32-char Extension ID → paste into **Posthaze → Settings → Integration** & click **REGISTER NATIVE HOST** → confirm **UI ONLINE** / **HOST READY** in toolbar popup.

---

## 5. Missing GitHub Repo Essentials Checklist

Audited against the current repository root and `.github/` directory:

| File / Setting | Current Status | Priority | Why It Matters |
|---|---|---|---|
| **`LICENSE`** (repo root) | **Missing** *(only `license = "MIT"` in `Cargo.toml`)* | **Critical** | GitHub's license detector only recognizes a root `LICENSE` / `LICENSE.md` file. Without it, the About sidebar shows no license, Shields.io license badges fail, and downstream users cannot confirm legal terms at a glance. |
| **`CONTRIBUTING.md`** | **Missing** | **High** | Required by GitHub Community Standards; points contributors to `CODING_STANDARDS.md`, `scripts/check-repo.ps1`, `cargo clippy`, `cargo doc -D warnings`, and `git config core.hooksPath .githooks`. |
| **`.github/ISSUE_TEMPLATE/`** (`bug_report.yml`, `feature_request.yml`, `config.yml`) | **Missing** | **High** | Structured YAML Issue Forms ensure bug reports include Windows build, Posthaze version, install vs. portable mode, and `posthaze.log` details. |
| **`SECURITY.md`** | **Missing** | **High** | Important for a Windows app that ships an auto-updater (Ed25519 + SHA-256), handles browser session cookies via Native Messaging, and distributes unsigned binaries. Explains responsible disclosure and how to verify `SHA256SUMS.txt` / `.sig` files. |
| **`.github/PULL_REQUEST_TEMPLATE.md`** | **Missing** | **Medium** | Reminds contributors to run `cargo test --workspace`, `cargo clippy --workspace --all-targets -- -D warnings`, `RUSTDOCFLAGS="-D warnings" cargo doc --workspace --no-deps`, and `./scripts/check-repo.ps1`. |
| **Social Preview Image** (Settings → General → Social preview) | **Verify in GitHub UI** | **Medium** | Upload a `1280×640` PNG featuring the Chronophotographic Dark UI so shared links on Reddit, Hacker News, X, and Discord render cleanly. |
| **`CODE_OF_CONDUCT.md`** | **Missing** | **Optional** | Completes the remaining 100% check in GitHub's **Insights → Community Standards** panel. |
| **`.github/workflows/ci.yml` & `release.yml`** | **Present** | Done | Already live; ready for README badges. |
| **`CHANGELOG.md` & `.github/release.yml`** | **Present** | Done | Already integrated with automated GitHub Releases. |

---

## 6. Recommended `README.md` Structure & Ready-to-Use Draft

### Design Principles Applied
1. **Plain English first:** Lead with what Posthaze does for a Windows user in one sentence—save crate names, SQLite WAL details, and CLI commands for a clean collapsible **Technical Details** section.
2. **Easy on the eye:** Centered header, compact badge row (Release, CI, Platform, License), adaptive Light/Dark hero screenshot via `<picture>`, scannable 2-line feature bullets, and concise tables.
3. **Zero invented claims:** Every benchmark, memory figure, theme name, installer behavior, and crate credit is cited from verified repo files.
4. **Actionable onboarding:** Users can go from landing on the page → installing → verifying SmartScreen hash → connecting Chrome/Edge/Brave in under 2 minutes.

### Ready-to-Use `README.md` Draft

````markdown
<div align="center">

# Posthaze Download Manager

**A fast, lightweight Windows 10/11 download manager built in Rust.**  
Multi-connection acceleration, crash-safe resume, native Windows 11 Mica themes, and one-click Chrome/Edge/Brave browser capture—without Electron bloat.

[![Latest Release](https://img.shields.io/github/v/release/prag-matic/posthaze-download-manager?style=flat-square&color=E58A35)](https://github.com/prag-matic/posthaze-download-manager/releases/latest)
[![CI](https://github.com/prag-matic/posthaze-download-manager/actions/workflows/ci.yml/badge.svg?branch=main)](https://github.com/prag-matic/posthaze-download-manager/actions/workflows/ci.yml)
[![Platform](https://img.shields.io/badge/platform-Windows%2010%2F11%20x64-0078D4?style=flat-square)](https://github.com/prag-matic/posthaze-download-manager/releases/latest)
[![License: MIT](https://img.shields.io/badge/license-MIT-22C55E?style=flat-square)](Cargo.toml)

[**Download Latest Release**](https://github.com/prag-matic/posthaze-download-manager/releases/latest) · [**Browser Extension**](#browser-extension-chrome--edge--brave) · [**Benchmarks**](#benchmarks) · [**Documentation**](docs/README.md)

</div>

---

<p align="center">
  <picture>
    <source media="(prefers-color-scheme: dark)" srcset="docs/screenshots/posthaze-chronophotographic.png">
    <source media="(prefers-color-scheme: light)" srcset="docs/screenshots/posthaze-archival-light.png">
    <img src="docs/screenshots/posthaze-chronophotographic.png" alt="Posthaze Download Manager — Desktop Interface" width="100%">
  </picture>
</p>

<details>
<summary><b>More screenshots (Windows 11 Mica Dark &amp; Windows 11 Installer)</b></summary>

<br>

**Windows 11 Mica Dark**
![Posthaze — Windows 11 Mica Dark](docs/screenshots/posthaze-mica-dark.png)

**Windows 11 Installer (adapts to Windows Light/Dark mode)**
<p align="center">
  <img src="docs/screenshots/installer-welcome.png" alt="Posthaze Setup — Welcome" width="48%" />
  <img src="docs/screenshots/installer-options.png" alt="Posthaze Setup — Options" width="48%" />
</p>

</details>

---

## Why Posthaze?

- **Faster downloads automatically:** Splits files across multiple parallel connections and dynamically reassigns idle workers to finish slow segments (**3.17× faster** than static splitting in real-world tests).
- **Crash-safe pause & resume:** Progress is continuously saved to a local SQLite database. Pause anytime, reboot, or recover cleanly from dropped connections without losing downloaded data.
- **Native Windows feel, tiny footprint:** Built with pure Rust and GPU-accelerated GPUI—uses **~86 MiB RAM** and **0% CPU** when minimized to the System Tray, even with 500 downloads in your queue.
- **Browser capture for Chrome, Edge & Brave:** Hand off downloads and right-click links—including login cookies, referrers, and user-agents—straight into Posthaze.
- **8 built-in themes + Windows 11 Mica:** Includes **Mica (System)**, **Mica Dark**, **Mica Light**, and 5 opaque themes (with automatic fallback on Windows 10).
- **Power-user controls:** Speed limits, HTTP/HTTPS/SOCKS5 proxies, custom headers, **Paste as cURL**, FIFO download queues, System Tray mode, and a full headless CLI (`posthaze-cli`).
- **Signed auto-updates:** Checks GitHub Releases and verifies every installer with both **Ed25519** signatures and **SHA-256** checksums before upgrading.

---

## Quick Install

Download the latest release from [**GitHub Releases**](https://github.com/prag-matic/posthaze-download-manager/releases/latest) (Windows 10 1809+ or Windows 11, 64-bit):

| Option | File | Best For |
|---|---|---|
| **Windows Installer** | `posthaze-*-windows-x64-setup.exe` | Most users. Per-user install (no admin required), Start Menu shortcuts, adds `posthaze-cli` to `PATH`, bundles the browser extension, and supports one-click auto-updates. |
| **Portable ZIP** | `posthaze-*-windows-x64-portable.zip` | USB drives or zero-install setups. Extract and run anywhere—keeps all database files and logs inside `./data/` via the `posthaze.portable` marker. |

> [!NOTE]
> **Windows SmartScreen:** Release binaries do not currently use a paid Authenticode certificate. If Windows SmartScreen warns you on first run, verify the file hash against `SHA256SUMS.txt` from the release page, then click **More info → Run anyway**:
> ```powershell
> Get-FileHash .\posthaze-0.2.0-windows-x64-setup.exe -Algorithm SHA256
> ```

---

## Browser Extension (Chrome / Edge / Brave)

Posthaze includes a Manifest V3 browser extension in `extensions/chrome/` (bundled inside both the Windows Installer at `%LOCALAPPDATA%\Programs\Posthaze\extensions\chrome` and the Portable ZIP).

1. Open `chrome://extensions` (or `edge://extensions` / `brave://extensions`).
2. Turn on **Developer mode**, click **Load unpacked**, and select the `extensions/chrome/` folder.
3. Copy the 32-character **Extension ID** shown on the Posthaze extension card.
4. Open **Posthaze → Settings → Integration**, paste the **Extension ID**, and click **REGISTER NATIVE HOST**.
5. Click the **Posthaze** icon in your browser toolbar and confirm the status pill shows **UI ONLINE** or **HOST READY**.

*(Full extension details, capture filters, and CLI fallback: [`extensions/chrome/README.md`](extensions/chrome/README.md).)*

---

## Benchmarks

All numbers below are measured on Windows 11 Pro (Intel Core i5-11320H, 16 GiB RAM, NVMe SSD) using release builds. Full methodology and raw tables live in [`docs/benchmarks/`](docs/benchmarks/ram-and-performance-optimization.md).

| Metric | Measured Result | Source |
|---|---|---|
| **Real-world work-stealing speedup** (`115.46 MiB` GitHub MSI, default 8 connections) | **3.17× faster** (`38.35 s` → **`12.08 s`**, `80.17 Mbps` avg; `192.51 Mbps` peak at 32 conn) | [Work-Stealing Benchmark](docs/benchmarks/ticket-04-work-stealing.md) |
| **Sustained multi-Gbps loopback** (`1 GiB` transfer, `16 MiB` bounded writer buffer) | **`1.8–4.2 Gbps`** medians (peaks up to **`7.0–7.36 Gbps`**) | [RAM & Perf Results](docs/benchmarks/ram-and-performance-optimization.md) |
| **SQLite WAL checkpoint overhead** (`posthaze` vs. `posthaze (db)`) | **~0–5%** (within loopback variance) | [SQLite Checkpoint Benchmark](docs/benchmarks/milestone-02-sqlite-checkpoint-overhead.md) |
| **Memory — Cold Idle (Tray / Visible)** | **`85.76 MiB`** (Tray) / **`97.13 MiB`** (Visible) Working Set | [RAM & Perf Results](docs/benchmarks/ram-and-performance-optimization.md) |
| **Memory & CPU — 500 Downloads in Queue (Tray)** | **`86.73 MiB`** (+0.97 MiB delta) & **`0.0000 s` CPU** over 10 s | [RAM & Perf Results](docs/benchmarks/ram-and-performance-optimization.md) |

---

## Technical Details & Build From Source

<details>
<summary><b>Architecture, workspace crates, and CLI usage</b></summary>

### Workspace Structure (Rust Edition 2024)

| Crate | Purpose |
|---|---|
| `crates/posthaze-engine` | Decoupled HTTP/HTTPS multi-connection engine, adaptive work-stealing scheduler, positional disk writer, SQLite WAL persistence, BLAKE3 hashing |
| `crates/posthaze-ui` | Native Windows 10/11 GPUI + GPUI Kit desktop app, System Tray, Win32 DWM Mica themes, Ed25519 + SHA-256 auto-updater |
| `crates/posthaze-cli` | Headless CLI (`download`, `list`, `resume`, `cancel`, `delete`, `bench`) and Chromium Native Messaging Host |
| `crates/posthaze-keytool` | Internal release tool (`publish = false`) for Ed25519 signing and verification |

### Stack Overview
- **Engine & Async:** Tokio (bounded worker/blocking pools), `reqwest` (`rustls-tls`), `bytes` recycled chunk pool
- **Storage:** SQLite via `sqlx` (WAL mode, `synchronous = NORMAL`, automatic memory shrinking on idle)
- **Desktop UI:** GPUI + `gpui-kit`, `tray-icon`, Win32 DWM (`windows-sys`)
- **Security & Integrity:** `ed25519-dalek` + `sha2` (auto-updater verification), `blake3` (file integrity), per-user HKCU installation via Inno Setup 6.7+

### Build & Run From Source

```powershell
# Run full test suite
cargo test --workspace

# Launch the desktop UI (release)
cargo run --release -p posthaze-ui

# Use the headless CLI
cargo run --release -p posthaze-cli -- download "<URL>"
cargo run --release -p posthaze-cli -- list
cargo run --release -p posthaze-cli -- resume <DOWNLOAD_ID>
cargo run --release -p posthaze-cli -- bench --sizes 100M -c 1,4,8,16 --runs 3
```

</details>

---

## Documentation

- [Documentation Index](docs/README.md) — Architecture, milestones, ADRs, and API doc builds
- [Auto-Updater & Signature Verification](docs/auto-updater.md) — Ed25519 + SHA-256 upgrades and portable-mode behavior
- [Release Packaging & Portable Mode](docs/release-packaging-and-portable-mode.md) — Windows installer, `posthaze.portable`, System Tray, and logs
- [Creating Themes](docs/creating-themes.md) — 8 built-in themes (including Windows 11 Mica) and palette token reference
- [Settings, Custom Headers & Context Menu](docs/settings-and-context-menu.md) — 9-tab Settings, proxies, throttling, and Paste as cURL
- [Benchmarking & Stress-Testing Guide](docs/benchmarking.md) — How to reproduce throughput and memory results

---

## Credits

Posthaze is made possible by these open-source projects:

- **UI & Windows Integration:** [GPUI](https://www.gpui.rs/) (Zed Industries), [gpui-kit](https://crates.io/crates/gpui-kit), [tray-icon](https://crates.io/crates/tray-icon), [windows-sys](https://crates.io/crates/windows-sys), [image](https://crates.io/crates/image), [tauri-winres](https://crates.io/crates/tauri-winres)
- **Networking & Async Runtime:** [Tokio](https://tokio.rs/), [reqwest](https://crates.io/crates/reqwest) + [rustls](https://github.com/rustls/rustls), [futures-util](https://crates.io/crates/futures-util), [bytes](https://crates.io/crates/bytes)
- **Storage & Data:** [SQLx](https://crates.io/crates/sqlx) (SQLite), [Serde](https://serde.rs/), [uuid](https://crates.io/crates/uuid), [semver](https://crates.io/crates/semver)
- **Cryptography & Integrity:** [BLAKE3](https://github.com/BLAKE3-team/BLAKE3), [ed25519-dalek](https://crates.io/crates/ed25519-dalek), [sha2](https://crates.io/crates/sha2)
- **CLI, Diagnostics & Packaging:** [clap](https://crates.io/crates/clap), [tracing](https://crates.io/crates/tracing), [thiserror](https://crates.io/crates/thiserror), [Criterion.rs](https://crates.io/crates/criterion), [Inno Setup](https://jrsoftware.org/isinfo.php)

---

## Contributing & License

- **Contributing:** Bug reports, feature requests, and pull requests are welcome. Before opening a PR, please review [`CODING_STANDARDS.md`](CODING_STANDARDS.md) and run `cargo test --workspace`, `cargo clippy --workspace --all-targets -- -D warnings`, `cargo doc --workspace --no-deps`, and `./scripts/check-repo.ps1`.
- **License:** Released under the **MIT License** (see [`Cargo.toml`](Cargo.toml); add a root `LICENSE` file for GitHub license detection).
````

---

## 7. Sources Consulted

### Repository Primary Sources
- [`Cargo.toml`](../../Cargo.toml), [`crates/posthaze-engine/Cargo.toml`](../../crates/posthaze-engine/Cargo.toml), [`crates/posthaze-cli/Cargo.toml`](../../crates/posthaze-cli/Cargo.toml), [`crates/posthaze-ui/Cargo.toml`](../../crates/posthaze-ui/Cargo.toml), [`crates/posthaze-keytool/Cargo.toml`](../../crates/posthaze-keytool/Cargo.toml)
- [`README.md`](../../README.md), [`PRODUCT.md`](../../PRODUCT.md), [`CHANGELOG.md`](../../CHANGELOG.md)
- [`extensions/chrome/README.md`](../../extensions/chrome/README.md), [`extensions/chrome/manifest.json`](../../extensions/chrome/manifest.json)
- [`docs/benchmarks/ram-and-performance-optimization.md`](../benchmarks/ram-and-performance-optimization.md)
- [`docs/benchmarks/ticket-04-work-stealing.md`](../benchmarks/ticket-04-work-stealing.md)
- [`docs/benchmarks/milestone-02-sqlite-checkpoint-overhead.md`](../benchmarks/milestone-02-sqlite-checkpoint-overhead.md)
- [`docs/release-packaging-and-portable-mode.md`](../release-packaging-and-portable-mode.md)
- [`docs/auto-updater.md`](../auto-updater.md)
- [`docs/creating-themes.md`](../creating-themes.md)
- [`.github/workflows/ci.yml`](../../.github/workflows/ci.yml), [`.github/workflows/release.yml`](../../.github/workflows/release.yml)
- `docs/screenshots/` (`posthaze-chronophotographic.png`, `posthaze-archival-light.png`, `posthaze-mica-dark.png`, `installer-welcome.png`, `installer-options.png`)

### Official GitHub Documentation
- [Classifying your repository with topics](https://docs.github.com/en/repositories/managing-your-repositorys-settings-and-features/customizing-your-repository/classifying-your-repository-with-topics)
- [Basic writing and formatting syntax — Specifying the theme an image is shown to](https://docs.github.com/en/get-started/writing-on-github/getting-started-with-writing-and-formatting-on-github/basic-writing-and-formatting-syntax#specifying-the-theme-an-image-is-shown-to)
- [Organizing information with collapsed sections](https://docs.github.com/en/get-started/writing-on-github/working-with-advanced-formatting/organizing-information-with-collapsed-sections)
- [Adding a workflow status badge](https://docs.github.com/en/actions/monitoring-and-troubleshooting-workflows/monitoring-workflows/adding-a-workflow-status-badge)
- [About community profiles for public repositories](https://docs.github.com/en/communities/setting-up-your-project-for-healthy-contributions/about-community-profiles-for-public-repositories)
- [Adding a license to a repository](https://docs.github.com/en/communities/setting-up-your-project-for-healthy-contributions/adding-a-license-to-a-repository)
