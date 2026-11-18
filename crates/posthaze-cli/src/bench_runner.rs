use std::path::{Path, PathBuf};
use std::process::{Command, ExitCode};
use std::time::Instant;

use posthaze_engine::{DownloadEngine, DownloadRequest};
use serde::Serialize;

use crate::bench_server::{BenchServerConfig, parse_byte_size, start_bench_server};

#[derive(Debug, Clone)]
pub struct BenchOptions {
    pub sizes: Vec<String>,
    pub connections: Vec<usize>,
    pub runs: usize,
    pub per_conn_mbps: Option<f64>,
    pub latency_ms: u64,
    pub drop_pct: f64,
    pub error_503_pct: f64,
    pub include_external: bool,
    pub report_path: Option<PathBuf>,
}

#[derive(Debug, Serialize)]
struct BenchRow {
    tool: String,
    size_bytes: u64,
    connections: usize,
    runs: usize,
    median_transfer_s: f64,
    median_mbps: f64,
    min_mbps: f64,
    max_mbps: f64,
    median_splits: usize,
    median_retries: usize,
}

pub async fn run_bench(opts: BenchOptions) -> ExitCode {
    let mut parsed_sizes = Vec::new();
    for raw in &opts.sizes {
        match parse_byte_size(raw) {
            Ok(bytes) => parsed_sizes.push((raw.clone(), bytes)),
            Err(err) => {
                eprintln!("Invalid --size '{raw}': {err}");
                return ExitCode::FAILURE;
            }
        }
    }

    let has_curl = opts.include_external && command_exists("curl", &["--version"]);
    let has_aria2 = opts.include_external && command_exists("aria2c", &["--version"]);

    let temp_root = std::env::temp_dir().join(format!("posthaze-bench-{}", std::process::id()));
    if let Err(err) = std::fs::create_dir_all(&temp_root) {
        eprintln!("Failed to create temp dir {}: {err}", temp_root.display());
        return ExitCode::FAILURE;
    }

    let mut rows = Vec::new();
    let engine = DownloadEngine::new();

    for (size_label, size_bytes) in &parsed_sizes {
        let server = match start_bench_server(BenchServerConfig {
            size: *size_bytes,
            port: 0,
            per_conn_mbps: opts.per_conn_mbps,
            latency_ms: opts.latency_ms,
            drop_pct: opts.drop_pct,
            error_503_pct: opts.error_503_pct,
            block_head: false,
            no_range: false,
        })
        .await
        {
            Ok(s) => s,
            Err(err) => {
                eprintln!("Failed to start local bench server: {err}");
                let _ = std::fs::remove_dir_all(&temp_root);
                return ExitCode::FAILURE;
            }
        };

        let url = server.url();
        println!(
            "Benchmarking size={size_label} ({size_bytes} bytes) against {} (runs={})",
            url, opts.runs
        );

        for &c in &opts.connections {
            let mut transfer_secs = Vec::with_capacity(opts.runs);
            let mut mbps_samples = Vec::with_capacity(opts.runs);
            let mut splits = Vec::with_capacity(opts.runs);
            let mut retries = Vec::with_capacity(opts.runs);

            for run_idx in 0..opts.runs {
                let out_path = temp_root.join(format!("ph-{size_label}-c{c}-r{run_idx}.bin"));
                let mut req = DownloadRequest::new(&url, &temp_root);
                req.output_path = Some(out_path.clone());
                req.max_workers = c;

                match engine.download(req, None).await {
                    Ok(outcome) => {
                        let secs = outcome.transfer_duration.as_secs_f64().max(0.0001);
                        let mbps = (outcome.average_bytes_per_sec() * 8.0) / 1_000_000.0;
                        transfer_secs.push(secs);
                        mbps_samples.push(mbps);
                        splits.push(outcome.splits_performed);
                        retries.push(outcome.retries_performed);
                    }
                    Err(err) => {
                        eprintln!("  [posthaze c={c} run={run_idx}] failed: {err}");
                    }
                }
                let _ = std::fs::remove_file(&out_path);
            }

            if !mbps_samples.is_empty() {
                rows.push(summarize_row(
                    "posthaze",
                    *size_bytes,
                    c,
                    &mut transfer_secs,
                    &mut mbps_samples,
                    &mut splits,
                    &mut retries,
                ));
            }

            if has_aria2 && c <= 16 {
                let mut a_secs = Vec::with_capacity(opts.runs);
                let mut a_mbps = Vec::with_capacity(opts.runs);
                for run_idx in 0..opts.runs {
                    let filename = format!("aria-{size_label}-c{c}-r{run_idx}.bin");
                    if let Some(secs) = run_aria2(&url, &temp_root, &filename, c) {
                        let mbps = ((*size_bytes as f64 / secs) * 8.0) / 1_000_000.0;
                        a_secs.push(secs);
                        a_mbps.push(mbps);
                    }
                    let _ = std::fs::remove_file(temp_root.join(&filename));
                }
                if !a_mbps.is_empty() {
                    rows.push(summarize_row(
                        "aria2c",
                        *size_bytes,
                        c,
                        &mut a_secs,
                        &mut a_mbps,
                        &mut [0],
                        &mut [0],
                    ));
                }
            }

            if has_curl && c == 1 {
                let mut c_secs = Vec::with_capacity(opts.runs);
                let mut c_mbps = Vec::with_capacity(opts.runs);
                for run_idx in 0..opts.runs {
                    let out_path = temp_root.join(format!("curl-{size_label}-r{run_idx}.bin"));
                    if let Some(secs) = run_curl(&url, &out_path) {
                        let mbps = ((*size_bytes as f64 / secs) * 8.0) / 1_000_000.0;
                        c_secs.push(secs);
                        c_mbps.push(mbps);
                    }
                    let _ = std::fs::remove_file(&out_path);
                }
                if !c_mbps.is_empty() {
                    rows.push(summarize_row(
                        "curl",
                        *size_bytes,
                        1,
                        &mut c_secs,
                        &mut c_mbps,
                        &mut [0],
                        &mut [0],
                    ));
                }
            }
        }

        server.handle.abort();
    }

    let _ = std::fs::remove_dir_all(&temp_root);

    let markdown = render_markdown_table(&rows);
    println!("\n{markdown}");

    if let Some(report_path) = opts.report_path {
        let content = if report_path.extension().and_then(|e| e.to_str()) == Some("json") {
            serde_json::to_string_pretty(&rows).unwrap_or_default()
        } else {
            markdown
        };
        if let Err(err) = std::fs::write(&report_path, content) {
            eprintln!("Failed to write report to {}: {err}", report_path.display());
            return ExitCode::FAILURE;
        }
        println!("Report saved to {}", report_path.display());
    }

    ExitCode::SUCCESS
}

fn summarize_row(
    tool: &str,
    size_bytes: u64,
    connections: usize,
    secs: &mut [f64],
    mbps: &mut [f64],
    splits: &mut [usize],
    retries: &mut [usize],
) -> BenchRow {
    secs.sort_by(|a, b| a.total_cmp(b));
    mbps.sort_by(|a, b| a.total_cmp(b));
    splits.sort_unstable();
    retries.sort_unstable();

    let mid = mbps.len() / 2;
    BenchRow {
        tool: tool.to_string(),
        size_bytes,
        connections,
        runs: mbps.len(),
        median_transfer_s: secs[mid],
        median_mbps: mbps[mid],
        min_mbps: *mbps.first().unwrap_or(&0.0),
        max_mbps: *mbps.last().unwrap_or(&0.0),
        median_splits: splits[splits.len() / 2],
        median_retries: retries[retries.len() / 2],
    }
}

fn render_markdown_table(rows: &[BenchRow]) -> String {
    let mut out = String::from(
        "| Tool | Size (MiB) | Connections | Runs | Median Time (s) | Median Mbps | Min–Max Mbps | Splits | Retries |\n|---|---:|---:|---:|---:|---:|---:|---:|---:|\n",
    );
    for r in rows {
        let mib = r.size_bytes as f64 / (1024.0 * 1024.0);
        out.push_str(&format!(
            "| {} | {:.1} | {} | {} | {:.3} | {:.1} | {:.1}–{:.1} | {} | {} |\n",
            r.tool,
            mib,
            r.connections,
            r.runs,
            r.median_transfer_s,
            r.median_mbps,
            r.min_mbps,
            r.max_mbps,
            r.median_splits,
            r.median_retries
        ));
    }
    out
}

fn command_exists(bin: &str, args: &[&str]) -> bool {
    Command::new(bin)
        .args(args)
        .output()
        .is_ok_and(|o| o.status.success())
}

fn run_aria2(url: &str, dir: &Path, filename: &str, connections: usize) -> Option<f64> {
    let c_str = connections.to_string();
    let start = Instant::now();
    let status = Command::new("aria2c")
        .args([
            "--quiet=true",
            "--allow-overwrite=true",
            "-x",
            &c_str,
            "-s",
            &c_str,
            "-d",
            &dir.to_string_lossy(),
            "-o",
            filename,
            url,
        ])
        .status()
        .ok()?;
    if status.success() {
        Some(start.elapsed().as_secs_f64().max(0.0001))
    } else {
        None
    }
}

fn run_curl(url: &str, out_path: &Path) -> Option<f64> {
    let start = Instant::now();
    let status = Command::new("curl")
        .args(["-sS", "-o", &out_path.to_string_lossy(), url])
        .status()
        .ok()?;
    if status.success() {
        Some(start.elapsed().as_secs_f64().max(0.0001))
    } else {
        None
    }
}
