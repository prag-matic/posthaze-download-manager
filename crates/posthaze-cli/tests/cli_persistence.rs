use tempfile::TempDir;
use tokio::io::{AsyncReadExt, AsyncWriteExt};
use tokio::net::TcpListener;
use tokio::process::Command;

async fn spawn_simple_http_server(payload: Vec<u8>) -> String {
    let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
    let addr = listener.local_addr().unwrap();

    tokio::spawn(async move {
        while let Ok((mut stream, _)) = listener.accept().await {
            let payload = payload.clone();
            tokio::spawn(async move {
                let mut buf = vec![0u8; 4096];
                let Ok(n) = stream.read(&mut buf).await else {
                    return;
                };
                if n == 0 {
                    return;
                }
                let req = String::from_utf8_lossy(&buf[..n]);
                let method = req.lines().next().unwrap_or("").split_whitespace().next().unwrap_or("");
                if method == "HEAD" {
                    let resp = format!(
                        "HTTP/1.1 200 OK\r\nContent-Length: {}\r\nAccept-Ranges: bytes\r\nETag: \"cli-v1\"\r\nConnection: close\r\n\r\n",
                        payload.len()
                    );
                    let _ = stream.write_all(resp.as_bytes()).await;
                } else if method == "GET" {
                    let header = format!(
                        "HTTP/1.1 200 OK\r\nContent-Length: {}\r\nETag: \"cli-v1\"\r\nConnection: close\r\n\r\n",
                        payload.len()
                    );
                    let _ = stream.write_all(header.as_bytes()).await;
                    let _ = stream.write_all(&payload).await;
                }
            });
        }
    });

    format!("http://{addr}")
}

#[tokio::test]
async fn cli_download_and_list_persist_to_custom_db() {
    let payload = b"posthaze cli persistence test payload".to_vec();
    let base_url = spawn_simple_http_server(payload.clone()).await;

    let dir = TempDir::new().unwrap();
    let db_path = dir.path().join("nested").join("state.db");
    let out_path = dir.path().join("out.bin");
    let bin = env!("CARGO_BIN_EXE_posthaze-cli");

    let dl = Command::new(bin)
        .args([
            "download",
            "--db",
            db_path.to_str().unwrap(),
            "-o",
            out_path.to_str().unwrap(),
            &format!("{base_url}/out.bin"),
        ])
        .output()
        .await
        .unwrap();
    assert!(
        dl.status.success(),
        "download failed: {}",
        String::from_utf8_lossy(&dl.stderr)
    );
    assert_eq!(std::fs::read(&out_path).unwrap(), payload);

    let list = Command::new(bin)
        .args(["list", "--db", db_path.to_str().unwrap()])
        .output()
        .await
        .unwrap();
    assert!(list.status.success());
    let stdout = String::from_utf8_lossy(&list.stdout);
    assert!(stdout.contains("Completed"), "stdout was: {stdout}");
    assert!(stdout.contains("out.bin"), "stdout was: {stdout}");
}
