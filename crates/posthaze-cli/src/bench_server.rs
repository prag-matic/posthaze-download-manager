use std::net::SocketAddr;
use std::sync::Arc;
use std::sync::atomic::{AtomicU64, Ordering};
use std::time::{Duration, SystemTime, UNIX_EPOCH};

use tokio::io::{AsyncBufReadExt, AsyncWriteExt, BufReader};
use tokio::net::TcpListener;
use tokio::task::JoinHandle;

const PATTERN_LEN: usize = 64 * 1024;

#[derive(Debug, Clone)]
pub struct BenchServerConfig {
    pub size: u64,
    pub port: u16,
    pub per_conn_mbps: Option<f64>,
    pub latency_ms: u64,
    pub drop_pct: f64,
    pub error_503_pct: f64,
    pub block_head: bool,
    pub no_range: bool,
}

pub struct RunningBenchServer {
    pub addr: SocketAddr,
    pub handle: JoinHandle<()>,
}

impl RunningBenchServer {
    pub fn url(&self) -> String {
        format!("http://{}/bench.bin", self.addr)
    }
}

pub fn parse_byte_size(raw: &str) -> Result<u64, String> {
    let s = raw.trim();
    if s.is_empty() {
        return Err("size cannot be empty".to_string());
    }
    let lower = s.to_ascii_lowercase();
    let (num_str, mult) = if let Some(n) = lower.strip_suffix("gib").or_else(|| lower.strip_suffix('g')) {
        (n, 1024u64 * 1024 * 1024)
    } else if let Some(n) = lower.strip_suffix("mib").or_else(|| lower.strip_suffix('m')) {
        (n, 1024u64 * 1024)
    } else if let Some(n) = lower.strip_suffix("kib").or_else(|| lower.strip_suffix('k')) {
        (n, 1024u64)
    } else if let Some(n) = lower.strip_suffix('b') {
        (n, 1u64)
    } else {
        (lower.as_str(), 1u64)
    };

    let num: u64 = num_str
        .trim()
        .parse()
        .map_err(|_| format!("invalid byte size: {raw}"))?;
    num.checked_mul(mult)
        .ok_or_else(|| format!("byte size overflow: {raw}"))
}

pub async fn start_bench_server(config: BenchServerConfig) -> Result<RunningBenchServer, std::io::Error> {
    let listener = TcpListener::bind(("127.0.0.1", config.port)).await?;
    let addr = listener.local_addr()?;

    let mut pat = vec![0u8; PATTERN_LEN];
    for (i, b) in pat.iter_mut().enumerate() {
        *b = ((i.wrapping_mul(31)) ^ (i >> 8)) as u8;
    }
    let pattern = Arc::new(pat);
    let config = Arc::new(config);
    let rng_counter = Arc::new(AtomicU64::new(
        SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .map_or(1, |d| d.as_nanos() as u64),
    ));

    let handle = tokio::spawn(async move {
        while let Ok((stream, _)) = listener.accept().await {
            let config = Arc::clone(&config);
            let pattern = Arc::clone(&pattern);
            let rng_counter = Arc::clone(&rng_counter);

            tokio::spawn(async move {
                let _ = stream.set_nodelay(true);
                let (read_half, mut write_half) = stream.into_split();
                let mut reader = BufReader::new(read_half);
                let mut request_line = String::new();
                if reader.read_line(&mut request_line).await.unwrap_or(0) == 0 {
                    return;
                }

                let mut range_header = None;
                let mut line = String::new();
                loop {
                    line.clear();
                    if reader.read_line(&mut line).await.unwrap_or(0) == 0 || line == "\r\n" {
                        break;
                    }
                    if let Some((name, value)) = line.split_once(':')
                        && name.trim().eq_ignore_ascii_case("range")
                    {
                        range_header = Some(value.trim().to_string());
                    }
                }

                let method = request_line.split_whitespace().next().unwrap_or("");
                if config.latency_ms > 0 {
                    tokio::time::sleep(Duration::from_millis(config.latency_ms)).await;
                }

                if method == "HEAD" {
                    if config.block_head {
                        let _ = write_half
                            .write_all(b"HTTP/1.1 405 Method Not Allowed\r\nContent-Length: 0\r\nConnection: close\r\n\r\n")
                            .await;
                    } else {
                        let accept = if config.no_range { "none" } else { "bytes" };
                        let hdr = format!(
                            "HTTP/1.1 200 OK\r\nContent-Length: {}\r\nAccept-Ranges: {accept}\r\nETag: \"bench-v1\"\r\nConnection: close\r\n\r\n",
                            config.size
                        );
                        let _ = write_half.write_all(hdr.as_bytes()).await;
                    }
                    let _ = write_half.shutdown().await;
                    return;
                }

                if should_trigger(config.error_503_pct, &rng_counter) {
                    let _ = write_half
                        .write_all(b"HTTP/1.1 503 Service Unavailable\r\nContent-Length: 0\r\nConnection: close\r\n\r\n")
                        .await;
                    let _ = write_half.shutdown().await;
                    return;
                }

                let (start, end, is_partial) = if !config.no_range
                    && let Some(ref rh) = range_header
                    && config.size > 0
                {
                    let (s, e) = parse_range(rh, config.size);
                    (s, e, true)
                } else {
                    (0, config.size.saturating_sub(1), false)
                };

                let total_to_send = if config.size == 0 { 0 } else { end - start + 1 };
                let hdr = if is_partial {
                    format!(
                        "HTTP/1.1 206 Partial Content\r\nContent-Length: {total_to_send}\r\nContent-Range: bytes {start}-{end}/{}\r\nETag: \"bench-v1\"\r\nConnection: close\r\n\r\n",
                        config.size
                    )
                } else {
                    format!(
                        "HTTP/1.1 200 OK\r\nContent-Length: {total_to_send}\r\nETag: \"bench-v1\"\r\nConnection: close\r\n\r\n"
                    )
                };

                if write_half.write_all(hdr.as_bytes()).await.is_err() {
                    return;
                }

                let drop_at = if total_to_send > 64 * 1024 && should_trigger(config.drop_pct, &rng_counter) {
                    Some(total_to_send / 3)
                } else {
                    None
                };

                let mut sent = 0u64;
                let mut cur = start;
                while sent < total_to_send {
                    let remaining = (total_to_send - sent) as usize;
                    let pat_offset = (cur as usize) % PATTERN_LEN;
                    let chunk_len = remaining.min(PATTERN_LEN - pat_offset);
                    if write_half
                        .write_all(&pattern[pat_offset..pat_offset + chunk_len])
                        .await
                        .is_err()
                    {
                        return;
                    }
                    sent += chunk_len as u64;
                    cur += chunk_len as u64;

                    if let Some(limit) = drop_at
                        && sent >= limit
                    {
                        return;
                    }

                    if let Some(mbps) = config.per_conn_mbps
                        && mbps > 0.0
                    {
                        let bytes_per_sec = (mbps * 1_000_000.0) / 8.0;
                        let delay_secs = chunk_len as f64 / bytes_per_sec;
                        if delay_secs > 0.0005 {
                            tokio::time::sleep(Duration::from_secs_f64(delay_secs)).await;
                        }
                    }
                }
                let _ = write_half.shutdown().await;
            });
        }
    });

    Ok(RunningBenchServer { addr, handle })
}

fn parse_range(header: &str, total_len: u64) -> (u64, u64) {
    let spec = header.trim().strip_prefix("bytes=").unwrap_or("0-");
    let (s, e) = spec.split_once('-').unwrap_or(("0", ""));
    let start: u64 = s.parse().unwrap_or(0).min(total_len.saturating_sub(1));
    let end: u64 = if e.is_empty() {
        total_len.saturating_sub(1)
    } else {
        e.parse::<u64>()
            .unwrap_or(total_len.saturating_sub(1))
            .min(total_len.saturating_sub(1))
    };
    (start, end.max(start))
}

fn should_trigger(pct: f64, counter: &AtomicU64) -> bool {
    if pct <= 0.0 {
        return false;
    }
    let mut x = counter.fetch_add(1, Ordering::Relaxed);
    x ^= x << 13;
    x ^= x >> 7;
    x ^= x << 17;
    let roll = (x % 10_000) as f64 / 100.0;
    roll < pct
}
