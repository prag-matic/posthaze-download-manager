mod bench_runner;
mod bench_server;
mod native_host;

use std::ffi::OsString;
use std::io::Write;
use std::path::{Path, PathBuf};
use std::process::ExitCode;
use std::time::{Duration, Instant};

use clap::{Parser, Subcommand};
use posthaze_engine::{
    DownloadEngine, DownloadHandle, DownloadId, DownloadOutcome, DownloadRequest, DownloadState,
    EngineError, ProgressSnapshot, default_db_path,
};
use serde::Serialize;

use crate::bench_runner::{BenchOptions, run_bench};
use crate::bench_server::{BenchServerConfig, parse_byte_size, start_bench_server};

#[derive(Parser)]
#[command(name = "posthaze", about = "Posthaze Download Manager CLI")]
struct Cli {
    /// Path to the SQLite persistence database (defaults to %LOCALAPPDATA%\posthaze\state.db).
    #[arg(long, global = true, value_name = "PATH")]
    db: Option<PathBuf>,
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
        /// Call `File::sync_all()` before renaming `.part` to the final file.
        #[arg(long)]
        sync_on_complete: bool,
        /// Save detailed JSON download statistics and throughput samples (`<file>.stats.json`).
        #[arg(long)]
        debug: bool,
        /// Custom path for the debug stats JSON file (implies --debug).
        #[arg(long, value_name = "PATH")]
        stats_out: Option<PathBuf>,
    },
    /// List persisted downloads from the SQLite database.
    List,
    /// Resume a Paused or resumable Failed download by DownloadId.
    Resume {
        /// Persistent DownloadId (UUID) to resume.
        id: String,
        /// Save detailed JSON download statistics and throughput samples (`<file>.stats.json`).
        #[arg(long)]
        debug: bool,
        /// Custom path for the debug stats JSON file (implies --debug).
        #[arg(long, value_name = "PATH")]
        stats_out: Option<PathBuf>,
    },
    /// Cancel an active or persisted download by DownloadId and delete its `.part` file.
    Cancel {
        /// Persistent DownloadId (UUID) to cancel.
        id: String,
    },
    /// Delete a download record and `.part` file, optionally removing the completed output file.
    Delete {
        /// Persistent DownloadId (UUID) to delete.
        id: String,
        /// Also delete the final output file from disk if present.
        #[arg(long)]
        delete_file: bool,
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
    /// Run a customizable benchmark matrix against an in-process server or external URL.
    Bench {
        /// Optional external URL to benchmark instead of spawning the local server.
        #[arg(long)]
        url: Option<String>,
        /// Comma-separated virtual file sizes (e.g. 100M or 100M,1G).
        #[arg(long, value_delimiter = ',', default_value = "100M")]
        sizes: Vec<String>,
        /// Comma-separated worker counts to sweep.
        #[arg(short = 'c', long, value_delimiter = ',', default_value = "1,2,4,8,16,32")]
        connections: Vec<usize>,
        /// Number of iterations per configuration.
        #[arg(long, default_value_t = 3)]
        runs: usize,
        /// Comma-separated tools to compare (`auto`, `posthaze`, `posthaze-sync`, `posthaze-db`, `aria2c`, `curl`).
        #[arg(long, value_delimiter = ',', default_value = "auto")]
        tools: Vec<String>,
        /// Extra CLI flags passed to `aria2c`.
        #[arg(long, default_value = "--min-split-size=2M", allow_hyphen_values = true)]
        aria2_args: String,
        /// Extra CLI flags passed to `curl`.
        #[arg(long, default_value = "", allow_hyphen_values = true)]
        curl_args: String,
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
        /// Return 405 Method Not Allowed on HEAD requests in custom scenario.
        #[arg(long)]
        block_head: bool,
        /// Enable `sync_all()` durability on `posthaze` runs.
        #[arg(long)]
        sync_on_complete: bool,
        /// Include an extra `posthaze (sync)` A/B row alongside `posthaze`.
        #[arg(long)]
        compare_sync: bool,
        /// Include an extra `posthaze (db)` A/B row with SQLite persistence and periodic `Checkpoint`s.
        #[arg(long)]
        compare_db: bool,
        /// Alternate tool execution order on odd/even runs (enabled by default).
        #[arg(long, default_value_t = true, action = clap::ArgAction::Set)]
        alternate: bool,
        /// Comma-separated server scenarios (`clean`, `throttled`, `head-blocked`, `flaky`).
        #[arg(long, value_delimiter = ',')]
        scenarios: Vec<String>,
        /// Skip external tools (shorthand for `--tools posthaze`).
        #[arg(long)]
        no_external: bool,
        /// Optional path to save the Markdown or JSON (.json) benchmark report.
        #[arg(long, value_name = "PATH")]
        report: Option<PathBuf>,
    },
    /// Chrome/Edge Native Messaging host entrypoint (reads length-prefixed JSON on stdin).
    #[command(name = "native-host", hide = true)]
    NativeHost {
        /// Positional origin / window flags passed by the browser.
        #[arg(trailing_var_arg = true, allow_hyphen_values = true)]
        _browser_args: Vec<String>,
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

    let db_path = cli.db.unwrap_or_else(default_db_path);

    match cli.command {
        Commands::Download {
            url,
            output,
            connections,
            sync_on_complete,
            debug,
            stats_out,
        } => {
            run_download(
                db_path,
                url,
                output,
                connections,
                sync_on_complete,
                debug || stats_out.is_some(),
                stats_out,
            )
            .await
        }
        Commands::List => run_list(db_path).await,
        Commands::Resume {
            id,
            debug,
            stats_out,
        } => run_resume(db_path, id, debug || stats_out.is_some(), stats_out).await,
        Commands::Cancel { id } => run_cancel(db_path, id).await,
        Commands::Delete { id, delete_file } => run_delete(db_path, id, delete_file).await,
        Commands::NativeHost { .. } => native_host::run_native_host(db_path).await,
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
            url,
            sizes,
            connections,
            runs,
            tools,
            aria2_args,
            curl_args,
            per_conn_mbps,
            latency_ms,
            drop_pct,
            error_503_pct,
            block_head,
            sync_on_complete,
            compare_sync,
            compare_db,
            alternate,
            scenarios,
            no_external,
            report,
        } => {
            let effective_tools = if no_external {
                vec!["posthaze".to_string()]
            } else {
                tools
            };
            run_bench(BenchOptions {
                url,
                sizes,
                connections,
                runs: runs.max(1),
                tools: effective_tools,
                aria2_args,
                curl_args,
                per_conn_mbps,
                latency_ms,
                drop_pct,
                error_503_pct,
                block_head,
                sync_on_complete,
                compare_sync,
                compare_db,
                alternate,
                scenarios,
                report_path: report,
            })
            .await
        }
    }
}

async fn run_list(db_path: PathBuf) -> ExitCode {
    let engine = match DownloadEngine::open(&db_path).await {
        Ok(engine) => engine,
        Err(err) => {
            eprintln!("Failed to open database {}: {err}", db_path.display());
            return ExitCode::FAILURE;
        }
    };

    let records = match engine.list_downloads().await {
        Ok(records) => records,
        Err(err) => {
            eprintln!("Failed to list downloads: {err}");
            return ExitCode::FAILURE;
        }
    };

    if records.is_empty() {
        println!("No downloads recorded in {}", db_path.display());
        return ExitCode::SUCCESS;
    }

    println!(
        "{:<36}  {:<10}  {:<21}  TARGET / URL",
        "ID", "STATE", "PROGRESS"
    );
    for rec in records {
        let progress = match rec.content_length {
            Some(total) if total > 0 => format!(
                "{}/{}",
                format_bytes(rec.bytes_downloaded),
                format_bytes(total)
            ),
            _ => format_bytes(rec.bytes_downloaded),
        };
        let target = rec
            .output_path
            .as_ref()
            .map(|p| p.display().to_string())
            .unwrap_or_else(|| rec.url.clone());
        println!(
            "{:<36}  {:<10}  {:<21}  {}",
            rec.id,
            rec.state.as_str(),
            progress,
            target
        );
    }
    ExitCode::SUCCESS
}

async fn open_engine_or_exit(db_path: &Path) -> Option<DownloadEngine> {
    match DownloadEngine::open(db_path).await {
        Ok(engine) => Some(engine),
        Err(err) => {
            eprintln!("Failed to open database {}: {err}", db_path.display());
            None
        }
    }
}

async fn run_cancel(db_path: PathBuf, id: String) -> ExitCode {
    let Some(engine) = open_engine_or_exit(&db_path).await else {
        return ExitCode::FAILURE;
    };

    let id = DownloadId::from(id);
    match engine.cancel(&id).await {
        Ok(()) => {
            println!("Cancelled download {id}");
            ExitCode::SUCCESS
        }
        Err(err) => {
            eprintln!("Failed to cancel download {id}: {err}");
            ExitCode::FAILURE
        }
    }
}

async fn run_delete(db_path: PathBuf, id: String, delete_file: bool) -> ExitCode {
    let Some(engine) = open_engine_or_exit(&db_path).await else {
        return ExitCode::FAILURE;
    };

    let id = DownloadId::from(id);
    match engine.delete(&id, delete_file).await {
        Ok(()) => {
            println!("Deleted download {id}");
            ExitCode::SUCCESS
        }
        Err(err) => {
            eprintln!("Failed to delete download {id}: {err}");
            ExitCode::FAILURE
        }
    }
}

async fn run_resume(
    db_path: PathBuf,
    id: String,
    record_samples: bool,
    stats_out: Option<PathBuf>,
) -> ExitCode {
    let engine = match DownloadEngine::open(&db_path).await {
        Ok(engine) => engine,
        Err(err) => {
            eprintln!("Failed to open database {}: {err}", db_path.display());
            return ExitCode::FAILURE;
        }
    };

    let download_id = DownloadId::from(id);
    let handle = match engine.resume(&download_id).await {
        Ok(handle) => handle,
        Err(err) => {
            eprintln!("Failed to resume download {download_id}: {err}");
            return ExitCode::FAILURE;
        }
    };

    let output_dir = std::env::current_dir().unwrap_or_else(|_| PathBuf::from("."));
    drive_handle(handle, None, output_dir, record_samples, stats_out).await
}

async fn run_download(
    db_path: PathBuf,
    url: String,
    output: Option<PathBuf>,
    connections: usize,
    sync_on_complete: bool,
    record_samples: bool,
    stats_out: Option<PathBuf>,
) -> ExitCode {
    let engine = match DownloadEngine::open(&db_path).await {
        Ok(engine) => engine,
        Err(err) => {
            eprintln!("Failed to open database {}: {err}", db_path.display());
            return ExitCode::FAILURE;
        }
    };

    let output_dir = std::env::current_dir().unwrap_or_else(|_| PathBuf::from("."));
    let mut request = DownloadRequest::new(url.clone(), output_dir.clone());
    request.output_path = output.clone();
    request.max_workers = connections.max(1);
    request.sync_on_complete = sync_on_complete;

    let handle = match engine.start(request).await {
        Ok(handle) => handle,
        Err(err) => {
            eprintln!("Failed to start download: {err}");
            return ExitCode::FAILURE;
        }
    };

    println!("Download ID: {}", handle.id());
    drive_handle(
        handle,
        Some((url, output)),
        output_dir,
        record_samples,
        stats_out,
    )
    .await
}

async fn drive_handle(
    mut handle: DownloadHandle,
    debug_context: Option<(String, Option<PathBuf>)>,
    output_dir: PathBuf,
    record_samples: bool,
    stats_out: Option<PathBuf>,
) -> ExitCode {
    let id = handle.id().clone();
    let mut rx = handle.subscribe();

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

    let selected = tokio::select! {
        res = handle.wait() => Some(res),
        sig = tokio::signal::ctrl_c() => {
            if sig.is_ok() {
                eprintln!("\nPausing download {id}...");
                handle.pause();
            }
            None
        }
    };
    let result = match selected {
        Some(res) => res,
        None => handle.wait().await,
    };
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
        Err(EngineError::Paused) => {
            println!("Paused download {id} (resume with: posthaze resume {id})");
            ExitCode::SUCCESS
        }
        Err(err) => {
            eprintln!("Download failed: {err}");
            if record_samples && let Some((url, output)) = debug_context {
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
        DownloadState::Completed
            | DownloadState::Failed
            | DownloadState::Cancelled
            | DownloadState::Paused
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

#[cfg(test)]
mod tests {
    use super::*;
    use crate::bench_runner::{render_markdown_table, split_cli_args, summarize_row};

    #[test]
    fn parses_byte_sizes() {
        assert_eq!(parse_byte_size("100M").unwrap(), 100 * 1024 * 1024);
        assert_eq!(parse_byte_size("1G").unwrap(), 1024 * 1024 * 1024);
        assert_eq!(parse_byte_size("512KiB").unwrap(), 512 * 1024);
        assert_eq!(parse_byte_size("4096").unwrap(), 4096);
        assert!(parse_byte_size("").is_err());
    }

    #[test]
    fn parses_custom_bench_cli_flags() {
        let cli = Cli::try_parse_from([
            "posthaze",
            "bench",
            "--url",
            "https://example.com/file.bin",
            "--tools",
            "posthaze,aria2c",
            "--aria2-args",
            "--min-split-size=1M -k 1M",
            "--compare-sync",
            "--compare-db",
            "--scenarios",
            "clean,flaky",
            "-c",
            "1,8",
            "--runs",
            "2",
        ])
        .unwrap();

        match cli.command {
            Commands::Bench {
                url,
                tools,
                aria2_args,
                compare_sync,
                compare_db,
                scenarios,
                connections,
                runs,
                ..
            } => {
                assert_eq!(url.as_deref(), Some("https://example.com/file.bin"));
                assert_eq!(tools, vec!["posthaze", "aria2c"]);
                assert_eq!(aria2_args, "--min-split-size=1M -k 1M");
                assert!(compare_sync);
                assert!(compare_db);
                assert_eq!(scenarios, vec!["clean", "flaky"]);
                assert_eq!(connections, vec![1, 8]);
                assert_eq!(runs, 2);
            }
            _ => panic!("expected Bench subcommand"),
        }
    }

    #[test]
    fn summarizes_and_renders_markdown_row() {
        let mut samples = crate::bench_runner::RunSamples {
            secs: vec![2.2, 2.0, 2.4],
            mbps: vec![240.0, 260.0, 220.0],
            splits: vec![0, 2, 1],
            retries: vec![0, 0, 1],
            sizes: vec![],
        };
        let row = summarize_row("clean", "posthaze", 64 * 1024 * 1024, 8, &mut samples);
        assert_eq!(row.median_transfer_s, 2.2);
        assert_eq!(row.median_mbps, 240.0);
        assert_eq!(row.min_mbps, 220.0);
        assert_eq!(row.max_mbps, 260.0);
        assert_eq!(row.median_splits, 1);
        assert_eq!(split_cli_args("--min-split-size=2M -k 1M").len(), 3);

        let table = render_markdown_table(&[row]);
        assert!(table.contains("| clean | posthaze | 64.0 | 8 | 3 | 2.200 | 240.0 | 220.0–260.0 | 1 | 0 |"));
    }

    #[test]
    fn parses_persistence_cli_subcommands_and_db_flag() {
        let list_cli =
            Cli::try_parse_from(["posthaze", "--db", "custom/state.db", "list"]).unwrap();
        assert_eq!(list_cli.db.as_deref(), Some(Path::new("custom/state.db")));
        assert!(matches!(list_cli.command, Commands::List));

        let resume_cli =
            Cli::try_parse_from(["posthaze", "resume", "abc-123", "--db", "my.db"]).unwrap();
        assert_eq!(resume_cli.db.as_deref(), Some(Path::new("my.db")));
        match resume_cli.command {
            Commands::Resume { id, .. } => assert_eq!(id, "abc-123"),
            _ => panic!("expected Resume"),
        }

        let cancel_cli = Cli::try_parse_from(["posthaze", "cancel", "xyz-789"]).unwrap();
        assert!(cancel_cli.db.is_none());
        match cancel_cli.command {
            Commands::Cancel { id } => assert_eq!(id, "xyz-789"),
            _ => panic!("expected Cancel"),
        }
    }
}
