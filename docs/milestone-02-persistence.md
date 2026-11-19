# Milestone 02 — Persistence, Pause/Resume, and Crash Recovery

Implementation documentation for Milestone 02 (`posthaze-engine` and `posthaze-cli`), grounded in [ADR-0006](adr/0006-sqlite-persistence-sqlx.md), [ADR-0007](adr/0007-writer-acked-checkpoints.md), and [`GLOSSARY.md`](../GLOSSARY.md).

## Ticket 01 — SQLite Store, `DownloadId`/`DownloadHandle`, and Lifecycle Persistence

### Schema & Store (`sqlx`)

`posthaze-engine` embeds SQLite migrations (`crates/posthaze-engine/migrations/0001_init.sql`) via `sqlx::migrate!` and uses runtime `sqlx::query` calls with bundled SQLite (`WAL` mode, `synchronous = NORMAL`, foreign keys enabled).

- **`downloads`**: Stores `id` (UUID v4 text primary key), `url`, `final_url`, `output_path`, `part_path`, `state` (`DownloadState`), `content_length`, `supports_ranges`, `etag`, `last_modified`, `max_workers`, `sync_on_complete`, `bytes_downloaded`, `error_message`, `created_at`, and `updated_at`.
- **`segments`**: Stores `id`, `download_id` (foreign key with `ON DELETE CASCADE`), `segment_index`, `start_offset`, `cursor_offset`, `end_offset` (nullable for unbounded single-stream `Segment`s), and `completed`.

### Engine API

- `DownloadId`: Persistent UUID v4 identifier for a `Download`.
- `DownloadHandle`: Async handle returned by `DownloadEngine::start(request)` exposing `id()`, `subscribe() -> watch::Receiver<ProgressSnapshot>`, and `wait() -> Result<DownloadOutcome, EngineError>`.
- `DownloadEngine::open(db_path)`: Opens or creates the SQLite database at `db_path` and runs embedded migrations.
- `DownloadEngine::in_memory()`: Opens an ephemeral in-memory SQLite-backed engine.
- `DownloadEngine::new()`: Remains available as a synchronous ephemeral constructor without SQLite persistence.
- `DownloadEngine::list_downloads()` and `DownloadEngine::get_download(id)`: Query persisted `DownloadRecord`s.
- `DownloadEngine::download(request, progress_tx)`: Preserved as a convenience wrapper sharing the same `run_download` lifecycle (`Queued` → `Probing` → `Active` → `Completed`/`Failed`).
