# Benchmarking & Stress-Testing Guide

Posthaze includes a built-in controlled HTTP/1.1 range server, an automated matrix runner, Criterion disk-writer microbenchmarks, and chaos/edge-case integration tests.

## 1. Automated Matrix Runner (`posthaze-cli bench`)

Spawns an in-process controlled server, sweeps connection counts across multiple runs, and automatically compares against `curl` and `aria2c` when available on `PATH`.

```powershell
# Fast 100 MiB sweep across 1,2,4,8,16,32 workers (3 runs each)
cargo run --release -p posthaze-cli -- bench --sizes 100M -c 1,2,4,8,16,32 --runs 3

# Both 100 MiB and 1 GiB with per-connection throttling & Markdown report export
cargo run --release -p posthaze-cli -- bench --sizes 100M,1G -c 1,4,8,16 --per-conn-mbps 40 --report .\bench-report.md

# Chaos/retry stress benchmark (5% mid-stream drops + 5% HTTP 503s)
cargo run --release -p posthaze-cli -- bench --sizes 100M -c 8 --drop-pct 5 --error-503-pct 5

# Multi-scenario + A/B sync comparison against aria2c only
cargo run --release -p posthaze-cli -- bench --sizes 64M -c 1,8 --tools posthaze,aria2c --compare-sync --scenarios clean,head-blocked,flaky

# Benchmark an external URL with custom aria2c args
cargo run --release -p posthaze-cli -- bench --url "https://example.com/file.bin" -c 1,8 --tools posthaze,aria2c --aria2-args "--min-split-size=1M -k 1M"
```

### `bench` Flags

| Flag | Default | Description |
|---|---|---|
| `--url <URL>` | none | Benchmark an external URL instead of spawning the local server |
| `--sizes <LIST>` | `100M` | Comma-separated virtual file sizes (e.g. `100M,1G`) |
| `-c, --connections <LIST>` | `1,2,4,8,16,32` | Comma-separated `Worker` counts to sweep |
| `--runs <N>` | `3` | Iterations per `(size, connections)` cell (reports median, min, max) |
| `--tools <LIST>` | `auto` | Comma-separated tools (`auto`, `posthaze`, `posthaze-sync`, `aria2c`, `curl`) |
| `--aria2-args <STR>` | `--min-split-size=2M` | Extra CLI flags passed to `aria2c` |
| `--curl-args <STR>` | `""` | Extra CLI flags passed to `curl` |
| `--scenarios <LIST>` | custom | Comma-separated server presets (`clean`, `throttled`, `head-blocked`, `flaky`) |
| `--per-conn-mbps <FLOAT>` | unlimited | Per-connection bandwidth cap in Mbps |
| `--latency-ms <U64>` | `0` | Artificial first-byte latency in ms |
| `--drop-pct <FLOAT>` | `0.0` | Mid-stream connection drop probability (`0..100`) |
| `--error-503-pct <FLOAT>` | `0.0` | HTTP `503` injection probability on `GET` (`0..100`) |
| `--block-head` | `false` | Return `405 Method Not Allowed` on `HEAD` in custom scenario |
| `--sync-on-complete` | `false` | Enable `File::sync_all()` durability on `posthaze` runs |
| `--compare-sync` | `false` | Include a `posthaze (sync)` A/B row alongside `posthaze` |
| `--alternate <BOOL>` | `true` | Alternate tool execution order across odd/even runs |
| `--no-external` | `false` | Shorthand for `--tools posthaze` |
| `--report <PATH>` | none | Save report as Markdown (or JSON when ending in `.json`) |

### Local Smoke Baseline (`32M`, `40 Mbps`/conn, `runs=2`)

| Tool | Size (MiB) | Connections | Median Time (s) | Median Mbps | Min–Max Mbps |
|---|---:|---:|---:|---:|---:|
| `posthaze` | 32.0 | 1 | 8.026 | 33.7 | 33.4–33.7 |
| `curl` | 32.0 | 1 | 8.184 | 32.9 | 32.8–32.9 |
| `posthaze` | 32.0 | 4 | 2.032 | 133.4 | 132.1–133.4 |
| `posthaze` | 32.0 | 8 | 1.035 | 266.4 | 259.4–266.4 |

## 2. Standalone Controlled Server (`posthaze-cli serve`)

Run a standalone local HTTP/1.1 range server for external tool comparisons (`IDM`, `FDM`, `aria2c`, `curl`, `hyperfine`). Generates deterministic bytes from a 64 KiB pattern buffer (`< 1 MiB` RAM even for `5G` payloads):

```powershell
cargo run --release -p posthaze-cli -- serve --size 1G --port 8080 --per-conn-mbps 50 --latency-ms 20
```

| Flag | Default | Description |
|---|---|---|
| `--size <SIZE>` | `100M` | Virtual payload size (`K`, `M`, `G`, or raw bytes) |
| `--port <PORT>` | `0` | TCP port to bind (`0` = ephemeral) |
| `--per-conn-mbps <FLOAT>` | unlimited | Per-connection bandwidth cap in Mbps |
| `--latency-ms <U64>` | `0` | First-byte latency in ms |
| `--drop-pct <FLOAT>` | `0.0` | Mid-stream TCP drop probability (`0..100`) |
| `--error-503-pct <FLOAT>` | `0.0` | HTTP `503` injection probability (`0..100`) |
| `--block-head` | `false` | Return `405 Method Not Allowed` on `HEAD` |
| `--no-range` | `false` | Disable `Accept-Ranges` and ignore `Range` headers |

## 3. Engine Telemetry & `--debug` Stats

[`DownloadOutcome`](../crates/posthaze-engine/src/lib.rs) and `posthaze-cli download --debug` now record:

- `splits_performed`: Number of dynamic `Segment` work-stealing bisections.
- `retries_performed`: Number of transient `Worker` retries.

## 4. Criterion Microbenchmarks & Edge-Case Tests

- **Disk-writer microbenchmark ([`writer_bench.rs`](../crates/posthaze-engine/benches/writer_bench.rs)):** Measures `PartFileWriter` throughput across `64 KiB`, `128 KiB`, `256 KiB`, and `512 KiB` `Chunk` sizes:
  ```powershell
  cargo bench -p posthaze-engine
  ```
- **Edge-case & chaos tests ([`edge_cases.rs`](../crates/posthaze-engine/tests/edge_cases.rs)):** Verifies 0-byte transfers, Windows reserved device name sanitization (`CON.txt` → `_CON.txt`), immediate failure on non-transient `404`, and 16 MiB 8-`Worker` chaos recovery (`503` + mid-stream drop + work stealing).
