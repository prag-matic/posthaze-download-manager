//! Human-readable, actionable diagnostics translated from engine and OS error strings.

/// User-facing diagnostic summary derived from an engine or probe failure message.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct ErrorDiagnostic {
    /// Short plain-English title suitable for headers and errata banners.
    pub headline: String,
    /// Clear explanation of what went wrong.
    pub detail: String,
    /// Optional concrete next step the user can take.
    pub tip: Option<&'static str>,
    /// Whether suggesting DevTools `Copy as cURL` is relevant.
    pub suggest_curl: bool,
}

/// Maps a raw persisted or live error string into an actionable [`ErrorDiagnostic`].
pub(crate) fn describe_error(raw: &str, is_resource_changed: bool) -> ErrorDiagnostic {
    if is_resource_changed {
        return ErrorDiagnostic {
            headline: "Remote File Changed".to_string(),
            detail: "The server updated this file since it was paused, so partial bytes can no longer be resumed safely.".to_string(),
            tip: Some("Click Restart Fresh to re-download the updated file from byte 0."),
            suggest_curl: false,
        };
    }

    let trimmed = raw.trim();
    let lower = trimmed.to_ascii_lowercase();

    if lower.contains("unexpected http status 401") || lower.contains("unexpected http status 403") {
        let code = if lower.contains("401") { "401" } else { "403" };
        return ErrorDiagnostic {
            headline: format!("Access Denied (HTTP {code})"),
            detail: "The server rejected this request. Many download hosts require fresh browser cookies, Referer headers, or a one-time link.".to_string(),
            tip: Some("Tip: Use DevTools -> Copy as cURL in your browser and paste it into Posthaze, or re-open the download link."),
            suggest_curl: true,
        };
    }

    if lower.contains("unexpected http status 404") || lower.contains("unexpected http status 410") {
        let code = if lower.contains("410") { "410" } else { "404" };
        return ErrorDiagnostic {
            headline: format!("Link Missing or Expired (HTTP {code})"),
            detail: "The file is no longer available at this URL (broken link or expired signed download token).".to_string(),
            tip: Some("Return to the source page to generate a fresh link."),
            suggest_curl: false,
        };
    }

    if lower.contains("throttled")
        || lower.contains("unexpected http status 429")
        || lower.contains("unexpected http status 503")
    {
        return ErrorDiagnostic {
            headline: "Server Busy or Rate-Limited".to_string(),
            detail: "The server asked clients to slow down or returned temporary unavailability (HTTP 429 / 503).".to_string(),
            tip: Some("Wait a moment and resume, or lower Max Workers / Speed Limit in Settings."),
            suggest_curl: false,
        };
    }

    if lower.contains("html instead of file") {
        return ErrorDiagnostic {
            headline: "Web Page Returned Instead of File".to_string(),
            detail: format!("The server responded with an HTML page (login wall, captcha, or expired link) instead of file bytes. ({trimmed})"),
            tip: Some("Tip: Download through your browser or paste a fresh DevTools Copy as cURL command."),
            suggest_curl: true,
        };
    }

    if lower.contains("insufficient disk space") {
        return ErrorDiagnostic {
            headline: "Not Enough Free Disk Space".to_string(),
            detail: trimmed.to_string(),
            tip: Some("Free up space on the target drive or choose a different destination folder."),
            suggest_curl: false,
        };
    }

    if lower.contains("exceeds filesystem limit") || lower.contains("fat32") {
        return ErrorDiagnostic {
            headline: "File Exceeds Drive Limit (FAT32)".to_string(),
            detail: "FAT32 volumes cannot store single files larger than 4 GiB - 1.".to_string(),
            tip: Some("Save this download to an NTFS or exFAT drive instead."),
            suggest_curl: false,
        };
    }

    if lower.contains("os error 206")
        || lower.contains("filename or extension is too long")
        || lower.contains("path too long")
    {
        return ErrorDiagnostic {
            headline: "Save Path Too Long for Windows".to_string(),
            detail: "The full save path (including the temporary .part suffix) exceeded Windows' path limit.".to_string(),
            tip: Some("Choose a shorter destination folder or shorten the filename."),
            suggest_curl: false,
        };
    }

    if lower.contains("certificate")
        || lower.contains("tls")
        || lower.contains("ssl")
        || lower.contains("handshake")
    {
        return ErrorDiagnostic {
            headline: "TLS / Certificate Verification Failed".to_string(),
            detail: trimmed.to_string(),
            tip: Some("Check your PC system clock, VPN/proxy settings, or antivirus HTTPS scanning."),
            suggest_curl: false,
        };
    }

    if lower.contains("timed out")
        || lower.contains("timeout")
        || lower.contains("dns")
        || lower.contains("failed to lookup")
        || lower.contains("connect")
    {
        return ErrorDiagnostic {
            headline: "Connection Interrupted or Timed Out".to_string(),
            detail: trimmed.to_string(),
            tip: Some("Check your internet or proxy connection and click Resume to continue from saved progress."),
            suggest_curl: false,
        };
    }

    ErrorDiagnostic {
        headline: "Download Failed".to_string(),
        detail: if trimmed.is_empty() {
            "Transfer failed with an unspecified error.".to_string()
        } else {
            trimmed.to_string()
        },
        tip: None,
        suggest_curl: false,
    }
}

#[cfg(test)]
mod tests {
    use super::describe_error;

    #[test]
    fn maps_common_errors_to_actionable_diagnostics() {
        let forbidden = describe_error("unexpected HTTP status 403 for https://cdn.example/a.zip", false);
        assert!(forbidden.headline.contains("403"));
        assert!(forbidden.suggest_curl);

        let html = describe_error("server returned HTML instead of file for https://x/y: title: \"Link Expired\"", false);
        assert_eq!(html.headline, "Web Page Returned Instead of File");
        assert!(html.suggest_curl);

        let path_long = describe_error("I/O error: The filename or extension is too long. (os error 206)", false);
        assert_eq!(path_long.headline, "Save Path Too Long for Windows");
        assert!(!path_long.suggest_curl);

        let fat32 = describe_error("file size (5000000000 bytes) exceeds filesystem limit (4294967295 bytes) at E:\\", false);
        assert_eq!(fat32.headline, "File Exceeds Drive Limit (FAT32)");

        let changed = describe_error("ignored", true);
        assert_eq!(changed.headline, "Remote File Changed");
    }
}
