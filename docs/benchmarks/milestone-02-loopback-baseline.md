# Benchmark — Milestone 02 Zero-SQLite Loopback Baseline

Loopback benchmark captured after Milestone 02 Ticket 01 (`1c91b4a`) using `posthaze-cli bench` (`DownloadEngine::new()`, `store: None`). Because `bench_runner` runs ephemerally without SQLite or periodic `Checkpoint`s (Ticket 04), this serves as the zero-SQLite loopback baseline for measuring future `Checkpoint` overhead.

## Command & Target

- **Command:** `cargo run --release -p posthaze-cli -- bench`
- **Target:** In-process loopback HTTP server (`100.0 MiB` / `104,857,600` bytes, `runs = 3`)
- **Date:** 2026-11-19

## Results

| Scenario | Tool | Size (MiB) | Connections | Runs | Median Time (s) | Median Mbps | Min–Max Mbps | Splits | Retries |
|---|---|---:|---:|---:|---:|---:|---:|---:|---:|
| custom | posthaze | 100.0 | 1 | 3 | 0.465 | 1802.4 | 1672.7–2012.2 | 0 | 0 |
| custom | aria2c | 100.0 | 1 | 3 | 0.762 | 1101.1 | 1021.2–1107.7 | 0 | 0 |
| custom | curl | 100.0 | 1 | 3 | 0.290 | 2894.0 | 2839.3–3166.0 | 0 | 0 |
| custom | posthaze | 100.0 | 2 | 3 | 0.195 | 4306.1 | 3554.5–4531.0 | 0 | 0 |
| custom | aria2c | 100.0 | 2 | 3 | 0.913 | 919.1 | 838.2–1104.0 | 0 | 0 |
| custom | posthaze | 100.0 | 4 | 3 | 0.182 | 4618.1 | 3131.3–5748.7 | 1 | 0 |
| custom | aria2c | 100.0 | 4 | 3 | 1.270 | 660.4 | 623.0–1416.1 | 0 | 0 |
| custom | posthaze | 100.0 | 8 | 3 | 0.166 | 5050.9 | 4732.2–5458.9 | 2 | 0 |
| custom | aria2c | 100.0 | 8 | 3 | 0.604 | 1388.9 | 242.0–1523.7 | 0 | 0 |
| custom | posthaze | 100.0 | 16 | 3 | 0.180 | 4667.7 | 4544.7–4968.0 | 0 | 0 |
| custom | aria2c | 100.0 | 16 | 3 | 0.600 | 1397.8 | 1270.3–1430.3 | 0 | 0 |
| custom | posthaze | 100.0 | 32 | 3 | 0.161 | 5209.9 | 5130.3–5301.4 | 1 | 0 |
