#![allow(dead_code)]

use std::future::Future;
use std::sync::Arc;

use tokio::io::{AsyncBufReadExt, AsyncWriteExt, BufReader};
use tokio::net::TcpListener;
use tokio::net::tcp::OwnedWriteHalf;

pub struct MockResponse {
    pub status: &'static str,
    pub headers: Vec<(String, String)>,
    pub body: Vec<u8>,
}

pub async fn spawn_raw_server<F, Fut>(handler: F) -> String
where
    F: Fn(String, String, Option<String>, OwnedWriteHalf) -> Fut + Send + Sync + 'static,
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

                let mut parts = request_line.split_whitespace();
                let method = parts.next().unwrap_or("").to_string();
                let path = parts.next().unwrap_or("/").to_string();
                handler(method, path, range_header, write_half).await;
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
    spawn_raw_server(move |method, path, range_header, mut write_half| {
        let handler = Arc::clone(&handler);
        async move {
            let resp = handler(&method, &path, range_header.as_deref());
            let mut raw = format!("HTTP/1.1 {}\r\n", resp.status);
            for (k, v) in &resp.headers {
                raw.push_str(&format!("{k}: {v}\r\n"));
            }
            raw.push_str("Connection: close\r\n\r\n");

            let _ = write_half.write_all(raw.as_bytes()).await;
            if method != "HEAD" {
                let _ = write_half.write_all(&resp.body).await;
            }
            let _ = write_half.shutdown().await;
        }
    })
    .await
}

pub fn parse_byte_range(header: &str, total_len: usize) -> (usize, usize) {
    let spec = header.trim().strip_prefix("bytes=").unwrap();
    let (s, e) = spec.split_once('-').unwrap();
    let start: usize = s.parse().unwrap();
    let end: usize = if e.is_empty() {
        total_len - 1
    } else {
        e.parse::<usize>().unwrap().min(total_len - 1)
    };
    (start, end)
}
