#![allow(dead_code)]

use std::future::Future;
use std::net::SocketAddr;
use std::path::Path;
use std::sync::Arc;
use std::sync::atomic::{AtomicUsize, Ordering};

use sqlx::sqlite::{SqliteConnectOptions, SqlitePoolOptions};
use tokio::io::{AsyncBufReadExt, AsyncReadExt, AsyncWriteExt, BufReader};
use tokio::net::tcp::OwnedWriteHalf;
use tokio::net::{TcpListener, TcpStream};

pub struct MockResponse {
    pub status: &'static str,
    pub headers: Vec<(String, String)>,
    pub body: Vec<u8>,
}

#[derive(Debug, Clone)]
pub struct CapturedRequest {
    pub method: String,
    pub path: String,
    pub range: Option<String>,
    pub headers: Vec<(String, String)>,
}

impl CapturedRequest {
    pub fn header(&self, name: &str) -> Option<&str> {
        self.headers
            .iter()
            .find(|(k, _)| k.eq_ignore_ascii_case(name))
            .map(|(_, v)| v.as_str())
    }
}

pub fn deterministic_payload(len: usize) -> Vec<u8> {
    (0..len)
        .map(|i| ((i.wrapping_mul(31).wrapping_add(i >> 8)) % 251) as u8)
        .collect()
}

pub fn blake3_hex(data: &[u8]) -> String {
    blake3::hash(data).to_hex().to_string()
}

pub fn file_blake3_hex(path: &Path) -> String {
    let bytes = std::fs::read(path).expect("read downloaded file for blake3");
    blake3_hex(&bytes)
}

pub fn parse_byte_range(header: &str, total_len: usize) -> (usize, usize) {
    let spec = header.trim().strip_prefix("bytes=").unwrap();
    let (s, e) = spec.split_once('-').unwrap();
    let start: usize = s.parse().unwrap();
    let end: usize = if e.is_empty() {
        total_len.saturating_sub(1)
    } else {
        e.parse::<usize>()
            .unwrap()
            .min(total_len.saturating_sub(1))
    };
    (start, end)
}

pub async fn spawn_raw_server<F, Fut>(handler: F) -> String
where
    F: Fn(CapturedRequest, OwnedWriteHalf) -> Fut + Send + Sync + 'static,
    Fut: Future<Output = ()> + Send + 'static,
{
    let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
    let addr = listener.local_addr().unwrap();
    let handler = Arc::new(handler);

    tokio::spawn(async move {
        while let Ok((stream, _)) = listener.accept().await {
            let handler = Arc::clone(&handler);
            tokio::spawn(async move {
                let (read_half, write_half) = stream.into_split();
                let mut reader = BufReader::new(read_half);
                let mut request_line = String::new();
                if reader.read_line(&mut request_line).await.unwrap_or(0) == 0 {
                    return;
                }

                let mut range_header = None;
                let mut headers = Vec::new();
                let mut line = String::new();
                loop {
                    line.clear();
                    if reader.read_line(&mut line).await.unwrap_or(0) == 0 || line == "\r\n" {
                        break;
                    }
                    if let Some((name, value)) = line.split_once(':') {
                        let name_s = name.trim().to_string();
                        let val_s = value.trim().to_string();
                        if name_s.eq_ignore_ascii_case("range") {
                            range_header = Some(val_s.clone());
                        }
                        headers.push((name_s, val_s));
                    }
                }

                let mut parts = request_line.split_whitespace();
                let method = parts.next().unwrap_or("").to_string();
                let path = parts.next().unwrap_or("/").to_string();
                let req = CapturedRequest {
                    method,
                    path,
                    range: range_header,
                    headers,
                };
                handler(req, write_half).await;
            });
        }
    });

    format!("http://{addr}")
}

pub async fn spawn_mock_server<F>(handler: F) -> String
where
    F: Fn(&str, &str, Option<&str>) -> MockResponse + Send + Sync + 'static,
{
    let handler = Arc::new(handler);
    spawn_raw_server(move |req, mut write_half| {
        let handler = Arc::clone(&handler);
        async move {
            let resp = handler(&req.method, &req.path, req.range.as_deref());
            let mut raw = format!("HTTP/1.1 {}\r\n", resp.status);
            for (k, v) in &resp.headers {
                raw.push_str(&format!("{k}: {v}\r\n"));
            }
            raw.push_str("Connection: close\r\n\r\n");

            let _ = write_half.write_all(raw.as_bytes()).await;
            if req.method != "HEAD" {
                let _ = write_half.write_all(&resp.body).await;
            }
            let _ = write_half.shutdown().await;
        }
    })
    .await
}

/// Spawns an HTTP/1.1 server that keeps TCP connections alive across multiple requests,
/// returning `(base_url, tcp_connection_count, http_request_count)`.
pub async fn spawn_keepalive_range_server(
    payload: Arc<Vec<u8>>,
) -> (String, Arc<AtomicUsize>, Arc<AtomicUsize>) {
    let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
    let addr = listener.local_addr().unwrap();
    let conn_count = Arc::new(AtomicUsize::new(0));
    let req_count = Arc::new(AtomicUsize::new(0));

    let conn_srv = Arc::clone(&conn_count);
    let req_srv = Arc::clone(&req_count);

    tokio::spawn(async move {
        while let Ok((stream, _)) = listener.accept().await {
            conn_srv.fetch_add(1, Ordering::SeqCst);
            let payload = Arc::clone(&payload);
            let req_srv = Arc::clone(&req_srv);
            tokio::spawn(async move {
                let (read_half, mut write_half) = stream.into_split();
                let mut reader = BufReader::new(read_half);
                loop {
                    let mut request_line = String::new();
                    match reader.read_line(&mut request_line).await {
                        Ok(0) | Err(_) => break,
                        Ok(_) => {}
                    }
                    if request_line.trim().is_empty() {
                        continue;
                    }
                    let mut range_header = None;
                    let mut line = String::new();
                    loop {
                        line.clear();
                        if reader.read_line(&mut line).await.unwrap_or(0) == 0 || line == "\r\n" {
                            break;
                        }
                        if let Some((k, v)) = line.split_once(':')
                            && k.trim().eq_ignore_ascii_case("range")
                        {
                            range_header = Some(v.trim().to_string());
                        }
                    }

                    req_srv.fetch_add(1, Ordering::SeqCst);
                    let mut parts = request_line.split_whitespace();
                    let method = parts.next().unwrap_or("");
                    if method == "HEAD" {
                        let hdr = format!(
                            "HTTP/1.1 200 OK\r\nContent-Length: {}\r\nAccept-Ranges: bytes\r\nConnection: keep-alive\r\n\r\n",
                            payload.len()
                        );
                        if write_half.write_all(hdr.as_bytes()).await.is_err() {
                            break;
                        }
                    } else if let Some(r) = range_header.as_deref() {
                        let (start, end) = parse_byte_range(r, payload.len());
                        let slice = &payload[start..=end];
                        let hdr = format!(
                            "HTTP/1.1 206 Partial Content\r\nContent-Length: {}\r\nContent-Range: bytes {start}-{end}/{}\r\nAccept-Ranges: bytes\r\nConnection: keep-alive\r\n\r\n",
                            slice.len(),
                            payload.len()
                        );
                        if write_half.write_all(hdr.as_bytes()).await.is_err() {
                            break;
                        }
                        if write_half.write_all(slice).await.is_err() {
                            break;
                        }
                    } else {
                        let hdr = format!(
                            "HTTP/1.1 200 OK\r\nContent-Length: {}\r\nAccept-Ranges: bytes\r\nConnection: keep-alive\r\n\r\n",
                            payload.len()
                        );
                        if write_half.write_all(hdr.as_bytes()).await.is_err() {
                            break;
                        }
                        if write_half.write_all(&payload).await.is_err() {
                            break;
                        }
                    }
                    let _ = write_half.flush().await;
                }
            });
        }
    });

    (format!("http://{addr}"), conn_count, req_count)
}

/// Spawns a lightweight HTTP forward proxy that forwards plain HTTP requests to `target_addr`
/// (stripping absolute-URI scheme/authority to origin-form) and counts proxied requests.
pub async fn spawn_http_proxy_server(target_url: &str) -> (String, Arc<AtomicUsize>) {
    let authority = target_url
        .trim_start_matches("http://")
        .trim_end_matches('/');
    let target_addr: SocketAddr = authority.parse().expect("target url socket addr");
    let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
    let proxy_addr = listener.local_addr().unwrap();
    let proxy_hits = Arc::new(AtomicUsize::new(0));
    let hits_srv = Arc::clone(&proxy_hits);

    tokio::spawn(async move {
        while let Ok((mut client_stream, _)) = listener.accept().await {
            let hits_srv = Arc::clone(&hits_srv);
            tokio::spawn(async move {
                let mut raw = Vec::new();
                let mut buf = [0u8; 4096];
                let header_end = loop {
                    match client_stream.read(&mut buf).await {
                        Ok(0) | Err(_) => return,
                        Ok(n) => {
                            raw.extend_from_slice(&buf[..n]);
                            if let Some(pos) = raw.windows(4).position(|w| w == b"\r\n\r\n") {
                                break pos + 4;
                            }
                        }
                    }
                };

                hits_srv.fetch_add(1, Ordering::SeqCst);
                let head_str = String::from_utf8_lossy(&raw[..header_end]);
                let mut lines = head_str.split("\r\n");
                let first = lines.next().unwrap_or("");
                let mut parts = first.split_whitespace();
                let method = parts.next().unwrap_or("GET");
                let uri = parts.next().unwrap_or("/");
                let version = parts.next().unwrap_or("HTTP/1.1");

                let origin_path = if let Some(rest) = uri.strip_prefix("http://") {
                    rest.find('/').map(|idx| &rest[idx..]).unwrap_or("/")
                } else {
                    uri
                };

                let mut rewritten = format!("{method} {origin_path} {version}\r\n");
                for line in lines {
                    if line.is_empty() {
                        continue;
                    }
                    let lower = line.to_ascii_lowercase();
                    if lower.starts_with("proxy-connection:") || lower.starts_with("connection:") {
                        continue;
                    }
                    rewritten.push_str(line);
                    rewritten.push_str("\r\n");
                }
                rewritten.push_str("Connection: close\r\n\r\n");

                let Ok(mut upstream) = TcpStream::connect(target_addr).await else {
                    let _ = client_stream
                        .write_all(b"HTTP/1.1 502 Bad Gateway\r\nConnection: close\r\n\r\n")
                        .await;
                    return;
                };
                if upstream.write_all(rewritten.as_bytes()).await.is_err() {
                    return;
                }
                if raw.len() > header_end && upstream.write_all(&raw[header_end..]).await.is_err() {
                    return;
                }
                let _ = tokio::io::copy(&mut upstream, &mut client_stream).await;
                let _ = client_stream.shutdown().await;
            });
        }
    });

    (format!("http://{proxy_addr}"), proxy_hits)
}

/// Creates a synthetic pre-0010 (v0009) SQLite database matching migrations `0001..0009`,
/// optionally seeding sample `downloads` and `segments` rows.
pub async fn create_pre_0010_db(db_path: &Path, seed_download_id: Option<&str>) {
    let options = SqliteConnectOptions::new()
        .filename(db_path)
        .create_if_missing(true)
        .foreign_keys(true);
    let pool = SqlitePoolOptions::new()
        .max_connections(1)
        .connect_with(options)
        .await
        .unwrap();

    let statements = [
        "CREATE TABLE IF NOT EXISTS _sqlx_migrations (
            version BIGINT PRIMARY KEY,
            description TEXT NOT NULL,
            installed_on TIMESTAMP NOT NULL DEFAULT CURRENT_TIMESTAMP,
            success BOOLEAN NOT NULL,
            checksum BLOB NOT NULL,
            execution_time BIGINT NOT NULL
        )",
        "CREATE TABLE IF NOT EXISTS downloads (
            id TEXT PRIMARY KEY NOT NULL,
            url TEXT NOT NULL,
            final_url TEXT,
            output_path TEXT,
            part_path TEXT,
            state TEXT NOT NULL,
            content_length INTEGER,
            supports_ranges INTEGER NOT NULL DEFAULT 0,
            etag TEXT,
            last_modified TEXT,
            max_workers INTEGER NOT NULL,
            sync_on_complete INTEGER NOT NULL DEFAULT 0,
            bytes_downloaded INTEGER NOT NULL DEFAULT 0,
            error_message TEXT,
            created_at INTEGER NOT NULL,
            updated_at INTEGER NOT NULL,
            output_dir TEXT,
            request_headers TEXT NOT NULL DEFAULT ''
        )",
        "CREATE TABLE IF NOT EXISTS segments (
            id INTEGER PRIMARY KEY AUTOINCREMENT,
            download_id TEXT NOT NULL REFERENCES downloads(id) ON DELETE CASCADE,
            segment_index INTEGER NOT NULL,
            start_offset INTEGER NOT NULL,
            cursor_offset INTEGER NOT NULL,
            end_offset INTEGER,
            completed INTEGER NOT NULL DEFAULT 0
        )",
        "CREATE INDEX IF NOT EXISTS idx_segments_download_id ON segments(download_id)",
        "CREATE TABLE IF NOT EXISTS settings (
            id INTEGER PRIMARY KEY CHECK (id = 1),
            default_output_dir TEXT NOT NULL,
            max_workers INTEGER NOT NULL,
            max_concurrent_downloads INTEGER NOT NULL,
            sync_on_complete INTEGER NOT NULL DEFAULT 0,
            proxy_mode TEXT NOT NULL DEFAULT 'system',
            proxy_url TEXT NOT NULL DEFAULT '',
            download_speed_limit_kib INTEGER NOT NULL DEFAULT 0,
            auto_categorize_folders INTEGER NOT NULL DEFAULT 0,
            collision_policy TEXT NOT NULL DEFAULT 'rename',
            delete_action_default TEXT NOT NULL DEFAULT 'ask',
            auto_retry_count INTEGER NOT NULL DEFAULT 0,
            completion_cmd_enabled INTEGER NOT NULL DEFAULT 0,
            completion_cmd_path TEXT NOT NULL DEFAULT '',
            completion_cmd_args TEXT NOT NULL DEFAULT '%path%',
            notify_on_added INTEGER NOT NULL DEFAULT 0,
            notify_on_completed INTEGER NOT NULL DEFAULT 1,
            notify_on_failed INTEGER NOT NULL DEFAULT 1,
            notify_only_inactive INTEGER NOT NULL DEFAULT 1,
            ui_click_row_opens_inspector INTEGER NOT NULL DEFAULT 1,
            ui_show_add_paused INTEGER NOT NULL DEFAULT 1,
            ui_zoom_percent INTEGER NOT NULL DEFAULT 100,
            ui_theme TEXT NOT NULL DEFAULT 'chronophotographic',
            ui_table_density TEXT NOT NULL DEFAULT 'standard',
            ui_show_speed_col INTEGER NOT NULL DEFAULT 1,
            ui_show_eta_col INTEGER NOT NULL DEFAULT 1,
            ui_show_segs_col INTEGER NOT NULL DEFAULT 1,
            ui_show_toolbar_labels INTEGER NOT NULL DEFAULT 1,
            ui_show_scrollbar_minimap INTEGER NOT NULL DEFAULT 1,
            ui_show_titlebar_telemetry INTEGER NOT NULL DEFAULT 1,
            ui_start_sidebar_collapsed INTEGER NOT NULL DEFAULT 0,
            ui_accent_override TEXT,
            ui_lock_file_extension INTEGER NOT NULL DEFAULT 1,
            ui_close_to_tray INTEGER NOT NULL DEFAULT 1,
            launch_on_startup INTEGER NOT NULL DEFAULT 0,
            auto_check_updates INTEGER NOT NULL DEFAULT 1
        )",
    ];
    for sql in statements {
        sqlx::query(sql).execute(&pool).await.unwrap();
    }

    let migrations = [
        (1_i64, "init"),
        (2, "settings"),
        (3, "request headers"),
        (4, "extended settings"),
        (5, "ui theme"),
        (6, "ui customization"),
        (7, "lock file extension"),
        (8, "tray and startup"),
        (9, "auto check updates"),
    ];
    for (ver, desc) in migrations {
        sqlx::query(
            "INSERT INTO _sqlx_migrations (version, description, success, checksum, execution_time)
             VALUES (?, ?, 1, zeroblob(48), 1)",
        )
        .bind(ver)
        .bind(desc)
        .execute(&pool)
        .await
        .unwrap();
    }

    sqlx::query(
        "INSERT INTO settings (id, default_output_dir, max_workers, max_concurrent_downloads, sync_on_complete)
         VALUES (1, 'C:\\Downloads', 6, 4, 1)",
    )
    .execute(&pool)
    .await
    .unwrap();

    if let Some(dl_id) = seed_download_id {
        sqlx::query(
            "INSERT INTO downloads (
                id, url, state, content_length, supports_ranges, max_workers,
                bytes_downloaded, created_at, updated_at
             ) VALUES (?, 'http://127.0.0.1/legacy.bin', 'Paused', 4096, 1, 4, 1024, 1000, 1000)",
        )
        .bind(dl_id)
        .execute(&pool)
        .await
        .unwrap();

        for (seg_idx, start, cursor, end) in [(0_i64, 0_i64, 1024_i64, 2048_i64), (1, 2048, 2048, 4096)] {
            sqlx::query(
                "INSERT INTO segments (download_id, segment_index, start_offset, cursor_offset, end_offset, completed)
                 VALUES (?, ?, ?, ?, ?, 0)",
            )
            .bind(dl_id)
            .bind(seg_idx)
            .bind(start)
            .bind(cursor)
            .bind(end)
            .execute(&pool)
            .await
            .unwrap();
        }
    }

    pool.close().await;
}
