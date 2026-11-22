//! Signed GitHub Releases auto-updater for installed Windows builds.
//!
//! Portable builds (indicated by [`posthaze_engine::PORTABLE_MARKER_FILE`] beside the executable)
//! only check and notify with a link to the release page; installed builds can download, verify
//! (SHA-256 + Ed25519 signature), and run the Inno Setup installer silently.

use std::io::Write as _;
use std::path::{Path, PathBuf};
use std::time::Duration;

use base64::Engine as _;
use base64::engine::general_purpose::STANDARD as BASE64;
use ed25519_dalek::{Signature, Verifier as _, VerifyingKey};
use futures_util::StreamExt as _;
use posthaze_engine::PORTABLE_MARKER_FILE;
use semver::Version;
use serde::Deserialize;
use sha2::{Digest as _, Sha256};

/// GitHub Releases API endpoint for the latest stable Posthaze release.
pub(crate) const GITHUB_LATEST_RELEASE_URL: &str =
    "https://api.github.com/repos/prag-matic/posthaze-download-manager/releases/latest";

/// Trusted asset download URL prefix enforced in release builds.
const ALLOWED_ASSET_PREFIX: &str =
    "https://github.com/prag-matic/posthaze-download-manager/releases/download/";

/// Maximum accepted installer download size (200 MiB).
const MAX_INSTALLER_BYTES: u64 = 200 * 1024 * 1024;

/// Maximum accepted metadata asset size (`.sig` or `SHA256SUMS.txt`, 1 MiB).
const MAX_METADATA_BYTES: usize = 1024 * 1024;

/// Interval between background update checks when `auto_check_updates` is enabled.
pub(crate) const AUTO_CHECK_INTERVAL: Duration = Duration::from_secs(24 * 60 * 60);

/// Base64-encoded Ed25519 public keys accepted for verifying release installer signatures.
///
/// Kept as a slice so key rotation can be pre-announced by shipping a second key before retiring
/// an older one.
pub(crate) const ACCEPTED_PUBLIC_KEYS: &[&str] =
    &["q7n1Jb4hkKZofVUXFt+D8lnsWMCCwUvECB9FV+Q9Zeo="];

/// Metadata for a newer release published on GitHub Releases.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct ReleaseInfo {
    /// Parsed SemVer version of the candidate release.
    pub version: Version,
    /// Human-facing GitHub release notes URL.
    pub notes_url: String,
    /// Installer filename (e.g. `posthaze-0.2.0-windows-x64-setup.exe`).
    pub setup_name: String,
    /// Direct download URL for the Inno Setup installer.
    pub setup_url: String,
    /// Direct download URL for the Ed25519 `.sig` file.
    pub sig_url: String,
    /// Direct download URL for `SHA256SUMS.txt`.
    pub sums_url: String,
}

/// UI state machine for the auto-updater.
#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub(crate) enum UpdateState {
    #[default]
    Idle,
    Checking,
    UpToDate,
    Available(ReleaseInfo),
    Downloading {
        info: ReleaseInfo,
        pct: u8,
    },
    Installing,
    Failed(String),
}

impl UpdateState {
    /// Returns the available release info if an update is ready to inspect or install.
    pub(crate) fn available_release(&self) -> Option<&ReleaseInfo> {
        match self {
            Self::Available(info) | Self::Downloading { info, .. } => Some(info),
            _ => None,
        }
    }
}

/// Decodes [`ACCEPTED_PUBLIC_KEYS`] into Ed25519 [`VerifyingKey`]s.
pub(crate) fn embedded_verifying_keys() -> Result<Vec<VerifyingKey>, String> {
    ACCEPTED_PUBLIC_KEYS
        .iter()
        .map(|b64| decode_verifying_key(b64))
        .collect()
}

fn decode_verifying_key(b64: &str) -> Result<VerifyingKey, String> {
    let bytes = BASE64
        .decode(b64.trim())
        .map_err(|err| format!("Invalid embedded public key base64: {err}"))?;
    let arr: [u8; 32] = bytes
        .try_into()
        .map_err(|_| "Embedded public key must be 32 bytes".to_string())?;
    VerifyingKey::from_bytes(&arr)
        .map_err(|err| format!("Invalid Ed25519 public key: {err}"))
}

/// Returns `true` if `exe_dir` contains [`PORTABLE_MARKER_FILE`].
pub(crate) fn is_portable_install(exe_dir: &Path) -> bool {
    exe_dir.join(PORTABLE_MARKER_FILE).is_file()
}

/// Returns `true` if the running Posthaze executable is in portable mode.
pub(crate) fn is_current_install_portable() -> bool {
    std::env::current_exe()
        .ok()
        .and_then(|p| p.parent().map(is_portable_install))
        .unwrap_or(false)
}

/// Returns `true` when `candidate` is strictly newer than `current` (no downgrades).
pub(crate) fn is_newer(current: &str, candidate: &Version) -> bool {
    let Ok(cur) = Version::parse(current.trim().trim_start_matches('v')) else {
        return false;
    };
    candidate > &cur
}

#[derive(Debug, Deserialize)]
struct GitHubReleasePayload {
    tag_name: String,
    #[serde(default)]
    draft: bool,
    #[serde(default)]
    prerelease: bool,
    html_url: String,
    #[serde(default)]
    assets: Vec<GitHubAssetPayload>,
}

#[derive(Debug, Deserialize)]
struct GitHubAssetPayload {
    name: String,
    browser_download_url: String,
}

/// Parses a GitHub Releases API JSON response into [`ReleaseInfo`].
///
/// Returns `Ok(None)` for draft or prerelease entries. When `enforce_url_prefix` is `true`,
/// all asset URLs must start with [`ALLOWED_ASSET_PREFIX`].
pub(crate) fn parse_release(
    json: &[u8],
    enforce_url_prefix: bool,
) -> Result<Option<ReleaseInfo>, String> {
    let payload: GitHubReleasePayload =
        serde_json::from_slice(json).map_err(|err| format!("Invalid release JSON: {err}"))?;
    if payload.draft || payload.prerelease {
        return Ok(None);
    }

    let version_str = payload.tag_name.trim().trim_start_matches('v');
    let version = Version::parse(version_str)
        .map_err(|err| format!("Invalid release tag '{}': {err}", payload.tag_name))?;
    if !version.pre.is_empty() {
        return Ok(None);
    }

    let setup = payload
        .assets
        .iter()
        .find(|a| a.name.ends_with("-windows-x64-setup.exe"))
        .ok_or_else(|| "Release is missing the Windows setup installer asset".to_string())?;
    let expected_sig = format!("{}.sig", setup.name);
    let sig = payload
        .assets
        .iter()
        .find(|a| a.name == expected_sig)
        .ok_or_else(|| format!("Release is missing signature asset '{expected_sig}'"))?;
    let sums = payload
        .assets
        .iter()
        .find(|a| a.name == "SHA256SUMS.txt")
        .ok_or_else(|| "Release is missing SHA256SUMS.txt".to_string())?;

    if enforce_url_prefix {
        for url in [
            &setup.browser_download_url,
            &sig.browser_download_url,
            &sums.browser_download_url,
        ] {
            if !url.starts_with(ALLOWED_ASSET_PREFIX) {
                return Err(format!("Untrusted release asset URL: {url}"));
            }
        }
    }

    Ok(Some(ReleaseInfo {
        version,
        notes_url: payload.html_url,
        setup_name: setup.name.clone(),
        setup_url: setup.browser_download_url.clone(),
        sig_url: sig.browser_download_url.clone(),
        sums_url: sums.browser_download_url.clone(),
    }))
}

/// Verifies an Ed25519 signature over `setup_path` against any key in `keys`.
fn verify_ed25519_file(
    setup_path: &Path,
    sig_b64: &str,
    keys: &[VerifyingKey],
) -> Result<(), String> {
    if keys.is_empty() {
        return Err("No update verification keys configured".to_string());
    }
    let sig_bytes = BASE64
        .decode(sig_b64.trim())
        .map_err(|err| format!("Invalid base64 signature: {err}"))?;
    let sig_arr: [u8; 64] = sig_bytes
        .try_into()
        .map_err(|_| "Ed25519 signature must be 64 bytes".to_string())?;
    let signature = Signature::from_bytes(&sig_arr);

    let payload = std::fs::read(setup_path)
        .map_err(|err| format!("Could not read downloaded installer: {err}"))?;
    let verified = keys.iter().any(|k| k.verify(&payload, &signature).is_ok());
    if !verified {
        return Err("Ed25519 signature verification failed for downloaded installer".to_string());
    }
    Ok(())
}

/// Verifies a pre-hashed downloaded installer against `SHA256SUMS.txt` and an Ed25519 signature.
fn verify_prehashed_download(
    setup_path: &Path,
    setup_name: &str,
    actual_sha256: &str,
    sig_b64: &str,
    sums_text: &str,
    keys: &[VerifyingKey],
) -> Result<(), String> {
    if keys.is_empty() {
        return Err("No update verification keys configured".to_string());
    }
    let expected_hash = extract_expected_sha256(sums_text, setup_name)
        .ok_or_else(|| format!("SHA256SUMS.txt does not list '{setup_name}'"))?;
    if actual_sha256 != expected_hash {
        return Err(format!(
            "SHA-256 mismatch for {setup_name} (expected {expected_hash}, got {actual_sha256})"
        ));
    }
    verify_ed25519_file(setup_path, sig_b64, keys)
}

/// Verifies a downloaded installer against both `SHA256SUMS.txt` and an Ed25519 signature.
#[cfg(test)]
pub(crate) fn verify_download(
    setup_path: &Path,
    setup_name: &str,
    sig_b64: &str,
    sums_text: &str,
    keys: &[VerifyingKey],
) -> Result<(), String> {
    use std::io::Read as _;

    if keys.is_empty() {
        return Err("No update verification keys configured".to_string());
    }
    let mut file = std::fs::File::open(setup_path)
        .map_err(|err| format!("Could not read downloaded installer: {err}"))?;
    let mut hasher = Sha256::new();
    let mut buf = [0u8; 64 * 1024];
    loop {
        let n = file
            .read(&mut buf)
            .map_err(|err| format!("Could not read downloaded installer: {err}"))?;
        if n == 0 {
            break;
        }
        hasher.update(&buf[..n]);
    }
    let actual_hash = format!("{:x}", hasher.finalize());
    verify_prehashed_download(setup_path, setup_name, &actual_hash, sig_b64, sums_text, keys)
}

fn extract_expected_sha256(sums_text: &str, filename: &str) -> Option<String> {
    for line in sums_text.lines() {
        let trimmed = line.trim();
        if trimmed.is_empty() {
            continue;
        }
        let mut parts = trimmed.split_whitespace();
        let hash = parts.next()?;
        let name = parts.next()?.trim_start_matches('*');
        if name.eq_ignore_ascii_case(filename) && hash.len() == 64 {
            return Some(hash.to_ascii_lowercase());
        }
    }
    None
}

const UPDATER_USER_AGENT: &str = concat!("posthaze-updater/", env!("CARGO_PKG_VERSION"));

fn resolve_release_endpoint() -> (String, bool) {
    #[cfg(debug_assertions)]
    if let Ok(override_url) = std::env::var("POSTHAZE_UPDATE_URL") {
        let trimmed = override_url.trim();
        if !trimmed.is_empty() {
            return (trimmed.to_string(), false);
        }
    }
    (GITHUB_LATEST_RELEASE_URL.to_string(), true)
}

/// Checks GitHub Releases for a version newer than `current_version` using the shared HTTP client.
pub(crate) async fn check_latest_release(
    client: &reqwest::Client,
    current_version: &str,
) -> Result<Option<ReleaseInfo>, String> {
    let (url, enforce_prefix) = resolve_release_endpoint();
    check_release_from_url(client, &url, current_version, enforce_prefix).await
}

async fn check_release_from_url(
    client: &reqwest::Client,
    url: &str,
    current_version: &str,
    enforce_prefix: bool,
) -> Result<Option<ReleaseInfo>, String> {
    let resp = client
        .get(url)
        .header(reqwest::header::USER_AGENT, UPDATER_USER_AGENT)
        .header("Accept", "application/vnd.github+json")
        .timeout(Duration::from_secs(30))
        .send()
        .await
        .map_err(|err| format!("Update check failed: {err}"))?;
    if resp.status() == reqwest::StatusCode::NOT_FOUND {
        // GitHub's `/releases/latest` returns 404 when no non-draft, non-prerelease
        // release has been published yet. Treat that as "no newer stable release".
        return Ok(None);
    }
    if !resp.status().is_success() {
        return Err(format!("Update server returned HTTP {}", resp.status()));
    }
    let body = resp
        .bytes()
        .await
        .map_err(|err| format!("Could not read release response: {err}"))?;
    let Some(info) = parse_release(&body, enforce_prefix)? else {
        return Ok(None);
    };
    if is_newer(current_version, &info.version) {
        Ok(Some(info))
    } else {
        Ok(None)
    }
}

/// Downloads the installer, `.sig`, and `SHA256SUMS.txt` into a fresh temp directory using the
/// shared proxy-aware HTTP client, streams writes + SHA-256 hashing on a blocking thread, verifies
/// the Ed25519 signature off async workers, and returns the verified installer path.
pub(crate) async fn download_and_verify_update<F>(
    client: &reqwest::Client,
    info: &ReleaseInfo,
    keys: &[VerifyingKey],
    on_progress: F,
) -> Result<PathBuf, String>
where
    F: Fn(u8),
{
    let update_dir = std::env::temp_dir().join(format!(
        "posthaze-update-{}",
        std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap_or_default()
            .as_nanos()
    ));
    let dir_for_create = update_dir.clone();
    tokio::task::spawn_blocking(move || std::fs::create_dir_all(&dir_for_create))
        .await
        .map_err(|err| format!("Update folder task failed: {err}"))?
        .map_err(|err| format!("Could not create temp update folder: {err}"))?;
    let setup_path = update_dir.join(&info.setup_name);

    let result = async {
        // 1. Download metadata (.sig and SHA256SUMS.txt)
        let sig_text = fetch_small_text(client, &info.sig_url, "signature").await?;
        let sums_text = fetch_small_text(client, &info.sums_url, "SHA256SUMS.txt").await?;

        // 2. Stream installer to a blocking buffered writer while computing SHA-256 incrementally.
        let resp = client
            .get(&info.setup_url)
            .header(reqwest::header::USER_AGENT, UPDATER_USER_AGENT)
            .send()
            .await
            .map_err(|err| format!("Failed to download installer: {err}"))?;
        if !resp.status().is_success() {
            return Err(format!("Installer download returned HTTP {}", resp.status()));
        }
        let total = resp.content_length();
        if let Some(len) = total
            && len > MAX_INSTALLER_BYTES
        {
            return Err(format!("Installer exceeds {MAX_INSTALLER_BYTES} byte limit"));
        }

        let (chunk_tx, mut chunk_rx) = tokio::sync::mpsc::channel::<bytes::Bytes>(16);
        let writer_path = setup_path.clone();
        let writer_handle = tokio::task::spawn_blocking(move || -> Result<String, String> {
            let file = std::fs::File::create(&writer_path)
                .map_err(|err| format!("Could not create temp installer file: {err}"))?;
            let mut writer = std::io::BufWriter::with_capacity(64 * 1024, file);
            let mut hasher = Sha256::new();
            while let Some(bytes) = chunk_rx.blocking_recv() {
                hasher.update(&bytes);
                writer
                    .write_all(&bytes)
                    .map_err(|err| format!("Could not write installer bytes: {err}"))?;
            }
            writer
                .flush()
                .map_err(|err| format!("Could not flush installer file: {err}"))?;
            Ok(format!("{:x}", hasher.finalize()))
        });

        let mut downloaded: u64 = 0;
        let mut last_pct: u8 = 0;
        let mut stream = resp.bytes_stream();
        while let Some(chunk) = stream.next().await {
            let bytes = chunk.map_err(|err| format!("Download interrupted: {err}"))?;
            downloaded = downloaded.saturating_add(bytes.len() as u64);
            if downloaded > MAX_INSTALLER_BYTES {
                drop(chunk_tx);
                let _ = writer_handle.await;
                return Err(format!("Installer exceeds {MAX_INSTALLER_BYTES} byte limit"));
            }
            if chunk_tx.send(bytes).await.is_err() {
                break;
            }
            if let Some(len) = total
                && len > 0
            {
                let pct = ((downloaded.saturating_mul(100)) / len).min(100) as u8;
                if pct != last_pct {
                    last_pct = pct;
                    on_progress(pct);
                }
            }
        }
        drop(chunk_tx);
        let actual_sha256 = writer_handle
            .await
            .map_err(|err| format!("Installer writer task panicked: {err}"))??;
        on_progress(100);

        // 3. Verify incremental SHA-256 + Ed25519 signature on a blocking thread.
        let verify_path = setup_path.clone();
        let setup_name = info.setup_name.clone();
        let keys_owned = keys.to_vec();
        tokio::task::spawn_blocking(move || {
            verify_prehashed_download(
                &verify_path,
                &setup_name,
                &actual_sha256,
                &sig_text,
                &sums_text,
                &keys_owned,
            )
        })
        .await
        .map_err(|err| format!("Verification task failed: {err}"))??;
        Ok(setup_path.clone())
    }
    .await;

    if result.is_err() {
        let dir_for_cleanup = update_dir.clone();
        let _ = tokio::task::spawn_blocking(move || std::fs::remove_dir_all(&dir_for_cleanup)).await;
    }
    result
}

async fn fetch_small_text(
    client: &reqwest::Client,
    url: &str,
    label: &str,
) -> Result<String, String> {
    let resp = client
        .get(url)
        .header(reqwest::header::USER_AGENT, UPDATER_USER_AGENT)
        .timeout(Duration::from_secs(30))
        .send()
        .await
        .map_err(|err| format!("Failed to fetch {label}: {err}"))?;
    if !resp.status().is_success() {
        return Err(format!("Fetching {label} returned HTTP {}", resp.status()));
    }
    let bytes = resp
        .bytes()
        .await
        .map_err(|err| format!("Failed reading {label}: {err}"))?;
    if bytes.len() > MAX_METADATA_BYTES {
        return Err(format!("{label} exceeds maximum allowed size"));
    }
    String::from_utf8(bytes.to_vec()).map_err(|err| format!("{label} is not valid UTF-8: {err}"))
}

/// Spawns the verified Inno Setup installer silently and detached from this process.
pub(crate) fn launch_silent_installer(setup_path: &Path) -> Result<(), String> {
    let mut cmd = std::process::Command::new(setup_path);
    cmd.args(["/SILENT", "/CLOSEAPPLICATIONS", "/RESTARTAPPLICATIONS"])
        .stdin(std::process::Stdio::null())
        .stdout(std::process::Stdio::null())
        .stderr(std::process::Stdio::null());
    #[cfg(target_os = "windows")]
    {
        use std::os::windows::process::CommandExt as _;
        const DETACHED_PROCESS: u32 = 0x0000_0008;
        const CREATE_NEW_PROCESS_GROUP: u32 = 0x0000_0200;
        cmd.creation_flags(DETACHED_PROCESS | CREATE_NEW_PROCESS_GROUP);
    }
    cmd.spawn()
        .map(|_| ())
        .map_err(|err| format!("Failed to launch installer: {err}"))
}

#[cfg(test)]
mod tests {
    use super::*;
    use ed25519_dalek::{Signer as _, SigningKey};
    use tokio::io::{AsyncReadExt as _, AsyncWriteExt as _};
    use tokio::net::TcpListener;

    fn sample_release_json(tag: &str, draft: bool, prerelease: bool, base_url: &str) -> Vec<u8> {
        let ver = tag.trim_start_matches('v');
        let setup_name = format!("posthaze-{ver}-windows-x64-setup.exe");
        serde_json::json!({
            "tag_name": tag,
            "draft": draft,
            "prerelease": prerelease,
            "html_url": format!("https://github.com/prag-matic/posthaze-download-manager/releases/tag/{tag}"),
            "assets": [
                {
                    "name": setup_name,
                    "browser_download_url": format!("{base_url}/{setup_name}")
                },
                {
                    "name": format!("{setup_name}.sig"),
                    "browser_download_url": format!("{base_url}/{setup_name}.sig")
                },
                {
                    "name": "SHA256SUMS.txt",
                    "browser_download_url": format!("{base_url}/SHA256SUMS.txt")
                }
            ]
        })
        .to_string()
        .into_bytes()
    }

    #[test]
    fn embedded_public_keys_decode_cleanly() {
        let keys = embedded_verifying_keys().expect("embedded public keys must decode");
        assert!(!keys.is_empty());
    }

    #[test]
    fn is_newer_compares_semver_without_downgrades() {
        assert!(is_newer("0.1.1", &Version::new(0, 2, 0)));
        assert!(is_newer("v0.1.1", &Version::new(0, 1, 2)));
        assert!(!is_newer("0.2.0", &Version::new(0, 2, 0)));
        assert!(!is_newer("0.2.0", &Version::new(0, 1, 9)));
    }

    #[test]
    fn parse_release_skips_draft_and_prerelease_and_enforces_url_prefix() {
        let trusted = "https://github.com/prag-matic/posthaze-download-manager/releases/download/v0.2.0";
        let draft = sample_release_json("v0.2.0", true, false, trusted);
        assert_eq!(parse_release(&draft, true).unwrap(), None);

        let pre = sample_release_json("v0.2.0-rc.1", false, true, trusted);
        assert_eq!(parse_release(&pre, true).unwrap(), None);

        let untrusted = sample_release_json("v0.2.0", false, false, "https://evil.example/dl");
        assert!(parse_release(&untrusted, true).is_err());

        let valid = sample_release_json("v0.2.0", false, false, trusted);
        let parsed = parse_release(&valid, true).unwrap().unwrap();
        assert_eq!(parsed.version, Version::new(0, 2, 0));
        assert_eq!(parsed.setup_name, "posthaze-0.2.0-windows-x64-setup.exe");
    }

    #[test]
    fn verify_download_accepts_valid_signature_and_rejects_tampering_or_wrong_key() {
        let sk = SigningKey::from_bytes(&[7_u8; 32]);
        let other_sk = SigningKey::from_bytes(&[9_u8; 32]);
        let vk = sk.verifying_key();
        let other_vk = other_sk.verifying_key();

        let dir = std::env::temp_dir().join(format!("posthaze-upd-test-{}", uuid::Uuid::new_v4()));
        std::fs::create_dir_all(&dir).unwrap();
        let setup_name = "posthaze-0.2.0-windows-x64-setup.exe";
        let setup_path = dir.join(setup_name);
        let payload = b"authentic installer binary bytes";
        std::fs::write(&setup_path, payload).unwrap();

        let hash = format!("{:x}", Sha256::digest(payload));
        let sums = format!("{hash}  {setup_name}\n");
        let sig_b64 = BASE64.encode(sk.sign(payload).to_bytes());

        assert!(verify_download(&setup_path, setup_name, &sig_b64, &sums, &[vk]).is_ok());
        assert!(verify_download(&setup_path, setup_name, &sig_b64, &sums, &[other_vk]).is_err());
        assert!(
            verify_download(
                &setup_path,
                setup_name,
                &sig_b64,
                &format!("{:064x}  {setup_name}\n", 0),
                &[vk]
            )
            .is_err()
        );

        std::fs::write(&setup_path, b"tampered bytes").unwrap();
        assert!(verify_download(&setup_path, setup_name, &sig_b64, &sums, &[vk]).is_err());
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn detects_portable_marker_beside_executable() {
        let dir =
            std::env::temp_dir().join(format!("posthaze-portable-upd-{}", uuid::Uuid::new_v4()));
        std::fs::create_dir_all(&dir).unwrap();
        assert!(!is_portable_install(&dir));
        std::fs::write(dir.join(PORTABLE_MARKER_FILE), b"").unwrap();
        assert!(is_portable_install(&dir));
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[tokio::test]
    async fn download_and_verify_round_trips_against_local_http_server() {
        let sk = SigningKey::from_bytes(&[42_u8; 32]);
        let vk = sk.verifying_key();
        let payload = b"signed posthaze installer payload".to_vec();
        let hash = format!("{:x}", Sha256::digest(&payload));
        let setup_name = "posthaze-9.9.9-windows-x64-setup.exe";
        let sums = format!("{hash}  {setup_name}\n").into_bytes();
        let sig = format!("{}\n", BASE64.encode(sk.sign(&payload).to_bytes())).into_bytes();

        let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
        let addr = listener.local_addr().unwrap();
        let base = format!("http://{addr}");
        let release_json = sample_release_json("v9.9.9", false, false, &base);

        let routes = vec![
            ("/latest".to_string(), release_json),
            (format!("/{setup_name}"), payload.clone()),
            (format!("/{setup_name}.sig"), sig),
            ("/SHA256SUMS.txt".to_string(), sums),
        ];

        let server = tokio::spawn(async move {
            for _ in 0..16 {
                let Ok((mut sock, _)) = listener.accept().await else {
                    break;
                };
                let routes = routes.clone();
                tokio::spawn(async move {
                    let mut buf = [0_u8; 1024];
                    let n = sock.read(&mut buf).await.unwrap_or(0);
                    let req = String::from_utf8_lossy(&buf[..n]);
                    let path = req
                        .lines()
                        .next()
                        .and_then(|l| l.split_whitespace().nth(1))
                        .unwrap_or("/");
                    if let Some((_, body)) = routes.iter().find(|(p, _)| p == path) {
                        let header = format!(
                            "HTTP/1.1 200 OK\r\nContent-Length: {}\r\nConnection: close\r\n\r\n",
                            body.len()
                        );
                        let _ = sock.write_all(header.as_bytes()).await;
                        let _ = sock.write_all(body).await;
                    } else {
                        let _ = sock
                            .write_all(b"HTTP/1.1 404 Not Found\r\nContent-Length: 0\r\n\r\n")
                            .await;
                    }
                });
            }
        });

        let client = reqwest::Client::new();

        // GitHub returns 404 on /releases/latest when no stable release exists yet.
        let missing =
            check_release_from_url(&client, &format!("{base}/no-stable-release"), "0.1.1", false)
                .await
                .expect("HTTP 404 should resolve cleanly to Ok(None)");
        assert!(missing.is_none());

        let found = check_release_from_url(&client, &format!("{base}/latest"), "0.1.1", false)
            .await
            .unwrap()
            .expect("should detect v9.9.9");
        assert_eq!(found.version, Version::new(9, 9, 9));

        let verified_path = download_and_verify_update(&client, &found, &[vk], |_| {})
            .await
            .expect("valid signed release should verify");
        assert_eq!(std::fs::read(&verified_path).unwrap(), payload);
        if let Some(parent) = verified_path.parent() {
            let _ = std::fs::remove_dir_all(parent);
        }

        // Wrong key must reject and clean up.
        let wrong_vk = SigningKey::from_bytes(&[1_u8; 32]).verifying_key();
        assert!(
            download_and_verify_update(&client, &found, &[wrong_vk], |_| {})
                .await
                .is_err()
        );
        server.abort();
    }
}
