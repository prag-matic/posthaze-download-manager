# Dedicated background Tokio runtime bridged to GPUI via `EngineEvent` broadcast

- **Status:** accepted

`posthaze-ui` runs `DownloadEngine` (which relies on Tokio, `reqwest`, `sqlx`, and `tokio::fs`) on a dedicated background multi-thread Tokio runtime while `gpui-kit` owns the main Win32/DirectX UI thread, bridging live engine state into GPUI views via `DownloadEngine::subscribe_events()` (`broadcast::Receiver<EngineEvent>`) consumed by `cx.spawn(...)` + `cx.notify()`.

## Considered Options

- **Run HTTP/SQLite tasks directly on GPUI's built-in executor:** Avoids a second runtime, but `reqwest` and `sqlx` (`runtime-tokio`) require a Tokio reactor and blocking-pool context, and sharing the UI executor risks coupling networking I/O with frame rendering.
- **Dedicated background Tokio runtime + `EngineEvent` broadcast channel:** Preserves `posthaze-engine` unmodified for both `posthaze-cli` and `posthaze-ui` ([ADR-0001](0001-multi-crate-cargo-workspace.md)), isolates all HTTP/SQLite/disk work off the UI thread, and feeds GPUI views reactively without polling SQLite.
