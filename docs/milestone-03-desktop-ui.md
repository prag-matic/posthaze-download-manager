# Milestone 03: Queue, Settings, Telemetry & GPUI Desktop UI

**Status:** Implemented (`crates/posthaze-engine`, `crates/posthaze-cli`, `crates/posthaze-ui`)

Milestone 03 extends `posthaze-engine` with a concurrency-governed FIFO [`Queue`](../GLOSSARY.md), persisted [`Settings`](../crates/posthaze-engine/migrations/0002_settings.sql), non-mutating pre-flight `probe()`, filename collision disambiguation, direct ID-based controls, and engine-wide `EngineEvent` telemetry—and ships the pure-Rust Windows desktop application in [`crates/posthaze-ui`](../crates/posthaze-ui/) powered by **GPUI** and **GPUI Kit `0.10.0`** ([ADR-0008](adr/0008-gpui-and-gpui-kit-desktop-ui.md)).

---

## 1. Engine & CLI Additions

- **Persisted `Settings` ([`0002_settings.sql`](../crates/posthaze-engine/migrations/0002_settings.sql), [ADR-0009](adr/0009-sqlite-settings-and-queue-admission.md))**:
  - `DownloadEngine::get_settings()` and `DownloadEngine::update_settings(Settings)` persist `default_output_dir`, `max_workers` (default `8`), `max_concurrent_downloads` (default `3`), and `sync_on_complete` (default `false`) inside `state.db`.
- **FIFO `Queue` Admission ([`queue.rs`](../crates/posthaze-engine/src/queue.rs))**:
  - `DownloadEngine::start` and `DownloadEngine::resume` return `DownloadHandle` immediately in `DownloadState::Queued` and admit waiting `Download`s in strict FIFO order up to `max_concurrent_downloads`.
  - Raising `max_concurrent_downloads` via `update_settings` wakes waiting `Queued` `Download`s immediately; `pause()` and `cancel()` while `Queued` remove the entry without waiting for a concurrency slot; `DownloadEngine::open` reconciles stale `Queued` rows to `Paused`.
- **Pre-Flight `probe`, Disambiguation & ID Controls ([`lib.rs`](../crates/posthaze-engine/src/lib.rs))**:
  - `DownloadEngine::probe(url, output_dir) -> PreflightInfo` inspects HTTP `HEAD` / `Range: bytes=0-0` metadata without inserting SQLite rows or creating `.part` files, and disambiguates collisions (`<name> (1).<ext>`, `<name> (2).<ext>`) against both final and `.part` paths.
  - Added `DownloadEngine::pause(&id)`, `DownloadEngine::delete(&id, delete_file)`, `DownloadEngine::restart_fresh(&id)` (`ResourceChanged` recovery), and `posthaze-cli delete <id> [--delete-file]`.
- **Per-`Segment` Telemetry & `EngineEvent` Broadcast**:
  - `ProgressSnapshot` includes smoothed `bytes_per_sec`, `eta_secs`, and `segments: Vec<SegmentProgress>` (`start <= checkpointed_cursor <= cursor`).
  - `DownloadEngine::subscribe_events() -> broadcast::Receiver<EngineEvent>` emits `StateChanged`, `ProgressUpdated`, `QueueUpdated`, and `SettingsUpdated`, and `DownloadEngine::list_segments(&id)` loads persisted `Segment` `Checkpoint`s for inactive `Download`s.

---

## 2. GPUI + GPUI Kit Desktop Application (`crates/posthaze-ui`)

- **Tokio↔GPUI Runtime Bridge ([`bridge.rs`](../crates/posthaze-ui/src/bridge.rs), [ADR-0010](adr/0010-tokio-gpui-runtime-bridge.md))**:
  - Runs `DownloadEngine::open(default_db_path())` on a dedicated multi-thread Tokio runtime and bridges `EngineEvent`s into GPUI views via `cx.spawn(...)` + `cx.notify()`.
- **Chronophotographic Interval Plate Interface ([`main.rs`](../crates/posthaze-ui/src/main.rs), [`inspector.rs`](../crates/posthaze-ui/src/inspector.rs), [`modals.rs`](../crates/posthaze-ui/src/modals.rs), [`DESIGN.md`](../DESIGN.md))**:
  - **Left Filter Rail (`220px`, `#181715`)**: Queue filters (`All Transfers`, `Active`, `Queued`, `Paused`, `Completed`, `Failed`), aggregate throughput and active slot telemetry, and **Settings** modal trigger.
  - **Top Dispatch Toolbar (`56px`)**: Monospaced URL `Input`, **Paste & Probe**, **Resume All**, **Pause All**, and **Clear Completed**.
  - **Pre-Flight Confirmation Modal**: Resolves `PreflightInfo` before committing disk space, with editable filename/directory, worker count selector (`1..32`), **Download Now** / **Add to Queue**, and retry/edit on probe errors.
  - **Center Sortable Downloads Table (`#F2EFE9`)**: Fixed-cell geometric state marks, `#1 NEXT` / `#N` FIFO queue rank badges, dual-tone progress bars (Cyanotype Blue `#2563EB` disk checkpoints vs Selenium Amber `#D97724` in-flight bytes), inline row actions, and clipboard-ready empty state.
  - **Bottom Selected Download Inspector (`240px`, `#181715`)**: Renders the **Graduated Interval Byte-Map Ruler**, compact tabular `Segment` grid, stamped HTTP `Validator` badges, and inline Errata Vermilion (`#DC2626`) **Restart Fresh** / **Delete** recovery on `EngineError::ResourceChanged`.

---

## 3. Running & Testing

```powershell
cargo test --workspace
cargo run --release -p posthaze-ui
```
