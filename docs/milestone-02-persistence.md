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

## Ticket 02 — Graceful `pause()`, Immediate `cancel()`, and Persisted `cancel(id)`

- **`DownloadHandle::pause()`**: Signals active `Worker`s (and `Probing`) via `ControlSignal::Pause`, flushes all buffered `Chunk`s in `PartFileWriter::flush_and_close()` without renaming `.part`, writes exact `Segment` `[start, cursor, end, completed]` `Checkpoint`s to SQLite, and transitions the `Download` to `Paused` (`EngineError::Paused`).
- **`DownloadHandle::cancel()` & `DownloadEngine::cancel(id)`**: Aborts active `Worker`s and `PartFileWriter` immediately without draining (`PartFileWriter::abort()`), deletes the `.part` file if present, and marks the SQLite record `Cancelled` (`EngineError::Cancelled`). `DownloadEngine::cancel(id)` works uniformly for both live active transfers and persisted `Paused`, `Failed`, or `Queued` records.
- **Resumable `Failed` retention**: Range-capable `Download`s that fail flush their buffered `Chunk`s, persist `Segment` `Checkpoint`s, and retain `.part` on disk.

## Ticket 03 — `resume(id)` with `Validator` Checking & Dynamic `Segment` Continuation

- **`DownloadEngine::resume(id)`**: Reloads a `Paused` or resumable `Failed` `DownloadRecord` and its `SegmentCheckpoint`s from SQLite, verifies `.part` exists on disk, re-probes the original `url`, and resumes incomplete `Segment`s (`cursor_offset < end_offset`) from their saved `cursor_offset` using `PartFileWriter::spawn_existing_with_sync`.
- **`Validator` Verification**: Compares `Content-Length` and any previously recorded `ETag` or `Last-Modified`. Any mismatch fails with `EngineError::ResourceChanged` while preserving `.part`. If neither `ETag` nor `Last-Modified` was recorded originally and neither is returned on re-probe, `resume` proceeds when `Content-Length` matches (logging a `tracing::warn!`).
- **Dynamic Split Persistence**: When a `Worker` steals and bisects a `Segment`, `DownloadStore::split_segment` atomically updates the incumbent `Segment`'s `end_offset` and inserts the new `Segment` row in SQLite.

## Ticket 04 — Periodic Writer-Acked `Checkpoint`s & Startup Crash Reconciliation

- **Writer-Acknowledged `Checkpoint`s (ADR-0007)**: `PartFileWriter` tracks per-`Segment` written offsets inside the blocking writer loop and flushes + emits non-blocking (`try_send`) `Checkpoint`s whenever `>= 8 MiB` have been written or `>= 2 seconds` have elapsed, strictly after `file.write_all` + `file.flush()`. A dedicated async task persists `MAX(cursor_offset, new_cursor)` and recomputes `bytes_downloaded` in SQLite without blocking `Worker`s.
- **Startup Crash Reconciliation**: `DownloadEngine::open(db_path)` reconciles stale range-capable `Active` rows to `Paused` (resumable) and stale `Probing` or non-range `Active` rows to `Failed`.
