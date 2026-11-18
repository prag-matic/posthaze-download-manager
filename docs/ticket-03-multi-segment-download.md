# Ticket 03 — Multi-`Segment` Concurrent `Download`

Ticket 03 adds concurrent multi-`Segment` HTTP `Range` downloading in [`crates/posthaze-engine/src/transfer.rs`](../crates/posthaze-engine/src/transfer.rs), coordinated with [`PartFileWriter`](../crates/posthaze-engine/src/writer.rs) and [`DownloadEngine::download`](../crates/posthaze-engine/src/lib.rs).

## Scheduling & Execution Flow

1. **Eligibility:** Multi-`Segment` mode activates only when `Probing` confirms both `supports_ranges == true` and a known `content_length`.
2. **Pre-allocation:** `PartFileWriter::spawn` creates `<final_path>.part` and pre-allocates `Content-Length` bytes via `File::set_len` before any `Worker` starts.
3. **`Segment` planning (`plan_segments`):**
   - Enforces a 2 MiB minimum `Segment` floor (`MIN_SEGMENT_SIZE`).
   - Computes `worker_count = (total_bytes / MIN_SEGMENT_SIZE).max(1).min(max_workers.max(1))`.
   - Partitions `[0, total_bytes)` into `worker_count` contiguous `[start, end)` `Segment`s, distributing remainder bytes evenly.
4. **Concurrent `Worker`s:**
   - Spawns one async `Worker` task per `Segment` in a `tokio::task::JoinSet`.
   - Each `Worker` sends `GET` with `Range: bytes={start}-{end - 1}`, verifies `206 Partial Content`, coalesces incoming bytes into 128 KiB `Chunk`s, and writes at positional offsets via the shared bounded `PartFileWriter` channel.
   - Shared `AtomicU64` (`downloaded`) and `AtomicUsize` (`active_workers`) counters feed a 50 ms progress reporter task emitting `ProgressSnapshot`s.
5. **Finalization:** Once all `Worker`s complete and flush, `PartFileWriter::finish` calls `sync_all()` and atomically renames `.part` to the final path.

## Verification

[`crates/posthaze-engine/tests/multi_segment_download.rs`](../crates/posthaze-engine/tests/multi_segment_download.rs) verifies:

- `downloads_multi_segment_payload_and_verifies_blake3` — 10 MiB payload across 4 concurrent `Worker`s with BLAKE3 verification and `.part` cleanup.
- `downloads_multi_segment_after_head_blocked_range_probe_fallback` — 8 MiB payload after `HEAD` `405` → `GET Range: bytes=0-0` fallback across 4 `Worker`s.
- `clamps_workers_to_one_when_below_min_segment_floor` — 512 KiB payload clamps `workers_used` to `1` even with `max_workers = 8`.
