# 03: Multi-Segment concurrent Download with pre-allocated .part file

**What to build:** When `Probing` confirms byte-range support and `Content-Length`, pre-allocate the `.part` file to the target size, spawn a bounded pool of `Worker`s (default 8, configurable via `-c <connections>`) streaming `Chunk`s at their byte offsets through the dedicated writer channel, and surface active `Worker` count in progress snapshots.

**Blocked by:** 01: Single-stream Download end-to-end, 02: HEAD-blocked Range probe fallback and unknown Content-Length handling

**Status:** ready-for-agent

- [ ] Pre-allocates the `.part` file to `Content-Length` before starting `Worker`s in multi-`Segment` mode.
- [ ] Spawns up to the configured `Worker` count (default 8) fetching distinct `Segment`s concurrently and writing `Chunk`s at positional offsets via the dedicated writer channel.
- [ ] `posthaze-cli download <url> [-o <path>] [-c <connections>]` accepts `-c` and renders active `Worker` count alongside throughput and progress.
- [ ] Mock-server integration test downloads a multi-megabyte payload across multiple `Segment`s (including `HEAD`-blocked `Range: bytes=0-0` fallback) and verifies byte-for-byte BLAKE3 equality.
- [ ] `cargo test` and `cargo clippy -- -D warnings` pass cleanly.
