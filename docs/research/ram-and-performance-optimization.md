# Research: RAM Usage & Performance Optimization Without Compromising Download Speed

> **Scope:** Static codebase audit and primary-source systems analysis of Posthaze Download Manager (`posthaze-engine`, `posthaze-ui`, `posthaze-cli`) targeting lower resident RAM (Working Set / Private Bytes), reduced heap churn, and lower idle/active CPU usage while preserving multi-Gbps download throughput.
> **Non-goal:** No application source code is modified by this document. Every recommendation includes an explicit throughput-risk assessment and verification plan.

---

## 1. Executive Summary & Prioritized Recommendations

Posthaze Download Manager is a pure-Rust application (`posthaze-engine` on Tokio + `reqwest`/`rustls` + `sqlx`/SQLite; `posthaze-ui` on GPUI via `gpui-kit` 0.10.0). Because it avoids Electron/WebView2, its baseline architecture is already lean—but the current implementation leaves substantial RAM and CPU savings on the table across **five subsystems**:

1. **Over-provisioned transfer buffers & `BytesMut` slab pinning** (`64 MiB` channel + `1 MiB` per-worker slabs per active download).
2. **Unbounded Tokio worker threads** (`1` per logical CPU core, e.g., 16–32 always-live threads even while idle in the system tray).
3. **Per-frame full-queue cloning and 60 FPS rendering while hidden to tray** in `posthaze-ui`.
4. **Unnecessary polling thread (`tray.rs`), duplicate `reqwest::Client` rebuilds, and full-file RAM buffering during update verification**.
5. **Uncapped idle HTTP/TLS pools and oversized SQLite connection/page caches**.

### Priority Matrix

| Priority | Optimization | Primary Subsystem | Est. RAM / CPU Impact | Effort | Throughput Risk |
|---|---|---|---|---|---|
| **P0** | Bound `WRITER_CHANNEL_CAPACITY` (`128 → 16–32`) & fix `BytesMut` slab pinning (`CHUNK_BUFFER_SIZE * 2` → exact `CHUNK_BUFFER_SIZE` or buffer pool) | Engine / Buffers | **High RAM** (saves ~48–190+ MiB under backpressure / multi-download) | Low | **Low** (verify with `writer_bench` + `bench`) |
| **P0** | Cap `EngineBridge` Tokio `worker_threads` (`2..=4`), `max_blocking_threads(16)`, `thread_keep_alive(5s)` | Runtime / Threading | **High RAM & Idle** (eliminates 12–28 idle threads on 16–32T CPUs) | Low | **None** (2–4 workers saturate multi-Gbps TLS) |
| **P0** | Pause `MotionState` loop & skip `cx.notify()` / rendering while hidden to system tray | UI / Idle | **High CPU/GPU + RAM** (~0% UI CPU when hidden during downloads) | Low | **None** (frees CPU for engine) |
| **P0** | Cache/memoize derived `DownloadRowView` + filtered/sorted indices in `PosthazeApp::render()` | UI / Rendering | **High CPU & Heap Churn** (eliminates ~240k allocs/s at 60 FPS with 500 rows) | Medium | **None** (improves UI responsiveness) |
| **P1** | Replace 50ms polling thread in `tray.rs` with `TrayIconEvent::set_event_handler` / `MenuEvent::set_event_handler` | UI / Tray | **Medium Idle** (eliminates 1 OS thread + 20 wakeups/s forever) | Low | **None** |
| **P1** | Use `FileExt::seek_write` on Windows in `PartFileWriter` instead of `seek()` + `write_all()` | Engine / Disk I/O | **Medium CPU/I/O** (cuts interleaved disk syscalls in half: 2 → 1) | Low | **Positive** (improves multi-segment write speed) |
| **P1** | Only rebuild `reqwest::Client` in `update_settings()` when `proxy_mode` / `proxy_url` changes; set `pool_max_idle_per_host` & `pool_idle_timeout` | Engine / HTTP | **Medium RAM** (bounds idle TLS sessions & prevents root-store reloads) | Low | **Positive** (preserves warm TLS pool) |
| **P1** | Guard `sync_theme_and_backdrops()` in `apply_snapshot()` & `set_rem_size()` in `render()` | UI / Win32 | **Medium CPU** (eliminates Registry + DWM calls on every queue event) | Low | **None** |
| **P1** | Tune SQLite pool (`max_connections(2)`, `idle_timeout(30s)`, `cache_size = -512`, `statement_cache_capacity(32)`) + add composite index on `segments(download_id, segment_index)` | Storage / SQLite | **Medium RAM & I/O** (~6–7 MiB saved; faster checkpoints) | Low | **Positive** (faster checkpoint commits) |
| **P2** | Prune `MotionState::entries` & `QueueController::persisted_segments`; reduce `broadcast::channel(1024)` + `PROGRESS_EMIT_INTERVAL` (`50ms → 100ms`) | Engine / UI State | **Low–Medium RAM** (fixes unbounded long-session map growth) | Low | **None** (EMA updates every 200ms anyway) |
| **P2** | Avoid loading entire installer (up to 200 MiB) into RAM in `updater.rs`; offload file writes off Tokio worker thread; reuse HTTP client | UI / Updater | **High Peak RAM** (eliminates up to 200 MiB transient spike) | Low–Medium | **None** |
| **P2** | Trim `tokio = { features = ["full"] }` to minimal required features | Build / Footprint | **Low RAM / Binary Size** (smaller `.text` pages mapped in working set) | Low | **None** |
| **P3** | Evaluate `mimalloc` (v2/v3) global allocator on Windows with tuned `purge_delay` | Allocator | **Medium Fragmentation / Throughput** (must A/B test Private Bytes vs Working Set) | Low | **Positive / Neutral** (benchmark required) |

---

## 2. Current Architecture & Memory/Performance Profile

Based on the workspace configuration ([Cargo.toml](file:///e:/Development/posthaze-download-manager/Cargo.toml)) and source tree:

- **Workspace Crates:**
  - `posthaze-engine`: Headless HTTP/HTTPS download engine using `tokio` (`features = ["full"]`), `reqwest` 0.12 (`rustls-tls`, `stream`), `sqlx` 0.8 (`runtime-tokio`, `sqlite`, `migrate`, `macros`), `bytes`, `blake3`.
  - `posthaze-ui`: Native desktop UI built with `gpui-kit` 0.10.0 (DirectX/DirectComposition GPU-accelerated GPUI), `tray-icon` 0.21, `image` (`png`), `ed25519-dalek`, `windows-sys` 0.60.
  - `posthaze-cli`: CLI frontend, Chrome Native Messaging host, and built-in HTTP/1.1 benchmark harness.
- **Release Profile ([Cargo.toml:25-28](file:///e:/Development/posthaze-download-manager/Cargo.toml#L25-L28)):** Already enables `lto = true`, `codegen-units = 1`, and `strip = "symbols"`.
- **Embedded Assets ([crates/posthaze-ui/src/assets.rs:20-23](file:///e:/Development/posthaze-download-manager/crates/posthaze-ui/src/assets.rs#L20-L23)):** Total embedded brand assets in `assets/` are tiny (`~18.6 KiB` total across PNG/SVG/ICO), so embedded assets are **not** a significant RAM contributor.
- **Established Throughput Baseline:**
  - Loopback (`100 MiB`, SQLite WAL + checkpoints): peaks at **7,364.3 Mbps** (`c=4`) and **5,249.8 Mbps** (`c=8`) ([docs/benchmarks/milestone-02-sqlite-checkpoint-overhead.md:18-22](file:///e:/Development/posthaze-download-manager/docs/benchmarks/milestone-02-sqlite-checkpoint-overhead.md#L18-L22)).
  - Real-world GitHub release (`115.46 MiB`): **80.17 Mbps** avg at `c=8` (3.17× speedup from dynamic segment splitting) and **87.08 Mbps** avg at `c=16` ([docs/benchmarks/ticket-04-work-stealing.md:17-28](file:///e:/Development/posthaze-download-manager/docs/benchmarks/ticket-04-work-stealing.md#L17-L28)).

---

## 3. Concrete Optimization Opportunities by Subsystem

### 3.1 Download Engine, Buffers & Disk I/O (`posthaze-engine`)

#### A. Bound Writer Channel Capacity & Eliminate `BytesMut` Slab Pinning (P0)
- **Codebase Evidence:**
  - `WRITER_CHANNEL_CAPACITY = 128` ([crates/posthaze-engine/src/writer.rs:15](file:///e:/Development/posthaze-download-manager/crates/posthaze-engine/src/writer.rs#L15)).
  - `CHUNK_BUFFER_SIZE = 512 * 1024` (512 KiB) ([crates/posthaze-engine/src/transfer.rs:20](file:///e:/Development/posthaze-download-manager/crates/posthaze-engine/src/transfer.rs#L20)).
  - Each worker allocates `BytesMut::with_capacity(CHUNK_BUFFER_SIZE * 2)` (`1 MiB`) ([crates/posthaze-engine/src/transfer.rs:610](file:///e:/Development/posthaze-download-manager/crates/posthaze-engine/src/transfer.rs#L610), [line 751](file:///e:/Development/posthaze-download-manager/crates/posthaze-engine/src/transfer.rs#L751)), then flushes with:
    ```rust
    let block = buffer.split().freeze();
    buffer.reserve(CHUNK_BUFFER_SIZE);
    writer.write_segment_at(seg.id, write_offset, block).await?;
    ```
    ([crates/posthaze-engine/src/transfer.rs:663-669](file:///e:/Development/posthaze-download-manager/crates/posthaze-engine/src/transfer.rs#L663-L669), [lines 784-790](file:///e:/Development/posthaze-download-manager/crates/posthaze-engine/src/transfer.rs#L784-L790)).
- **Primary-Source Analysis:**
  - Per the official `bytes` crate documentation for `BytesMut::split` and `BytesMut::reserve` ([docs.rs/bytes — `BytesMut`](https://docs.rs/bytes/latest/bytes/struct.BytesMut.html)), `split()` shares the same reference-counted backing allocation between `block` and `buffer`. `reserve()` can only reclaim space in-place when all other handles (`block`) referencing the backing allocation have been dropped.
  - Because `buffer.reserve(CHUNK_BUFFER_SIZE)` executes **before** `block` is even sent to `PartFileWriter` (and while up to 128 `block`s can sit queued in the `mpsc` channel), the `Arc` refcount is always `>= 2`. On every second split, `buffer` has `0` remaining tail capacity, forcing a new `1 MiB` heap allocation while the previous `1 MiB` slab remains pinned in RAM until **both** 512 KiB halves are written and dropped by `PartFileWriter`.
  - **Worst-case RAM math:** `128 chunks × 512 KiB = 64 MiB` of payload in the channel per active download, plus `1 MiB × max_workers` (`8–32 MiB`) in worker buffers—reaching **72–96 MiB per active download**, or **216–288 MiB** with 3 concurrent downloads when disk writes lag (HDD, USB, Defender real-time scan, or slow NAS).
- **Recommendation:**
  1. Reduce `WRITER_CHANNEL_CAPACITY` from `128` to `16`–`32` (`8–16 MiB` max in-flight per download). At 100 MiB/s (800 Mbps), `16 × 512 KiB = 8 MiB` still absorbs an **80 ms** disk stall without stalling network reads; `32 × 512 KiB = 16 MiB` absorbs **160 ms** (or 16+ workers with 2 chunks queued each).
  2. Replace `BytesMut::with_capacity(CHUNK_BUFFER_SIZE * 2)` + `split().freeze()` with a **fixed-size buffer pool** (e.g., a bounded `mpsc` / `crossbeam` return channel of `N` pre-allocated `Vec<u8>` buffers of capacity `CHUNK_BUFFER_SIZE` recycled by `PartFileWriter` after `write_all()`), or allocate exact `BytesMut::with_capacity(CHUNK_BUFFER_SIZE)` so a single 512 KiB chunk never pins a 1 MiB slab. A recycled buffer pool eliminates **100% of steady-state chunk heap allocations** and gives strict, deterministic RAM bounds (`pool_size × CHUNK_BUFFER_SIZE`).
- **Throughput Risk:** **Low** (and potentially **positive** with buffer pooling due to zero allocator traffic and better L3 cache reuse). Verify via `cargo bench -p posthaze-engine` and `posthaze-cli bench --sizes 100M,1G -c 1,4,8,16,32`.

#### B. Use Single-Syscall Positional Writes (`FileExt::seek_write`) on Windows (P1)
- **Codebase Evidence:**
  - [docs/adr/0005-dedicated-writer-channel.md:5](file:///e:/Development/posthaze-download-manager/docs/adr/0005-dedicated-writer-channel.md#L5) specifies using `FileExt::seek_write` on Windows, but [crates/posthaze-engine/src/writer.rs:117-121](file:///e:/Development/posthaze-download-manager/crates/posthaze-engine/src/writer.rs#L117-L121) currently executes:
    ```rust
    if chunk.offset != cursor {
        file.seek(SeekFrom::Start(chunk.offset))?;
        cursor = chunk.offset;
    }
    file.write_all(&chunk.data)?;
    ```
- **Primary-Source Analysis:**
  - Under 8–32 concurrent workers, incoming chunks arrive interleaved across segment offsets, so `chunk.offset != cursor` is true on almost every chunk—issuing **two Win32 syscalls** (`SetFilePointerEx` + `WriteFile`) per chunk.
  - Per Rust stdlib documentation for [`std::os::windows::fs::FileExt::seek_write`](https://doc.rust-lang.org/std/os/windows/fs/trait.FileExt.html#tymethod.seek_write), `seek_write(buf, offset)` performs a single `WriteFile` with the offset encoded in the `OVERLAPPED` structure, halving kernel transitions (note: loop until all bytes are written to handle short writes).
- **Throughput Risk:** **Positive** (reduces writer-thread CPU overhead and syscall latency).

#### C. Reduce Progress Snapshot Frequency & Broadcast Ring Footprint (P2)
- **Codebase Evidence:**
  - `PROGRESS_EMIT_INTERVAL = Duration::from_millis(50)` (20 Hz per active download) ([crates/posthaze-engine/src/runner.rs:26](file:///e:/Development/posthaze-download-manager/crates/posthaze-engine/src/runner.rs#L26)), whereas speed EMA is only recalculated every `200 ms` ([runner.rs:947](file:///e:/Development/posthaze-download-manager/crates/posthaze-engine/src/runner.rs#L947)).
  - Every 50ms per active download, `collect_segment_progress` locks 3 mutexes (`live_segments_slot`, `checkpointed_cursors`, `live_segments`), allocates and sorts a `Vec<SegmentProgress>` ([runner.rs:989-1015](file:///e:/Development/posthaze-download-manager/crates/posthaze-engine/src/runner.rs#L989-L1015)), clones the entire `ProgressSnapshot` in `ProgressSink::send` ([runner.rs:563-571](file:///e:/Development/posthaze-download-manager/crates/posthaze-engine/src/runner.rs#L563-L571)), and pushes it into `broadcast::channel(1024)` ([crates/posthaze-engine/src/lib.rs:96](file:///e:/Development/posthaze-download-manager/crates/posthaze-engine/src/lib.rs#L96)).
- **Recommendation:**
  - Increase `PROGRESS_EMIT_INTERVAL` to `100 ms` (10 Hz); `posthaze-ui`'s `MotionState` already interpolates bar movement smoothly at 60 FPS between snapshots.
  - Skip emitting when `bytes_downloaded` and segment count have not changed since the last tick (e.g., stalled connections).
  - Reduce `broadcast::channel(1024)` to `256` (a `broadcast::channel` retains all `N` slots' heap payloads in its ring buffer until overwritten).
- **Throughput Risk:** **None** (slightly reduces atomic/mutex contention with workers).

---

### 3.2 Tokio Runtime & Threading (`posthaze-ui` & Workspace)

#### A. Bound `EngineBridge` Tokio Thread Pools (P0)
- **Codebase Evidence:**
  - [crates/posthaze-ui/src/bridge.rs:75-79](file:///e:/Development/posthaze-download-manager/crates/posthaze-ui/src/bridge.rs#L75-L79):
    ```rust
    let runtime = Builder::new_multi_thread()
        .enable_all()
        .thread_name("posthaze-engine-rt")
        .build()
    ```
- **Primary-Source Analysis:**
  - Per official Tokio documentation ([docs.rs/tokio — `tokio::runtime::Builder`](https://docs.rs/tokio/latest/tokio/runtime/struct.Builder.html)):
    - `worker_threads` defaults to `std::thread::available_parallelism()` (all logical CPU cores). These threads remain **permanently alive** for the lifetime of the runtime.
    - `max_blocking_threads` defaults to `512`, with `thread_keep_alive` defaulting to `10 seconds`.
  - On a modern 16- to 32-thread Windows PC, `posthaze-ui` spawns **16–32 always-alive Tokio worker threads** on top of GPUI's own thread pool, main UI thread, Mica listener thread, and tray thread. Each idle thread carries OS kernel structures, default stack reservation/commit, and per-thread allocator caches.
- **Recommendation:**
  ```rust
  let workers = std::thread::available_parallelism()
      .map(|n| n.get().clamp(2, 4))
      .unwrap_or(2);
  let runtime = Builder::new_multi_thread()
      .worker_threads(workers)
      .max_blocking_threads(16)
      .thread_keep_alive(Duration::from_secs(5))
      .enable_all()
      .thread_name("posthaze-engine-rt")
      .build()?;
  ```
- **Throughput Risk:** **None.** Async network I/O + `rustls` + `BytesMut` framing for 1–3 concurrent downloads (8–32 sockets) is I/O-bound and easily saturates multi-Gbps links on 2–4 worker threads. Blocking file writers use the separate blocking pool (`1` thread per active download, well within `max_blocking_threads(16)`).

#### B. Replace `tokio = { features = ["full"] }` with Minimal Features (P2)
- **Codebase Evidence:** [Cargo.toml:21](file:///e:/Development/posthaze-download-manager/Cargo.toml#L21) pulls `tokio = { version = "1", features = ["full"] }` into all workspace crates.
- **Recommendation:** Restrict to the exact features used (`rt-multi-thread`, `macros`, `sync`, `time`, `fs`, `io-util`, `net`) to shrink compiled code pages mapped into the process working set and speed up builds.
- **Throughput Risk:** **None.**

---

### 3.3 HTTP / Networking (`reqwest` & `rustls`)

#### A. Bound Idle Connection Pool & Avoid Unnecessary `reqwest::Client` Rebuilds (P1)
- **Codebase Evidence:**
  - `build_http_client()` ([crates/posthaze-engine/src/lib.rs:623-649](file:///e:/Development/posthaze-download-manager/crates/posthaze-engine/src/lib.rs#L623-L649)) leaves `pool_idle_timeout` and `pool_max_idle_per_host` at defaults.
  - `DownloadEngine::update_settings()` ([crates/posthaze-engine/src/lib.rs:469-479](file:///e:/Development/posthaze-download-manager/crates/posthaze-engine/src/lib.rs#L469-L479)) unconditionally rebuilds `reqwest::Client` on **every** settings save—even when only UI theme, table density, zoom, or speed limit changed.
- **Primary-Source Analysis:**
  - Per official `reqwest` documentation ([docs.rs/reqwest — `ClientBuilder`](https://docs.rs/reqwest/latest/reqwest/struct.ClientBuilder.html)):
    - `pool_idle_timeout` defaults to **90 seconds**.
    - `pool_max_idle_per_host` defaults to **`usize::MAX`** (unbounded).
  - After a 16- or 32-worker download completes, up to 32 idle TLS connections (each retaining `rustls` session buffers + OS socket buffers) linger in memory for 90 seconds per host.
  - Rebuilding `reqwest::Client` unnecessarily re-initializes the `rustls::RootCertStore` (parsing/loading OS/webpki root certificates) and throws away warm connections.
- **Recommendation:**
  1. Set `.pool_max_idle_per_host(4)` (or `8`) and `.pool_idle_timeout(Duration::from_secs(30))` in `build_http_client()`.
  2. In `update_settings()`, compare `(new.proxy_mode, new.proxy_url)` against the previous values and **only** rebuild/swap `self.client` when proxy settings actually changed.
- **Throughput Risk:** **Positive.** Active connections are never evicted by `pool_max_idle_per_host`, and avoiding gratuitous client rebuilds keeps warm TLS connections alive.

---

### 3.4 Storage & Persistence (`sqlx` / SQLite)

#### A. Tune SQLite Connection Pool, Page Cache, and Statement Cache (P1)
- **Codebase Evidence:**
  - [crates/posthaze-engine/src/store.rs:121-131](file:///e:/Development/posthaze-download-manager/crates/posthaze-engine/src/store.rs#L121-L131) opens `SqlitePoolOptions::new().max_connections(4)` with no `idle_timeout`, default `cache_size`, and default `statement_cache_capacity`.
- **Primary-Source Analysis:**
  - Per official SQLite PRAGMA documentation ([sqlite.org/pragma.html](https://www.sqlite.org/pragma.html)):
    - `PRAGMA cache_size` defaults to `-2000` (~2 MiB) **per open connection** (`4 × 2 MiB = ~8 MiB` just for SQLite page caches on a tiny metadata DB).
    - `PRAGMA shrink_memory` invokes `sqlite3_db_release_memory()` to free cached heap/page memory immediately when idle.
  - Per `sqlx` documentation ([docs.rs/sqlx — `SqliteConnectOptions`](https://docs.rs/sqlx/latest/sqlx/sqlite/struct.SqliteConnectOptions.html), [`PoolOptions`](https://docs.rs/sqlx/latest/sqlx/pool/struct.PoolOptions.html)):
    - `statement_cache_capacity` defaults to `100` prepared statements per connection (`400` across 4 connections).
    - `idle_timeout` reaps idle connections automatically.
- **Recommendation:**
  - Set `max_connections(2)` (sufficient for 1 WAL writer + 1 concurrent UI snapshot reader), `min_connections(0)`, and `idle_timeout(Some(Duration::from_secs(30)))`.
  - Configure `.pragma("cache_size", "-512")` (512 KiB per connection) and `.statement_cache_capacity(32)`.
  - Optionally run `PRAGMA shrink_memory;` when the last active download finishes and the UI is idle/hidden.
- **Throughput Risk:** **None** (the DB holds small metadata rows, not file payloads, and checkpoints run asynchronously).

#### B. Add Composite Index on `segments(download_id, segment_index)` (P1)
- **Codebase Evidence:**
  - [crates/posthaze-engine/migrations/0001_init.sql:30](file:///e:/Development/posthaze-download-manager/crates/posthaze-engine/migrations/0001_init.sql#L30) only indexes `segments(download_id)`.
  - Checkpoint updates ([store.rs:368](file:///e:/Development/posthaze-download-manager/crates/posthaze-engine/src/store.rs#L368), [store.rs:530](file:///e:/Development/posthaze-download-manager/crates/posthaze-engine/src/store.rs#L530)) and splits ([store.rs:454](file:///e:/Development/posthaze-download-manager/crates/posthaze-engine/src/store.rs#L454)) filter on `WHERE download_id = ? AND segment_index = ?`, while `get_segments` ([store.rs:570](file:///e:/Development/posthaze-download-manager/crates/posthaze-engine/src/store.rs#L570)) sorts by `ORDER BY segment_index ASC`.
- **Recommendation:** Add a migration creating `CREATE UNIQUE INDEX IF NOT EXISTS idx_segments_download_seg ON segments(download_id, segment_index);` (and optionally drop the redundant prefix index `idx_segments_download_id`).
- **Throughput Risk:** **Positive** (faster checkpoint transactions, no temp B-tree sort).

---

### 3.5 Desktop UI, Rendering & Idle/Tray Behavior (`posthaze-ui`)

#### A. Stop 60 FPS Animation Loop & Skip Renders While Hidden to System Tray (P0)
- **Codebase Evidence:**
  - Closing to tray hides the HWND via `ShowWindowAsync(hwnd, SW_HIDE)` ([crates/posthaze-ui/src/platform_hooks.rs:611-623](file:///e:/Development/posthaze-download-manager/crates/posthaze-ui/src/platform_hooks.rs#L611-L623)).
  - However, `start_event_listener()` ([crates/posthaze-ui/src/main.rs:587-601](file:///e:/Development/posthaze-download-manager/crates/posthaze-ui/src/main.rs#L587-L601)) unconditionally calls `app.motion.on_progress(...)`, starts the 16ms (60 FPS) `ensure_motion_loop()` ([main.rs:656-699](file:///e:/Development/posthaze-download-manager/crates/posthaze-ui/src/main.rs#L656-L699)), and triggers `cx.notify()`—running full GPUI layout/paint passes for an invisible window during active downloads!
- **Recommendation:**
  - Track `window_hidden: bool` (updated on `hide_window_to_tray` / `show_window_from_tray` and verified via `is_window_visible`).
  - While `window_hidden` is true: update `self.queue.live_map` (or only track state changes), **do not** run `motion.on_progress()` / `ensure_motion_loop()`, and **do not** call `cx.notify()`.
  - On restore from tray, sync `MotionState` targets immediately to current `live_map` values and call `cx.notify()` once.
- **Throughput Risk:** **Positive** (eliminates UI thread + GPU compositor load while downloading in the background).

#### B. Memoize Derived Table Rows & Avoid Per-Frame Cloning in `PosthazeApp::render()` (P0)
- **Codebase Evidence:**
  - On **every single frame** (up to 60 FPS during active downloads), `PosthazeApp::render()` ([crates/posthaze-ui/src/main.rs:1489-1522](file:///e:/Development/posthaze-download-manager/crates/posthaze-ui/src/main.rs#L1489-L1522), [1688-1690](file:///e:/Development/posthaze-download-manager/crates/posthaze-ui/src/main.rs#L1688-L1690)):
    1. Calls `compute_fifo_ranks(...)` → allocates a `Vec`, sorts it, and allocates a `HashMap<DownloadId, usize>` cloning every queued `DownloadId` ([view_model.rs:328-346](file:///e:/Development/posthaze-download-manager/crates/posthaze-ui/src/view_model.rs#L328-L346)).
    2. Builds `all_rows: Vec<DownloadRowView>` by cloning every `DownloadRecord`'s strings (`id`, `url`, `final_url`, `filename`, `output_path`, `etag`, `last_modified`, `error_message`) ([view_model.rs:219-278](file:///e:/Development/posthaze-download-manager/crates/posthaze-ui/src/view_model.rs#L219-L278)).
    3. Calls `filter_and_sort_rows_with_query(&all_rows, ...)` → allocates a **second** `Vec<DownloadRowView>` and **deep-clones** every matching row ([view_model.rs:451-460](file:///e:/Development/posthaze-download-manager/crates/posthaze-ui/src/view_model.rs#L451-L460)), then sorts it.
    4. Builds `visible_ids: Arc<Vec<DownloadId>>` → clones every visible `DownloadId` a third time ([main.rs:1688-1689](file:///e:/Development/posthaze-download-manager/crates/posthaze-ui/src/main.rs#L1688-L1689)).
  - With 500 historical downloads and 1 active download at 60 FPS, this performs **~4,000+ heap allocations per frame (~240,000 allocs/sec)** on the UI thread, causing allocator fragmentation and CPU overhead.
- **Recommendation:**
  1. Cache `derived_rows: Vec<DownloadRowView>` and `visible_indices: Vec<usize>` (and `visible_ids: Arc<Vec<DownloadId>>`) on `PosthazeApp` / `QueueController`, rebuilt only when `records`, `filter`, `search_query`, `sort_col`, or `sort_dir` changes.
  2. On `ProgressUpdated { id, snapshot }`, update only that single row's numeric fields in-place (and re-sort only if `sort_col` is `Progress` or `Speed`).
  3. Change `DownloadId` internally from `String` to `Arc<str>` (or `uuid::Uuid` `[u8; 16]`) so ID clones across maps, events, and closures are zero-allocation / `Copy`.
- **Throughput Risk:** **None.**

#### C. Eliminate 50ms Polling Thread in `tray.rs` (P1)
- **Codebase Evidence:**
  - [crates/posthaze-ui/src/tray.rs:75-110](file:///e:/Development/posthaze-download-manager/crates/posthaze-ui/src/tray.rs#L75-L110) spawns an OS thread (`posthaze-tray-events`) that loops `std::thread::sleep(Duration::from_millis(50))` indefinitely to poll `MenuEvent::receiver()` and `TrayIconEvent::receiver()`.
- **Primary-Source Analysis:**
  - Per official `tray-icon` documentation ([docs.rs/tray-icon — `TrayIconEvent::set_event_handler`](https://docs.rs/tray-icon/latest/tray_icon/enum.TrayIconEvent.html#method.set_event_handler), [`MenuEvent::set_event_handler`](https://docs.rs/tray-icon/latest/tray_icon/menu/struct.MenuEvent.html#method.set_event_handler)), registering event handlers dispatches callbacks directly on OS tray/menu events instead of queueing into the default crossbeam channel.
- **Recommendation:** Register `MenuEvent::set_event_handler` and `TrayIconEvent::set_event_handler` to forward directly into `tokio::sync::mpsc::UnboundedSender<TrayAction>`. This removes an entire OS thread and eliminates 20 timer wakeups/sec when idle.
- **Throughput Risk:** **None.**

#### D. Guard `sync_theme_and_backdrops()` & `window.set_rem_size()` (P1)
- **Codebase Evidence:**
  - `PosthazeApp::apply_snapshot()` ([crates/posthaze-ui/src/main.rs:446-447](file:///e:/Development/posthaze-download-manager/crates/posthaze-ui/src/main.rs#L446-L447)) calls `sync_theme_and_backdrops(...)` on every snapshot refresh (every download start/pause/complete/fail/delete). That invokes `RegGetValueW` + `SystemParametersInfoW` + `DwmSetWindowAttribute` + `win.refresh()` on every open window ([platform_hooks.rs:214-236, 291-353](file:///e:/Development/posthaze-download-manager/crates/posthaze-ui/src/platform_hooks.rs#L214-L236)).
  - `PosthazeApp::render()` ([main.rs:1463-1464](file:///e:/Development/posthaze-download-manager/crates/posthaze-ui/src/main.rs#L1463-L1464)) calls `window.set_rem_size(px(zoom_rem))` on every frame.
- **Recommendation:** Cache `last_applied_theme: (String, Option<String>)` and `last_zoom_percent: u16`; only invoke `sync_theme_and_backdrops` / `set_rem_size` when values actually change.
- **Throughput Risk:** **None.**

#### E. Prune Unbounded UI Maps (`MotionState::entries` & `persisted_segments`) (P2)
- **Codebase Evidence:**
  - `MotionState::entries` ([crates/posthaze-ui/src/motion.rs:52, 186, 233](file:///e:/Development/posthaze-download-manager/crates/posthaze-ui/src/motion.rs#L52)) only removes entries on `DownloadState::Cancelled`, leaking entries for every completed/paused/deleted download across a long session.
  - `QueueController::persisted_segments` ([crates/posthaze-ui/src/view_model.rs:682](file:///e:/Development/posthaze-download-manager/crates/posthaze-ui/src/view_model.rs#L682), [main.rs:498](file:///e:/Development/posthaze-download-manager/crates/posthaze-ui/src/main.rs#L498)) caches `Vec<SegmentProgress>` for every download ever clicked, never evicting deleted or deselected items.
- **Recommendation:** Remove settled non-active entries from `MotionState::entries` once their flashes expire, and retain only the currently selected download's `persisted_segments` (or prune on `apply_snapshot` against existing IDs + cap to an LRU of 4–8 entries).
- **Throughput Risk:** **None.**

#### F. Eliminate 200 MiB RAM Spike & Async-Blocking Disk Writes in `updater.rs` (P2)
- **Codebase Evidence:**
  - `verify_download()` ([crates/posthaze-ui/src/updater.rs:217-245](file:///e:/Development/posthaze-download-manager/crates/posthaze-ui/src/updater.rs#L217-L245)) calls `std::fs::read(setup_path)`, loading the entire installer (up to `MAX_INSTALLER_BYTES = 200 MiB`) into a contiguous `Vec<u8>` in RAM.
  - `download_and_verify_update()` ([updater.rs:369-394](file:///e:/Development/posthaze-download-manager/crates/posthaze-ui/src/updater.rs#L369-L394)) performs synchronous `std::fs::File::write_all` directly on a Tokio worker thread, and both `check_release_from_url` ([line 295](file:///e:/Development/posthaze-download-manager/crates/posthaze-ui/src/updater.rs#L295)) and `download_and_verify_update` ([line 334](file:///e:/Development/posthaze-download-manager/crates/posthaze-ui/src/updater.rs#L334)) construct throwaway `reqwest::Client` instances.
- **Recommendation:**
  1. Stream SHA-256 hashing while writing chunks to disk (in `spawn_blocking` or via `BufWriter`), eliminating one full-file read pass.
  2. For Ed25519 verification, run `verify_download` inside `spawn_blocking` (and in a future release format, consider signing the SHA-256 digest / `SHA256SUMS.txt` itself so installer verification uses O(1) RAM instead of reading the full `.exe` into memory).
  3. Reuse the engine's configured `reqwest::Client` (also ensures updater honors user proxy settings).
- **Throughput Risk:** **None** (keeps Tokio worker threads unblocked during update downloads).

---

### 3.6 Windows Memory Management & Global Allocator

#### A. Caution on `EmptyWorkingSet` / `SetProcessWorkingSetSizeEx`
- **Primary-Source Analysis:** Per Microsoft Learn documentation for [`SetProcessWorkingSetSizeEx`](https://learn.microsoft.com/en-us/windows/win32/api/memoryapi/nf-memoryapi-setprocessworkingsetsizeex) and [`EmptyWorkingSet`](https://learn.microsoft.com/en-us/windows/win32/api/psapi/nf-psapi-emptyworkingset), passing `(SIZE_T)-1` removes pages from the process's physical working set (moving them to the standby/modified list or pagefile), **without** freeing heap allocations. When the app next runs or restores from tray, Windows incurs a burst of soft/hard page faults, causing noticeable UI stutter.
- **Recommendation:** Do **not** rely on `EmptyWorkingSet` as a substitute for freeing real allocations. Prioritize the structural fixes above (bounded buffers, capped Tokio threads, SQLite cache sizing, zero-render tray mode). Only consider optional working-set trimming if the app is hidden in the tray **and** has zero active downloads.

#### B. Evaluate `mimalloc` Global Allocator on Windows (P3)
- **Primary-Source Analysis:** Per the official Microsoft `mimalloc` repository and documentation ([github.com/microsoft/mimalloc](https://github.com/microsoft/mimalloc), [microsoft.github.io/mimalloc](https://microsoft.github.io/mimalloc/)), `mimalloc` uses per-page free-list sharding and lock-free cross-thread frees, which significantly reduces heap fragmentation and contention under multi-threaded producer-consumer workloads (like Tokio workers passing `Bytes` to `PartFileWriter` and GPUI rendering).
- **Important Caveat:** On Windows, `mimalloc` can report higher **Committed** memory by default unless purge options (`MIMALLOC_PURGE_DELAY`, `MIMALLOC_PURGE_DECOMMITS=1`) are tuned, even when Working Set is lower.
- **Recommendation:** A/B benchmark `mimalloc` (v2/v3) against the default Windows heap on both **Private Working Set** and **Commit Size** during idle, active multi-download, and post-download states before adopting.

---

## 4. What NOT to Change (Download Speed Protections)

To guarantee zero throughput regressions, preserve the following invariants verified by Posthaze's benchmarks:

1. **Do NOT shrink `CHUNK_BUFFER_SIZE` (`512 KiB`) blindly** ([transfer.rs:20](file:///e:/Development/posthaze-download-manager/crates/posthaze-engine/src/transfer.rs#L20)): Smaller chunks increase `WriteFile` syscalls and `mpsc` channel operations per GiB. Only test `256 KiB` vs `512 KiB` through `writer_bench.rs` and `posthaze-cli bench` on both NVMe and SATA/HDD.
2. **Do NOT disable `tcp_nodelay(true)`** ([lib.rs:627](file:///e:/Development/posthaze-download-manager/crates/posthaze-engine/src/lib.rs#L627)): Prevents Nagle + delayed-ACK stalls on range `GET` requests and retries.
3. **Do NOT remove dynamic segment splitting or reduce `MIN_SEGMENT_SIZE` (`2 MiB`)** ([transfer.rs:19, 465](file:///e:/Development/posthaze-download-manager/crates/posthaze-engine/src/transfer.rs#L19)): Work stealing delivered a **3.17× speedup** at `c=8` ([ticket-04-work-stealing.md](file:///e:/Development/posthaze-download-manager/docs/benchmarks/ticket-04-work-stealing.md)).
4. **Do NOT drop Tokio below 2 worker threads (`current_thread`)**: Multi-segment TLS decryption + HTTP framing + SQLite/IPC benefit from 2–4 async worker threads on multi-core CPUs.
5. **Do NOT switch SQLite out of `WAL` + `synchronous = NORMAL`** ([store.rs:124-125](file:///e:/Development/posthaze-download-manager/crates/posthaze-engine/src/store.rs#L124-L125)) or make checkpoints synchronous on `PartFileWriter`: Async `try_send` checkpoints add ~0–5% overhead at 7.3 Gbps ([milestone-02-sqlite-checkpoint-overhead.md](file:///e:/Development/posthaze-download-manager/docs/benchmarks/milestone-02-sqlite-checkpoint-overhead.md)).
6. **Do NOT enable `sync_on_complete` (`FlushFileBuffers`) by default** and **do NOT call `EmptyWorkingSet` while downloads are active**.

---

## 5. Measurement & Benchmarking Plan

Before and after implementing each priority tier, capture both **memory/CPU** and **throughput** metrics on release builds (`cargo build --release`):

### 1. Throughput Regression Gate (must remain within noise / improve)
```powershell
# Disk-writer microbenchmark (64/128/256/512 KiB chunks)
cargo bench -p posthaze-engine

# Loopback multi-Gbps + SQLite checkpoint A/B
cargo run --release -p posthaze-cli -- bench --sizes 100M,1G -c 1,4,8,16,32 --runs 3 --no-external --compare-db

# Flaky / throttled / work-stealing stress test
cargo run --release -p posthaze-cli -- bench --sizes 100M -c 8 --per-conn-mbps 40 --drop-pct 5 --error-503-pct 5 --no-external
```

### 2. RAM & CPU Profiling Matrix (`posthaze-ui.exe`)
Record **Private Working Set**, **Commit Size**, **Thread Count**, and **CPU %** (via Windows Performance Monitor / Process Explorer / `Get-Process posthaze-ui | Select-Object WorkingSet64, PrivateMemorySize64, Threads, CPU`) across five states:
1. **Cold idle (empty queue):** Launch → wait 30s → record baseline RAM & thread count.
2. **Large queue idle (500 persisted records):** Scroll/filter/sort → record UI frame time & heap growth.
3. **Active download (visible window):** 3 concurrent 1 GiB downloads (`c=8` each) against `posthaze-cli serve --size 1G` → record peak RAM, steady-state RAM, UI CPU %, and aggregate Mbps.
4. **Active download (hidden in system tray):** Same 3 concurrent 1 GiB downloads with window closed to tray → verify UI CPU drops near 0% and aggregate Mbps is unchanged or higher.
5. **Post-download idle (60s after completion):** Verify idle TLS connections, SQLite connections, and transfer buffers are released back to baseline (no retained 64–200 MiB plateau).

---

## 6. Primary Sources Cited

- **Repository Codebase:**
  - [Cargo.toml](file:///e:/Development/posthaze-download-manager/Cargo.toml)
  - [crates/posthaze-engine/src/writer.rs](file:///e:/Development/posthaze-download-manager/crates/posthaze-engine/src/writer.rs)
  - [crates/posthaze-engine/src/transfer.rs](file:///e:/Development/posthaze-download-manager/crates/posthaze-engine/src/transfer.rs)
  - [crates/posthaze-engine/src/runner.rs](file:///e:/Development/posthaze-download-manager/crates/posthaze-engine/src/runner.rs)
  - [crates/posthaze-engine/src/lib.rs](file:///e:/Development/posthaze-download-manager/crates/posthaze-engine/src/lib.rs)
  - [crates/posthaze-engine/src/store.rs](file:///e:/Development/posthaze-download-manager/crates/posthaze-engine/src/store.rs)
  - [crates/posthaze-engine/migrations/0001_init.sql](file:///e:/Development/posthaze-download-manager/crates/posthaze-engine/migrations/0001_init.sql)
  - [crates/posthaze-ui/src/bridge.rs](file:///e:/Development/posthaze-download-manager/crates/posthaze-ui/src/bridge.rs)
  - [crates/posthaze-ui/src/main.rs](file:///e:/Development/posthaze-download-manager/crates/posthaze-ui/src/main.rs)
  - [crates/posthaze-ui/src/view_model.rs](file:///e:/Development/posthaze-download-manager/crates/posthaze-ui/src/view_model.rs)
  - [crates/posthaze-ui/src/motion.rs](file:///e:/Development/posthaze-download-manager/crates/posthaze-ui/src/motion.rs)
  - [crates/posthaze-ui/src/tray.rs](file:///e:/Development/posthaze-download-manager/crates/posthaze-ui/src/tray.rs)
  - [crates/posthaze-ui/src/platform_hooks.rs](file:///e:/Development/posthaze-download-manager/crates/posthaze-ui/src/platform_hooks.rs)
  - [crates/posthaze-ui/src/updater.rs](file:///e:/Development/posthaze-download-manager/crates/posthaze-ui/src/updater.rs)
  - [crates/posthaze-ui/src/assets.rs](file:///e:/Development/posthaze-download-manager/crates/posthaze-ui/src/assets.rs)
  - [docs/adr/0005-dedicated-writer-channel.md](file:///e:/Development/posthaze-download-manager/docs/adr/0005-dedicated-writer-channel.md)
  - [docs/benchmarks/milestone-02-sqlite-checkpoint-overhead.md](file:///e:/Development/posthaze-download-manager/docs/benchmarks/milestone-02-sqlite-checkpoint-overhead.md)
  - [docs/benchmarks/ticket-04-work-stealing.md](file:///e:/Development/posthaze-download-manager/docs/benchmarks/ticket-04-work-stealing.md)
- **External Official Documentation & Specifications:**
  - `bytes` crate (`BytesMut::split`, `BytesMut::reserve`): https://docs.rs/bytes/latest/bytes/struct.BytesMut.html
  - `tokio` crate (`runtime::Builder::worker_threads`, `max_blocking_threads`, `thread_keep_alive`): https://docs.rs/tokio/latest/tokio/runtime/struct.Builder.html
  - `reqwest` crate (`ClientBuilder::pool_idle_timeout`, `pool_max_idle_per_host`): https://docs.rs/reqwest/latest/reqwest/struct.ClientBuilder.html
  - Rust stdlib (`std::os::windows::fs::FileExt::seek_write`): https://doc.rust-lang.org/std/os/windows/fs/trait.FileExt.html#tymethod.seek_write
  - SQLite PRAGMA Reference (`cache_size`, `shrink_memory`, `wal_autocheckpoint`): https://www.sqlite.org/pragma.html
  - `sqlx` crate (`SqliteConnectOptions`, `PoolOptions`): https://docs.rs/sqlx/latest/sqlx/sqlite/struct.SqliteConnectOptions.html
  - `tray-icon` crate (`TrayIconEvent::set_event_handler`, `MenuEvent::set_event_handler`): https://docs.rs/tray-icon/latest/tray_icon/enum.TrayIconEvent.html#method.set_event_handler
  - Microsoft Learn (`SetProcessWorkingSetSizeEx`, `EmptyWorkingSet`): https://learn.microsoft.com/en-us/windows/win32/api/memoryapi/nf-memoryapi-setprocessworkingsetsizeex and https://learn.microsoft.com/en-us/windows/win32/api/psapi/nf-psapi-emptyworkingset
  - Microsoft `mimalloc` repository & tuning docs: https://github.com/microsoft/mimalloc and https://microsoft.github.io/mimalloc/
