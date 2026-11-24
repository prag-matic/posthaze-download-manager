//! Native in-process Windows Toast notifications with AUMID registration and click activation.

use std::path::{Path, PathBuf};

use posthaze_engine::DownloadId;
use tokio::sync::mpsc;

use crate::view_model::format_bytes;

/// Persistent Windows Application User Model ID shared with installed Start Menu shortcuts.
pub(crate) const POSTHAZE_AUMID: &str = "Posthaze.DownloadManager";

#[cfg(target_os = "windows")]
const TOAST_ICON_PNG: &[u8] = include_bytes!("../../../assets/App Icon/App Icon 256x256.png");

/// High-level notification payloads emitted by the UI and queue controller.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) enum ToastRequest {
    Queued {
        id: Option<DownloadId>,
        filename: String,
        output_dir: Option<PathBuf>,
        total_bytes: Option<u64>,
    },
    Started {
        id: DownloadId,
        filename: String,
        output_path: Option<PathBuf>,
        total_bytes: Option<u64>,
    },
    Completed {
        id: DownloadId,
        filename: String,
        output_path: Option<PathBuf>,
        total_bytes: Option<u64>,
    },
    Failed {
        id: DownloadId,
        filename: String,
        reason: String,
        resumable: bool,
    },
    UpdateAvailable {
        version: String,
    },
}

/// Actions dispatched back to [`PosthazeApp`](crate::PosthazeApp) when the user clicks a toast
/// body or inline action button.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) enum ToastActivation {
    OpenApp,
    OpenDownload(DownloadId),
    OpenFile(PathBuf),
    ShowInFolder(PathBuf),
    RetryDownload(DownloadId),
    OpenUpdates,
}

#[derive(Debug, Clone)]
struct ToastContext {
    body_action: ToastActivation,
    open_file_path: Option<PathBuf>,
    show_folder_path: Option<PathBuf>,
    retry_id: Option<DownloadId>,
}

/// Handle for non-blocking background Windows Toast dispatch without spawning external processes.
#[derive(Clone)]
pub(crate) struct ToastDispatcher {
    tx: Option<mpsc::UnboundedSender<ToastRequest>>,
}

impl ToastDispatcher {
    pub(crate) fn show(&self, req: ToastRequest) {
        if let Some(tx) = &self.tx {
            let _ = tx.send(req);
        }
    }
}

/// Registers Posthaze's Windows AUMID + icon and starts the in-process WinRT Toast dispatcher
/// thread, returning a [`ToastDispatcher`] and receiver of user [`ToastActivation`] clicks.
pub(crate) fn init_toast_notifications()
-> (ToastDispatcher, mpsc::UnboundedReceiver<ToastActivation>) {
    let (act_tx, act_rx) = mpsc::unbounded_channel();

    #[cfg(target_os = "windows")]
    {
        let (req_tx, mut req_rx) = mpsc::unbounded_channel::<ToastRequest>();
        std::thread::Builder::new()
            .name("posthaze-toast-dispatcher".to_string())
            .spawn(move || {
                run_windows_toast_loop(&mut req_rx, &act_tx);
            })
            .ok();
        (ToastDispatcher { tx: Some(req_tx) }, act_rx)
    }

    #[cfg(not(target_os = "windows"))]
    {
        let _ = act_tx;
        (ToastDispatcher { tx: None }, act_rx)
    }
}

/// Formats a concise display folder label for notification subtitles.
pub(crate) fn format_folder_label(path: &Path) -> String {
    if let Some(name) = path.file_name().and_then(|n| n.to_str()).filter(|s| !s.is_empty()) {
        let full = path.display().to_string();
        if full.len() <= 36 {
            full
        } else {
            format!("...\\{name}")
        }
    } else {
        path.display().to_string()
    }
}

fn escape_xml(input: &str) -> String {
    let mut escaped = String::with_capacity(input.len());
    for ch in input.chars() {
        match ch {
            '&' => escaped.push_str("&amp;"),
            '<' => escaped.push_str("&lt;"),
            '>' => escaped.push_str("&gt;"),
            '"' => escaped.push_str("&quot;"),
            '\'' => escaped.push_str("&apos;"),
            other => escaped.push(other),
        }
    }
    escaped
}

fn toast_display_name(filename: &str) -> &str {
    if filename.trim().is_empty() {
        "Download"
    } else {
        filename
    }
}

fn format_transfer_detail(
    total_bytes: Option<u64>,
    folder_path: Option<&Path>,
    folder_prefix: &str,
    size_only_suffix: &str,
    empty_fallback: &str,
) -> String {
    let size_label = total_bytes.filter(|b| *b > 0).map(format_bytes);
    let folder_label = folder_path.map(format_folder_label);
    match (size_label, folder_label) {
        (Some(sz), Some(dir)) => format!("{sz} · {folder_prefix} {dir}"),
        (Some(sz), None) => format!("{sz} · {size_only_suffix}"),
        (None, Some(dir)) => format!("{folder_prefix} {dir}"),
        (None, None) => empty_fallback.to_string(),
    }
}

fn build_toast_parts(req: &ToastRequest) -> (String, ToastContext) {
    match req {
        ToastRequest::Queued {
            id,
            filename,
            output_dir,
            total_bytes,
        } => {
            let name = toast_display_name(filename);
            let title = match total_bytes.filter(|b| *b > 0) {
                Some(bytes) => format!("Download Queued · {}", format_bytes(bytes)),
                None => "Download Queued".to_string(),
            };
            let detail = match output_dir {
                Some(dir) => format!("Saving to {}", format_folder_label(dir)),
                None => "Click to view transfer in Posthaze.".to_string(),
            };
            let xml = format!(
                "<toast launch=\"open\">\
                    <visual><binding template=\"ToastGeneric\">\
                        <text>{}</text>\
                        <text>{}</text>\
                        <text>{}</text>\
                    </binding></visual>\
                 </toast>",
                escape_xml(&title),
                escape_xml(name),
                escape_xml(&detail),
            );
            let ctx = ToastContext {
                body_action: id
                    .clone()
                    .map_or(ToastActivation::OpenApp, ToastActivation::OpenDownload),
                open_file_path: None,
                show_folder_path: None,
                retry_id: None,
            };
            (xml, ctx)
        }
        ToastRequest::Started {
            id,
            filename,
            output_path,
            total_bytes,
        } => {
            let name = toast_display_name(filename);
            let detail = format_transfer_detail(
                *total_bytes,
                output_path.as_deref().and_then(|p| p.parent()),
                "Saving to",
                "Transfer started",
                "Transfer started · Click to view",
            );
            let xml = format!(
                "<toast launch=\"open\">\
                    <visual><binding template=\"ToastGeneric\">\
                        <text>Download Started</text>\
                        <text>{}</text>\
                        <text>{}</text>\
                    </binding></visual>\
                 </toast>",
                escape_xml(name),
                escape_xml(&detail),
            );
            let ctx = ToastContext {
                body_action: ToastActivation::OpenDownload(id.clone()),
                open_file_path: None,
                show_folder_path: output_path.clone(),
                retry_id: None,
            };
            (xml, ctx)
        }
        ToastRequest::Completed {
            id,
            filename,
            output_path,
            total_bytes,
        } => {
            let name = toast_display_name(filename);
            let detail = format_transfer_detail(
                *total_bytes,
                output_path.as_deref().and_then(|p| p.parent()),
                "Saved in",
                "Transfer complete",
                "Transfer complete · Click to view",
            );
            let actions_xml = if output_path.is_some() {
                "<actions>\
                    <action content=\"Open File\" arguments=\"open_file\" activationType=\"foreground\"/>\
                    <action content=\"Show in Folder\" arguments=\"show_folder\" activationType=\"foreground\"/>\
                 </actions>"
            } else {
                ""
            };
            let xml = format!(
                "<toast launch=\"open\">\
                    <visual><binding template=\"ToastGeneric\">\
                        <text>Download Complete</text>\
                        <text>{}</text>\
                        <text>{}</text>\
                    </binding></visual>\
                    {actions_xml}\
                 </toast>",
                escape_xml(name),
                escape_xml(&detail),
            );
            let ctx = ToastContext {
                body_action: ToastActivation::OpenDownload(id.clone()),
                open_file_path: output_path.clone(),
                show_folder_path: output_path.clone(),
                retry_id: None,
            };
            (xml, ctx)
        }
        ToastRequest::Failed {
            id,
            filename,
            reason,
            resumable,
        } => {
            let name = if filename.trim().is_empty() {
                "Download"
            } else {
                filename.as_str()
            };
            let detail = if reason.trim().is_empty() {
                "Transfer stopped with an error. Click to inspect details."
            } else {
                reason.trim()
            };
            let actions_xml = if *resumable {
                "<actions>\
                    <action content=\"Retry\" arguments=\"retry\" activationType=\"foreground\"/>\
                    <action content=\"Inspect Error\" arguments=\"open\" activationType=\"foreground\"/>\
                 </actions>"
            } else {
                "<actions>\
                    <action content=\"Inspect Error\" arguments=\"open\" activationType=\"foreground\"/>\
                 </actions>"
            };
            let xml = format!(
                "<toast launch=\"open\">\
                    <visual><binding template=\"ToastGeneric\">\
                        <text>Download Failed</text>\
                        <text>{}</text>\
                        <text>{}</text>\
                    </binding></visual>\
                    {actions_xml}\
                 </toast>",
                escape_xml(name),
                escape_xml(detail),
            );
            let ctx = ToastContext {
                body_action: ToastActivation::OpenDownload(id.clone()),
                open_file_path: None,
                show_folder_path: None,
                retry_id: if *resumable { Some(id.clone()) } else { None },
            };
            (xml, ctx)
        }
        ToastRequest::UpdateAvailable { version } => {
            let xml = format!(
                "<toast launch=\"open_updates\">\
                    <visual><binding template=\"ToastGeneric\">\
                        <text>Update Available · v{}</text>\
                        <text>A signed Posthaze update is ready.</text>\
                        <text>Click to view release notes or install.</text>\
                    </binding></visual>\
                 </toast>",
                escape_xml(version),
            );
            let ctx = ToastContext {
                body_action: ToastActivation::OpenUpdates,
                open_file_path: None,
                show_folder_path: None,
                retry_id: None,
            };
            (xml, ctx)
        }
    }
}

fn resolve_toast_activation(args: &str, ctx: &ToastContext) -> ToastActivation {
    match args.trim() {
        "open_file" => ctx
            .open_file_path
            .clone()
            .map_or_else(|| ctx.body_action.clone(), ToastActivation::OpenFile),
        "show_folder" => ctx
            .show_folder_path
            .clone()
            .map_or_else(|| ctx.body_action.clone(), ToastActivation::ShowInFolder),
        "retry" => ctx
            .retry_id
            .clone()
            .map_or_else(|| ctx.body_action.clone(), ToastActivation::RetryDownload),
        "open_updates" => ToastActivation::OpenUpdates,
        _ => ctx.body_action.clone(),
    }
}

#[cfg(target_os = "windows")]
fn ensure_toast_icon_file() -> Option<PathBuf> {
    let data_dir = posthaze_engine::default_data_dir();
    if std::fs::create_dir_all(&data_dir).is_err() {
        return None;
    }
    let icon_path = data_dir.join("toast-icon.png");
    let up_to_date = std::fs::metadata(&icon_path)
        .map(|m| m.len() == TOAST_ICON_PNG.len() as u64)
        .unwrap_or(false);
    if !up_to_date && let Err(err) = std::fs::write(&icon_path, TOAST_ICON_PNG) {
        tracing::debug!("failed writing toast icon file {}: {err}", icon_path.display());
        return None;
    }
    Some(icon_path)
}

#[cfg(target_os = "windows")]
fn ensure_windows_aumid_registration() {
    use std::os::windows::ffi::OsStrExt as _;
    use windows::Win32::UI::Shell::SetCurrentProcessExplicitAppUserModelID;
    use windows::core::HSTRING;
    use windows_sys::Win32::System::Registry::{
        HKEY, HKEY_CURRENT_USER, REG_SZ, RegCloseKey, RegCreateKeyW, RegSetValueExW,
    };

    // SAFETY: Setting the process AUMID once with a static identifier is thread-safe.
    unsafe {
        if let Err(err) = SetCurrentProcessExplicitAppUserModelID(&HSTRING::from(POSTHAZE_AUMID)) {
            tracing::debug!("SetCurrentProcessExplicitAppUserModelID failed: {err}");
        }
    }

    let subkey: Vec<u16> = format!("Software\\Classes\\AppUserModelId\\{POSTHAZE_AUMID}")
        .encode_utf16()
        .chain(std::iter::once(0))
        .collect();
    let mut hkey: HKEY = std::ptr::null_mut();
    // SAFETY: `subkey` is null-terminated UTF-16 and `hkey` is closed before returning.
    let status = unsafe { RegCreateKeyW(HKEY_CURRENT_USER, subkey.as_ptr(), &mut hkey) };
    if status != 0 {
        tracing::debug!("RegCreateKeyW failed for toast AUMID (Win32 error {status})");
        return;
    }

    let write_reg_sz = |name: &str, value: &std::ffi::OsStr| {
        let wname: Vec<u16> = name.encode_utf16().chain(std::iter::once(0)).collect();
        let wval: Vec<u16> = value.encode_wide().chain(std::iter::once(0)).collect();
        let byte_len = u32::try_from(wval.len() * std::mem::size_of::<u16>()).unwrap_or(u32::MAX);
        // SAFETY: Both UTF-16 buffers remain alive and null-terminated for the duration of the call.
        unsafe {
            RegSetValueExW(
                hkey,
                wname.as_ptr(),
                0,
                REG_SZ,
                wval.as_ptr().cast::<u8>(),
                byte_len,
            );
        }
    };

    write_reg_sz("DisplayName", std::ffi::OsStr::new("Posthaze"));
    if let Some(icon_path) = ensure_toast_icon_file() {
        write_reg_sz("IconUri", icon_path.as_os_str());
    }

    // SAFETY: `hkey` was opened by `RegCreateKeyW`.
    unsafe {
        RegCloseKey(hkey);
    }
}

#[cfg(target_os = "windows")]
fn run_windows_toast_loop(
    req_rx: &mut mpsc::UnboundedReceiver<ToastRequest>,
    act_tx: &mpsc::UnboundedSender<ToastActivation>,
) {
    use std::collections::VecDeque;
    use windows::Data::Xml::Dom::XmlDocument;
    use windows::Foundation::TypedEventHandler;
    use windows::UI::Notifications::{
        ToastActivatedEventArgs, ToastDismissedEventArgs, ToastFailedEventArgs, ToastNotification,
        ToastNotificationManager,
    };
    use windows::Win32::System::Com::{COINIT_MULTITHREADED, CoInitializeEx, CoUninitialize};
    use windows::core::{HSTRING, IInspectable, Interface as _};

    ensure_windows_aumid_registration();

    // SAFETY: Initialize COM MTA for WinRT toast APIs on this dedicated worker thread.
    let com_hr = unsafe { CoInitializeEx(None, COINIT_MULTITHREADED) };
    let aumid = HSTRING::from(POSTHAZE_AUMID);
    let mut cached_notifier = ToastNotificationManager::CreateToastNotifierWithId(&aumid).ok();
    let mut active_toasts: VecDeque<ToastNotification> = VecDeque::new();
    const MAX_RETAINED_TOASTS: usize = 24;

    while let Some(req) = req_rx.blocking_recv() {
        if cached_notifier.is_none() {
            cached_notifier = ToastNotificationManager::CreateToastNotifierWithId(&aumid).ok();
        }
        let Some(notifier) = cached_notifier.as_ref() else {
            tracing::warn!("Windows ToastNotifier unavailable for AUMID {POSTHAZE_AUMID}");
            continue;
        };

        let (xml_body, ctx) = build_toast_parts(&req);
        let Ok(doc) = XmlDocument::new() else {
            continue;
        };
        if let Err(err) = doc.LoadXml(&HSTRING::from(xml_body)) {
            tracing::warn!("Failed loading toast XML: {err}");
            continue;
        }
        let Ok(toast) = ToastNotification::CreateToastNotification(&doc) else {
            continue;
        };

        let act_sender = act_tx.clone();
        let _ = toast.Activated(&TypedEventHandler::<ToastNotification, IInspectable>::new(
            move |_, args| {
                let raw_args = args
                    .as_ref()
                    .and_then(|insp| insp.cast::<ToastActivatedEventArgs>().ok())
                    .and_then(|a| a.Arguments().ok())
                    .map(|h| h.to_string())
                    .unwrap_or_default();
                let activation = resolve_toast_activation(&raw_args, &ctx);
                let _ = act_sender.send(activation);
                Ok(())
            },
        ));

        let _ = toast.Dismissed(&TypedEventHandler::<
            ToastNotification,
            ToastDismissedEventArgs,
        >::new(|_, _| Ok(())));

        let _ = toast.Failed(&TypedEventHandler::<ToastNotification, ToastFailedEventArgs>::new(
            |_, args| {
                if let Some(failed) = args.as_ref()
                    && let Ok(code) = failed.ErrorCode()
                {
                    tracing::warn!("Windows toast failed in Action Center: {code}");
                }
                Ok(())
            },
        ));

        if let Err(err) = notifier.Show(&toast) {
            tracing::warn!("Failed showing Windows toast notification: {err}");
            cached_notifier = None;
            continue;
        }

        // Keep recent ToastNotification handles alive so COM activation callbacks fire reliably.
        active_toasts.push_back(toast);
        while active_toasts.len() > MAX_RETAINED_TOASTS {
            active_toasts.pop_front();
        }
    }

    if com_hr.is_ok() {
        // SAFETY: Matched with successful `CoInitializeEx` on this thread.
        unsafe {
            CoUninitialize();
        }
    }
}

#[cfg(test)]
mod tests {
    use std::path::PathBuf;

    use posthaze_engine::DownloadId;

    use super::{
        ToastActivation, ToastRequest, build_toast_parts, escape_xml, format_folder_label,
        resolve_toast_activation,
    };

    #[test]
    fn xml_escaping_handles_ampersands_quotes_and_angles() {
        assert_eq!(
            escape_xml(r#"A & B <tag> "quote" 'single'"#),
            "A &amp; B &lt;tag&gt; &quot;quote&quot; &apos;single&apos;"
        );
    }

    #[test]
    fn builds_clear_completed_and_failed_toast_xml_and_actions() {
        let id = DownloadId::from("dl-1".to_string());
        let file_path = PathBuf::from(r"D:\Downloads\Anime\Episode & Special <01>.mkv");

        let (completed_xml, completed_ctx) = build_toast_parts(&ToastRequest::Completed {
            id: id.clone(),
            filename: "Episode & Special <01>.mkv".to_string(),
            output_path: Some(file_path.clone()),
            total_bytes: Some(256 * 1024 * 1024),
        });
        assert!(completed_xml.contains("Download Complete"));
        assert!(completed_xml.contains("Episode &amp; Special &lt;01&gt;.mkv"));
        assert!(completed_xml.contains("256.0 MiB · Saved in D:\\Downloads\\Anime"));
        assert!(completed_xml.contains("Open File"));
        assert!(completed_xml.contains("Show in Folder"));

        assert_eq!(
            resolve_toast_activation("open", &completed_ctx),
            ToastActivation::OpenDownload(id.clone())
        );
        assert_eq!(
            resolve_toast_activation("open_file", &completed_ctx),
            ToastActivation::OpenFile(file_path.clone())
        );
        assert_eq!(
            resolve_toast_activation("show_folder", &completed_ctx),
            ToastActivation::ShowInFolder(file_path)
        );

        let (failed_xml, failed_ctx) = build_toast_parts(&ToastRequest::Failed {
            id: id.clone(),
            filename: "archive.zip".to_string(),
            reason: "HTTP 403 Forbidden — Link may have expired".to_string(),
            resumable: true,
        });
        assert!(failed_xml.contains("Download Failed"));
        assert!(failed_xml.contains("HTTP 403 Forbidden — Link may have expired"));
        assert!(failed_xml.contains("arguments=\"retry\""));
        assert_eq!(
            resolve_toast_activation("retry", &failed_ctx),
            ToastActivation::RetryDownload(id.clone())
        );

        let started_path = PathBuf::from(r"D:\Downloads\installer.exe");
        let (started_xml, started_ctx) = build_toast_parts(&ToastRequest::Started {
            id: id.clone(),
            filename: "installer.exe".to_string(),
            output_path: Some(started_path),
            total_bytes: Some(64 * 1024 * 1024),
        });
        assert!(started_xml.contains("Download Started"));
        assert!(started_xml.contains("installer.exe"));
        assert!(started_xml.contains("64.0 MiB · Saving to D:\\Downloads"));
        assert_eq!(
            resolve_toast_activation("open", &started_ctx),
            ToastActivation::OpenDownload(id)
        );
    }

    #[test]
    fn formats_folder_label_concisely() {
        assert_eq!(
            format_folder_label(PathBuf::from(r"D:\Downloads").as_path()),
            r"D:\Downloads"
        );
        let long_path = PathBuf::from(r"D:\Very\Long\Nested\Directory\Structure\Releases");
        assert_eq!(format_folder_label(&long_path), r"...\Releases");
    }
}
