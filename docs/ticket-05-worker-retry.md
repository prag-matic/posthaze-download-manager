# Ticket 05 — Transient `Worker` Retry with Exponential Backoff & Jitter

Ticket 05 adds resilient per-`Segment` retry recovery in [`crates/posthaze-engine/src/transfer.rs`](../crates/posthaze-engine/src/transfer.rs).

## Retry Policy

- **Transient errors (`is_transient_error`):**
  - Transport/stream errors (`EngineError::Http`)
  - HTTP `408 Request Timeout`, `429 Too Many Requests`, and `5xx` statuses (`EngineError::HttpStatus`)
  - Premature stream truncation (`std::io::ErrorKind::UnexpectedEof`)
- **Fatal errors (fail immediately):** Non-retryable `4xx` responses (e.g., `404`, `403`, `416`) and local disk/writer errors (`EngineError::Writer`, non-EOF `EngineError::Io`).
- **Resume from `cursor`:** Before returning an error on a failed attempt, any buffered `Chunk`s are flushed to [`PartFileWriter`](../crates/posthaze-engine/src/writer.rs) so `seg.cursor` and disk state stay synchronized. The next attempt issues `Range: bytes={cursor}-{end - 1}` rather than restarting from `seg.start`.
- **Backoff & attempt budget:** Retries use exponential backoff (`100 ms` base, `5s` cap) with jitter (`backoff_with_jitter`) up to `MAX_SEGMENT_RETRIES = 5`. If an attempt makes forward progress (`cursor > attempt_start`), the consecutive-retry counter resets to `0`.

## Verification

[`crates/posthaze-engine/tests/worker_retry.rs`](../crates/posthaze-engine/tests/worker_retry.rs) verifies:

- `recovers_from_503_and_mid_segment_drop_resuming_from_cursor` — Recovers from an initial `503` and a mid-stream connection drop at `512 KiB`, resuming from `cursor >= 512 KiB` and passing BLAKE3 verification.
- `fails_cleanly_when_max_retries_exhausted` — Exhausts 5 retries (6 total `GET` attempts) against a persistent `503` server and returns `EngineError::HttpStatus { status: 503, .. }`.
