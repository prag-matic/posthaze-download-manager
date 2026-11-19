//! Platform-specific OS helpers for path disambiguation, Windows notifications, and completion hooks.

use std::path::{Path, PathBuf};

/// Returns a non-colliding file path in `dir` by appending ` (1)`, ` (2)`, etc. when needed.
pub(crate) fn disambiguate_ui_path(dir: &Path, filename: &str) -> PathBuf {
    let candidate = dir.join(filename);
    if !candidate.exists() {
        return candidate;
    }
    let path = Path::new(filename);
    let stem = path.file_stem().and_then(|s| s.to_str()).unwrap_or(filename);
    let ext = path.extension().and_then(|e| e.to_str());
    for idx in 1..=1000 {
        let numbered = match ext {
            Some(ext) if !ext.is_empty() => format!("{stem} ({idx}).{ext}"),
            _ => format!("{stem} ({idx})"),
        };
        let next = dir.join(numbered);
        if !next.exists() {
            return next;
        }
    }
    candidate
}

/// Dispatches a non-blocking Windows Toast notification via PowerShell WinRT.
pub(crate) fn spawn_windows_toast(title: &str, body: &str) {
    #[cfg(target_os = "windows")]
    {
        let title_esc = title.replace('\'', "''");
        let body_esc = body.replace('\'', "''");
        let script = format!(
            "[Windows.UI.Notifications.ToastNotificationManager, Windows.UI.Notifications, ContentType = WindowsRuntime] > $null; \
             $template = [Windows.UI.Notifications.ToastTemplateType]::ToastText02; \
             $xml = [Windows.UI.Notifications.ToastNotificationManager]::GetTemplateContent($template); \
             $nodes = $xml.GetElementsByTagName('text'); \
             $nodes.Item(0).AppendChild($xml.CreateTextNode('{title_esc}')) > $null; \
             $nodes.Item(1).AppendChild($xml.CreateTextNode('{body_esc}')) > $null; \
             $toast = [Windows.UI.Notifications.ToastNotification]::new($xml); \
             [Windows.UI.Notifications.ToastNotificationManager]::CreateToastNotifier('Posthaze Download Manager').Show($toast)"
        );
        std::thread::spawn(move || {
            let _ = std::process::Command::new("powershell.exe")
                .args(["-NoProfile", "-WindowStyle", "Hidden", "-Command", &script])
                .spawn();
        });
    }
    #[cfg(not(target_os = "windows"))]
    let _ = (title, body);
}

/// Runs a configured external executable/antivirus hook asynchronously after completion.
pub(crate) fn spawn_completion_hook(exe_path: &str, args_template: &str, file_path: &Path) {
    let trimmed = exe_path.trim();
    if trimmed.is_empty() {
        return;
    }
    let exe = trimmed.to_string();
    let path_str = file_path.to_string_lossy().into_owned();
    let args_line = args_template.replace("%path%", &path_str);
    std::thread::spawn(move || {
        #[cfg(target_os = "windows")]
        {
            let full_cmd = format!("\"{exe}\" {args_line}");
            let _ = std::process::Command::new("cmd.exe")
                .args(["/C", &full_cmd])
                .spawn();
        }
        #[cfg(not(target_os = "windows"))]
        let _ = (exe, args_line);
    });
}
