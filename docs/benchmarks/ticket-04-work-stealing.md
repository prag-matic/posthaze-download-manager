# Benchmark — Ticket 04 (Dynamic `Segment` Splitting / Work Stealing)

Throughput comparison of `posthaze-cli` (release build) with dynamic `Segment` splitting (Ticket 04) against the [Ticket 03 static baseline](ticket-03-baseline.md).

## Test Target

- **Artifact:** `vectorcraft-0.24.1-windows-arm64.msi` (`121,065,472` bytes / `115.46 MiB`)
- **URL:** `https://github.com/storytold/vectorcraft/releases/download/v0.24.1/vectorcraft-0.24.1-windows-arm64.msi`
- **Verified SHA-256 (all runs):** `6AF622E8F8F3783F79A4B5C0AB4F6621823DE6AA17C21C415D5583E56AA3102F`
- **Date:** 2026-11-19

## Results

| `-c` | Probe (ms) | Transfer (s) | Total (s) | Avg Speed | Peak Speed |
|---:|---:|---:|---:|---:|---:|
| 2 | 4601.7 | 39.56 | 44.16 | 2.92 MiB/s (24.48 Mbps) | 5.94 MiB/s (49.86 Mbps) |
| 8 | 497.7 | 12.08 | 12.58 | 9.56 MiB/s (80.17 Mbps) | 16.94 MiB/s (142.14 Mbps) |
| 16 | 732.5 | 11.12 | 11.85 | 10.38 MiB/s (87.08 Mbps) | 16.79 MiB/s (140.82 Mbps) |
| 32 | 737.0 | 12.43 | 13.17 | 9.29 MiB/s (77.89 Mbps) | 22.95 MiB/s (192.51 Mbps) |

## Comparison vs Ticket 03 Baseline

| `-c` | Ticket 03 (Static) | Ticket 04 (Work Stealing) | Speedup |
|---:|---:|---:|---:|
| 2 | 58.47s (1.97 MiB/s) | 39.56s (2.92 MiB/s) | 1.48× |
| 8 | 38.35s (3.01 MiB/s) | 12.08s (9.56 MiB/s) | **3.17×** |
| 16 | 17.07s (6.76 MiB/s) | 11.12s (10.38 MiB/s) | 1.54× |
| 32 | 13.42s (8.61 MiB/s) | 12.43s (9.29 MiB/s) | 1.08× |

## Observations

- **Default `-c 8` saturation:** Eliminating the static slow tail raised `-c 8` throughput from 25.26 Mbps to 80.17 Mbps (**3.17×** speedup) and brought it within ~1 second of `-c 16`.
- **Diminishing returns above `-c 16`:** `-c 16` achieved the fastest overall transfer (11.12s / 87.08 Mbps), while `-c 32` hit the highest peak (192.51 Mbps) with slightly more connection/teardown overhead.
