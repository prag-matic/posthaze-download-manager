//! Interactive Download runner and subcommand handlers for posthaze-cli.

use std::io::Write;
use std::path::{Path, PathBuf};
use std::process::ExitCode;
use std::time::{Duration, Instant};

use posthaze_engine::{
    DownloadEngine, DownloadHandle, DownloadId, DownloadOutcome, DownloadRequest, DownloadState,
    EngineError, ProgressSnapshot,
};

use crate::stats::{
    ProgressStats, default_stats_path, write_failure_stats, write_success_stats,
};

pub(crate) async fn run_list(db_path: PathBuf) -> ExitCode {
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

pub(crate) async fn run_cancel(db_path: PathBuf, id: String) -> ExitCode {
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

pub(crate) async fn run_delete(db_path: PathBuf, id: String, delete_file: bool) -> ExitCode {
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

pub(crate) async fn run_resume(
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

pub(crate) async fn run_download(
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
        let mut stats = ProgressStats::default();

        loop {
            tokio::select! {
                _ = ticker.tick() => {
                    let snap = rx.borrow().clone();
                    let now = Instant::now();
                    let dt = now.duration_since(last_tick).as_secs_f64();
                    last_tick = now;

                    let elapsed = start.elapsed();
                    let inst_bps = stats.record_tick(snap.clone(), elapsed, dt, record_samples);
                    print_progress(&snap, elapsed, inst_bps);
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
                        stats.record_terminal(snap.clone(), elapsed, record_samples);
                        print_progress(&snap, elapsed, 0.0);
                        break;
                    }
                }
            }
        }
        eprintln!();
        stats
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
