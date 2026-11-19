//! Chrome/Edge Native Messaging host (`posthaze-cli native-host`).
//!
//! Reads length-prefixed JSON messages (4-byte little-endian `u32` + UTF-8 JSON)
//! from `stdin`, forwards download requests to a running `posthaze-ui` instance over
//! `\\.\pipe\posthaze-browser-bridge`, or falls back to enqueuing directly into
//! `posthaze-engine` when the desktop UI is closed.

use std::io::{self, Read, Write};
use std::path::PathBuf;
use std::process::ExitCode;

use posthaze_engine::{DownloadEngine, DownloadRequest};
use serde::{Deserialize, Serialize};

/// Windows Named Pipe path shared with `posthaze-ui`.
#[cfg(windows)]
const PIPE_NAME: &str = r"\\.\pipe\posthaze-browser-bridge";

/// Maximum accepted Native Messaging frame size (1 MiB, matching Chrome's limit).
const MAX_FRAME_BYTES: usize = 1024 * 1024;

#[derive(Debug, Clone, Deserialize, Serialize, PartialEq, Eq)]
pub struct NativeRequest {
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

#[derive(Debug, Clone, Deserialize, Serialize, PartialEq, Eq)]
pub struct NativeResponse {
    pub ok: bool,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub mode: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub message: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub error: Option<String>,
}

impl NativeResponse {
    fn success(mode: impl Into<String>, message: impl Into<String>) -> Self {
        Self {
            ok: true,
            mode: Some(mode.into()),
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

/// Reads one length-prefixed JSON frame from `reader`. Returns `Ok(None)` on clean EOF.
pub fn read_frame<R: Read>(reader: &mut R) -> Result<Option<Vec<u8>>, String> {
    let mut len_buf = [0_u8; 4];
    match reader.read_exact(&mut len_buf) {
        Ok(()) => {}
        Err(err) if err.kind() == io::ErrorKind::UnexpectedEof => return Ok(None),
        Err(err) => return Err(format!("failed reading frame header: {err}")),
    }

    let len = u32::from_le_bytes(len_buf) as usize;
    if len == 0 || len > MAX_FRAME_BYTES {
        return Err(format!("invalid native messaging frame length: {len}"));
    }

    let mut payload = vec![0_u8; len];
    reader
        .read_exact(&mut payload)
        .map_err(|err| format!("failed reading frame payload: {err}"))?;
    Ok(Some(payload))
}

/// Writes one length-prefixed JSON frame to `writer`.
pub fn write_frame<W: Write>(writer: &mut W, payload: &[u8]) -> Result<(), String> {
    let len = u32::try_from(payload.len())
        .map_err(|_| "native messaging response exceeds u32 length".to_string())?;
    writer
        .write_all(&len.to_le_bytes())
        .and_then(|()| writer.write_all(payload))
        .and_then(|()| writer.flush())
        .map_err(|err| format!("failed writing native messaging frame: {err}"))
}

pub async fn run_native_host(db_path: PathBuf) -> ExitCode {
    let mut stdin = io::stdin().lock();
    let mut stdout = io::stdout().lock();

    let payload = match read_frame(&mut stdin) {
        Ok(Some(bytes)) => bytes,
        Ok(None) => return ExitCode::SUCCESS,
        Err(err) => {
            let _ = send_response(&mut stdout, &NativeResponse::failure(err));
            return ExitCode::FAILURE;
        }
    };

    let req: NativeRequest = match serde_json::from_slice(&payload) {
        Ok(parsed) => parsed,
        Err(err) => {
            let _ = send_response(
                &mut stdout,
                &NativeResponse::failure(format!("invalid JSON request: {err}")),
            );
            return ExitCode::FAILURE;
        }
    };

    let resp = handle_request(req, &db_path).await;
    if let Err(err) = send_response(&mut stdout, &resp) {
        eprintln!("{err}");
        return ExitCode::FAILURE;
    }

    if resp.ok {
        ExitCode::SUCCESS
    } else {
        ExitCode::FAILURE
    }
}

fn send_response<W: Write>(writer: &mut W, resp: &NativeResponse) -> Result<(), String> {
    let bytes = serde_json::to_vec(resp).map_err(|err| format!("failed encoding JSON: {err}"))?;
    write_frame(writer, &bytes)
}

async fn handle_request(req: NativeRequest, db_path: &PathBuf) -> NativeResponse {
    if req.kind == "ping" {
        return match try_forward_to_ui(&req).await {
            Ok(resp) => resp,
            Err(_) => NativeResponse::success("host-only", "Native host ready (UI not running)"),
        };
    }

    if !req.url.starts_with("http://") && !req.url.starts_with("https://") {
        return NativeResponse::failure("Only http:// and https:// URLs are supported.");
    }

    if let Ok(resp) = try_forward_to_ui(&req).await {
        return resp;
    }

    // Fallback: stage into posthaze-engine as Paused when UI is not running.
    match DownloadEngine::open(db_path).await {
        Ok(engine) => {
            let settings = engine.get_settings().await.unwrap_or_default();
            let mut download_req =
                DownloadRequest::new(req.url.clone(), settings.default_output_dir.clone());
            if let Some(name) = req.filename.filter(|s| !s.trim().is_empty())
                && let Some(base) = std::path::Path::new(&name).file_name()
            {
                download_req.output_path = Some(settings.default_output_dir.join(base));
            }
            if !req.headers.is_empty() {
                download_req.headers = req.headers;
            }
            match engine.enqueue_paused(download_req).await {
                Ok(id) => NativeResponse::success(
                    "queued-headless",
                    format!("Staged download {} in state.db (UI closed)", id.as_str()),
                ),
                Err(err) => NativeResponse::failure(format!("engine enqueue failed: {err}")),
            }
        }
        Err(err) => NativeResponse::failure(format!("failed opening engine database: {err}")),
    }
}

#[cfg(windows)]
async fn try_forward_to_ui(req: &NativeRequest) -> Result<NativeResponse, String> {
    use tokio::io::{AsyncBufReadExt, AsyncWriteExt, BufReader};
    use tokio::net::windows::named_pipe::ClientOptions;

    let mut client = ClientOptions::new()
        .open(PIPE_NAME)
        .map_err(|err| format!("pipe connect failed: {err}"))?;

    let mut line = serde_json::to_vec(req).map_err(|err| err.to_string())?;
    line.push(b'\n');
    client
        .write_all(&line)
        .await
        .map_err(|err| format!("pipe write failed: {err}"))?;
    client
        .flush()
        .await
        .map_err(|err| format!("pipe flush failed: {err}"))?;

    let mut reader = BufReader::new(client);
    let mut resp_line = String::new();
    reader
        .read_line(&mut resp_line)
        .await
        .map_err(|err| format!("pipe read failed: {err}"))?;

    serde_json::from_str::<NativeResponse>(resp_line.trim())
        .map_err(|err| format!("invalid pipe response: {err}"))
}

#[cfg(not(windows))]
async fn try_forward_to_ui(_req: &NativeRequest) -> Result<NativeResponse, String> {
    Err("named pipe bridge is Windows-only".to_string())
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::io::Cursor;

    #[test]
    fn round_trips_length_prefixed_native_messaging_frames() {
        let req = NativeRequest {
            kind: "enqueue".to_string(),
            url: "https://example.com/archive.zip".to_string(),
            filename: Some("archive.zip".to_string()),
            headers: vec![("Referer".to_string(), "https://example.com/".to_string())],
        };
        let encoded = serde_json::to_vec(&req).unwrap();

        let mut buf = Vec::new();
        write_frame(&mut buf, &encoded).unwrap();
        assert_eq!(&buf[..4], &(encoded.len() as u32).to_le_bytes());

        let mut cursor = Cursor::new(buf);
        let decoded_bytes = read_frame(&mut cursor).unwrap().unwrap();
        let decoded: NativeRequest = serde_json::from_slice(&decoded_bytes).unwrap();
        assert_eq!(decoded, req);
        assert_eq!(read_frame(&mut cursor).unwrap(), None);
    }

    #[test]
    fn rejects_empty_or_oversized_native_frames() {
        let mut zero_cursor = Cursor::new(0_u32.to_le_bytes().to_vec());
        assert!(read_frame(&mut zero_cursor).is_err());

        let mut huge_cursor = Cursor::new(((MAX_FRAME_BYTES as u32) + 1).to_le_bytes().to_vec());
        assert!(read_frame(&mut huge_cursor).is_err());
    }
}
