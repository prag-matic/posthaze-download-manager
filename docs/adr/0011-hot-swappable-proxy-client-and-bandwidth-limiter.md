# ADR 0011: Hot-Swappable Proxy Client and Shared Bandwidth Limiter

- **Status:** Accepted
- **Date:** 2026-11-19

## Context

Users can update proxy settings (`System`, `None`, `Manual` URL) and global download speed limits (`download_speed_limit_kib`) at runtime through `DownloadEngine::update_settings` while other `Download`s and `Worker`s are active.

## Decision

1. **Hot-Swappable HTTP Client (`Arc<std::sync::RwLock<reqwest::Client>>`):**
   `DownloadEngine` holds its `reqwest::Client` behind an `Arc<std::sync::RwLock<_>>`. On `update_settings`, the engine builds and validates the new `reqwest::Client` before persisting settings, then swaps it atomically so subsequent probes and transfers use the new proxy without restarting the engine.
2. **Shared Async Token-Bucket (`BandwidthLimiter`):**
   `DownloadEngine` shares a single `BandwidthLimiter` across all active single-stream and multi-segment `Worker`s, backed by an `AtomicU64` byte-per-second target and a `tokio::sync::Mutex` refill state. Changing `download_speed_limit_kib` updates the atomic rate immediately, throttling or unthrottling active `Download`s live without interrupting active `Segment`s.
3. **UI-Side Completion Hooks & Notifications:**
   Windows Toast notifications and post-completion external commands (`%path%`) are dispatched from `posthaze-ui` (`platform_hooks.rs`) on `EngineEvent::StateChanged` so `posthaze-engine` stays headless and free of desktop shell dependencies.

## Consequences

- `posthaze-engine` remains cleanly decoupled from OS notification/shell APIs.
- In-flight HTTP connections continue with the client they opened, while any new probe, retry, or `Download` immediately picks up updated proxy settings.
- Bandwidth limits apply globally across all concurrent `Worker`s rather than per-`Segment`.
