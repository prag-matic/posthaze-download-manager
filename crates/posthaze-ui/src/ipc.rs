//! Windows Named Pipe IPC bridge for browser extension Native Messaging handoff.

use std::sync::Arc;

use serde::{Deserialize, Serialize};
use tokio::runtime::Runtime;
use tokio::sync::mpsc;

/// Windows Named Pipe path listened on by `posthaze-ui`.
#[cfg(windows)]
const PIPE_NAME: &str = r"\\.\pipe\posthaze-browser-bridge";

/// Download handoff payload forwarded from `posthaze-cli native-host`.
#[derive(Debug, Clone, Deserialize, Serialize, PartialEq, Eq)]
pub struct BrowserDownloadRequest {
    #[serde(rename = "type", default = "default_req_type")]
    pub kind: String,
    #[serde(default)]
    pub url: String,
    #[serde(default)]
    pub filename: Option<String>,
    #[serde(default)]
    pub headers: Vec<(String, String)>,
}

fn default_req_type() -> String {
    "enqueue".to_string()
}

/// JSON acknowledgement returned to `posthaze-cli native-host`.
#[derive(Debug, Clone, Deserialize, Serialize, PartialEq, Eq)]
pub struct PipeResponse {
    pub ok: bool,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub mode: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub message: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub error: Option<String>,
}

impl PipeResponse {
    fn success(mode: &str, message: impl Into<String>) -> Self {
        Self {
            ok: true,
            mode: Some(mode.to_string()),
            message: Some(message.into()),
            error: None,
        }
    }

    fn failure(error: impl Into<String>) -> Self {
        Self {
            ok: false,
            mode: None,
            message: None,
            error: Some(error.into()),
        }
    }
}

/// Validates and parses one newline-delimited JSON IPC message.
pub fn parse_pipe_line(line: &str) -> Result<Option<BrowserDownloadRequest>, String> {
    let req: BrowserDownloadRequest =
        serde_json::from_str(line.trim()).map_err(|err| format!("invalid JSON: {err}"))?;
    if req.kind == "ping" {
        return Ok(None);
    }
    if !req.url.starts_with("http://") && !req.url.starts_with("https://") {
        return Err("Only http:// and https:// URLs are supported.".to_string());
    }
    Ok(Some(req))
}

/// Starts the background Named Pipe server on `runtime` and returns incoming browser requests.
#[cfg(windows)]
pub fn start_named_pipe_server(
    runtime: &Arc<Runtime>,
) -> mpsc::UnboundedReceiver<BrowserDownloadRequest> {
    use tokio::io::{AsyncBufReadExt, AsyncWriteExt, BufReader};
    use tokio::net::windows::named_pipe::ServerOptions;

    let (tx, rx) = mpsc::unbounded_channel();

    runtime.spawn(async move {
        let Ok(mut server) = ServerOptions::new()
            .first_pipe_instance(true)
            .create(PIPE_NAME)
        else {
            return;
        };

        loop {
            if server.connect().await.is_err() {
                break;
            }
            let connected = server;
            let next_server = ServerOptions::new().create(PIPE_NAME);
            let tx = tx.clone();

            tokio::spawn(async move {
                let mut reader = BufReader::new(connected);
                let mut line = String::new();
                if reader.read_line(&mut line).await.is_ok() && !line.trim().is_empty() {
                    let resp = match parse_pipe_line(&line) {
                        Ok(None) => PipeResponse::success("pong", "Posthaze UI is running"),
                        Ok(Some(req)) => {
                            let url = req.url.clone();
                            if tx.send(req).is_ok() {
                                PipeResponse::success(
                                    "preflight",
                                    format!("Sent to Posthaze UI: {url}"),
                                )
                            } else {
                                PipeResponse::failure("Posthaze UI receiver closed")
                            }
                        }
                        Err(err) => PipeResponse::failure(err),
                    };
                    if let Ok(mut bytes) = serde_json::to_vec(&resp) {
                        bytes.push(b'\n');
                        let mut pipe = reader.into_inner();
                        let _ = pipe.write_all(&bytes).await;
                        let _ = pipe.flush().await;
                    }
                }
            });

            match next_server {
                Ok(s) => server = s,
                Err(_) => break,
            }
        }
    });

    rx
}

#[cfg(not(windows))]
pub fn start_named_pipe_server(
    _runtime: &Arc<Runtime>,
) -> mpsc::UnboundedReceiver<BrowserDownloadRequest> {
    let (_tx, rx) = mpsc::unbounded_channel();
    rx
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parses_ping_and_enqueue_and_rejects_non_http() {
        assert_eq!(parse_pipe_line(r#"{"type":"ping"}"#).unwrap(), None);

        let req = parse_pipe_line(
            r#"{"type":"enqueue","url":"https://example.com/a.zip","headers":[["Cookie","a=1"]]}"#,
        )
        .unwrap()
        .unwrap();
        assert_eq!(req.url, "https://example.com/a.zip");
        assert_eq!(req.headers, vec![("Cookie".to_string(), "a=1".to_string())]);

        assert!(parse_pipe_line(r#"{"type":"enqueue","url":"ftp://example.com/a.zip"}"#).is_err());
        assert!(parse_pipe_line("not json").is_err());
    }
}
