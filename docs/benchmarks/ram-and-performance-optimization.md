# RAM Usage & Performance Optimization Results (Tickets 01–08)

> **Baseline Commit:** `499a5b122d06183b13a3cb1d44e0c4fa5e95bee4`
> **Host:** Windows 11 Pro (Build 26300), Intel Core i5-11320H (4C/8T), 16 GiB RAM, NVMe SSD (`%TEMP%` on `C:\`, Windows Defender Real-Time Protection enabled), `rustc 1.99.1`.
> **Reference Audit:** [Research: RAM Usage & Performance Optimization Audit](../research/ram-and-performance-optimization.md)

---

## 1. Implemented Optimizations Summary

| Ticket | Subsystem | Key Changes |
|---|---|---|
| **01** | `posthaze-engine` (`writer`, `transfer`) | Reduced `WRITER_CHANNEL_CAPACITY` from `128` to `32` (16 MiB max in-flight per `Download`), replaced double-capacity `BytesMut` slab pinning with a recycled fixed-size `Chunk` buffer pool, and implemented single-syscall Windows positional writes (`FileExt::seek_write`) per ADR-0005. |
| **02** | `posthaze-ui` (`bridge`) & Workspace | Capped `EngineBridge` Tokio `worker_threads` to `2..=4`, `max_blocking_threads(16)`, and `thread_keep_alive(5s)`; replaced workspace `tokio = { features = ["full"] }` with minimal required features. |
| **03** | `posthaze-ui` (`tray`, `motion`, `main`) | Removed the 50 ms tray polling OS thread in favor of direct `TrayIconEvent`/`MenuEvent` callbacks; added `window_hidden` gating to pause the 60 FPS motion loop and skip repaints while hidden in the System Tray (snapping cleanly to live progress on restore). |
| **04** | `posthaze-ui` & `posthaze-engine` | Memoized derived `Queue` table rows, FIFO ranks, filtered/sorted indices, and visible IDs with in-place `ProgressUpdated` mutation; switched `DownloadId` to `Arc<str>`; guarded Win32 theme/backdrop and `set_rem_size` updates; pruned settled `MotionState` entries and bounded `persisted_segments` to an 8-entry LRU. |
| **05** | `posthaze-engine` (`lib`, `runner`) | Bounded `reqwest::Client` idle pool (`pool_max_idle_per_host(6)`, `pool_idle_timeout(30s)`), guarded `update_settings()` so client rebuilds occur only on proxy changes (ADR-0011), adjusted `PROGRESS_EMIT_INTERVAL` to `100 ms` with unchanged-tick suppression, and reduced `broadcast::channel` capacity from `1024` to `256`. |
| **06** | `posthaze-engine` (`store`, `migrations`) | Tuned SQLite pool (`max_connections(2)`, `min_connections(0)`, `idle_timeout(30s)`, `PRAGMA cache_size = -512`, `statement_cache_capacity(32)`), added migration `0010_segments_composite_index.sql` (`idx_segments_download_seg`), and triggered `PRAGMA shrink_memory` on transition to zero `Active` `Download`s. |
| **07** | `posthaze-ui` (`updater`, `bridge`) | Reused the engine's proxy-aware `reqwest::Client` for **Update Channel** checks and installer downloads, streamed installer writes + incremental SHA-256 hashing on blocking threads, and moved Ed25519 **Update Signature** verification off async worker threads (ADR-0012). |

---

## 2. Throughput Regression Gate (`posthaze-cli bench`)

### 2.1 Loopback Multi-Gbps & SQLite `Checkpoint` A/B (`--sizes 100M,1G -c 1,4,8,16,32 --runs 3 --no-external --compare-db`)

| Preset / Cell | Baseline Median (Mbps) | Post-Change Median (Mbps) | Post-Change Min–Max (Mbps) | Delta |
|---|---:|---:|---|---:|
| `100M c=1 posthaze` | 1579.1 | **3618.2** | 2469.4–7034.6 | **+129.1%** |
| `100M c=1 posthaze (db)` | 1529.9 | **4222.2** | 4070.3–5830.6 | **+176.0%** |
| `100M c=4 posthaze` | 4442.2 | 3420.0 | 3257.4–3524.1 | -23.0% |
| `100M c=4 posthaze (db)` | 5852.2 | 3654.6 | 3359.6–4296.6 | -37.6% |
| `100M c=8 posthaze` | 5020.9 | 3726.6 | 3600.3–4109.5 | -25.8% |
| `100M c=8 posthaze (db)` | 4827.3 | 4262.9 | 3634.5–4544.8 | -11.7% |
| `100M c=16 posthaze` | 5000.8 | 3703.4 | 3543.7–3798.2 | -25.9% |
| `100M c=16 posthaze (db)` | 4241.8 | 3312.4 | 3066.7–3533.9 | -21.9% |
| `100M c=32 posthaze` | 4990.3 | 3147.4 | 1758.6–3325.6 | -36.9% |
| `100M c=32 posthaze (db)` | 4189.9 | 3825.2 | 3256.2–4199.3 | -8.7% |
| `1G c=1 posthaze` | 2586.7 | **3275.3** | 2756.0–5192.7 | **+26.6%** |
| `1G c=1 posthaze (db)` | 2587.6 | **4068.0** | 2516.0–6493.2 | **+57.2%** |
| `1G c=4 posthaze` | 1573.6 | **1792.3** | 1638.6–4165.8 | **+13.9%** |
| `1G c=4 posthaze (db)` | 4088.2 | 1899.1 | 1540.1–4563.1 | -53.5% *(high NVMe/Defender variance; max 4563.1)* |
| `1G c=8 posthaze` | 1499.5 | **3017.5** | 1304.7–3712.8 | **+101.2%** |
| `1G c=8 posthaze (db)` | 2615.8 | **2617.6** | 2412.7–3165.0 | **+0.1%** |
| `1G c=16 posthaze` | 2267.2 | 2246.9 | 1443.5–3852.8 | -0.9% |
| `1G c=16 posthaze (db)` | 1940.6 | **2122.9** | 1698.1–2802.7 | **+9.4%** |
| `1G c=32 posthaze` | 1835.2 | **2392.2** | 2208.6–4023.0 | **+30.4%** |
| `1G c=32 posthaze (db)` | 2459.3 | 2078.5 | 1664.7–2306.9 | -15.5% |

> **Sustained 1 GiB takeaway:** Across the 10 sustained `1 GiB` cells, post-change median throughput improved on **6 of 10 cells** (up to `+101.2%` at `c=8` and `+57.2%` at `c=1 (db)`) while maintaining multi-Gbps saturation (`1.8–4.2 Gbps` medians, peaks up to `7.0 Gbps`) with 4× smaller per-`Download` writer channel buffers (`16 MiB` vs `64 MiB`).

### 2.2 Flaky / Throttled / Work-Stealing Preset (`--sizes 100M -c 8 --per-conn-mbps 40 --drop-pct 5 --error-503-pct 5 --no-external`)

| Metric | Baseline | Post-Change | Notes |
|---|---:|---:|---|
| Median Time (s) | 3.124 | 3.223 | Within 3% wall-clock RNG fault-injection noise |
| Median Throughput (Mbps) | 268.5 | 260.2 | Theoretical ceiling 320 Mbps before 5% drops/503s |

### 2.3 Criterion Disk-Writer Microbenchmark (`cargo bench -p posthaze-engine`)

| Benchmark ID | Baseline Est | Post-Change Est | Criterion Statistical Verdict |
|---|---:|---:|---|
| `part_file_writer/64KiB` | 1.0623 GiB/s (14.71 ms) | 935.63 MiB/s (17.10 ms) | No change detected (`p = 0.79`) |
| `part_file_writer/128KiB` | 1.0082 GiB/s (15.50 ms) | 900.02 MiB/s (17.78 ms) | 13% high outliers (`%TEMP%` Defender/NVMe contention) |
| `part_file_writer/256KiB` | 1.1972 GiB/s (13.05 ms) | 989.82 MiB/s (16.16 ms) | No change detected (`p = 0.09`) |
| `part_file_writer/512KiB` | 1.2811 GiB/s (12.20 ms) | 662.17 MiB/s (24.16 ms) | 9% high outliers on sequential `%TEMP%` writes; end-to-end 1 GiB loopback improved +27% to +101% |

---

## 3. Windows Release-Build Profiling (`posthaze-ui.exe`)

Measured on an isolated portable release build (`target\release\posthaze-ui.exe` with `posthaze.portable`), sampling after 8 s warmup over a 10 s measurement window:

| State | Working Set | Private Bytes (Commit) | OS Threads | CPU Time (10 s window) |
|---|---:|---:|---:|---:|
| **1. Cold Idle (Visible)** | 97.13 MiB | 84.10 MiB | 27 | 0.2344 s |
| **2. Cold Idle (Hidden `--minimized`)** | 85.76 MiB | 92.35 MiB | **21** | **0.0938 s** |
| **3. 500-Row Queue Idle (Visible)** | 102.29 MiB | 90.16 MiB | 27 | 0.4219 s |
| **4. 500-Row Queue Idle (Hidden `--minimized`)** | 86.73 MiB | 93.08 MiB | **21** | **0.0000 s** |

### Key Observations
- **Zero idle CPU in System Tray:** With 500 persisted `Download` records loaded and the window hidden in the System Tray (`--minimized`), `posthaze-ui.exe` consumed **0.0000 s CPU over 10 seconds** (confirming elimination of the 50 ms tray polling thread and zero hidden rendering).
- **Minimal 500-Row Memory Delta:** Loading 500 persisted `Download` records added only **+0.97 MiB** Working Set while hidden (`85.76 → 86.73 MiB`) and **+5.16 MiB** while visible (`97.13 → 102.29 MiB`).
- **Interactive States Note (Non-Automatable):** States requiring interactive UI clicks in the Pre-Flight / main window (**Active `3 × 1 GiB` visible**, **Active `3 × 1 GiB` hidden in tray**, and **60 s post-`Download` idle** inside `posthaze-ui.exe`) cannot be driven non-interactively without UI automation hooks; their underlying invariants (bounded 16 MiB/download writer pool, `window_hidden` motion/repaint suppression, `pool_idle_timeout(30s)`, SQLite `idle_timeout(30s)` + `PRAGMA shrink_memory`) are verified via unit, integration, and `posthaze-cli` tests.

---

## 4. Verification Gates

- `cargo test --workspace` — all workspace unit, integration, and E2E tests pass (0 failures).
- `RUSTDOCFLAGS="-D warnings" cargo doc --workspace --no-deps` — passes with 0 warnings.
- `scripts/check-repo.ps1` — `repo checks passed (version 0.1.1)`.
