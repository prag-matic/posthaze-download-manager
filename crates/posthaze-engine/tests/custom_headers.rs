use std::sync::Arc;

use posthaze_engine::{DownloadEngine, DownloadRequest, DownloadState, EngineError};
use tempfile::tempdir;
use tokio::io::{AsyncReadExt, AsyncWriteExt};
use tokio::net::TcpListener;

async fn spawn_header_checking_server(payload: Arc<Vec<u8>>) -> String {
    let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
    let addr = listener.local_addr().unwrap();

    tokio::spawn(async move {
        loop {
            let Ok((mut socket, _)) = listener.accept().await else {
                break;
            };
            let payload = Arc::clone(&payload);
            tokio::spawn(async move {
                let mut buf = [0u8; 4096];
                let n = socket.read(&mut buf).await.unwrap_or(0);
                if n == 0 {
                    return;
                }
                let req_str = String::from_utf8_lossy(&buf[..n]).to_string();
                let lower = req_str.to_ascii_lowercase();
                let method = req_str
                    .lines()
                    .next()
                    .and_then(|l| l.split_whitespace().next())
                    .unwrap_or("GET");

                let has_cookie = lower.contains("cookie: session=ok");
                let has_referer = lower.contains("referer: https://portal.example/");
                let has_browser_ua = lower.contains("user-agent: mozilla/5.0");

                if !(has_cookie && has_referer && has_browser_ua) {
                    let resp = "HTTP/1.1 403 Forbidden\r\nContent-Length: 9\r\nConnection: close\r\n\r\nForbidden";
                    let _ = socket.write_all(resp.as_bytes()).await;
                    return;
                }

                let body_len = payload.len();
                let headers = format!(
                    "HTTP/1.1 200 OK\r\nContent-Length: {body_len}\r\nAccept-Ranges: bytes\r\nConnection: close\r\n\r\n"
                );
                let _ = socket.write_all(headers.as_bytes()).await;
                if method != "HEAD" {
                    let _ = socket.write_all(&payload).await;
                }
            });
        }
    });

    format!("http://{addr}")
}

#[tokio::test]
async fn custom_headers_are_sent_on_probe_transfer_and_persisted_across_reopen() {
    let dir = tempdir().unwrap();
    let db_path = dir.path().join("state.db");
    let payload = Arc::new(vec![0x5Au8; 8192]);
    let base_url = spawn_header_checking_server(Arc::clone(&payload)).await;
    let url = format!("{base_url}/protected.bin");

    let custom_headers = vec![
        ("Cookie".to_string(), "session=ok".to_string()),
        (
            "Referer".to_string(),
            "https://portal.example/".to_string(),
        ),
    ];

    {
        let engine = DownloadEngine::open(&db_path).await.unwrap();

        // Without custom headers, server rejects with 403.
        let err = engine.probe(&url, dir.path()).await.unwrap_err();
        assert!(matches!(err, EngineError::HttpStatus { status: 403, .. }));

        // With custom headers (plus engine's default Mozilla/5.0 User-Agent), probe & download succeed.
        let info = engine
            .probe_with_headers(&url, dir.path(), &custom_headers)
            .await
            .unwrap();
        assert_eq!(info.content_length, Some(payload.len() as u64));

        let mut req = DownloadRequest::new(&url, dir.path());
        req.headers = custom_headers.clone();
        let mut handle = engine.start(req).await.unwrap();
        let outcome = handle.wait().await.unwrap();
        assert_eq!(outcome.bytes_downloaded, payload.len() as u64);
    }

    // Reopen SQLite store and verify headers persisted on the record.
    let reopened = DownloadEngine::open(&db_path).await.unwrap();
    let records = reopened.list_downloads().await.unwrap();
    assert_eq!(records.len(), 1);
    assert_eq!(records[0].state, DownloadState::Completed);
    assert_eq!(records[0].headers, custom_headers);
}
