use std::path::Path;

use percent_encoding::percent_decode_str;
use reqwest::StatusCode;
use reqwest::header::{
    ACCEPT_RANGES, CONTENT_DISPOSITION, CONTENT_LENGTH, CONTENT_RANGE, ETAG, LAST_MODIFIED, RANGE,
};

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

/// Probes `url` with `HEAD` and falls back to `GET` (`Range: bytes=0-0`) when `HEAD` is blocked
/// (`403`/`405`/`501`) or omits range/length metadata.
pub async fn probe_url(client: &reqwest::Client, url: &str) -> Result<ProbeResult, EngineError> {
    let head_resp = client.head(url).send().await?;
    let head_status = head_resp.status();
    let head_final_url = head_resp.url().to_string();

    if head_status.is_success() {
        let head_probe = extract_probe_from_head(&head_resp);
        if head_probe.supports_ranges && head_probe.content_length.is_some() {
            return Ok(head_probe);
        }

        // HEAD succeeded but omitted range or length metadata; verify via Range GET.
        let mut range_probe = probe_range_get(client, &head_final_url).await?;
        if range_probe.etag.is_none() {
            range_probe.etag = head_probe.etag;
        }
        if range_probe.last_modified.is_none() {
            range_probe.last_modified = head_probe.last_modified;
        }
        if range_probe.content_length.is_none() {
            range_probe.content_length = head_probe.content_length;
        }
        if range_probe.suggested_filename == "download.bin" {
            range_probe.suggested_filename = head_probe.suggested_filename;
        }
        return Ok(range_probe);
    }

    if matches!(
        head_status,
        StatusCode::FORBIDDEN | StatusCode::METHOD_NOT_ALLOWED | StatusCode::NOT_IMPLEMENTED
    ) {
        return probe_range_get(client, &head_final_url).await;
    }

    Err(EngineError::HttpStatus {
        status: head_status.as_u16(),
        url: head_final_url,
    })
}

fn extract_probe_from_head(resp: &reqwest::Response) -> ProbeResult {
    let headers = resp.headers();
    let content_length = parse_content_length(headers);
    let accept_ranges_bytes = headers
        .get(ACCEPT_RANGES)
        .and_then(|v| v.to_str().ok())
        .is_some_and(|s| s.trim().eq_ignore_ascii_case("bytes"));

    let supports_ranges = accept_ranges_bytes && content_length.is_some();

    ProbeResult {
        final_url: resp.url().to_string(),
        content_length,
        supports_ranges,
        etag: header_string(headers, ETAG),
        last_modified: header_string(headers, LAST_MODIFIED),
        suggested_filename: resolve_filename(headers, resp.url()),
    }
}

async fn probe_range_get(client: &reqwest::Client, url: &str) -> Result<ProbeResult, EngineError> {
    let resp = client.get(url).header(RANGE, "bytes=0-0").send().await?;
    let status = resp.status();
    let final_url = resp.url().to_string();

    if !status.is_success() {
        return Err(EngineError::HttpStatus {
            status: status.as_u16(),
            url: final_url,
        });
    }

    let headers = resp.headers();
    let (content_length, supports_ranges) = if status == StatusCode::PARTIAL_CONTENT {
        let total = headers
            .get(CONTENT_RANGE)
            .and_then(|v| v.to_str().ok())
            .and_then(parse_content_range_total);
        (total, total.is_some())
    } else {
        (parse_content_length(headers), false)
    };

    Ok(ProbeResult {
        final_url,
        content_length,
        supports_ranges,
        etag: header_string(headers, ETAG),
        last_modified: header_string(headers, LAST_MODIFIED),
        suggested_filename: resolve_filename(headers, resp.url()),
    })
}

fn parse_content_length(headers: &reqwest::header::HeaderMap) -> Option<u64> {
    headers
        .get(CONTENT_LENGTH)
        .and_then(|v| v.to_str().ok())
        .and_then(|s| s.trim().parse::<u64>().ok())
}

fn parse_content_range_total(header: &str) -> Option<u64> {
    let (_, total_part) = header.trim().split_once('/')?;
    let total_str = total_part.trim();
    if total_str == "*" {
        return None;
    }
    total_str.parse::<u64>().ok()
}

fn resolve_filename(headers: &reqwest::header::HeaderMap, url: &reqwest::Url) -> String {
    headers
        .get(CONTENT_DISPOSITION)
        .and_then(|v| v.to_str().ok())
        .and_then(parse_content_disposition_filename)
        .or_else(|| filename_from_url(url))
        .unwrap_or_else(|| "download.bin".to_string())
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
