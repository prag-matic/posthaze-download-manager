# posthaze-download-manager

A Windows-native, multi-connection HTTP/HTTPS download manager built with a decoupled Rust engine (`posthaze-engine`), a headless CLI (`posthaze-cli`), and a pure-Rust GPUI + GPUI Kit desktop interface (`posthaze-ui`).

## Documentation

- [Documentation Index](docs/README.md) — Milestone docs, benchmarks, ADRs, and API doc instructions
- [Comprehensive Settings, Custom Headers & Context Menu](docs/settings-and-context-menu.md) — 7-tab Settings, proxy/throttling, Paste as cURL, notifications/hooks, and row context menu
- [Table-Grid Cell UI System & Interface Customization](docs/table-grid-ui-and-customization.md) — `grid_cells.rs` primitives, scrubber minimap, multi-select, density/columns, and accent overrides
- [Independent Pre-Flight Window, Folder Picker & Extension Lock](docs/preflight-window-and-pickers.md) — Standalone `PreflightWindow`, `ADD DOWNLOAD` / `OPTIONS` titlebar tabs, native folder picker, and extension lock
- [Release Packaging & Portable Mode](docs/release-packaging-and-portable-mode.md) — Release profile, console-free Windows GUI subsystem, and `posthaze.portable` data routing
- [Creating Themes for Posthaze](docs/creating-themes.md) — 5 built-in themes, `ThemePalette` token reference, and guide to adding new themes
- [Milestone 03 — Queue, Settings, Telemetry & GPUI Desktop UI](docs/milestone-03-desktop-ui.md) — FIFO `Queue`, `Settings`, `probe()`, `EngineEvent`s, and `posthaze-ui`
- [Milestone 02 — Persistence, Pause/Resume, and Crash Recovery](docs/milestone-02-persistence.md) — SQLite store, `Checkpoint`s, `Validator`s, and CLI persistence commands
- [Design System (`DESIGN.md`)](DESIGN.md) — Chronophotographic Interval Plate tokens and component grammar
- [Architecture Proposal](docs/Architecture%20Proposal%20-%20Windows%20Download%20Manager.md)
- [Domain Glossary](GLOSSARY.md)
- [Product Context](PRODUCT.md)
- [Benchmarking & Stress-Testing Guide](docs/benchmarking.md)

## Quick Start

```powershell
cargo test --workspace
cargo run --release -p posthaze-ui
cargo run --release -p posthaze-cli -- download "<URL>" --debug
cargo run --release -p posthaze-cli -- list
cargo run --release -p posthaze-cli -- resume <DOWNLOAD_ID>
cargo run --release -p posthaze-cli -- cancel <DOWNLOAD_ID>
cargo run --release -p posthaze-cli -- delete <DOWNLOAD_ID> --delete-file
cargo run --release -p posthaze-cli -- bench --sizes 100M -c 1,4,8,16 --runs 3
```
