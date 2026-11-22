//! Parser for plain URLs and Browser DevTools "Copy as cURL" commands.

/// Parsed download target and optional custom HTTP request headers.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct ParsedInput {
    pub url: String,
    pub headers: Vec<(String, String)>,
}

/// Parses either a plain URL or a `curl` command (Bash or Windows `cmd` syntax).
pub(crate) fn parse_url_or_curl(input: &str) -> ParsedInput {
    let trimmed = input.trim();
    if !is_curl_command(trimmed) {
        return ParsedInput {
            url: trimmed.to_string(),
            headers: Vec::new(),
        };
    }

    let tokens = tokenize_curl(trimmed);
    let mut url: Option<String> = None;
    let mut headers: Vec<(String, String)> = Vec::new();
    let mut i = 1; // skip "curl" / "curl.exe"

    while i < tokens.len() {
        let tok = tokens[i].as_str();
        match tok {
            "-H" | "--header" => {
                if let Some(raw_header) = tokens.get(i + 1) {
                    if let Some((name, value)) = raw_header.split_once(':') {
                        push_filtered_header(&mut headers, name, value);
                    }
                    i += 2;
                    continue;
                }
            }
            "-b" | "--cookie" => {
                if let Some(cookie) = tokens.get(i + 1) {
                    push_filtered_header(&mut headers, "Cookie", cookie);
                    i += 2;
                    continue;
                }
            }
            "-A" | "--user-agent" => {
                if let Some(ua) = tokens.get(i + 1) {
                    push_filtered_header(&mut headers, "User-Agent", ua);
                    i += 2;
                    continue;
                }
            }
            "-e" | "--referer" => {
                if let Some(referer) = tokens.get(i + 1) {
                    push_filtered_header(&mut headers, "Referer", referer);
                    i += 2;
                    continue;
                }
            }
            "--url" => {
                if let Some(u) = tokens.get(i + 1) {
                    url = Some(u.clone());
                    i += 2;
                    continue;
                }
            }
            "-X" | "--request" | "-d" | "--data" | "--data-raw" | "--data-binary" => {
                i += 2;
                continue;
            }
            _ => {
                if !tok.starts_with('-')
                    && url.is_none()
                    && (tok.starts_with("http://") || tok.starts_with("https://"))
                {
                    url = Some(tok.to_string());
                }
            }
        }
        i += 1;
    }

    ParsedInput {
        url: url.unwrap_or_else(|| trimmed.to_string()),
        headers,
    }
}

/// Formats a masked, tooltip-safe summary of custom headers (`Cookie` / `Authorization` redacted).
pub(crate) fn mask_header_summary(headers: &[(String, String)]) -> String {
    headers
        .iter()
        .map(|(name, value)| {
            let lower = name.to_ascii_lowercase();
            if matches!(
                lower.as_str(),
                "cookie" | "authorization" | "proxy-authorization" | "x-api-key"
            ) {
                format!("{name}: <redacted ({} chars)>", value.len())
            } else {
                format!("{name}: {value}")
            }
        })
        .collect::<Vec<_>>()
        .join("\n")
}

/// Extracts the current value of a header (case-insensitive).
pub(crate) fn find_header_value<'a>(headers: &'a [(String, String)], name: &str) -> &'a str {
    headers
        .iter()
        .rfind(|(k, _)| k.eq_ignore_ascii_case(name))
        .map(|(_, v)| v.as_str())
        .unwrap_or("")
}

/// Updates or removes a header (case-insensitive) in `headers`.
pub(crate) fn set_or_remove_header(headers: &mut Vec<(String, String)>, name: &str, value: &str) {
    headers.retain(|(k, _)| !k.eq_ignore_ascii_case(name));
    let trimmed = value.trim();
    if !trimmed.is_empty() {
        headers.push((name.to_string(), trimmed.to_string()));
    }
}

fn is_curl_command(trimmed: &str) -> bool {
    let lower = trimmed.to_ascii_lowercase();
    lower.starts_with("curl ") || lower.starts_with("curl.exe ")
}

fn push_filtered_header(headers: &mut Vec<(String, String)>, name: &str, value: &str) {
    let clean_name = name.trim();
    let clean_val = value.trim();
    if clean_name.is_empty() || clean_val.is_empty() {
        return;
    }
    let lower = clean_name.to_ascii_lowercase();
    if matches!(
        lower.as_str(),
        "range"
            | "host"
            | "content-length"
            | "accept-encoding"
            | "connection"
            | "if-modified-since"
            | "if-none-match"
    ) {
        return;
    }
    headers.retain(|(k, _)| !k.eq_ignore_ascii_case(clean_name));
    headers.push((clean_name.to_string(), clean_val.to_string()));
}

fn tokenize_curl(raw: &str) -> Vec<String> {
    // Normalize Bash line continuations (`\` + newline) and Windows cmd (`^` + newline).
    let normalized = raw
        .replace("\\\r\n", " ")
        .replace("\\\n", " ")
        .replace("^\r\n", " ")
        .replace("^\n", " ");

    let mut tokens = Vec::new();
    let mut current = String::new();
    let mut chars = normalized.chars();
    let mut in_single = false;
    let mut in_double = false;
    let mut token_started = false;

    while let Some(ch) = chars.next() {
        if in_single {
            if ch == '\'' {
                in_single = false;
            } else {
                current.push(ch);
            }
            continue;
        }

        if in_double {
            if ch == '"' {
                in_double = false;
            } else if ch == '\\' {
                if let Some(next) = chars.as_str().chars().next()
                    && matches!(next, '"' | '\\' | '$' | '`')
                {
                    current.push(chars.next().unwrap());
                } else {
                    current.push(ch);
                }
            } else if ch == '^' {
                let rest = chars.as_str();
                if rest.starts_with("\\^\"") {
                    // Windows cmd Copy-as-cURL escaped inner quote: `^\^"` -> `"`
                    chars.next();
                    chars.next();
                    chars.next();
                    current.push('"');
                } else if rest.starts_with("\\\"") {
                    chars.next();
                    chars.next();
                    current.push('"');
                } else if let Some(next) = chars.next() {
                    if next == '"' {
                        in_double = false;
                    } else {
                        current.push(next);
                    }
                }
            } else {
                current.push(ch);
            }
            continue;
        }

        match ch {
            '\'' => {
                in_single = true;
                token_started = true;
            }
            '"' => {
                in_double = true;
                token_started = true;
            }
            '^' => {
                token_started = true;
                if let Some(next) = chars.next() {
                    if next == '"' {
                        in_double = true;
                    } else {
                        current.push(next);
                    }
                }
            }
            '\\' => {
                token_started = true;
                if let Some(next) = chars.next() {
                    current.push(next);
                }
            }
            c if c.is_whitespace() => {
                if token_started || !current.is_empty() {
                    tokens.push(std::mem::take(&mut current));
                    token_started = false;
                }
            }
            other => {
                token_started = true;
                current.push(other);
            }
        }
    }

    if token_started || !current.is_empty() {
        tokens.push(current);
    }

    tokens
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn passes_plain_url_through_without_headers() {
        let parsed = parse_url_or_curl("  https://example.com/archive.tar.zst  ");
        assert_eq!(parsed.url, "https://example.com/archive.tar.zst");
        assert!(parsed.headers.is_empty());
    }

    #[test]
    fn parses_chrome_bash_copy_as_curl() {
        let cmd = r#"curl 'https://cdn.example.com/video.mp4' \
  -H 'referer: https://example.com/' \
  -H 'user-agent: Mozilla/5.0 TestBrowser' \
  -H 'range: bytes=0-100' \
  -b 'cf_clearance=abc123; session=xyz'"#;
        let parsed = parse_url_or_curl(cmd);
        assert_eq!(parsed.url, "https://cdn.example.com/video.mp4");
        assert_eq!(
            parsed.headers,
            vec![
                ("referer".to_string(), "https://example.com/".to_string()),
                (
                    "user-agent".to_string(),
                    "Mozilla/5.0 TestBrowser".to_string()
                ),
                (
                    "Cookie".to_string(),
                    "cf_clearance=abc123; session=xyz".to_string()
                ),
            ]
        );
    }

    #[test]
    fn parses_windows_cmd_caret_escaped_curl() {
        let cmd = r#"curl --url ^"https://cdn.example.com/file.zip^" ^
  -H ^"sec-ch-ua-platform: ^\^"Windows^\^"^" ^
  -H ^"sec-ch-ua: ^\^"Chromium^\^";v=^\^"158^\^", ^\^"Not.A/Brand^\^";v=^\^"24^\^"^" ^
  -H ^"if-modified-since: Sun, 22 Nov 2026 12:03:23 GMT^" ^
  -H ^"if-none-match: ^\^"abc123^\^"^" ^
  -H ^"User-Agent: Mozilla/5.0 ^(Windows NT 10.0; Win64; x64^) Chrome/158^" ^
  -b ^"token=hello^""#;
        let parsed = parse_url_or_curl(cmd);
        assert_eq!(parsed.url, "https://cdn.example.com/file.zip");
        assert_eq!(
            parsed.headers,
            vec![
                ("sec-ch-ua-platform".to_string(), "\"Windows\"".to_string()),
                (
                    "sec-ch-ua".to_string(),
                    "\"Chromium\";v=\"158\", \"Not.A/Brand\";v=\"24\"".to_string()
                ),
                (
                    "User-Agent".to_string(),
                    "Mozilla/5.0 (Windows NT 10.0; Win64; x64) Chrome/158".to_string()
                ),
                ("Cookie".to_string(), "token=hello".to_string()),
            ]
        );
    }

    #[test]
    fn masks_sensitive_headers_in_summary() {
        let summary = mask_header_summary(&[
            ("Cookie".to_string(), "secret=12345".to_string()),
            ("Referer".to_string(), "https://example.com".to_string()),
        ]);
        assert!(summary.contains("Cookie: <redacted (12 chars)>"));
        assert!(summary.contains("Referer: https://example.com"));
        assert!(!summary.contains("secret=12345"));
    }
}
