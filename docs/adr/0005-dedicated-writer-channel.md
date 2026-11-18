# Dedicated blocking writer task over a bounded channel

- **Status:** accepted

Each active `Download` spawns a single dedicated blocking file-writer task that receives `(offset, Bytes)` `Chunk`s over a bounded `mpsc` channel and writes them to the `.part` file using positional OS writes (`FileExt::seek_write` on Windows), rather than having each `Worker` dispatch its own `spawn_blocking` writes or using `tokio::fs::File`.

## Considered Options

- **Per-`Worker` `spawn_blocking` writes on a shared `File`:** Parallelizes OS write syscalls, but saturates Tokio's blocking thread pool under high concurrency and offers no natural memory backpressure when disk I/O lags behind network throughput.
- **Dedicated writer task + bounded `mpsc` channel:** Serializes disk writes in one place with zero lock contention, applies automatic backpressure to fast `Worker`s when the channel fills, and provides a single owner responsible for flushing and syncing before the `.part` → final rename.
