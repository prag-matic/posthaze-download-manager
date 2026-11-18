# 01: Single-stream Download end-to-end

**What to build:** Initialize the multi-crate Cargo workspace (`posthaze-engine` and `posthaze-cli`), implement `Probing` (`HEAD`, redirect resolution, `Content-Disposition`/URL filename derivation, `ETag`/`Last-Modified` capture), stream a single `Segment` through a dedicated blocking `.part` writer task with flush/sync and atomic rename on completion, expose `posthaze-cli download <url> [-o <path>]` with live throughput/progress output, and prove correctness with an in-process mock HTTP server and BLAKE3 verification.

**Blocked by:** None (can start immediately)

**Status:** resolved

- [x] Multi-crate Cargo workspace compiles cleanly on Windows with `cargo test` and `cargo clippy -- -D warnings`.
- [x] `Probing` follows redirects, captures `ETag`/`Last-Modified`, and resolves filenames via `Content-Disposition` -> final URL path -> fallback.
- [x] Single-stream `Download` writes `Chunk`s through the dedicated blocking writer task to a `.part` file and atomically renames to the final path only on completion.
- [x] `posthaze-cli download <url> [-o <path>]` runs headlessly and displays live progress and throughput.
- [x] In-process mock HTTP server integration test verifies byte-for-byte BLAKE3 hash equality.
