# Ticket 04 — Dynamic `Segment` Splitting (Work Stealing)

Ticket 04 implements work-stealing `Segment` bisection in [`crates/posthaze-engine/src/transfer.rs`](../crates/posthaze-engine/src/transfer.rs) (per [ADR-0003](adr/0003-dynamic-segment-splitting.md)) so fast `Worker`s never sit idle waiting on a throttled connection.

## Mechanism

1. **Shared `ActiveSegment` state:** Each active `Segment` tracks its `cursor` (`AtomicU64`) and exclusive `end` boundary (`AtomicU64`).
2. **Bisection on completion (`steal_largest_segment`):** When a `Worker` finishes its assigned `Segment`, it removes that `Segment` from the active list and inspects the remaining active `Segment`s:
   - Finds the `Segment` with the largest `remaining = end - cursor`.
   - If `remaining > MIN_SEGMENT_SIZE` (`2 MiB`), computes `mid = cursor + remaining / 2` and atomically lowers `target.end` from `old_end` to `mid` via `compare_exchange`.
   - Assigns `[max(cursor_after, mid), old_end)` as a new `ActiveSegment` to the idle `Worker`.
3. **Zero-overhead stream truncation:** As the incumbent `Worker` reads incoming `Chunk`s, it checks `seg.end.load(Acquire)`. Once its `cursor` reaches `mid`, it flushes any buffered bytes and drops its HTTP stream cleanly without issuing extra requests.

## Verification

[`crates/posthaze-engine/tests/dynamic_splitting.rs`](../crates/posthaze-engine/tests/dynamic_splitting.rs) (`steals_work_from_throttled_worker_and_verifies_blake3`) downloads a 12 MiB payload with 2 `Worker`s where the first `Range` request is throttled to ~1 MiB/s (`60 ms` per 64 KiB):

- Without work stealing, the download took `6.20s` across `2` requests.
- With work stealing, `Worker` 1 repeatedly bisected `Segment` 0, completing in `1.77s` with byte-for-byte BLAKE3 equality.
