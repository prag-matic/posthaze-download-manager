# Benchmarking & Stress-Testing Suite

Posthaze includes a built-in controlled HTTP/1.1 range server, an automated matrix runner, Criterion disk-writer microbenchmarks, and chaos/edge-case integration tests.

## 1. Automated Matrix Runner (`posthaze-cli bench`)

Spawns an in-process controlled server, sweeps connection counts across multiple runs, and automatically compares against `curl` / `aria2c` when available on `PATH`:

```powershell
# Fast 100 MiB sweep across 1,2,4,8,16,32 workers (3 runs each)
cargo run --release -p posthaze-cli -- bench --sizes 100M -c 1,2,4,8,16,32 --runs 3

# Both 100 MiB and 1 GiB with per-connection throttling & report export
cargo run --release -p posthaze-cli -- bench --sizes 100M,1G -c 1,4,8,16 --per-conn-mbps 40 --report .\bench-report.md

# Chaos/retry stress benchmark (5% mid-stream drops + 5% HTTP 503s)
cargo run --release -p posthaze-cli -- bench --sizes 100M -c 8 --drop-pct 5 --error-503-pct 5
```

## 2. Standalone Controlled Server (`posthaze-cli serve`)

Run a standalone local HTTP/1.1 range server for external tool comparisons (`IDM`, `FDM`, `aria2c`, `curl`, `hyperfine`):

```powershell
cargo run --release -p posthaze-cli -- serve --size 1G --port 8080 --per-conn-mbps 50 --latency-ms 20
```

Supports `--block-head` (`405` on `HEAD`), `--no-range`, `--drop-pct`, and `--error-503-pct` while using `< 1 MiB` RAM even for multi-gigabyte virtual payloads.

## 3. Criterion Microbenchmarks (`writer_bench`)

Measure raw `PartFileWriter` disk throughput across `Chunk` buffer sizes (`64 KiB`–`512 KiB`):

```powershell
cargo bench -p posthaze-engine
```
