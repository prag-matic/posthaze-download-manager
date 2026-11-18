use std::path::Path;

use percent_encoding::percent_decode_str;
use reqwest::header::{ACCEPT_RANGES, CONTENT_DISPOSITION, CONTENT_LENGTH, ETAG, LAST_MODIFIED};

use crate::EngineError;

/// Metadata gathered during `Probing`.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ProbeResult {
    pub final_url: String,
    pub content_length: Option<u64>,
    pub supports_ranges: bool,
    pub etag: Option<String>,
    pub last_modified: Option<String>,
    pub suggested_filename: String,
}

/// Probes `url` with a `HEAD` request to resolve redirects, metadata, and filename.
pub async fn probe_url(client: &reqwest::Client, url: &str) -> Result<ProbeResult, EngineError> {
    let resp = client.head(url).send().await?;
    let status = resp.status();
    let final_url = resp.url().to_string();

    if !status.is_success() {
        return Err(EngineError::HttpStatus {
            status: status.as_u16(),
            url: final_url,
        });
    }

    let headers = resp.headers();
    let content_length = headers
        .get(CONTENT_LENGTH)
        .and_then(|v| v.to_str().ok())
        .and_then(|s| s.trim().parse::<u64>().ok());

    let supports_ranges = headers
        .get(ACCEPT_RANGES)
        .and_then(|v| v.to_str().ok())
        .is_some_and(|s| s.trim().eq_ignore_ascii_case("bytes"));

    let etag = header_string(headers, ETAG);
    let last_modified = header_string(headers, LAST_MODIFIED);

    let cd_filename = headers
        .get(CONTENT_DISPOSITION)
        .and_then(|v| v.to_str().ok())
        .and_then(parse_content_disposition_filename);

    let suggested_filename = cd_filename
        .or_else(|| filename_from_url(resp.url()))
        .unwrap_or_else(|| "download.bin".to_string());

    Ok(ProbeResult {
        final_url,
        content_length,
        supports_ranges,
        etag,
        last_modified,
        suggested_filename,
    })
}

fn header_string(
    headers: &reqwest::header::HeaderMap,
    name: reqwest::header::HeaderName,
) -> Option<String> {
    headers
        .get(name)
        .and_then(|v| v.to_str().ok())
        .map(|s| s.trim().to_string())
        .filter(|s| !s.is_empty())
}

fn parse_content_disposition_filename(header: &str) -> Option<String> {
    let mut fallback_filename = None;

    for part in header.split(';').map(str::trim) {
        if let Some((key, value)) = part.split_once('=') {
            let key = key.trim();
            let value = value.trim();

            // RFC 5987 `filename*=UTF-8''...` takes precedence over `filename=`.
            if key.eq_ignore_ascii_case("filename*") {
                let encoded = value
                    .split("''")
                    .nth(1)
                    .unwrap_or(value)
                    .trim_matches('"');
                let decoded = percent_decode_str(encoded).decode_utf8_lossy();
                if let Some(sanitized) = sanitize_filename(&decoded) {
                    return Some(sanitized);
                }
            } else if key.eq_ignore_ascii_case("filename") && fallback_filename.is_none() {
                let unquoted = value.trim_matches('"');
                fallback_filename = sanitize_filename(unquoted);
            }
        }
    }

    fallback_filename
}

fn filename_from_url(url: &reqwest::Url) -> Option<String> {
    let raw = url.path_segments()?.rfind(|seg| !seg.is_empty())?;
    let decoded = percent_decode_str(raw).decode_utf8_lossy();
    sanitize_filename(&decoded)
}

fn sanitize_filename(raw: &str) -> Option<String> {
    // Take only the terminal path component to prevent directory traversal.
    let base = Path::new(raw).file_name()?.to_str()?.trim();
    if base.is_empty() || base == "." || base == ".." {
        return None;
    }

    // Replace Windows-illegal and control characters, then strip trailing dots/spaces.
    let cleaned: String = base
        .chars()
        .map(|c| {
            if c.is_control() || matches!(c, '<' | '>' | ':' | '"' | '/' | '\\' | '|' | '?' | '*') {
                '_'
            } else {
                c
            }
        })
        .collect();

    let trimmed = cleaned.trim_end_matches(['.', ' ']);
    if trimmed.is_empty() || trimmed == "." || trimmed == ".." {
        return None;
    }

    Some(trimmed.to_string())
}
