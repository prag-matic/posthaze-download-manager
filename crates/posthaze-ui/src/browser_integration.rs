//! Built-in Chromium Native Messaging Host manifest generation and HKCU registration.

use std::path::{Path, PathBuf};

/// Registered Chromium Native Messaging host identifier.
pub(crate) const NATIVE_HOST_NAME: &str = "com.posthaze.host";

/// Deterministic Extension ID derived from the RSA public `"key"` in `extensions/chrome/manifest.json`.
pub(crate) const DEFAULT_UNPACKED_EXTENSION_ID: &str = "eiophahfcjidkfgdddgejjglglaopahi";

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

    pub(crate) fn hkcu_subkey(self) -> &'static str {
        match self {
            Self::Chrome => r"Software\Google\Chrome\NativeMessagingHosts\com.posthaze.host",
            Self::Edge => r"Software\Microsoft\Edge\NativeMessagingHosts\com.posthaze.host",
            Self::Brave => {
                r"Software\BraveSoftware\Brave-Browser\NativeMessagingHosts\com.posthaze.host"
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
    if trimmed.len() != 32 || !trimmed.bytes().all(|b| matches!(b, b'a'..=b'p')) {
        return Err("Extension ID must be 32 lowercase letters (a-p).".to_string());
    }
    Ok(format!("chrome-extension://{trimmed}/"))
}

/// Builds the JSON manifest for Chromium Native Messaging pointing directly at `cli_exe`.
pub(crate) fn build_native_host_manifest_json(
    cli_exe: &Path,
    extension_id: &str,
) -> Result<String, String> {
    let default_origin = format!("chrome-extension://{DEFAULT_UNPACKED_EXTENSION_ID}/");
    let mut allowed_origins = Vec::new();
    if !extension_id.trim().is_empty() {
        allowed_origins.push(format_extension_origin(extension_id)?);
    }
    if !allowed_origins.contains(&default_origin) {
        allowed_origins.push(default_origin);
    }

    let payload = serde_json::json!({
        "name": NATIVE_HOST_NAME,
        "description": "Posthaze Download Manager Native Messaging Host",
        "path": cli_exe.to_string_lossy(),
        "type": "stdio",
        "allowed_origins": allowed_origins,
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
        .map(|browser| (browser.label(), is_browser_registered(*browser)))
        .collect()
}

fn is_browser_registered(browser: BrowserKind) -> bool {
    #[cfg(target_os = "windows")]
    {
        use windows_sys::Win32::System::Registry::{HKEY_CURRENT_USER, RRF_RT_REG_SZ, RegGetValueW};

        let subkey: Vec<u16> = browser
            .hkcu_subkey()
            .encode_utf16()
            .chain(std::iter::once(0))
            .collect();
        let mut size: u32 = 0;
        // SAFETY: Querying the `(Default)` `REG_SZ` value length with a null output buffer.
        let status = unsafe {
            RegGetValueW(
                HKEY_CURRENT_USER,
                subkey.as_ptr(),
                std::ptr::null(),
                RRF_RT_REG_SZ,
                std::ptr::null_mut(),
                std::ptr::null_mut(),
                &mut size,
            )
        };
        status == 0 && size > std::mem::size_of::<u16>() as u32
    }
    #[cfg(not(target_os = "windows"))]
    {
        let _ = browser;
        false
    }
}

/// Writes `com.posthaze.host.json` directly referencing `cli_exe` into `data_dir` and updates
/// HKCU Native Messaging Host keys for Chrome, Edge, and Brave (removing any legacy `.cmd` shim).
pub(crate) fn register_native_host(
    data_dir: &Path,
    cli_exe: &Path,
    extension_id: &str,
) -> Result<PathBuf, String> {
    std::fs::create_dir_all(data_dir)
        .map_err(|err| format!("Could not create {}: {err}", data_dir.display()))?;

    // Clean up legacy batch shim written by versions <= 0.2.1.
    let _ = std::fs::remove_file(data_dir.join("posthaze-native-host.cmd"));

    let manifest_path = data_dir.join("com.posthaze.host.json");
    let manifest_json = build_native_host_manifest_json(cli_exe, extension_id)?;
    std::fs::write(&manifest_path, manifest_json)
        .map_err(|err| format!("Could not write {}: {err}", manifest_path.display()))?;

    #[cfg(target_os = "windows")]
    {
        use std::os::windows::ffi::OsStrExt as _;
        use windows_sys::Win32::System::Registry::{
            HKEY, HKEY_CURRENT_USER, REG_SZ, RegCloseKey, RegCreateKeyW, RegSetValueExW,
        };

        let wide_val: Vec<u16> = manifest_path
            .as_os_str()
            .encode_wide()
            .chain(std::iter::once(0))
            .collect();
        let byte_len = u32::try_from(wide_val.len() * std::mem::size_of::<u16>())
            .map_err(|_| "Manifest path is too long".to_string())?;

        for browser in BrowserKind::ALL {
            let subkey: Vec<u16> = browser
                .hkcu_subkey()
                .encode_utf16()
                .chain(std::iter::once(0))
                .collect();
            let mut hkey: HKEY = std::ptr::null_mut();
            // SAFETY: `subkey` is null-terminated UTF-16 and `hkey` is closed before returning.
            let create_status =
                unsafe { RegCreateKeyW(HKEY_CURRENT_USER, subkey.as_ptr(), &mut hkey) };
            if create_status != 0 {
                return Err(format!(
                    "Could not create {} registry key (Win32 error {create_status})",
                    browser.label()
                ));
            }
            let set_status = unsafe {
                RegSetValueExW(
                    hkey,
                    std::ptr::null(),
                    0,
                    REG_SZ,
                    wide_val.as_ptr().cast::<u8>(),
                    byte_len,
                )
            };
            unsafe { RegCloseKey(hkey) };
            if set_status != 0 {
                return Err(format!(
                    "Could not write {} registry value (Win32 error {set_status})",
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
        DEFAULT_UNPACKED_EXTENSION_ID, build_native_host_manifest_json, format_extension_origin,
    };

    #[test]
    fn formats_extension_origin_and_builds_direct_exe_manifest_json() {
        assert_eq!(
            format_extension_origin("abcdefghijklmnopabcdefghijklmnop").unwrap(),
            "chrome-extension://abcdefghijklmnopabcdefghijklmnop/"
        );
        assert_eq!(
            format_extension_origin(&format!(
                "chrome-extension://{DEFAULT_UNPACKED_EXTENSION_ID}/"
            ))
            .unwrap(),
            format!("chrome-extension://{DEFAULT_UNPACKED_EXTENSION_ID}/")
        );
        assert!(format_extension_origin("   ").is_err());
        assert!(format_extension_origin("too-short").is_err());
        assert!(format_extension_origin("zzzzzzzzzzzzzzzzzzzzzzzzzzzzzzzz").is_err());

        let cli_path = Path::new(r"C:\Program Files\Posthaze\posthaze-cli.exe");
        let default_json = build_native_host_manifest_json(cli_path, "").unwrap();
        let default_parsed: serde_json::Value = serde_json::from_str(&default_json).unwrap();
        assert_eq!(default_parsed["name"], "com.posthaze.host");
        assert_eq!(default_parsed["type"], "stdio");
        assert_eq!(
            default_parsed["path"],
            r"C:\Program Files\Posthaze\posthaze-cli.exe"
        );
        assert_eq!(
            default_parsed["allowed_origins"],
            serde_json::json!([format!("chrome-extension://{DEFAULT_UNPACKED_EXTENSION_ID}/")])
        );

        let custom_json =
            build_native_host_manifest_json(cli_path, "abcdefghijklmnopabcdefghijklmnop").unwrap();
        let custom_parsed: serde_json::Value = serde_json::from_str(&custom_json).unwrap();
        assert_eq!(
            custom_parsed["allowed_origins"],
            serde_json::json!([
                "chrome-extension://abcdefghijklmnopabcdefghijklmnop/",
                format!("chrome-extension://{DEFAULT_UNPACKED_EXTENSION_ID}/")
            ])
        );
    }
}
