# SQLite-backed settings and engine-owned FIFO queue admission

- **Status:** accepted

`posthaze-engine` persists application `Settings` (`default_output_dir`, `max_workers`, `max_concurrent_downloads`, `sync_on_complete`) in a `settings` table inside the same `state.db` SQLite database (`0002_settings.sql`) and enforces FIFO `Queue` concurrency admission directly inside the engine, rather than storing settings in a separate TOML/JSON config file or managing active concurrency limits inside `posthaze-ui`.

## Considered Options

- **Separate TOML/JSON settings file + UI-side queue coordinator:** Keeps SQLite focused strictly on transfers, but forces both `posthaze-cli` and `posthaze-ui` to load, watch, and synchronize two separate state files and risks splitting concurrency enforcement between CLI and GUI callers.
- **SQLite `settings` table + engine-owned FIFO `Queue`:** Guarantees atomic persistence alongside `downloads` and `segments` inside `state.db`, keeps `posthaze-ui` and `posthaze-cli` strictly decoupled from queue scheduling internals ([ADR-0001](0001-multi-crate-cargo-workspace.md)), and ensures `max_concurrent_downloads` is respected uniformly across all callers.

## Consequences

- `start(request)` and `resume(id)` persist/keep the record in `DownloadState::Queued`, return `DownloadHandle` immediately, and wait for a FIFO concurrency slot (up to `max_concurrent_downloads`) before entering `Probing` and `Active`, so `pause()`, `cancel()`, `subscribe()`, and `wait()` behave uniformly whether a `Download` is queued or active.
- Raising `max_concurrent_downloads` via `update_settings` wakes waiting `Queued` `Download`s immediately; lowering it lets in-flight `Active` transfers finish naturally without preemption.
- `DownloadEngine::open` reconciles stale `Queued` rows (alongside stale range-capable `Active` rows) to `Paused` on startup so opening the database (for example, `posthaze-cli list`) never silently initiates background network transfers.
