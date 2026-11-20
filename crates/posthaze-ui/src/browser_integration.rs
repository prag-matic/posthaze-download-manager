//! Built-in Chromium Native Messaging Host manifest generation and HKCU registration.

use std::path::{Path, PathBuf};

/// Registered Chromium Native Messaging host identifier.
pub(crate) const NATIVE_HOST_NAME: &str = "com.posthaze.host";

/// Supported Chromium-based browsers on Windows for HKCU Native Messaging registration.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum BrowserKind {
    Chrome,
    Edge,
    Brave,
}

impl BrowserKind {
    pub(crate) const ALL: [Self; 3] = [Self::Chrome, Self::Edge, Self::Brave];

    pub(crate) fn label(self) -> &'static str {
        match self {
            Self::Chrome => "Google Chrome",
            Self::Edge => "Microsoft Edge",
            Self::Brave => "Brave Browser",
        }
    }

    pub(crate) fn hkcu_key(self) -> &'static str {
        match self {
            Self::Chrome => r"HKCU\Software\Google\Chrome\NativeMessagingHosts\com.posthaze.host",
            Self::Edge => r"HKCU\Software\Microsoft\Edge\NativeMessagingHosts\com.posthaze.host",
            Self::Brave => {
                r"HKCU\Software\BraveSoftware\Brave-Browser\NativeMessagingHosts\com.posthaze.host"
            }
        }
    }
}

/// Normalizes and validates a Chromium Extension ID into a `chrome-extension://<id>/` origin.
pub(crate) fn format_extension_origin(extension_id: &str) -> Result<String, String> {
    let trimmed = extension_id
        .trim()
        .trim_start_matches("chrome-extension://")
        .trim_end_matches('/');
    if trimmed.is_empty() {
        return Err("Extension ID cannot be empty.".to_string());
    }
    if !trimmed
        .chars()
        .all(|ch| ch.is_ascii_alphanumeric() || ch == '-' || ch == '_')
    {
        return Err("Extension ID contains invalid characters.".to_string());
    }
    Ok(format!("chrome-extension://{trimmed}/"))
}

/// Builds the `.cmd` shim contents invoking `posthaze-cli.exe native-host`.
pub(crate) fn build_native_host_shim(cli_exe: &Path) -> String {
    format!("@echo off\r\n\"{}\" native-host %*\r\n", cli_exe.display())
}

/// Builds the JSON manifest for Chromium Native Messaging pointing at `shim_path`.
pub(crate) fn build_native_host_manifest_json(
    shim_path: &Path,
    extension_id: &str,
) -> Result<String, String> {
    let origin = format_extension_origin(extension_id)?;
    let payload = serde_json::json!({
        "name": NATIVE_HOST_NAME,
        "description": "Posthaze Download Manager Native Messaging Host",
        "path": shim_path.to_string_lossy(),
        "type": "stdio",
        "allowed_origins": [origin],
    });
    serde_json::to_string_pretty(&payload)
        .map_err(|err| format!("Failed to serialize host manifest: {err}"))
}

/// Resolves the `posthaze-cli.exe` binary located alongside the running desktop executable.
pub(crate) fn resolve_cli_executable() -> Result<PathBuf, String> {
    let current = std::env::current_exe()
        .map_err(|err| format!("Could not resolve current executable: {err}"))?;
    let exe_dir = current
        .parent()
        .ok_or_else(|| "Executable has no parent directory.".to_string())?;
    let cli_name = if cfg!(windows) {
        "posthaze-cli.exe"
    } else {
        "posthaze-cli"
    };
    let candidate = exe_dir.join(cli_name);
    if !candidate.is_file() {
        return Err(format!(
            "{cli_name} was not found beside posthaze-ui at {}",
            candidate.display()
        ));
    }
    Ok(candidate)
}

/// Queries live HKCU Native Messaging registration status for Chrome, Edge, and Brave.
pub(crate) fn query_browser_statuses() -> Vec<(&'static str, bool)> {
    BrowserKind::ALL
        .iter()
        .map(|browser| (*&browser.label(), is_browser_registered(*browser)))
        .collect()
}

fn is_browser_registered(browser: BrowserKind) -> bool {
    #[cfg(target_os = "windows")]
    {
        use std::os::windows::process::CommandExt as _;
        const CREATE_NO_WINDOW: u32 = 0x0800_0000;
        std::process::Command::new("reg.exe")
            .creation_flags(CREATE_NO_WINDOW)
            .args(["query", browser.hkcu_key(), "/ve"])
            .output()
            .is_ok_and(|out| out.status.success())
    }
    #[cfg(not(target_os = "windows"))]
    {
        let _ = browser;
        false
    }
}

/// Writes the `.cmd` shim and `com.posthaze.host.json` manifest into `data_dir` and registers
/// HKCU Native Messaging Host keys for Chrome, Edge, and Brave.
pub(crate) fn register_native_host(
    data_dir: &Path,
    cli_exe: &Path,
    extension_id: &str,
) -> Result<PathBuf, String> {
    std::fs::create_dir_all(data_dir)
        .map_err(|err| format!("Could not create {}: {err}", data_dir.display()))?;

    let shim_path = data_dir.join("posthaze-native-host.cmd");
    let shim_body = build_native_host_shim(cli_exe);
    std::fs::write(&shim_path, shim_body)
        .map_err(|err| format!("Could not write {}: {err}", shim_path.display()))?;

    let manifest_path = data_dir.join("com.posthaze.host.json");
    let manifest_json = build_native_host_manifest_json(&shim_path, extension_id)?;
    std::fs::write(&manifest_path, manifest_json)
        .map_err(|err| format!("Could not write {}: {err}", manifest_path.display()))?;

    #[cfg(target_os = "windows")]
    {
        use std::os::windows::process::CommandExt as _;
        const CREATE_NO_WINDOW: u32 = 0x0800_0000;
        let manifest_str = manifest_path.to_string_lossy().into_owned();
        for browser in BrowserKind::ALL {
            let status = std::process::Command::new("reg.exe")
                .creation_flags(CREATE_NO_WINDOW)
                .args([
                    "add",
                    browser.hkcu_key(),
                    "/ve",
                    "/t",
                    "REG_SZ",
                    "/d",
                    &manifest_str,
                    "/f",
                ])
                .status()
                .map_err(|err| format!("Could not register {}: {err}", browser.label()))?;
            if !status.success() {
                return Err(format!(
                    "reg.exe failed for {} with status {status}",
                    browser.label()
                ));
            }
        }
    }

    Ok(manifest_path)
}

#[cfg(test)]
mod tests {
    use std::path::Path;

    use super::{
        build_native_host_manifest_json, build_native_host_shim, format_extension_origin,
    };

    #[test]
    fn formats_extension_origin_and_builds_escaped_manifest_json() {
        assert_eq!(
            format_extension_origin("abcdefghijklmnopabcdefghijklmnop").unwrap(),
            "chrome-extension://abcdefghijklmnopabcdefghijklmnop/"
        );
        assert_eq!(
            format_extension_origin("chrome-extension://abcdefghijklmnopabcdefghijklmnop/").unwrap(),
            "chrome-extension://abcdefghijklmnopabcdefghijklmnop/"
        );
        assert!(format_extension_origin("   ").is_err());

        let shim = build_native_host_shim(Path::new(r"C:\Program Files\Posthaze\posthaze-cli.exe"));
        assert!(shim.contains("\"C:\\Program Files\\Posthaze\\posthaze-cli.exe\" native-host %*"));

        let json_str = build_native_host_manifest_json(
            Path::new(r"C:\Users\Test\AppData\Local\posthaze\posthaze-native-host.cmd"),
            "abcdefghijklmnopabcdefghijklmnop",
        )
        .unwrap();
        let parsed: serde_json::Value = serde_json::from_str(&json_str).unwrap();
        assert_eq!(parsed["name"], "com.posthaze.host");
        assert_eq!(parsed["type"], "stdio");
        assert_eq!(
            parsed["path"],
            r"C:\Users\Test\AppData\Local\posthaze\posthaze-native-host.cmd"
        );
        assert_eq!(
            parsed["allowed_origins"],
            serde_json::json!(["chrome-extension://abcdefghijklmnopabcdefghijklmnop/"])
        );
    }
}
