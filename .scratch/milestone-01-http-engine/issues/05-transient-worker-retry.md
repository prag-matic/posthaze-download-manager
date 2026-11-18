# 05: Transient Worker retry with exponential backoff and jitter

**What to build:** Recover from transient `Worker` errors (connection drops, timeouts, `429`, `5xx`) by retrying the remaining `[cursor, end)` sub-range from the last written `cursor` using exponential backoff with jitter up to 5 attempts before transitioning the `Download` to `Failed`.

**Blocked by:** 03: Multi-Segment concurrent Download with pre-allocated .part file

**Status:** ready-for-agent

- [ ] Transient `Worker` failures (`429`, `5xx`, timeouts, mid-stream drops) retry from the last written `cursor` rather than restarting the `Segment` from scratch.
- [ ] Retries apply exponential backoff with jitter and fail the `Download` cleanly once the max attempt limit (default 5) is exhausted.
- [ ] Mock-server integration tests verify recovery from mid-`Segment` connection drops and `503` responses with byte-for-byte BLAKE3 integrity, as well as clean failure when retries are exhausted.
- [ ] `cargo test` and `cargo clippy -- -D warnings` pass cleanly.
