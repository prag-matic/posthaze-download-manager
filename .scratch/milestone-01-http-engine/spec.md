# Milestone 01: Core Headless HTTP Download Engine

Status: ready-for-agent

## Problem Statement

Windows users downloading large files over HTTP/HTTPS face slow single-stream browser downloads, fragile transfers that fail on transient network drops, and heavy desktop download managers that couple their UI tightly to networking internals. Before building a Slint desktop interface, Posthaze Download Manager needs a fast, resilient, headless Rust download engine that saturates available bandwidth, recovers from transient connection failures, and can be verified and benchmarked independently of any GUI.

## Solution

Build the foundational Cargo workspace (`posthaze-engine` and `posthaze-cli`) implementing an asynchronous, multi-connection HTTP/HTTPS download engine. The engine probes server capabilities, splits range-capable `Download`s into dynamic `Segment`s across a bounded pool of `Worker`s, streams `Chunk`s to a single pre-allocated `.part` file via a dedicated blocking writer task, emits live progress snapshots, and exposes a minimal CLI (`posthaze-cli download`) for headless execution and benchmarking.

## User Stories

1. As a Windows power user, I want to download an HTTP/HTTPS file from a URL using multiple concurrent `Worker`s, so that I can saturate my available network bandwidth.
2. As a user, I want the engine to probe the server before downloading (`Probing`), so that it automatically uses multi-`Segment` mode when supported and safely falls back to a single unbounded `Segment` when it is not.
3. As a user downloading from servers that block or strip headers on `HEAD`, I want `Probing` to fall back to `GET` with `Range: bytes=0-0`, so that I still get multi-connection acceleration on strict CDNs.
4. As a user downloading dynamic or chunked responses without a `Content-Length`, I want the engine to degrade cleanly to single-stream mode, so that the `Download` succeeds without corrupting range assumptions.
5. As a user, I want redirects followed during `Probing` to capture the final effective URL, so that `Worker`s fetch from the resolved endpoint consistently.
6. As a user who does not specify an output path, I want the engine to derive the filename from `Content-Disposition`, then the final URL path, and finally a safe default fallback, so that downloaded files have sensible names automatically.
7. As a user, I want `Probing` to capture `ETag` and `Last-Modified` metadata when provided, so that future resume/validation layers can verify resource identity.
8. As a user downloading large files, I want the engine to pre-allocate a single `.part` file and write `Chunk`s directly at their byte offsets, so that I do not need double disk space or a slow post-download merge pass.
9. As a user, I want the `.part` file atomically renamed to the final target filename only after all `Segment`s flush and sync, so that partial files are never mistaken for completed downloads.
10. As a user, I want a slow or throttled `Worker` not to bottleneck the whole `Download`, so that faster `Worker`s dynamically split and steal work from the largest remaining `Segment`.
11. As a user, I want dynamic segment splitting to respect a minimum `Segment` size floor (2 MiB), so that the engine does not waste overhead creating tiny ranges near the end of a file.
12. As a user, I want transient network drops, timeouts, `429`, and `5xx` errors on a `Worker` to retry from its last written `cursor` with exponential backoff and jitter, so that temporary hiccups do not abort a large `Download`.
13. As a user, I want disk writes handled by a dedicated blocking writer task over a bounded channel, so that memory stays bounded even when network speed exceeds disk write speed.
14. As a developer, I want the engine to live in a separate crate from the CLI (and future Slint UI), so that the UI/engine boundary is enforced at compile time.
15. As a developer, I want to run `posthaze-cli download <url> [-o <path>] [-c <connections>]`, so that I can execute headless downloads and observe live throughput, progress, and active `Worker` counts.
16. As a maintainer, I want `cargo test` and `cargo clippy -- -D warnings` to pass cleanly on Windows with deterministic mock-server integration tests and BLAKE3 byte-for-byte verification, so that throughput and correctness regressions are caught automatically.

## Implementation Decisions

- **Workspace & crate boundaries (ADR-0001):** Initialize a multi-crate Cargo workspace separating the core engine crate (`posthaze-engine`) from the headless CLI binary crate (`posthaze-cli`), leaving room for a future `posthaze-ui` crate. The engine crate owns all HTTP networking, `Segment` scheduling, `Worker` execution, and file I/O, exposing an async control and progress-observation API.
- **Domain vocabulary (`GLOSSARY.md`):** Code, types, and events use the canonical terms `Download`, `Segment`, `Chunk`, and `Worker`, along with the `Download` lifecycle states (`Queued`, `Probing`, `Active`, `Paused`, `Completed`, `Failed`, `Cancelled`).
- **Capability probing (ADR-0004):** `Probing` issues a `HEAD` request first, follows redirects to capture the final URL, and falls back to `GET` with `Range: bytes=0-0` if `HEAD` returns `403`, `405`, `501`, or omits range/length headers. Multi-`Segment` mode activates only when both confirmed byte-range support and a known `Content-Length` exist; otherwise the `Download` runs as a single unbounded `Segment`. Filename resolution checks `Content-Disposition`, then the final URL path, then a fallback name; `ETag` and `Last-Modified` are recorded on the `Download` metadata.
- **Pre-allocated `.part` file & dedicated writer (ADR-0002, ADR-0005):** When `Content-Length` is known, the engine pre-allocates a single `.part` file to the target size. Each active `Download` spawns one dedicated blocking writer task receiving `(offset, Bytes)` `Chunk`s over a bounded `mpsc` channel and performing positional OS writes, flushing and syncing before atomically renaming `.part` to the final path.
- **Dynamic segment splitting (ADR-0003):** Default concurrency is 8 `Worker`s per `Download` (configurable via CLI/API) with a 2 MiB minimum `Segment` floor and 64 KiB–256 KiB `Chunk` buffers. Each `Segment` shares an `AtomicU64` end boundary; when a `Worker` finishes, the scheduler bisects the largest remaining `Segment` above the floor and lowers its `AtomicU64` end boundary to `mid`, causing the incumbent `Worker` to stop reading and drop its HTTP stream once its `cursor` reaches `mid`.
- **Retries & backoff:** On transient `Worker` failures (connection drop, timeout, `429`, `5xx`), the `Worker` retries its remaining `[cursor, end)` sub-range with exponential backoff and jitter up to a bounded attempt limit (default 5) before transitioning the `Download` to `Failed`.
- **In-memory persistence boundary:** Milestone 01 keeps `Download` and `Segment` state in-memory behind a clean state/persistence boundary so SQLite (`sqlx`) durability can be plugged in during Milestone 02 without rewriting the scheduler or `Worker` loop.
- **Progress reporting:** The engine emits periodic/throttled progress snapshots (bytes downloaded, total bytes, instantaneous/average throughput, active `Worker` count, `Download` state) consumed by `posthaze-cli` for terminal rendering.

## Testing Decisions

- **Primary test seam:** Test through the public `posthaze-engine` download API against an in-process mock HTTP server and real temporary filesystem directories, verifying the final output file byte-for-byte with BLAKE3 (no unit tests mocking internal `Worker` or writer channels).
- **Required scenarios:**
  1. Multi-`Segment` range download of a multi-megabyte payload with BLAKE3 hash verification.
  2. Non-range server fallback (no `Accept-Ranges`, single unbounded `Segment`, and unknown `Content-Length` chunked transfer).
  3. `HEAD`-blocked (`405`/`403`) server triggering `GET Range: bytes=0-0` fallback and still running multi-`Segment`.
  4. Dynamic work-stealing: one `Worker` artificially throttled while others finish and bisect its remaining `Segment`.
  5. Transient error recovery: mock server drops a connection or returns `503` mid-`Segment`, verifying retry from the last written `cursor` and clean completion.
- **Quality gate:** `cargo test` and `cargo clippy -- -D warnings` must pass on Windows.

## Out of Scope

- SQLite (`sqlx`) persistence, pause/resume across process restarts, and crash recovery (deferred to Milestone 02).
- Multi-download `Queue` scheduling and global bandwidth throttling.
- Slint desktop UI (`posthaze-ui`) and Windows OS notifications.
- Non-HTTP/HTTPS protocols, browser extensions, video extraction, and cloud sync.

## Further Notes

- Grounded in [`Architecture Proposal - Windows Download Manager.md`](../../docs/Architecture%20Proposal%20-%20Windows%20Download%20Manager.md), [`GLOSSARY.md`](../../GLOSSARY.md), and ADRs `0001`–`0005` in `docs/adr/`.
- Next step: run `/to-tickets` on this spec to break Milestone 01 into ordered tracer-bullet implementation tickets under `.scratch/milestone-01-http-engine/issues/`.
