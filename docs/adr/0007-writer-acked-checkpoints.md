# Writer-acknowledged `Checkpoint`s for crash recovery

- **Status:** accepted

`Segment` progress is persisted to SQLite only from `Checkpoint`s emitted by the dedicated `.part` file writer *after* the corresponding `Chunk`s have been written to the OS file, rather than persisting `Worker` network-read cursors directly or only saving state on graceful `Paused`.

## Considered Options

- **Pause-only persistence:** Simple and zero-overhead during active transfers, but an unexpected process crash or power loss discards all in-flight progress since the transfer started.
- **Network-side cursor persistence:** `Worker`s or the scheduler periodically write their read cursors to SQLite, which risks recording a cursor ahead of `Chunk`s still buffered in the bounded writer channel and leaving an un-written hole on crash recovery.
- **Writer-acknowledged `Checkpoint`s:** Because the dedicated writer task owns all positional writes and flushes, checkpoints emitted after write/flush guarantee SQLite never advances past what is actually on disk, bounding crash-recovery loss to one checkpoint interval.
