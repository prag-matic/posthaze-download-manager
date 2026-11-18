# 02: HEAD-blocked Range probe fallback and unknown Content-Length handling

**What to build:** Extend `Probing` to fall back to `GET` with `Range: bytes=0-0` whenever `HEAD` returns `403`, `405`, or `501` or omits range/length metadata, and ensure chunked or unknown-`Content-Length` responses degrade cleanly to single unbounded `Segment` mode without `.part` pre-allocation failures.

**Blocked by:** 01: Single-stream Download end-to-end

**Status:** ready-for-agent

- [ ] When `HEAD` returns `403`/`405`/`501` or omits `Accept-Ranges`/`Content-Length`, `Probing` issues `GET` with `Range: bytes=0-0` and parses `206 Partial Content` + `Content-Range`.
- [ ] Multi-`Segment` capability is flagged as supported only when both byte-range support and a known `Content-Length` are confirmed.
- [ ] Chunked / unknown-`Content-Length` responses complete cleanly in single-stream mode and pass BLAKE3 verification in mock-server integration tests.
- [ ] `cargo test` and `cargo clippy -- -D warnings` pass cleanly.
