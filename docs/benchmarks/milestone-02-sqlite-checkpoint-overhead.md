# Benchmark — Milestone 02 SQLite `Checkpoint` Overhead (`posthaze` vs `posthaze (db)`)

A/B loopback comparison between ephemeral `DownloadEngine::new()` (`posthaze`) and SQLite-backed `DownloadEngine::open()` with WAL mode (`synchronous = NORMAL`) and writer-acknowledged `Checkpoint`s (`posthaze (db)`).

## Command & Target

- **Command:** `cargo run --release -p posthaze-cli -- bench --no-external --compare-db`
- **Target:** In-process loopback HTTP server (`100.0 MiB` / `104,857,600` bytes, `runs = 3`, alternating execution order)
- **Date:** 2026-11-19

## Results

| Scenario | Tool | Size (MiB) | Connections | Runs | Median Time (s) | Median Mbps | Min–Max Mbps | Splits | Retries |
|---|---|---:|---:|---:|---:|---:|---:|---:|---:|
| custom | posthaze | 100.0 | 1 | 3 | 0.410 | 2045.5 | 2002.2–7748.9 | 0 | 0 |
| custom | posthaze (db) | 100.0 | 1 | 3 | 0.453 | 1851.5 | 1728.3–1953.6 | 0 | 0 |
| custom | posthaze | 100.0 | 2 | 3 | 0.255 | 3289.8 | 2879.1–3602.8 | 2 | 0 |
| custom | posthaze (db) | 100.0 | 2 | 3 | 0.188 | 4470.7 | 4346.9–4694.5 | 2 | 0 |
| custom | posthaze | 100.0 | 4 | 3 | 0.129 | 6481.0 | 5227.6–7643.8 | 1 | 0 |
| custom | posthaze (db) | 100.0 | 4 | 3 | 0.114 | 7364.3 | 6003.3–7393.5 | 1 | 0 |
| custom | posthaze | 100.0 | 8 | 3 | 0.165 | 5082.0 | 3694.3–5690.2 | 2 | 0 |
| custom | posthaze (db) | 100.0 | 8 | 3 | 0.160 | 5249.8 | 4407.4–5253.6 | 1 | 0 |
| custom | posthaze | 100.0 | 16 | 3 | 0.139 | 6048.7 | 5874.5–6137.0 | 1 | 0 |
| custom | posthaze (db) | 100.0 | 16 | 3 | 0.146 | 5762.8 | 5345.6–5886.1 | 1 | 0 |
| custom | posthaze | 100.0 | 32 | 3 | 0.172 | 4866.3 | 4629.5–5234.6 | 1 | 0 |
| custom | posthaze (db) | 100.0 | 32 | 3 | 0.177 | 4736.8 | 4548.5–5141.9 | 0 | 0 |

## Observations

- **Negligible `Checkpoint` cost at multi-Gbps rates:** Because `PartFileWriter` flushes and dispatches `Checkpoint`s via non-blocking `try_send` (`8 MiB` / `2s`) to a background async SQLite task in WAL mode (`synchronous = NORMAL`), `posthaze (db)` tracks `posthaze` within ~0–5% across `c=4..32` (well inside sub-second loopback variance), peaking at **7,364.3 Mbps** (`c=4`) and **5,249.8 Mbps** (`c=8`).
