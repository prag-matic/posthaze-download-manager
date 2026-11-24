use std::path::Path;
use std::time::{Duration, SystemTime, UNIX_EPOCH};

use percent_encoding::percent_decode_str;
use reqwest::StatusCode;
use reqwest::header::{
    ACCEPT_RANGES, CONTENT_DISPOSITION, CONTENT_LENGTH, CONTENT_RANGE, ETAG, HeaderMap,
    HeaderName, HeaderValue, LAST_MODIFIED, RANGE, RETRY_AFTER,
};

use crate::{ChecksumAlgorithm, EngineError};

const MAX_RETRY_AFTER_SECS: u64 = 300;

/// Applies validated custom HTTP headers to `req`, skipping `Range`/`Host`/`Content-Length`
/// and conditional cache validators (`If-Modified-Since`/`If-None-Match`).
pub(crate) fn apply_custom_headers(
    mut req: reqwest::RequestBuilder,
    headers: &[(String, String)],
) -> reqwest::RequestBuilder {
    for (name, value) in headers {
        let trimmed = name.trim();
        if trimmed.eq_ignore_ascii_case("range")
            || trimmed.eq_ignore_ascii_case("host")
            || trimmed.eq_ignore_ascii_case("content-length")
            || trimmed.eq_ignore_ascii_case("if-modified-since")
            || trimmed.eq_ignore_ascii_case("if-none-match")
        {
            continue;
        }
        if let (Ok(h_name), Ok(h_val)) = (
            HeaderName::from_bytes(trimmed.as_bytes()),
            HeaderValue::from_str(value.trim()),
        ) {
            req = req.header(h_name, h_val);
        }
    }
    req
}

/// Metadata gathered during `Probing`.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ProbeResult {
    pub final_url: String,
    pub content_length: Option<u64>,
    pub supports_ranges: bool,
    pub etag: Option<String>,
    pub last_modified: Option<String>,
    pub suggested_filename: String,
    pub server_digest: Option<(ChecksumAlgorithm, String)>,
}

/// Live HTTP response stream kept open after zero-waste transfer-start probing.
#[derive(Debug)]
pub(crate) struct InitialStream {
    pub(crate) response: reqwest::Response,
}

/// Parses an HTTP `Retry-After` header (integer seconds or IMF-fixdate HTTP-date), clamped to 300 s.
pub(crate) fn parse_retry_after(headers: &HeaderMap) -> Option<Duration> {
    let raw = headers.get(RETRY_AFTER)?.to_str().ok()?.trim();
    if raw.is_empty() {
        return None;
    }
    if let Ok(secs) = raw.parse::<u64>() {
        return Some(Duration::from_secs(secs.min(MAX_RETRY_AFTER_SECS)));
    }
    let target_unix = parse_http_date_unix(raw)?;
    let now_unix = SystemTime::now().duration_since(UNIX_EPOCH).ok()?.as_secs();
    let delta = target_unix.saturating_sub(now_unix).min(MAX_RETRY_AFTER_SECS);
    Some(Duration::from_secs(delta))
}

/// Maps a non-success HTTP response into [`EngineError::Throttled`] (when `429`/`503` includes
/// a valid `Retry-After` header) or [`EngineError::HttpStatus`].
pub(crate) fn status_to_error(status: StatusCode, headers: &HeaderMap, url: String) -> EngineError {
    if matches!(
        status,
        StatusCode::TOO_MANY_REQUESTS | StatusCode::SERVICE_UNAVAILABLE
    ) && let Some(retry_after) = parse_retry_after(headers)
    {
        return EngineError::Throttled {
            status: status.as_u16(),
            retry_after,
            url,
        };
    }
    EngineError::HttpStatus {
        status: status.as_u16(),
        url,
    }
}

/// Probes `url` for a fresh transfer using a single `Range: bytes=0-` `GET` request (retrying
/// transient errors with `Retry-After` / exponential backoff) and retains the open
/// [`InitialStream`] so Worker `0` (or single-stream transfer) starts without extra RTTs.
pub(crate) async fn probe_for_transfer_start(
    client: &reqwest::Client,
    url: &str,
    headers: &[(String, String)],
) -> Result<(ProbeResult, Option<InitialStream>), EngineError> {
    const MAX_PROBE_RETRIES: u32 = 5;
    let mut retries = 0u32;

    loop {
        let attempt = probe_for_transfer_start_once(client, url, headers).await;
        match attempt {
            Ok(out) => return Ok(out),
            Err(err) => {
                let is_transient = match &err {
                    EngineError::Http(_) | EngineError::Throttled { .. } => true,
                    EngineError::HttpStatus { status, .. } => {
                        matches!(*status, 408 | 429 | 500..=599)
                    }
                    _ => false,
                };
                retries += 1;
                if !is_transient || retries > MAX_PROBE_RETRIES {
                    return Err(err);
                }
                let delay = match &err {
                    EngineError::Throttled { retry_after, .. } => *retry_after,
                    _ => Duration::from_millis(
                        100u64
                            .saturating_mul(1u64 << (retries - 1).min(10))
                            .min(5_000),
                    ),
                };
                tokio::time::sleep(delay).await;
            }
        }
    }
}

async fn probe_for_transfer_start_once(
    client: &reqwest::Client,
    url: &str,
    headers: &[(String, String)],
) -> Result<(ProbeResult, Option<InitialStream>), EngineError> {
    let resp = apply_custom_headers(client.get(url).header(RANGE, "bytes=0-"), headers)
        .send()
        .await?;
    let status = resp.status();

    if status == StatusCode::RANGE_NOT_SATISFIABLE {
        let plain_resp = apply_custom_headers(client.get(url), headers)
            .send()
            .await?;
        let plain_status = plain_resp.status();
        let plain_url = plain_resp.url().to_string();
        if !plain_status.is_success() {
            return Err(status_to_error(
                plain_status,
                plain_resp.headers(),
                plain_url,
            ));
        }
        let probe = extract_probe_from_get(&plain_resp, false);
        return Ok((
            probe,
            Some(InitialStream {
                response: plain_resp,
            }),
        ));
    }

    let final_url = resp.url().to_string();
    if !status.is_success() {
        return Err(status_to_error(status, resp.headers(), final_url));
    }

    let is_partial = status == StatusCode::PARTIAL_CONTENT;
    let probe = extract_probe_from_get(&resp, is_partial);
    Ok((probe, Some(InitialStream { response: resp })))
}

fn extract_probe_from_get(resp: &reqwest::Response, is_partial: bool) -> ProbeResult {
    let headers = resp.headers();
    let (content_length, supports_ranges) = if is_partial {
        let total = headers
            .get(CONTENT_RANGE)
            .and_then(|v| v.to_str().ok())
            .and_then(parse_content_range_total);
        (total, total.is_some())
    } else {
        (parse_content_length(headers), false)
    };

    ProbeResult {
        final_url: resp.url().to_string(),
        content_length,
        supports_ranges,
        etag: header_string(headers, ETAG),
        last_modified: header_string(headers, LAST_MODIFIED),
        suggested_filename: resolve_filename(headers, resp.url()),
        server_digest: parse_server_digest(headers),
    }
}

/// Probes `url` with optional custom HTTP request `headers`, falling back to `Range: bytes=0-0`
/// `GET` when `HEAD` is blocked or omits range/length metadata.
pub async fn probe_url_with_headers(
    client: &reqwest::Client,
    url: &str,
    headers: &[(String, String)],
) -> Result<ProbeResult, EngineError> {
    let head_resp = apply_custom_headers(client.head(url), headers)
        .send()
        .await?;
    let head_status = head_resp.status();
    let head_final_url = head_resp.url().to_string();

    if head_status.is_success() {
        let head_probe = extract_probe_from_head(&head_resp);
        if head_probe.supports_ranges && head_probe.content_length.is_some() {
            return Ok(head_probe);
        }

        // HEAD succeeded but omitted range or length metadata; verify via Range GET.
        let mut range_probe = probe_range_get(client, &head_final_url, headers).await?;
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
        if range_probe.server_digest.is_none() {
            range_probe.server_digest = head_probe.server_digest;
        }
        return Ok(range_probe);
    }

    if matches!(
        head_status,
        StatusCode::FORBIDDEN | StatusCode::METHOD_NOT_ALLOWED | StatusCode::NOT_IMPLEMENTED
    ) {
        return probe_range_get(client, &head_final_url, headers).await;
    }

    Err(status_to_error(
        head_status,
        head_resp.headers(),
        head_final_url,
    ))
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
        server_digest: parse_server_digest(headers),
    }
}

async fn probe_range_get(
    client: &reqwest::Client,
    url: &str,
    headers: &[(String, String)],
) -> Result<ProbeResult, EngineError> {
    let resp = apply_custom_headers(client.get(url).header(RANGE, "bytes=0-0"), headers)
        .send()
        .await?;
    let status = resp.status();
    let final_url = resp.url().to_string();

    if !status.is_success() {
        return Err(status_to_error(status, resp.headers(), final_url));
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
        server_digest: parse_server_digest(headers),
    })
}

/// Parses RFC 9530 `Repr-Digest` (`sha-256=:...:`, `sha-512=:...:`) or `Content-MD5`
/// into a normalized `(ChecksumAlgorithm, lowercase_hex)` pair.
pub(crate) fn parse_server_digest(headers: &HeaderMap) -> Option<(ChecksumAlgorithm, String)> {
    if let Some(repr) = headers.get("repr-digest").and_then(|v| v.to_str().ok()) {
        let mut sha512_fallback = None;
        for part in repr.split(',') {
            let Some((alg_raw, val_raw)) = part.trim().split_once('=') else {
                continue;
            };
            let alg = alg_raw.trim().to_ascii_lowercase();
            let b64 = val_raw.trim().trim_matches(':');
            let Some(bytes) = decode_base64(b64) else {
                continue;
            };
            if alg == "sha-256" && bytes.len() == 32 {
                return Some((ChecksumAlgorithm::Sha256, bytes_to_lower_hex(&bytes)));
            }
            if alg == "sha-512" && bytes.len() == 64 && sha512_fallback.is_none() {
                sha512_fallback = Some((ChecksumAlgorithm::Sha512, bytes_to_lower_hex(&bytes)));
            }
        }
        if let Some(found) = sha512_fallback {
            return Some(found);
        }
    }

    if let Some(cmd5) = headers.get("content-md5").and_then(|v| v.to_str().ok())
        && let Some(bytes) = decode_base64(cmd5.trim())
        && bytes.len() == 16
    {
        return Some((ChecksumAlgorithm::Md5, bytes_to_lower_hex(&bytes)));
    }

    None
}

fn bytes_to_lower_hex(bytes: &[u8]) -> String {
    const HEX: &[u8; 16] = b"0123456789abcdef";
    let mut out = String::with_capacity(bytes.len() * 2);
    for &b in bytes {
        out.push(HEX[(b >> 4) as usize] as char);
        out.push(HEX[(b & 0x0f) as usize] as char);
    }
    out
}

fn decode_base64(input: &str) -> Option<Vec<u8>> {
    let s = input.trim();
    if s.is_empty() || !s.len().is_multiple_of(4) {
        return None;
    }
    let mut out = Vec::with_capacity((s.len() / 4) * 3);
    let bytes = s.as_bytes();
    for chunk in bytes.as_chunks::<4>().0 {
        let mut vals = [0u8; 4];
        let mut pad = 0usize;
        for (i, &b) in chunk.iter().enumerate() {
            if b == b'=' {
                if i < 2 {
                    return None;
                }
                pad += 1;
                vals[i] = 0;
            } else if pad > 0 {
                return None;
            } else {
                vals[i] = match b {
                    b'A'..=b'Z' => b - b'A',
                    b'a'..=b'z' => b - b'a' + 26,
                    b'0'..=b'9' => b - b'0' + 52,
                    b'+' => 62,
                    b'/' => 63,
                    _ => return None,
                };
            }
        }
        out.push((vals[0] << 2) | (vals[1] >> 4));
        if pad < 2 {
            out.push((vals[1] << 4) | (vals[2] >> 2));
        }
        if pad == 0 {
            out.push((vals[2] << 6) | vals[3]);
        }
    }
    Some(out)
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

pub(crate) fn fallback_filename_from_url(raw_url: &str) -> String {
    reqwest::Url::parse(raw_url)
        .ok()
        .and_then(|u| filename_from_url(&u))
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

pub(crate) fn sanitize_filename(raw: &str) -> Option<String> {
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

    if is_windows_reserved_name(trimmed) {
        Some(format!("_{trimmed}"))
    } else {
        Some(trimmed.to_string())
    }
}

fn is_windows_reserved_name(name: &str) -> bool {
    let stem = name.split('.').next().unwrap_or(name);
    let upper = stem.to_ascii_uppercase();
    matches!(
        upper.as_str(),
        "CON"
            | "PRN"
            | "AUX"
            | "NUL"
            | "COM1"
            | "COM2"
            | "COM3"
            | "COM4"
            | "COM5"
            | "COM6"
            | "COM7"
            | "COM8"
            | "COM9"
            | "LPT1"
            | "LPT2"
            | "LPT3"
            | "LPT4"
            | "LPT5"
            | "LPT6"
            | "LPT7"
            | "LPT8"
            | "LPT9"
    )
}

fn parse_http_date_unix(s: &str) -> Option<u64> {
    // Supports IMF-fixdate (`Sun, 06 Nov 1994 08:49:37 GMT`) and RFC 850/asctime variants.
    let trimmed = s.trim();
    let after_comma = trimmed
        .split_once(',')
        .map(|(_, rest)| rest.trim())
        .unwrap_or(trimmed);
    let parts: Vec<&str> = after_comma.split_whitespace().collect();
    let (day, month_str, year, hms) = match parts.as_slice() {
        // `06 Nov 1994 08:49:37 GMT`
        [d, m, y, t, tz] if tz.eq_ignore_ascii_case("GMT") || tz.eq_ignore_ascii_case("UTC") => {
            (d.parse::<u32>().ok()?, *m, y.parse::<i64>().ok()?, *t)
        }
        // `Nov 6 08:49:37 1994`
        [m, d, t, y] => (d.parse::<u32>().ok()?, *m, y.parse::<i64>().ok()?, *t),
        // `06-Nov-94 08:49:37 GMT`
        [dmy, t, tz] if tz.eq_ignore_ascii_case("GMT") || tz.eq_ignore_ascii_case("UTC") => {
            let mut segs = dmy.split('-');
            let d = segs.next()?.parse::<u32>().ok()?;
            let m = segs.next()?;
            let y_raw = segs.next()?.parse::<i64>().ok()?;
            let y = if y_raw < 100 { 2000 + y_raw } else { y_raw };
            (d, m, y, *t)
        }
        _ => return None,
    };

    let month: u32 = match month_str.to_ascii_lowercase().as_str() {
        "jan" => 1,
        "feb" => 2,
        "mar" => 3,
        "apr" => 4,
        "may" => 5,
        "jun" => 6,
        "jul" => 7,
        "aug" => 8,
        "sep" => 9,
        "oct" => 10,
        "nov" => 11,
        "dec" => 12,
        _ => return None,
    };
    if !(1..=31).contains(&day) || year < 1970 {
        return None;
    }

    let mut t_segs = hms.split(':');
    let hour = t_segs.next()?.parse::<u64>().ok()?;
    let min = t_segs.next()?.parse::<u64>().ok()?;
    let sec = t_segs.next()?.parse::<u64>().ok()?;
    if t_segs.next().is_some() || hour >= 24 || min >= 60 || sec >= 60 {
        return None;
    }

    let days = days_from_civil(year, month, day)?;
    if days < 0 {
        return None;
    }
    Some((days as u64) * 86_400 + hour * 3_600 + min * 60 + sec)
}

fn days_from_civil(y: i64, m: u32, d: u32) -> Option<i64> {
    if !(1..=12).contains(&m) || !(1..=31).contains(&d) {
        return None;
    }
    let y = y - i64::from(m <= 2);
    let era = y.div_euclid(400);
    let yoe = y.rem_euclid(400) as u64;
    let m_adj = u64::from(m) + if m > 2 { 0 } else { 12 } - 3;
    let doy = (153 * m_adj + 2) / 5 + u64::from(d) - 1;
    let doe = yoe * 365 + yoe / 4 - yoe / 100 + doy;
    Some(era * 146_097 + (doe as i64) - 719_468)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parse_retry_after_handles_seconds_cap_and_http_date() {
        let mut headers = HeaderMap::new();
        headers.insert(RETRY_AFTER, HeaderValue::from_static("12"));
        assert_eq!(parse_retry_after(&headers), Some(Duration::from_secs(12)));

        headers.insert(RETRY_AFTER, HeaderValue::from_static("9999"));
        assert_eq!(parse_retry_after(&headers), Some(Duration::from_secs(300)));

        headers.insert(
            RETRY_AFTER,
            HeaderValue::from_static("Wed, 21 Oct 2015 07:28:00 GMT"),
        );
        assert_eq!(parse_retry_after(&headers), Some(Duration::from_secs(0)));

        headers.insert(RETRY_AFTER, HeaderValue::from_static("not-a-date"));
        assert_eq!(parse_retry_after(&headers), None);
    }

    #[test]
    fn parse_server_digest_decodes_repr_digest_and_content_md5() {
        let mut headers = HeaderMap::new();
        headers.insert(
            "repr-digest",
            HeaderValue::from_static("sha-256=:uU0nuZNNPgilLlLX2n2r+sSE7+N6U4DukIj3rOLvzek=:"),
        );
        assert_eq!(
            parse_server_digest(&headers),
            Some((
                ChecksumAlgorithm::Sha256,
                "b94d27b9934d3e08a52e52d7da7dabfac484efe37a5380ee9088f7ace2efcde9".to_string(),
            ))
        );

        let mut md5_headers = HeaderMap::new();
        md5_headers.insert(
            "content-md5",
            HeaderValue::from_static("XrY7u+Ae7tCTyyK7j1rNww=="),
        );
        assert_eq!(
            parse_server_digest(&md5_headers),
            Some((
                ChecksumAlgorithm::Md5,
                "5eb63bbbe01eeed093cb22bb8f5acdc3".to_string(),
            ))
        );
    }
}
