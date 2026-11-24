# Dynamic segment splitting (work stealing)

- **Status:** accepted

The `Segment` scheduler dynamically splits the largest remaining active `Segment` whenever a `Worker` finishes early and the remaining un-downloaded range exceeds the minimum `Segment` size floor, rather than assigning fixed static ranges for the lifetime of a `Download`.

## Considered Options

- **Static equal-sized `Segment`s:** Simple to reason about, but a single throttled or stalled connection leaves a long tail where all other `Worker`s sit idle.
- **Dynamic segment splitting:** Keeps all `Worker`s saturated until completion, at the cost of requiring coordinated `Segment` end-offset updates between the scheduler and active `Worker`s.

## Amendments

- **Lock-free splits, worker failure isolation, and slow-worker shedding:** `steal_largest_segment()` aligns bisections to `4 KiB` in memory without awaiting synchronous SQLite transactions on the worker critical path; newly created `Segment`s are persisted asynchronously via `ON CONFLICT(download_id, segment_index)` upserts on periodic `Checkpoint`s and pause/failure finalization. When a `Worker` exhausts retries on a non-fatal error while surviving `Worker`s remain active, it releases its `Segment` and retires rather than aborting the whole `Download`. When `BandwidthLimiter` is unlimited, `should_shed_worker()` excludes disk backpressure stall time and reconnects severely degraded sockets (`< shed_bar / 4` after `2s` grace with `> 2s` remaining).
