//! Platform-specific OS helpers for file/folder shell launches, Windows notifications, and post-completion hooks.

use std::path::Path;

/// Opens a downloaded file using the OS shell (`explorer.exe` on Windows).
pub(crate) fn open_file_in_shell(path: &Path) -> Result<(), String> {
    if !path.exists() {
        return Err(format!("File not found on disk: {}", path.display()));
    }
    #[cfg(target_os = "windows")]
    let spawn_res = std::process::Command::new("explorer.exe").arg(path).spawn();
    #[cfg(not(target_os = "windows"))]
    let spawn_res: std::io::Result<std::process::Child> = Err(std::io::Error::new(
        std::io::ErrorKind::Unsupported,
        "Open File is only supported on Windows",
    ));

    spawn_res
        .map(|_| ())
        .map_err(|err| format!("Could not open file: {err}"))
}

/// Reveals a downloaded file or its parent directory in the OS file manager.
pub(crate) fn open_folder_in_shell(path: &Path) -> Result<(), String> {
    #[cfg(target_os = "windows")]
    let spawn_res = if path.exists() {
        std::process::Command::new("explorer.exe")
            .arg(format!("/select,{}", path.display()))
            .spawn()
    } else if let Some(parent) = path.parent().filter(|p| p.exists()) {
        std::process::Command::new("explorer.exe").arg(parent).spawn()
    } else {
        return Err(format!("Folder not found: {}", path.display()));
    };
    #[cfg(not(target_os = "windows"))]
    let spawn_res: std::io::Result<std::process::Child> = Err(std::io::Error::new(
        std::io::ErrorKind::Unsupported,
        "Open Folder is only supported on Windows",
    ));

    spawn_res
        .map(|_| ())
        .map_err(|err| format!("Could not open folder: {err}"))
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
