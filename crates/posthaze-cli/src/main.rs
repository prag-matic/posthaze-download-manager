mod bench_runner;
mod bench_server;

use std::ffi::OsString;
use std::io::Write;
use std::path::{Path, PathBuf};
use std::process::ExitCode;
use std::time::{Duration, Instant};

use clap::{Parser, Subcommand};
use posthaze_engine::{
    DownloadEngine, DownloadOutcome, DownloadRequest, DownloadState, ProgressSnapshot,
};
use serde::Serialize;
use tokio::sync::watch;

use crate::bench_runner::{BenchOptions, run_bench};
use crate::bench_server::{BenchServerConfig, parse_byte_size, start_bench_server};

#[derive(Parser)]
#[command(name = "posthaze", about = "Posthaze Download Manager CLI")]
struct Cli {
    #[command(subcommand)]
    command: Commands,
}

#[derive(Subcommand)]
enum Commands {
    /// Download a file over HTTP/HTTPS.
    Download {
        /// URL to download.
        url: String,
        /// Optional output file path.
        #[arg(short, long)]
        output: Option<PathBuf>,
        /// Maximum concurrent workers (used in multi-segment mode).
        #[arg(short = 'c', long = "connections", default_value_t = 8)]
        connections: usize,
        /// Save detailed JSON download statistics and throughput samples (`<file>.stats.json`).
        #[arg(long)]
        debug: bool,
        /// Custom path for the debug stats JSON file (implies --debug).
        #[arg(long, value_name = "PATH")]
        stats_out: Option<PathBuf>,
    },
    /// Run a controlled local HTTP/1.1 range server for repeatable benchmarking.
    Serve {
        /// Virtual file size (e.g. 100M, 1G, 5G).
        #[arg(long, default_value = "100M")]
        size: String,
        /// TCP port to bind (0 for ephemeral).
        #[arg(long, default_value_t = 0)]
        port: u16,
        /// Per-connection bandwidth cap in Mbps.
        #[arg(long)]
        per_conn_mbps: Option<f64>,
        /// Artificial first-byte latency in milliseconds.
        #[arg(long, default_value_t = 0)]
        latency_ms: u64,
        /// Mid-stream connection drop probability (0..100).
        #[arg(long, default_value_t = 0.0)]
        drop_pct: f64,
        /// HTTP 503 injection probability on GET (0..100).
        #[arg(long, default_value_t = 0.0)]
        error_503_pct: f64,
        /// Return 405 Method Not Allowed on HEAD requests.
        #[arg(long)]
        block_head: bool,
        /// Disable HTTP Range support.
        #[arg(long)]
        no_range: bool,
    },
    /// Run an automated benchmark matrix against an in-process local server.
    Bench {
        /// Comma-separated virtual file sizes (e.g. 100M or 100M,1G).
        #[arg(long, value_delimiter = ',', default_value = "100M")]
        sizes: Vec<String>,
        /// Comma-separated worker counts to sweep.
        #[arg(short = 'c', long, value_delimiter = ',', default_value = "1,2,4,8,16,32")]
        connections: Vec<usize>,
        /// Number of iterations per configuration.
        #[arg(long, default_value_t = 3)]
        runs: usize,
        /// Per-connection bandwidth cap in Mbps.
        #[arg(long)]
        per_conn_mbps: Option<f64>,
        /// Artificial first-byte latency in milliseconds.
        #[arg(long, default_value_t = 0)]
        latency_ms: u64,
        /// Mid-stream connection drop probability (0..100).
        #[arg(long, default_value_t = 0.0)]
        drop_pct: f64,
        /// HTTP 503 injection probability on GET (0..100).
        #[arg(long, default_value_t = 0.0)]
        error_503_pct: f64,
        /// Skip automatic comparison with curl / aria2c on PATH.
        #[arg(long)]
        no_external: bool,
        /// Optional path to save the Markdown or JSON (.json) benchmark report.
        #[arg(long, value_name = "PATH")]
        report: Option<PathBuf>,
    },
}

#[derive(Default)]
struct ProgressStats {
    peak_bps: f64,
    last_snapshot: Option<ProgressSnapshot>,
    samples: Vec<ThroughputSample>,
}

#[derive(Debug, Clone, Serialize)]
struct ThroughputSample {
    elapsed_ms: u64,
    bytes_downloaded: u64,
    inst_bytes_per_sec: f64,
    active_workers: usize,
}

#[derive(Debug, Serialize)]
struct DebugStatsReport {
    status: &'static str,
    error: Option<String>,
    final_path: Option<PathBuf>,
    final_url: String,
    bytes_downloaded: u64,
    content_length: Option<u64>,
    supports_ranges: bool,
    workers_used: usize,
    splits_performed: usize,
    retries_performed: usize,
    probe_ms: Option<f64>,
    transfer_ms: Option<f64>,
    total_ms: f64,
    avg_bytes_per_sec: f64,
    avg_mib_per_sec: f64,
    avg_mbps: f64,
    peak_bytes_per_sec: f64,
    peak_mib_per_sec: f64,
    peak_mbps: f64,
    etag: Option<String>,
    last_modified: Option<String>,
    samples: Vec<ThroughputSample>,
}

#[tokio::main]
async fn main() -> ExitCode {
    let cli = Cli::parse();

    match cli.command {
        Commands::Download {
            url,
            output,
            connections,
            debug,
            stats_out,
        } => run_download(url, output, connections, debug || stats_out.is_some(), stats_out).await,
        Commands::Serve {
            size,
            port,
            per_conn_mbps,
            latency_ms,
            drop_pct,
            error_503_pct,
            block_head,
            no_range,
        } => {
            let size_bytes = match parse_byte_size(&size) {
                Ok(b) => b,
                Err(err) => {
                    eprintln!("Invalid --size: {err}");
                    return ExitCode::FAILURE;
                }
            };
            let server = match start_bench_server(BenchServerConfig {
                size: size_bytes,
                port,
                per_conn_mbps,
                latency_ms,
                drop_pct,
                error_503_pct,
                block_head,
                no_range,
            })
            .await
            {
                Ok(s) => s,
                Err(err) => {
                    eprintln!("Failed to start server: {err}");
                    return ExitCode::FAILURE;
                }
            };
            println!("Serving {size_bytes} bytes at {}", server.url());
            println!("Press Ctrl+C to stop.");
            let _ = tokio::signal::ctrl_c().await;
            server.handle.abort();
            ExitCode::SUCCESS
        }
        Commands::Bench {
            sizes,
            connections,
            runs,
            per_conn_mbps,
            latency_ms,
            drop_pct,
            error_503_pct,
            no_external,
            report,
        } => {
            run_bench(BenchOptions {
                sizes,
                connections,
                runs: runs.max(1),
                per_conn_mbps,
                latency_ms,
                drop_pct,
                error_503_pct,
                include_external: !no_external,
                report_path: report,
            })
            .await
        }
    }
}

async fn run_download(
    url: String,
    output: Option<PathBuf>,
    connections: usize,
    record_samples: bool,
    stats_out: Option<PathBuf>,
) -> ExitCode {
    let output_dir = std::env::current_dir().unwrap_or_else(|_| PathBuf::from("."));
    let mut request = DownloadRequest::new(url.clone(), output_dir.clone());
    request.output_path = output.clone();
    request.max_workers = connections.max(1);

    let (tx, mut rx) = watch::channel(ProgressSnapshot {
        state: DownloadState::Queued,
        bytes_downloaded: 0,
        total_bytes: None,
        active_workers: 0,
    });

    let start = Instant::now();
    let progress_task = tokio::spawn(async move {
        let mut ticker = tokio::time::interval(Duration::from_millis(200));
        let mut last_tick = Instant::now();
        let mut last_bytes = 0u64;
        let mut peak_bps = 0.0_f64;
        let mut samples = Vec::new();
        let mut last_snapshot = None;

        loop {
            tokio::select! {
                _ = ticker.tick() => {
                    let snap = rx.borrow().clone();
                    let now = Instant::now();
                    let dt = now.duration_since(last_tick).as_secs_f64();
                    let inst_bps = if dt > 0.0 {
                        snap.bytes_downloaded.saturating_sub(last_bytes) as f64 / dt
                    } else {
                        0.0
                    };
                    if inst_bps > peak_bps {
                        peak_bps = inst_bps;
                    }
                    last_tick = now;
                    last_bytes = snap.bytes_downloaded;

                    let elapsed = start.elapsed();
                    if record_samples {
                        samples.push(ThroughputSample {
                            elapsed_ms: elapsed.as_millis() as u64,
                            bytes_downloaded: snap.bytes_downloaded,
                            inst_bytes_per_sec: inst_bps,
                            active_workers: snap.active_workers,
                        });
                    }

                    print_progress(&snap, elapsed, inst_bps);
                    last_snapshot = Some(snap.clone());
                    if is_terminal(snap.state) {
                        break;
                    }
                }
                changed = rx.changed() => {
                    if changed.is_err() {
                        break;
                    }
                    let snap = rx.borrow().clone();
                    if is_terminal(snap.state) {
                        let elapsed = start.elapsed();
                        if record_samples {
                            samples.push(ThroughputSample {
                                elapsed_ms: elapsed.as_millis() as u64,
                                bytes_downloaded: snap.bytes_downloaded,
                                inst_bytes_per_sec: 0.0,
                                active_workers: snap.active_workers,
                            });
                        }
                        print_progress(&snap, elapsed, 0.0);
                        last_snapshot = Some(snap);
                        break;
                    }
                }
            }
        }
        eprintln!();
        ProgressStats {
            peak_bps,
            last_snapshot,
            samples,
        }
    });

    let engine = DownloadEngine::new();
    let result = engine.download(request, Some(tx)).await;
    let total_elapsed = start.elapsed();
    let stats = progress_task.await.unwrap_or_default();

    match result {
        Ok(outcome) => {
            print_summary(&outcome, stats.peak_bps);
            if record_samples {
                let stats_path =
                    stats_out.unwrap_or_else(|| default_stats_path(&outcome.final_path));
                if let Err(err) = write_success_stats(&stats_path, &outcome, &stats) {
                    eprintln!("Failed to save debug stats to {}: {err}", stats_path.display());
                    return ExitCode::FAILURE;
                }
                println!("  Stats saved:    {}", stats_path.display());
            }
            ExitCode::SUCCESS
        }
        Err(err) => {
            eprintln!("Download failed: {err}");
            if record_samples {
                let stats_path = stats_out.unwrap_or_else(|| match &output {
                    Some(path) => default_stats_path(path),
                    None => output_dir.join("posthaze-failed.stats.json"),
                });
                if let Err(write_err) =
                    write_failure_stats(&stats_path, &url, &err.to_string(), total_elapsed, &stats)
                {
                    eprintln!(
                        "Failed to save debug stats to {}: {write_err}",
                        stats_path.display()
                    );
                } else {
                    eprintln!("Debug stats saved: {}", stats_path.display());
                }
            }
            ExitCode::FAILURE
        }
    }
}

fn default_stats_path(final_path: &Path) -> PathBuf {
    let mut os: OsString = final_path.as_os_str().to_os_string();
    os.push(".stats.json");
    PathBuf::from(os)
}

fn write_success_stats(
    path: &Path,
    outcome: &DownloadOutcome,
    stats: &ProgressStats,
) -> Result<(), std::io::Error> {
    let avg_bps = outcome.average_bytes_per_sec();
    let peak_bps = stats.peak_bps.max(avg_bps);
    let report = DebugStatsReport {
        status: "completed",
        error: None,
        final_path: Some(outcome.final_path.clone()),
        final_url: outcome.final_url.clone(),
        bytes_downloaded: outcome.bytes_downloaded,
        content_length: outcome.content_length,
        supports_ranges: outcome.supports_ranges,
        workers_used: outcome.workers_used,
        splits_performed: outcome.splits_performed,
        retries_performed: outcome.retries_performed,
        probe_ms: Some(outcome.probe_duration.as_secs_f64() * 1000.0),
        transfer_ms: Some(outcome.transfer_duration.as_secs_f64() * 1000.0),
        total_ms: outcome.total_duration.as_secs_f64() * 1000.0,
        avg_bytes_per_sec: avg_bps,
        avg_mib_per_sec: avg_bps / (1024.0 * 1024.0),
        avg_mbps: (avg_bps * 8.0) / 1_000_000.0,
        peak_bytes_per_sec: peak_bps,
        peak_mib_per_sec: peak_bps / (1024.0 * 1024.0),
        peak_mbps: (peak_bps * 8.0) / 1_000_000.0,
        etag: outcome.etag.clone(),
        last_modified: outcome.last_modified.clone(),
        samples: stats.samples.clone(),
    };

    let json = serde_json::to_string_pretty(&report).map_err(std::io::Error::other)?;
    std::fs::write(path, json)
}

fn write_failure_stats(
    path: &Path,
    url: &str,
    error: &str,
    total_elapsed: Duration,
    stats: &ProgressStats,
) -> Result<(), std::io::Error> {
    let bytes = stats
        .last_snapshot
        .as_ref()
        .map_or(0, |snap| snap.bytes_downloaded);
    let content_length = stats
        .last_snapshot
        .as_ref()
        .and_then(|snap| snap.total_bytes);
    let secs = total_elapsed.as_secs_f64();
    let avg_bps = if secs > 0.0 { bytes as f64 / secs } else { 0.0 };
    let peak_bps = stats.peak_bps.max(avg_bps);

    let report = DebugStatsReport {
        status: "failed",
        error: Some(error.to_string()),
        final_path: None,
        final_url: url.to_string(),
        bytes_downloaded: bytes,
        content_length,
        supports_ranges: false,
        workers_used: 0,
        splits_performed: 0,
        retries_performed: 0,
        probe_ms: None,
        transfer_ms: None,
        total_ms: secs * 1000.0,
        avg_bytes_per_sec: avg_bps,
        avg_mib_per_sec: avg_bps / (1024.0 * 1024.0),
        avg_mbps: (avg_bps * 8.0) / 1_000_000.0,
        peak_bytes_per_sec: peak_bps,
        peak_mib_per_sec: peak_bps / (1024.0 * 1024.0),
        peak_mbps: (peak_bps * 8.0) / 1_000_000.0,
        etag: None,
        last_modified: None,
        samples: stats.samples.clone(),
    };

    let json = serde_json::to_string_pretty(&report).map_err(std::io::Error::other)?;
    std::fs::write(path, json)
}

fn is_terminal(state: DownloadState) -> bool {
    matches!(
        state,
        DownloadState::Completed | DownloadState::Failed | DownloadState::Cancelled
    )
}

fn print_progress(snap: &ProgressSnapshot, elapsed: Duration, inst_bps: f64) {
    let avg_bps = snap.bytes_downloaded as f64 / elapsed.as_secs_f64().max(0.001);
    let inst_mib = inst_bps / (1024.0 * 1024.0);
    let avg_mib = avg_bps / (1024.0 * 1024.0);

    match snap.total_bytes {
        Some(total) if total > 0 => {
            let pct = (snap.bytes_downloaded as f64 / total as f64) * 100.0;
            let eta = if avg_bps > 0.0 && snap.bytes_downloaded < total {
                format!("{:.1}s", (total - snap.bytes_downloaded) as f64 / avg_bps)
            } else {
                "0.0s".to_string()
            };
            eprint!(
                "\r[{:?}] {}/{} ({:.1}%) | inst: {:.2} MiB/s | avg: {:.2} MiB/s | eta: {} | workers: {}   ",
                snap.state,
                format_bytes(snap.bytes_downloaded),
                format_bytes(total),
                pct,
                inst_mib,
                avg_mib,
                eta,
                snap.active_workers
            );
        }
        _ => {
            eprint!(
                "\r[{:?}] {} | inst: {:.2} MiB/s | avg: {:.2} MiB/s | workers: {}   ",
                snap.state,
                format_bytes(snap.bytes_downloaded),
                inst_mib,
                avg_mib,
                snap.active_workers
            );
        }
    }
    let _ = std::io::stderr().flush();
}

fn print_summary(outcome: &DownloadOutcome, peak_bps: f64) {
    let avg_bps = outcome.average_bytes_per_sec();
    let avg_mib = avg_bps / (1024.0 * 1024.0);
    let avg_mbps = (avg_bps * 8.0) / 1_000_000.0;
    let peak_mib = peak_bps.max(avg_bps) / (1024.0 * 1024.0);
    let peak_mbps = (peak_bps.max(avg_bps) * 8.0) / 1_000_000.0;

    println!("Download Summary");
    println!("  File:           {}", outcome.final_path.display());
    println!("  Final URL:      {}", outcome.final_url);
    println!(
        "  Downloaded:     {} ({} bytes)",
        format_bytes(outcome.bytes_downloaded),
        outcome.bytes_downloaded
    );
    println!(
        "  Total time:     {:.3}s (probe: {:.3}s, transfer: {:.3}s)",
        outcome.total_duration.as_secs_f64(),
        outcome.probe_duration.as_secs_f64(),
        outcome.transfer_duration.as_secs_f64()
    );
    println!("  Avg speed:      {avg_mib:.2} MiB/s ({avg_mbps:.2} Mbps)");
    println!("  Peak speed:     {peak_mib:.2} MiB/s ({peak_mbps:.2} Mbps)");
    println!(
        "  Workers used:   {} (splits: {}, retries: {})",
        outcome.workers_used, outcome.splits_performed, outcome.retries_performed
    );
    println!("  Range support:  {}", outcome.supports_ranges);
    if let Some(etag) = &outcome.etag {
        println!("  ETag:           {etag}");
    }
    if let Some(lm) = &outcome.last_modified {
        println!("  Last-Modified:  {lm}");
    }
}

fn format_bytes(bytes: u64) -> String {
    const KIB: f64 = 1024.0;
    const MIB: f64 = KIB * 1024.0;
    const GIB: f64 = MIB * 1024.0;

    let b = bytes as f64;
    if b >= GIB {
        format!("{:.2} GiB", b / GIB)
    } else if b >= MIB {
        format!("{:.2} MiB", b / MIB)
    } else if b >= KIB {
        format!("{:.1} KiB", b / KIB)
    } else {
        format!("{bytes} B")
    }
}
