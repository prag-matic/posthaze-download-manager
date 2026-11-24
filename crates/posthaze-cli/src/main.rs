mod bench_runner;
mod bench_server;
mod native_host;
mod runner;
mod stats;

use std::path::PathBuf;
use std::process::ExitCode;

use clap::{Parser, Subcommand};
use posthaze_engine::default_db_path;

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

fn is_browser_native_origin_arg(arg: &std::ffi::OsStr) -> bool {
    arg.to_str()
        .is_some_and(|s| s.starts_with("chrome-extension://") && s.ends_with('/'))
}

#[tokio::main]
async fn main() -> ExitCode {
    if std::env::args_os()
        .nth(1)
        .is_some_and(|arg| is_browser_native_origin_arg(&arg))
    {
        return native_host::run_native_host(default_db_path()).await;
    }

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
            runner::run_download(
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
        Commands::List => runner::run_list(db_path).await,
        Commands::Resume {
            id,
            debug,
            stats_out,
        } => runner::run_resume(db_path, id, debug || stats_out.is_some(), stats_out).await,
        Commands::Cancel { id } => runner::run_cancel(db_path, id).await,
        Commands::Delete { id, delete_file } => runner::run_delete(db_path, id, delete_file).await,
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

#[cfg(test)]
mod tests {
    use super::*;
    use std::path::Path;
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

    #[test]
    fn detects_chromium_native_host_origin_argument() {
        use std::ffi::OsStr;

        assert!(is_browser_native_origin_arg(OsStr::new(
            "chrome-extension://eiophahfcjidkfgdddgejjglglaopahi/"
        )));
        assert!(!is_browser_native_origin_arg(OsStr::new("native-host")));
        assert!(!is_browser_native_origin_arg(OsStr::new("download")));
        assert!(!is_browser_native_origin_arg(OsStr::new(
            "chrome-extension://incomplete"
        )));
    }
}
