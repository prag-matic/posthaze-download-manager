use std::path::{Path, PathBuf};
use std::process::{Command, ExitCode};
use std::time::Instant;

use posthaze_engine::{DownloadEngine, DownloadRequest};
use serde::Serialize;

use crate::bench_server::{BenchServerConfig, parse_byte_size, start_bench_server};

#[derive(Debug, Clone)]
pub struct BenchOptions {
    pub url: Option<String>,
    pub sizes: Vec<String>,
    pub connections: Vec<usize>,
    pub runs: usize,
    pub tools: Vec<String>,
    pub aria2_args: String,
    pub curl_args: String,
    pub per_conn_mbps: Option<f64>,
    pub latency_ms: u64,
    pub drop_pct: f64,
    pub error_503_pct: f64,
    pub block_head: bool,
    pub sync_on_complete: bool,
    pub compare_sync: bool,
    pub alternate: bool,
    pub scenarios: Vec<String>,
    pub report_path: Option<PathBuf>,
}

#[derive(Debug, Clone, Serialize)]
pub struct BenchRow {
    pub scenario: String,
    pub tool: String,
    pub size_bytes: u64,
    pub connections: usize,
    pub runs: usize,
    pub median_transfer_s: f64,
    pub median_mbps: f64,
    pub min_mbps: f64,
    pub max_mbps: f64,
    pub median_splits: usize,
    pub median_retries: usize,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum ToolKind {
    Posthaze,
    PosthazeSync,
    Aria2c,
    Curl,
}

impl ToolKind {
    fn label(self) -> &'static str {
        match self {
            Self::Posthaze => "posthaze",
            Self::PosthazeSync => "posthaze (sync)",
            Self::Aria2c => "aria2c",
            Self::Curl => "curl",
        }
    }
}

#[derive(Debug, Clone)]
struct ScenarioSpec {
    name: String,
    per_conn_mbps: Option<f64>,
    latency_ms: u64,
    drop_pct: f64,
    error_503_pct: f64,
    block_head: bool,
}

pub async fn run_bench(opts: BenchOptions) -> ExitCode {
    let selected_tools = match resolve_tools(&opts) {
        Ok(t) => t,
        Err(err) => {
            eprintln!("{err}");
            return ExitCode::FAILURE;
        }
    };

    let temp_root = std::env::temp_dir().join(format!("posthaze-bench-{}", std::process::id()));
    if let Err(err) = std::fs::create_dir_all(&temp_root) {
        eprintln!("Failed to create temp dir {}: {err}", temp_root.display());
        return ExitCode::FAILURE;
    }

    let engine = DownloadEngine::new();
    let aria2_extra = split_cli_args(&opts.aria2_args);
    let curl_extra = split_cli_args(&opts.curl_args);
    let mut rows = Vec::new();

    if let Some(ref external_url) = opts.url {
        println!(
            "Benchmarking external URL {} (runs={}, tools={})",
            external_url,
            opts.runs,
            selected_tools
                .iter()
                .map(|t| t.label())
                .collect::<Vec<_>>()
                .join(",")
        );
        run_matrix_against_url(
            "external",
            external_url,
            None,
            &opts,
            &selected_tools,
            &aria2_extra,
            &curl_extra,
            &engine,
            &temp_root,
            &mut rows,
        )
        .await;
    } else {
        let mut parsed_sizes = Vec::new();
        for raw in &opts.sizes {
            match parse_byte_size(raw) {
                Ok(bytes) => parsed_sizes.push((raw.clone(), bytes)),
                Err(err) => {
                    eprintln!("Invalid --size '{raw}': {err}");
                    let _ = std::fs::remove_dir_all(&temp_root);
                    return ExitCode::FAILURE;
                }
            }
        }

        let scenarios = build_scenarios(&opts);
        for scenario in &scenarios {
            for (size_label, size_bytes) in &parsed_sizes {
                let server = match start_bench_server(BenchServerConfig {
                    size: *size_bytes,
                    port: 0,
                    per_conn_mbps: scenario.per_conn_mbps,
                    latency_ms: scenario.latency_ms,
                    drop_pct: scenario.drop_pct,
                    error_503_pct: scenario.error_503_pct,
                    block_head: scenario.block_head,
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
                    "Benchmarking scenario={} size={size_label} ({size_bytes} bytes) at {url} (runs={})",
                    scenario.name, opts.runs
                );

                run_matrix_against_url(
                    &scenario.name,
                    &url,
                    Some(*size_bytes),
                    &opts,
                    &selected_tools,
                    &aria2_extra,
                    &curl_extra,
                    &engine,
                    &temp_root,
                    &mut rows,
                )
                .await;

                server.handle.abort();
            }
        }
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

#[derive(Default, Clone)]
pub struct RunSamples {
    pub secs: Vec<f64>,
    pub mbps: Vec<f64>,
    pub splits: Vec<usize>,
    pub retries: Vec<usize>,
    pub sizes: Vec<u64>,
}

#[allow(clippy::too_many_arguments)]
async fn run_matrix_against_url(
    scenario_name: &str,
    url: &str,
    known_size: Option<u64>,
    opts: &BenchOptions,
    selected_tools: &[ToolKind],
    aria2_extra: &[String],
    curl_extra: &[String],
    engine: &DownloadEngine,
    temp_root: &Path,
    rows: &mut Vec<BenchRow>,
) {
    for &c in &opts.connections {
        let applicable: Vec<ToolKind> = selected_tools
            .iter()
            .copied()
            .filter(|t| match t {
                ToolKind::Curl => c == 1,
                ToolKind::Aria2c => c <= 16,
                ToolKind::Posthaze | ToolKind::PosthazeSync => true,
            })
            .collect();

        if applicable.is_empty() {
            continue;
        }

        let mut acc = vec![RunSamples::default(); applicable.len()];

        for run_idx in 0..opts.runs {
            let mut order: Vec<usize> = (0..applicable.len()).collect();
            if opts.alternate && run_idx % 2 == 1 {
                order.reverse();
            }

            for idx in order {
                let tool = applicable[idx];
                match tool {
                    ToolKind::Posthaze | ToolKind::PosthazeSync => {
                        let out_path = temp_root.join(format!("ph-{scenario_name}-c{c}-r{run_idx}.bin"));
                        let mut req = DownloadRequest::new(url, temp_root);
                        req.output_path = Some(out_path.clone());
                        req.max_workers = c;
                        req.sync_on_complete =
                            matches!(tool, ToolKind::PosthazeSync) || opts.sync_on_complete;

                        match engine.download(req, None).await {
                            Ok(outcome) => {
                                let secs = outcome.transfer_duration.as_secs_f64().max(0.0001);
                                let mbps = (outcome.average_bytes_per_sec() * 8.0) / 1_000_000.0;
                                acc[idx].secs.push(secs);
                                acc[idx].mbps.push(mbps);
                                acc[idx].splits.push(outcome.splits_performed);
                                acc[idx].retries.push(outcome.retries_performed);
                                acc[idx].sizes.push(outcome.bytes_downloaded);
                            }
                            Err(err) => {
                                eprintln!("  [{} c={c} run={run_idx}] failed: {err}", tool.label());
                            }
                        }
                        let _ = std::fs::remove_file(&out_path);
                    }
                    ToolKind::Aria2c => {
                        let filename = format!("aria-{scenario_name}-c{c}-r{run_idx}.bin");
                        let file_path = temp_root.join(&filename);
                        if let Some(secs) = run_aria2(url, temp_root, &filename, c, aria2_extra) {
                            let bytes = known_size
                                .or_else(|| std::fs::metadata(&file_path).ok().map(|m| m.len()))
                                .unwrap_or(0);
                            let mbps = ((bytes as f64 / secs) * 8.0) / 1_000_000.0;
                            acc[idx].secs.push(secs);
                            acc[idx].mbps.push(mbps);
                            acc[idx].splits.push(0);
                            acc[idx].retries.push(0);
                            acc[idx].sizes.push(bytes);
                        }
                        let _ = std::fs::remove_file(&file_path);
                    }
                    ToolKind::Curl => {
                        let out_path = temp_root.join(format!("curl-{scenario_name}-r{run_idx}.bin"));
                        if let Some(secs) = run_curl(url, &out_path, curl_extra) {
                            let bytes = known_size
                                .or_else(|| std::fs::metadata(&out_path).ok().map(|m| m.len()))
                                .unwrap_or(0);
                            let mbps = ((bytes as f64 / secs) * 8.0) / 1_000_000.0;
                            acc[idx].secs.push(secs);
                            acc[idx].mbps.push(mbps);
                            acc[idx].splits.push(0);
                            acc[idx].retries.push(0);
                            acc[idx].sizes.push(bytes);
                        }
                        let _ = std::fs::remove_file(&out_path);
                    }
                }
            }
        }

        for (idx, tool) in applicable.into_iter().enumerate() {
            let mut samples = std::mem::take(&mut acc[idx]);
            if !samples.mbps.is_empty() {
                let size_bytes = known_size
                    .or_else(|| samples.sizes.first().copied())
                    .unwrap_or(0);
                rows.push(summarize_row(
                    scenario_name,
                    tool.label(),
                    size_bytes,
                    c,
                    &mut samples,
                ));
            }
        }
    }
}

fn resolve_tools(opts: &BenchOptions) -> Result<Vec<ToolKind>, String> {
    let mut out = Vec::new();
    let has_aria2 = command_exists("aria2c", &["--version"]);
    let has_curl = command_exists("curl", &["--version"]);

    for raw in &opts.tools {
        match raw.trim().to_ascii_lowercase().as_str() {
            "auto" => {
                push_unique(&mut out, ToolKind::Posthaze);
                if opts.compare_sync {
                    push_unique(&mut out, ToolKind::PosthazeSync);
                }
                if has_aria2 {
                    push_unique(&mut out, ToolKind::Aria2c);
                }
                if has_curl {
                    push_unique(&mut out, ToolKind::Curl);
                }
            }
            "posthaze" => {
                push_unique(&mut out, ToolKind::Posthaze);
                if opts.compare_sync {
                    push_unique(&mut out, ToolKind::PosthazeSync);
                }
            }
            "posthaze-sync" => push_unique(&mut out, ToolKind::PosthazeSync),
            "aria2c" | "aria2" => {
                if !has_aria2 {
                    return Err("Tool 'aria2c' requested via --tools, but not found on PATH".into());
                }
                push_unique(&mut out, ToolKind::Aria2c);
            }
            "curl" => {
                if !has_curl {
                    return Err("Tool 'curl' requested via --tools, but not found on PATH".into());
                }
                push_unique(&mut out, ToolKind::Curl);
            }
            other => {
                return Err(format!(
                    "Unknown tool '{other}' in --tools (expected auto, posthaze, posthaze-sync, aria2c, curl)"
                ));
            }
        }
    }
    Ok(out)
}

fn push_unique<T: PartialEq>(vec: &mut Vec<T>, item: T) {
    if !vec.contains(&item) {
        vec.push(item);
    }
}

fn build_scenarios(opts: &BenchOptions) -> Vec<ScenarioSpec> {
    if opts.scenarios.is_empty() {
        return vec![ScenarioSpec {
            name: "custom".to_string(),
            per_conn_mbps: opts.per_conn_mbps,
            latency_ms: opts.latency_ms,
            drop_pct: opts.drop_pct,
            error_503_pct: opts.error_503_pct,
            block_head: opts.block_head,
        }];
    }

    opts.scenarios
        .iter()
        .map(|s| match s.trim().to_ascii_lowercase().as_str() {
            "clean" => ScenarioSpec {
                name: "clean".into(),
                per_conn_mbps: None,
                latency_ms: 0,
                drop_pct: 0.0,
                error_503_pct: 0.0,
                block_head: false,
            },
            "throttled" => ScenarioSpec {
                name: "throttled".into(),
                per_conn_mbps: Some(opts.per_conn_mbps.unwrap_or(40.0)),
                latency_ms: opts.latency_ms,
                drop_pct: 0.0,
                error_503_pct: 0.0,
                block_head: false,
            },
            "head-blocked" => ScenarioSpec {
                name: "head-blocked".into(),
                per_conn_mbps: opts.per_conn_mbps,
                latency_ms: opts.latency_ms,
                drop_pct: opts.drop_pct,
                error_503_pct: opts.error_503_pct,
                block_head: true,
            },
            "flaky" => ScenarioSpec {
                name: "flaky".into(),
                per_conn_mbps: opts.per_conn_mbps,
                latency_ms: opts.latency_ms,
                drop_pct: if opts.drop_pct > 0.0 { opts.drop_pct } else { 5.0 },
                error_503_pct: if opts.error_503_pct > 0.0 {
                    opts.error_503_pct
                } else {
                    5.0
                },
                block_head: opts.block_head,
            },
            other => ScenarioSpec {
                name: other.to_string(),
                per_conn_mbps: opts.per_conn_mbps,
                latency_ms: opts.latency_ms,
                drop_pct: opts.drop_pct,
                error_503_pct: opts.error_503_pct,
                block_head: opts.block_head,
            },
        })
        .collect()
}

pub fn split_cli_args(raw: &str) -> Vec<String> {
    raw.split_whitespace().map(ToString::to_string).collect()
}

pub fn summarize_row(
    scenario: &str,
    tool: &str,
    size_bytes: u64,
    connections: usize,
    samples: &mut RunSamples,
) -> BenchRow {
    samples.secs.sort_by(|a, b| a.total_cmp(b));
    samples.mbps.sort_by(|a, b| a.total_cmp(b));
    samples.splits.sort_unstable();
    samples.retries.sort_unstable();

    let mid = samples.mbps.len() / 2;
    BenchRow {
        scenario: scenario.to_string(),
        tool: tool.to_string(),
        size_bytes,
        connections,
        runs: samples.mbps.len(),
        median_transfer_s: samples.secs[mid],
        median_mbps: samples.mbps[mid],
        min_mbps: *samples.mbps.first().unwrap_or(&0.0),
        max_mbps: *samples.mbps.last().unwrap_or(&0.0),
        median_splits: samples.splits[samples.splits.len() / 2],
        median_retries: samples.retries[samples.retries.len() / 2],
    }
}

pub fn render_markdown_table(rows: &[BenchRow]) -> String {
    let mut out = String::from(
        "| Scenario | Tool | Size (MiB) | Connections | Runs | Median Time (s) | Median Mbps | Min–Max Mbps | Splits | Retries |\n|---|---|---:|---:|---:|---:|---:|---:|---:|---:|\n",
    );
    for r in rows {
        let mib = r.size_bytes as f64 / (1024.0 * 1024.0);
        out.push_str(&format!(
            "| {} | {} | {:.1} | {} | {} | {:.3} | {:.1} | {:.1}–{:.1} | {} | {} |\n",
            r.scenario,
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

fn run_aria2(
    url: &str,
    dir: &Path,
    filename: &str,
    connections: usize,
    extra_args: &[String],
) -> Option<f64> {
    let c_str = connections.to_string();
    let dir_str = dir.to_string_lossy().to_string();
    let mut cmd = Command::new("aria2c");
    cmd.args([
        "--quiet=true",
        "--allow-overwrite=true",
        "-x",
        &c_str,
        "-s",
        &c_str,
        "-d",
        &dir_str,
        "-o",
        filename,
    ]);
    cmd.args(extra_args);
    cmd.arg(url);

    let start = Instant::now();
    let status = cmd.status().ok()?;
    if status.success() {
        Some(start.elapsed().as_secs_f64().max(0.0001))
    } else {
        None
    }
}

fn run_curl(url: &str, out_path: &Path, extra_args: &[String]) -> Option<f64> {
    let out_str = out_path.to_string_lossy().to_string();
    let mut cmd = Command::new("curl");
    cmd.args(["-sS", "-o", &out_str]);
    cmd.args(extra_args);
    cmd.arg(url);

    let start = Instant::now();
    let status = cmd.status().ok()?;
    if status.success() {
        Some(start.elapsed().as_secs_f64().max(0.0001))
    } else {
        None
    }
}
