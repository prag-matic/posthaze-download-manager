# Single pre-allocated `.part` file with positional writes

- **Status:** accepted

Active `Download`s with a known `Content-Length` pre-allocate a single `<filename>.part` file up front and stream each `Segment`'s `Chunk`s directly to their byte offsets via positional writes, then atomically rename to the final filename on completion.

## Considered Options

- **Per-`Segment` temporary files merged on completion:** Simpler sequential appends per `Worker`, but doubles peak disk space and adds a full read+write concatenation pass at the end of large downloads.
- **Single pre-allocated `.part` file:** Zero post-download merge I/O and bounded disk footprint, at the cost of reserving the full file size up front and requiring a `Segment` offset map for crash recovery.
