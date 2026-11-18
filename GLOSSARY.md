# Posthaze Download Manager

Core domain vocabulary for Posthaze Download Manager, a Windows-native multi-connection HTTP/HTTPS download manager with a decoupled Rust engine and Slint desktop UI.

## Language

**Download**:
A user-initiated transfer of a single URL to a target file path, progressing through `Queued`, `Probing`, `Active`, `Paused`, `Completed`, `Failed`, and `Cancelled` states.
_Avoid_: Task, job, transfer

**Segment**:
A contiguous `[start, end)` byte range of a `Download` assigned to at most one `Worker` at a time; a non-range server yields a single unbounded `Segment`.
_Avoid_: Part, range, slice

**Chunk**:
A transient in-memory I/O buffer streamed within a `Segment` before being flushed to disk, never persisted individually.
_Avoid_: Block, packet, frame

**Worker**:
An async execution unit that fetches a single assigned `Segment` over HTTP/HTTPS and streams its `Chunk`s to storage.
_Avoid_: Thread, connection, fetcher
