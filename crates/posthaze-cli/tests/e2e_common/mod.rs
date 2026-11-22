#![allow(dead_code)]

use std::io::{BufRead, BufReader, Read, Write};
use std::path::{Path, PathBuf};
use std::process::{Child, Command, ExitStatus, Output, Stdio};
use std::sync::Arc;
use std::sync::atomic::{AtomicBool, Ordering};
use std::time::{Duration, Instant};

use posthaze_engine::{DownloadEngine, DownloadId, DownloadRequest, DownloadState};
use serde_json::Value;
use tokio::io::{AsyncReadExt, AsyncWriteExt};
use tokio::net::TcpListener;

const BENCH_PATTERN_LEN: usize = 64 * 1024;

pub fn cli_bin() -> &'static str {
    env!("CARGO_BIN_EXE_posthaze-cli")
}

pub fn expected_bench_byte(offset: usize) -> u8 {
    let i = offset % BENCH_PATTERN_LEN;
    ((i.wrapping_mul(31)) ^ (i >> 8)) as u8
}

pub fn expected_bench_payload(size: usize) -> Vec<u8> {
    (0..size).map(expected_bench_byte).collect()
}

pub fn assert_matches_bench_pattern(actual: &[u8], expected_len: usize) {
    assert_eq!(
        actual.len(),
        expected_len,
        "downloaded file size mismatch: got {} bytes, expected {}",
        actual.len(),
        expected_len
    );
    for (idx, &byte) in actual.iter().enumerate() {
        let exp = expected_bench_byte(idx);
        assert_eq!(
            byte, exp,
            "byte mismatch at offset {idx}: got 0x{byte:02x}, expected 0x{exp:02x}"
        );
    }
}

/// RAII guard for a spawned `posthaze-cli serve` child process.
/// Ensures the background server is always killed and reaped on drop.
pub struct ServeGuard {
    child: Child,
    pub url: String,
    pub addr: String,
    pub size_bytes: u64,
}

impl ServeGuard {
    pub fn spawn(extra_args: &[&str]) -> Self {
        let mut cmd = Command::new(cli_bin());
        cmd.arg("serve")
            .args(extra_args)
            .stdin(Stdio::null())
            .stdout(Stdio::piped())
            .stderr(Stdio::null());

        let mut child = cmd.spawn().expect("failed to spawn posthaze-cli serve");
        let stdout = child.stdout.take().expect("serve stdout should be piped");
        let mut reader = BufReader::new(stdout);
        let mut first_line = String::new();
        reader
            .read_line(&mut first_line)
            .expect("failed to read startup banner from serve");

        if !first_line.starts_with("Serving ") {
            let _ = child.kill();
            let _ = child.wait();
            panic!("unexpected serve banner: {first_line:?}");
        }

        // Format: "Serving <size_bytes> bytes at http://127.0.0.1:<port>/bench.bin"
        let trimmed = first_line.trim();
        let rest = trimmed
            .strip_prefix("Serving ")
            .expect("missing 'Serving ' prefix");
        let (size_part, url_part) = rest
            .split_once(" bytes at ")
            .expect("missing ' bytes at ' in serve banner");
        let size_bytes: u64 = size_part.parse().expect("invalid size in serve banner");
        let url = url_part.to_string();
        let addr = url
            .strip_prefix("http://")
            .and_then(|s| s.split('/').next())
            .expect("invalid URL in serve banner")
            .to_string();

        // Drain stdout in background so the pipe never fills up.
        std::thread::spawn(move || {
            let mut sink = String::new();
            while reader.read_line(&mut sink).unwrap_or(0) > 0 {
                sink.clear();
            }
        });

        Self {
            child,
            url,
            addr,
            size_bytes,
        }
    }
}

impl Drop for ServeGuard {
    fn drop(&mut self) {
        let _ = self.child.kill();
        let _ = self.child.wait();
    }
}

pub fn run_cli(args: &[&str], cwd: Option<&Path>) -> Output {
    let mut cmd = Command::new(cli_bin());
    cmd.args(args);
    if let Some(dir) = cwd {
        cmd.current_dir(dir);
    }
    cmd.output().expect("failed to execute posthaze-cli")
}

pub async fn run_cli_async(args: &[&str], cwd: Option<&Path>) -> Output {
    let mut cmd = tokio::process::Command::new(cli_bin());
    cmd.args(args);
    if let Some(dir) = cwd {
        cmd.current_dir(dir);
    }
    cmd.output().await.expect("failed to execute posthaze-cli")
}

pub fn extract_download_id(stdout: &str) -> String {
    stdout
        .lines()
        .find_map(|line| line.strip_prefix("Download ID:"))
        .map(|s| s.trim().to_string())
        .expect("stdout did not contain 'Download ID: ...'")
}

pub fn encode_native_frame(payload: &[u8]) -> Vec<u8> {
    let mut buf = Vec::with_capacity(4 + payload.len());
    buf.extend_from_slice(&u32::try_from(payload.len()).unwrap().to_le_bytes());
    buf.extend_from_slice(payload);
    buf
}

pub fn decode_native_frame(bytes: &[u8]) -> Option<Value> {
    if bytes.is_empty() {
        return None;
    }
    assert!(
        bytes.len() >= 4,
        "native-host stdout truncated (<4 bytes): {} bytes",
        bytes.len()
    );
    let len = u32::from_le_bytes(bytes[..4].try_into().unwrap()) as usize;
    assert_eq!(
        bytes.len(),
        4 + len,
        "native-host stdout length mismatch: header={len}, total={}",
        bytes.len()
    );
    Some(serde_json::from_slice(&bytes[4..]).expect("native-host reply was not valid JSON"))
}

pub fn run_native_host_raw(db_path: &Path, raw_stdin: &[u8]) -> (ExitStatus, Vec<u8>, Vec<u8>) {
    let mut child = Command::new(cli_bin())
        .args(["--db", db_path.to_str().unwrap(), "native-host"])
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()
        .expect("failed to spawn native-host");

    {
        let mut stdin = child.stdin.take().unwrap();
        if !raw_stdin.is_empty() {
            stdin.write_all(raw_stdin).unwrap();
            stdin.flush().unwrap();
        }
    }

    let mut stdout_buf = Vec::new();
    let mut stderr_buf = Vec::new();
    let mut stdout = child.stdout.take().unwrap();
    let mut stderr = child.stderr.take().unwrap();
    stdout.read_to_end(&mut stdout_buf).unwrap();
    stderr.read_to_end(&mut stderr_buf).unwrap();

    let deadline = Instant::now() + Duration::from_secs(10);
    let status = loop {
        if let Some(status) = child.try_wait().unwrap() {
            break status;
        }
        assert!(Instant::now() < deadline, "native-host did not exit in time");
        std::thread::sleep(Duration::from_millis(25));
    };

    (status, stdout_buf, stderr_buf)
}

pub fn send_native_host_json(db_path: &Path, json_req: &Value) -> (ExitStatus, Value, Vec<u8>) {
    let payload = serde_json::to_vec(json_req).unwrap();
    let frame = encode_native_frame(&payload);
    let (status, stdout, stderr) = run_native_host_raw(db_path, &frame);
    let reply = decode_native_frame(&stdout).expect("expected framed JSON reply from native-host");
    (status, reply, stderr)
}

pub async fn configure_db_output_dir(db_path: &Path, output_dir: &Path) {
    let db_path = db_path.to_path_buf();
    let output_dir = output_dir.to_path_buf();
    tokio::task::spawn_blocking(move || {
        std::thread::spawn(move || {
            let rt = tokio::runtime::Builder::new_current_thread()
                .enable_all()
                .build()
                .unwrap();
            rt.block_on(async move {
                let engine = DownloadEngine::open(&db_path).await.unwrap();
                let mut settings = engine.get_settings().await.unwrap();
                settings.default_output_dir = output_dir;
                engine.update_settings(settings).await.unwrap();
            });
            drop(rt);
        })
        .join()
        .unwrap();
    })
    .await
    .unwrap();
}

pub async fn spawn_mock_http_server(
    payload: Arc<Vec<u8>>,
    throttle: Arc<AtomicBool>,
    not_found: bool,
) -> String {
    let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
    let addr = listener.local_addr().unwrap();

    tokio::spawn(async move {
        while let Ok((mut stream, _)) = listener.accept().await {
            let payload = Arc::clone(&payload);
            let throttle = Arc::clone(&throttle);
            tokio::spawn(async move {
                let mut buf = vec![0u8; 4096];
                let Ok(n) = stream.read(&mut buf).await else {
                    return;
                };
                if n == 0 {
                    return;
                }
                let req = String::from_utf8_lossy(&buf[..n]);
                let method = req
                    .lines()
                    .next()
                    .unwrap_or("")
                    .split_whitespace()
                    .next()
                    .unwrap_or("");

                if not_found {
                    let resp = "HTTP/1.1 404 Not Found\r\nContent-Length: 0\r\nConnection: close\r\n\r\n";
                    let _ = stream.write_all(resp.as_bytes()).await;
                    return;
                }

                let range_hdr = req.lines().find_map(|line| {
                    let (k, v) = line.split_once(':')?;
                    if k.eq_ignore_ascii_case("range") {
                        Some(v.trim().to_string())
                    } else {
                        None
                    }
                });

                if method == "HEAD" {
                    let resp = format!(
                        "HTTP/1.1 200 OK\r\nContent-Length: {}\r\nAccept-Ranges: bytes\r\nETag: \"mock-v1\"\r\nLast-Modified: Wed, 21 Oct 2015 07:28:00 GMT\r\nConnection: close\r\n\r\n",
                        payload.len()
                    );
                    let _ = stream.write_all(resp.as_bytes()).await;
                } else if method == "GET" {
                    if let Some(r) = range_hdr {
                        let r = r.strip_prefix("bytes=").unwrap_or(&r);
                        let (s, e) = r.split_once('-').unwrap();
                        let start: usize = s.parse().unwrap();
                        let end: usize = e
                            .parse::<usize>()
                            .unwrap_or(payload.len().saturating_sub(1))
                            .min(payload.len().saturating_sub(1));
                        let slice = &payload[start..=end];
                        let header = format!(
                            "HTTP/1.1 206 Partial Content\r\nContent-Length: {}\r\nContent-Range: bytes {start}-{end}/{}\r\nETag: \"mock-v1\"\r\nConnection: close\r\n\r\n",
                            slice.len(),
                            payload.len()
                        );
                        if stream.write_all(header.as_bytes()).await.is_err() {
                            return;
                        }
                        let slow = throttle.load(Ordering::Relaxed);
                        for chunk in slice.chunks(64 * 1024) {
                            if stream.write_all(chunk).await.is_err() {
                                return;
                            }
                            if slow {
                                tokio::time::sleep(Duration::from_millis(40)).await;
                            }
                        }
                    } else {
                        let header = format!(
                            "HTTP/1.1 200 OK\r\nContent-Length: {}\r\nETag: \"mock-v1\"\r\nConnection: close\r\n\r\n",
                            payload.len()
                        );
                        let _ = stream.write_all(header.as_bytes()).await;
                        let _ = stream.write_all(&payload).await;
                    }
                }
            });
        }
    });

    format!("http://{addr}")
}

pub async fn seed_paused_download(
    engine: &DownloadEngine,
    url: &str,
    out_path: &Path,
) -> (DownloadId, PathBuf) {
    let mut req = DownloadRequest::new(url, out_path.parent().unwrap());
    req.output_path = Some(out_path.to_path_buf());
    req.max_workers = 2;

    let mut handle = engine.start(req).await.unwrap();
    let id = handle.id().clone();
    let mut rx = handle.subscribe();
    tokio::time::timeout(Duration::from_secs(5), async {
        loop {
            if rx.borrow().state == DownloadState::Active && rx.borrow().bytes_downloaded > 0 {
                break;
            }
            rx.changed().await.unwrap();
        }
    })
    .await
    .unwrap();
    handle.pause();
    let _ = handle.wait().await;

    let part_path = engine
        .get_download(&id)
        .await
        .unwrap()
        .unwrap()
        .part_path
        .unwrap();
    (id, part_path)
}
