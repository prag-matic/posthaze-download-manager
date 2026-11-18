use std::io::Write;
use std::path::PathBuf;
use std::process::ExitCode;
use std::time::{Duration, Instant};

use clap::{Parser, Subcommand};
use posthaze_engine::{DownloadEngine, DownloadRequest, DownloadState, ProgressSnapshot};
use tokio::sync::watch;

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
    },
}

#[tokio::main]
async fn main() -> ExitCode {
    let cli = Cli::parse();

    match cli.command {
        Commands::Download {
            url,
            output,
            connections,
        } => run_download(url, output, connections).await,
    }
}

async fn run_download(url: String, output: Option<PathBuf>, connections: usize) -> ExitCode {
    let output_dir = std::env::current_dir().unwrap_or_else(|_| PathBuf::from("."));
    let mut request = DownloadRequest::new(url, output_dir);
    request.output_path = output;
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
        loop {
            tokio::select! {
                _ = ticker.tick() => {
                    let snap = rx.borrow().clone();
                    print_progress(&snap, start.elapsed());
                    if matches!(snap.state, DownloadState::Completed | DownloadState::Failed | DownloadState::Cancelled) {
                        break;
                    }
                }
                changed = rx.changed() => {
                    if changed.is_err() {
                        break;
                    }
                    let snap = rx.borrow().clone();
                    if matches!(snap.state, DownloadState::Completed | DownloadState::Failed | DownloadState::Cancelled) {
                        print_progress(&snap, start.elapsed());
                        break;
                    }
                }
            }
        }
        eprintln!();
    });

    let engine = DownloadEngine::new();
    let result = engine.download(request, Some(tx)).await;
    let _ = progress_task.await;

    match result {
        Ok(outcome) => {
            println!(
                "Downloaded {} bytes to {}",
                outcome.bytes_downloaded,
                outcome.final_path.display()
            );
            ExitCode::SUCCESS
        }
        Err(err) => {
            eprintln!("Download failed: {err}");
            ExitCode::FAILURE
        }
    }
}

fn print_progress(snap: &ProgressSnapshot, elapsed: Duration) {
    let secs = elapsed.as_secs_f64().max(0.001);
    let mb_per_sec = (snap.bytes_downloaded as f64 / (1024.0 * 1024.0)) / secs;
    match snap.total_bytes {
        Some(total) if total > 0 => {
            let pct = (snap.bytes_downloaded as f64 / total as f64) * 100.0;
            eprint!(
                "\r[{:?}] {}/{} bytes ({:.1}%) | {:.2} MiB/s | workers: {}   ",
                snap.state, snap.bytes_downloaded, total, pct, mb_per_sec, snap.active_workers
            );
        }
        _ => {
            eprint!(
                "\r[{:?}] {} bytes | {:.2} MiB/s | workers: {}   ",
                snap.state, snap.bytes_downloaded, mb_per_sec, snap.active_workers
            );
        }
    }
    let _ = std::io::stderr().flush();
}
