# Engine & CLI Readiness Assessment for GPUI + GPUI Kit Desktop UI (`posthaze-ui`)

Source-cited audit comparing `posthaze-engine` and `posthaze-cli` against [`Architecture Proposal - Windows Download Manager.md`](Architecture%20Proposal%20-%20Windows%20Download%20Manager.md) (superseding Slint with GPUI + GPUI Kit per [ADR-0008](adr/0008-gpui-and-gpui-kit-desktop-ui.md)), [`PRODUCT.md`](../PRODUCT.md), and the Milestone 01/02 specs.

---

## 1. What is 100% Complete & Verified Today

Both **Milestone 01 (Core Headless HTTP Download Engine)** ([`spec.md`](../.scratch/milestone-01-http-engine/spec.md)) and **Milestone 02 (Persistence, Pause/Resume, and Crash Recovery)** ([`spec.md`](../.scratch/milestone-02-persistence/spec.md)), plus the **Weeks 7–8 Benchmarking & Optimization Harness**, are complete, documented ([`docs/README.md`](README.md), [`milestone-02-persistence.md`](milestone-02-persistence.md)), and verified:

1. **HTTP Capability Probing & Filename Sanitization ([`probe.rs`](../crates/posthaze-engine/src/probe.rs))**:
   - Issues `HEAD` first, follows redirects, and captures `final_url`, `content_length`, `supports_ranges`, `etag`, `last_modified`, and `suggested_filename`.
   - Falls back to `GET` with `Range: bytes=0-0` on `403`/`405`/`501` or missing range/length headers, parsing `206 Partial Content` and `Content-Range` total length.
   - Derives filenames via RFC 5987 `Content-Disposition`, final URL path, and `"download.bin"` fallback, sanitizing Windows-illegal characters and Windows reserved device names (`CON`, `PRN`, `AUX`, `NUL`, `COM1..9`, `LPT1..9`).
2. **Multi-`Segment` Dynamic Work-Stealing & Transient Retry ([`transfer.rs`](../crates/posthaze-engine/src/transfer.rs))**:
   - Plans initial `Segment`s with a 2 MiB floor (`MIN_SEGMENT_SIZE`) and degrades cleanly to single-stream mode when byte ranges or `Content-Length` are unavailable.
   - Uses atomic `Segment` boundaries (`cursor: AtomicU64`, `end: AtomicU64`) and dynamic work-stealing (`steal_largest_segment`) both on startup after resume and whenever a `Worker` finishes a `Segment`, persisting bisections atomically via `DownloadStore::split_segment` ([`store.rs`](../crates/posthaze-engine/src/store.rs)).
   - Buffers 512 KiB `Chunk`s and retries transient errors (`reqwest::Error`, HTTP `408`/`429`/`500..=599`, `UnexpectedEof`) up to 5 times with exponential backoff and jitter from the latest `cursor`.
3. **Dedicated `.part` File Writer & Writer-Acked `Checkpoint`s ([`writer.rs`](../crates/posthaze-engine/src/writer.rs))**:
   - Runs one `spawn_blocking` writer task over a bounded channel (`WRITER_CHANNEL_CAPACITY = 128`), pre-allocating `.part` via `file.set_len(len)` and opening existing `.part` files without truncation on resume (`spawn_existing_with_sync`).
   - Emits periodic non-blocking writer-acknowledged `Checkpoint`s strictly after `file.write_all` + `file.flush()` every 2 seconds or 8 MiB written.
   - Supports graceful drain/flush on `Paused` (`flush_and_close`), immediate non-draining exit on `Cancelled` (`abort`), and atomic `.part` → final rename on `Completed` (`finish`).
4. **SQLite Persistence, Pause/Resume, `Validator` Checking & Crash Recovery ([`store.rs`](../crates/posthaze-engine/src/store.rs), [`lib.rs`](../crates/posthaze-engine/src/lib.rs), [`0001_init.sql`](../crates/posthaze-engine/migrations/0001_init.sql))**:
   - Bundled SQLite store with WAL mode, `synchronous = NORMAL`, foreign keys (`ON DELETE CASCADE`), and embedded `sqlx::migrate!`.
   - Startup crash reconciliation (`reconcile_crashed_downloads`) transitions stale range-capable `Active` rows to `Paused` and stale `Probing` or non-range `Active` rows to `Failed`.
   - Full lifecycle API on `DownloadEngine` and `DownloadHandle`: `open`, `in_memory`, `new`, `start`, `resume`, `cancel`, `list_downloads`, `get_download`, `download`, `handle.pause()`, `handle.cancel()`, `handle.subscribe()`, and cancellation-safe `handle.wait(&mut self)`.
   - Re-probes and verifies `Validator`s (`Content-Length`, `ETag`, `Last-Modified`) on `resume`, returning `EngineError::ResourceChanged` on mismatch while keeping `.part` intact.
5. **Headless CLI & Benchmarking Suite ([`main.rs`](../crates/posthaze-cli/src/main.rs), [`bench_runner.rs`](../crates/posthaze-cli/src/bench_runner.rs), [`bench_server.rs`](../crates/posthaze-cli/src/bench_server.rs))**:
   - Subcommands `download`, `list`, `resume <id>`, `cancel <id>`, `serve`, and `bench`, with `--db <path>` defaulting to `%LOCALAPPDATA%\posthaze\state.db` and `Ctrl+C` calling `handle.pause()`.
   - Controlled HTTP/1.1 range server and automated benchmark matrix comparing `posthaze`, `posthaze (sync)`, `posthaze (db)`, `aria2c`, and `curl`.

---

## 2. Roadmap & Architecture Comparison

### A. Weeks 7–8 Performance & Benchmarking Items Already Completed
Five of the six **Weeks 7–8** items in [`Architecture Proposal`](Architecture%20Proposal%20-%20Windows%20Download%20Manager.md#L911-L921) are already built and documented:
- Controlled benchmark server (`bench_server.rs`)
- Connection count scaling sweeps (`c=1..32` in `bench_runner.rs`)
- Disk write coalescing + Criterion microbenchmark (`writer_bench.rs`)
- SQLite `Checkpoint` A/B profiling (`--compare-db` in [`milestone-02-sqlite-checkpoint-overhead.md`](benchmarks/milestone-02-sqlite-checkpoint-overhead.md))
- Direct comparison against `aria2c` and `curl`

### B. Engine / Application-Service Gaps Between Current Engine and `posthaze-ui`

Comparing against [`PRODUCT.md`](../PRODUCT.md#L25-L35) (Dashboard, Download Queue, Settings views), [`Architecture Proposal`](Architecture%20Proposal%20-%20Windows%20Download%20Manager.md#L922-L931) (Weeks 9–10), and Milestone 02 [`spec.md`](../.scratch/milestone-02-persistence/spec.md#L65-L71) (*Out of Scope*):

1. **Multi-`Download` `Queue` Scheduler / Concurrency Admission**:
   - `DownloadEngine::start` currently spawns immediately (`Probing` → `Active`) with no concurrency cap (`max_concurrent_downloads`) or auto-promotion of `Queued` downloads when an `Active` transfer finishes, pauses, fails, or cancels.
   - `DownloadEngine::pause(&id)` is currently on `DownloadHandle` rather than directly callable by `DownloadId` from a UI action button (whereas `DownloadEngine::cancel(&id)` already looks up `self.active`).
2. **UI-Facing Observation, Speed/ETA & History Deletion**:
   - `ProgressSnapshot` currently reports `state`, `bytes_downloaded`, `total_bytes`, and `active_workers`. Instantaneous/average throughput and ETA are computed inside `posthaze-cli/src/main.rs` rather than provided by `posthaze-engine`.
   - `DownloadStore` has `ON DELETE CASCADE` on `segments`, but no `delete(id)` method on `DownloadEngine` to delete finished/cancelled rows (and leftover `.part`) from the UI history list.
   - Per-`Segment` progress is currently `pub(crate)` (`SegmentCheckpoint`), not exposed on snapshots if the UI renders a segmented progress bar.
   - A unified engine-level state/progress broadcast channel makes driving GPUI views via `Context::spawn` / `AsyncApp` + `cx.notify()` clean without polling SQLite.
3. **Persisted `Settings` Model**:
   - Default `output_dir`, `max_workers` per download, `max_concurrent_downloads`, and `sync_on_complete` are not yet persisted in SQLite or config.
4. **Optional / Stretch Engine Features (Not Required for Desktop MVP)**:
   - Adaptive concurrency (`v0.4` / Weeks 7–8 stretch) and global bandwidth limiter / time-of-day scheduler (`v0.7`).

---

## 3. Verdict & Recommended Path

**Verdict: Yes — `posthaze-engine` and `posthaze-cli` are ready to move onto the GPUI + GPUI Kit desktop UI (`posthaze-ui`).**

All single-file engine primitives, persistence, crash recovery, and benchmarking foundations (`Weeks 1–8` core) are complete, verified, and pass every quality gate.

### Recommended Next Milestone Structure
- **Option A (Recommended — Unified Milestone 03: Queue, Settings & GPUI Desktop UI):**
  Build the thin UI-boundary additions (`Queue` concurrency limiter, `Settings`, `engine.pause(id)` / `engine.delete(id)`, speed/ETA + event broadcast) in `posthaze-engine` together with `crates/posthaze-ui` (`gpui` + `gpui-kit`) in vertical tracer-bullet slices (Dashboard, Download Queue, Settings).
- **Option B (Two-Step — Milestone 03a Headless Queue & Settings → Milestone 03b GPUI Desktop UI):**
  Land and test the multi-`Download` `Queue`, persisted `Settings`, and `delete`/`pause` commands in `posthaze-engine` + `posthaze-cli` first, then build `crates/posthaze-ui` cleanly on top.
