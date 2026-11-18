# Benchmark Baseline — Ticket 03 (Static Multi-`Segment`)

Baseline throughput comparison of `posthaze-cli` (release build) prior to dynamic `Segment` splitting (Ticket 04).

## Test Target

- **Artifact:** `vectorcraft-0.24.1-windows-arm64.msi` (`121,065,472` bytes / `115.46 MiB`)
- **URL:** `https://github.com/storytold/vectorcraft/releases/download/v0.24.1/vectorcraft-0.24.1-windows-arm64.msi`
- **Verified SHA-256 (all runs):** `6AF622E8F8F3783F79A4B5C0AB4F6621823DE6AA17C21C415D5583E56AA3102F`
- **Date:** 2026-11-19

## Results

| `-c` | `Workers` used | Probe (ms) | Transfer (s) | Total (s) | Avg Speed | Peak Speed | Speedup vs `c=2` |
|---:|---:|---:|---:|---:|---:|---:|---:|
| 2 | 2 | 837.9 | 58.47 | 59.31 | 1.97 MiB/s (16.57 Mbps) | 9.12 MiB/s (76.52 Mbps) | 1.00× |
| 8 | 8 | 465.5 | 38.35 | 38.81 | 3.01 MiB/s (25.26 Mbps) | 16.05 MiB/s (134.65 Mbps) | 1.52× |
| 16 | 16 | 557.8 | 17.07 | 17.63 | 6.76 MiB/s (56.74 Mbps) | 16.16 MiB/s (135.60 Mbps) | 3.43× |
| 32 | 32 | 399.1 | 13.42 | 13.81 | 8.61 MiB/s (72.19 Mbps) | 18.07 MiB/s (151.61 Mbps) | 4.36× |

## Observations

- **Static tail bottleneck:** In the `-c 8` debug sample log, the final ~6 seconds ran with only `2` and then `1` active `Worker` transferring at ~0.5 MiB/s while the other `Worker`s were idle.
- **Next optimization:** Ticket 04 (dynamic `Segment` splitting / work stealing) targets this tail latency by bisecting the slowest remaining `Segment` whenever a `Worker` finishes early.
