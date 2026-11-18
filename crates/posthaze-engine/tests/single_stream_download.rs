use std::sync::Arc;
use tempfile::TempDir;
use tokio::io::{AsyncBufReadExt, AsyncWriteExt, BufReader};
use tokio::net::TcpListener;
use tokio::sync::watch;

use posthaze_engine::{DownloadEngine, DownloadRequest, DownloadState, ProgressSnapshot};

struct MockResponse {
    status: &'static str,
    headers: Vec<(String, String)>,
    body: Vec<u8>,
}

async fn spawn_mock_server<F>(handler: F) -> String
where
    F: Fn(&str, &str) -> MockResponse + Send + Sync + 'static,
{
    let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
    let addr = listener.local_addr().unwrap();
    let handler = Arc::new(handler);

    tokio::spawn(async move {
        while let Ok((stream, _)) = listener.accept().await {
            let handler = Arc::clone(&handler);
            tokio::spawn(async move {
                let (read_half, mut write_half) = stream.into_split();
                let mut reader = BufReader::new(read_half);
                let mut request_line = String::new();
                if reader.read_line(&mut request_line).await.unwrap_or(0) == 0 {
                    return;
                }
                // Drain headers
                let mut line = String::new();
                loop {
                    line.clear();
                    if reader.read_line(&mut line).await.unwrap_or(0) == 0 || line == "\r\n" {
                        break;
                    }
                }

                let mut parts = request_line.split_whitespace();
                let method = parts.next().unwrap_or("");
                let path = parts.next().unwrap_or("/");
                let resp = handler(method, path);

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
            });
        }
    });

    format!("http://{addr}")
}

#[tokio::test]
async fn downloads_single_stream_and_verifies_blake3_and_metadata() {
    let payload: Vec<u8> = (0..512 * 1024).map(|i| (i % 251) as u8).collect();
    let expected_hash = blake3::hash(&payload);
    let payload_clone = payload.clone();

    let base_url = spawn_mock_server(move |method, path| match (method, path) {
        ("HEAD" | "GET", "/redirect") => MockResponse {
            status: "302 Found",
            headers: vec![
                ("Location".into(), "/files/actual.bin".into()),
                ("Content-Length".into(), "0".into()),
            ],
            body: Vec::new(),
        },
        ("HEAD" | "GET", "/files/actual.bin") => MockResponse {
            status: "200 OK",
            headers: vec![
                ("Content-Length".into(), payload_clone.len().to_string()),
                (
                    "Content-Disposition".into(),
                    "attachment; filename=\"resolved-name.bin\"".into(),
                ),
                ("ETag".into(), "\"abc123\"".into()),
                (
                    "Last-Modified".into(),
                    "Wed, 21 Oct 2015 07:28:00 GMT".into(),
                ),
            ],
            body: payload_clone.clone(),
        },
        _ => MockResponse {
            status: "404 Not Found",
            headers: vec![("Content-Length".into(), "0".into())],
            body: Vec::new(),
        },
    })
    .await;

    let temp_dir = TempDir::new().unwrap();
    let engine = DownloadEngine::new();
    let (tx, rx) = watch::channel(ProgressSnapshot {
        state: DownloadState::Queued,
        bytes_downloaded: 0,
        total_bytes: None,
        active_workers: 0,
    });

    let outcome = engine
        .download(
            DownloadRequest::new(format!("{base_url}/redirect"), temp_dir.path()),
            Some(tx),
        )
        .await
        .expect("download should succeed");

    assert_eq!(
        outcome.final_path,
        temp_dir.path().join("resolved-name.bin")
    );
    assert_eq!(outcome.bytes_downloaded, payload.len() as u64);
    assert_eq!(outcome.etag.as_deref(), Some("\"abc123\""));
    assert_eq!(
        outcome.last_modified.as_deref(),
        Some("Wed, 21 Oct 2015 07:28:00 GMT")
    );
    assert_eq!(outcome.final_url, format!("{base_url}/files/actual.bin"));

    let snapshot = rx.borrow().clone();
    assert_eq!(snapshot.state, DownloadState::Completed);
    assert_eq!(snapshot.bytes_downloaded, payload.len() as u64);

    let part_path = temp_dir.path().join("resolved-name.bin.part");
    assert!(!part_path.exists(), ".part file must be renamed away");

    let disk_bytes = std::fs::read(&outcome.final_path).unwrap();
    assert_eq!(blake3::hash(&disk_bytes), expected_hash);
}
