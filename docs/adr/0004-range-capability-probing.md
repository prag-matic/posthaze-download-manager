# HTTP capability probing with HEAD and Range GET fallback

- **Status:** accepted

Before scheduling `Segment`s, `Probing` issues a `HEAD` request and falls back to `GET` with `Range: bytes=0-0` whenever `HEAD` is rejected (`403`/`405`/`501`) or omits range/length metadata. Multi-`Segment` mode is enabled only when both confirmed byte-range support (`206 Partial Content` with `Content-Range`, or `Accept-Ranges: bytes`) and a known `Content-Length` are present; otherwise the `Download` falls back to a single unbounded `Segment`.

## Consequences

- Servers that block `HEAD` or strip `Accept-Ranges` on `HEAD` still get multi-connection acceleration via the `bytes=0-0` probe.
- Dynamic/chunked responses without `Content-Length` cleanly degrade to single-stream mode without corrupting range assumptions.
