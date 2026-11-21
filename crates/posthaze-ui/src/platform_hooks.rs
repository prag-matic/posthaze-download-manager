//! Platform-specific OS helpers for file/folder shell launches, Windows notifications, and post-completion hooks.

use std::path::Path;

fn spawn_explorer(arg: impl AsRef<std::ffi::OsStr>, action_label: &str) -> Result<(), String> {
    #[cfg(target_os = "windows")]
    let spawn_res = std::process::Command::new("explorer.exe")
        .arg(arg.as_ref())
        .spawn();
    #[cfg(not(target_os = "windows"))]
    let spawn_res: std::io::Result<std::process::Child> = Err(std::io::Error::new(
        std::io::ErrorKind::Unsupported,
        format!("{action_label} is only supported on Windows"),
    ));

    spawn_res
        .map(|_| ())
        .map_err(|err| format!("Could not {action_label}: {err}"))
}

/// Formats the Windows `HKCU\...\Run` command line for launching Posthaze minimized on login.
pub(crate) fn format_startup_command(exe_path: &Path) -> String {
    format!("\"{}\" --minimized", exe_path.display())
}

#[cfg(target_os = "windows")]
fn win32_hwnd(window: &gpui_kit::Window) -> Option<windows_sys::Win32::Foundation::HWND> {
    use raw_window_handle::{HasWindowHandle, RawWindowHandle};
    let handle = HasWindowHandle::window_handle(window).ok()?;
    match handle.as_raw() {
        RawWindowHandle::Win32(h) => Some(h.hwnd.get() as windows_sys::Win32::Foundation::HWND),
        _ => None,
    }
}

/// Hides a GPUI window from both the desktop and the Windows taskbar.
pub(crate) fn hide_window_to_tray(window: &gpui_kit::Window) {
    #[cfg(target_os = "windows")]
    if let Some(hwnd) = win32_hwnd(window) {
        unsafe {
            windows_sys::Win32::UI::WindowsAndMessaging::ShowWindowAsync(
                hwnd,
                windows_sys::Win32::UI::WindowsAndMessaging::SW_HIDE,
            );
        }
        return;
    }
    window.minimize_window();
}

/// Restores a hidden/minimized GPUI window to the taskbar and activates it.
pub(crate) fn show_window_from_tray(window: &gpui_kit::Window) {
    #[cfg(target_os = "windows")]
    if let Some(hwnd) = win32_hwnd(window) {
        unsafe {
            windows_sys::Win32::UI::WindowsAndMessaging::ShowWindowAsync(
                hwnd,
                windows_sys::Win32::UI::WindowsAndMessaging::SW_SHOW,
            );
        }
    }
    window.activate_window();
}

/// Returns whether the GPUI window is currently visible (not hidden to tray).
pub(crate) fn is_window_visible(window: &gpui_kit::Window) -> bool {
    #[cfg(target_os = "windows")]
    if let Some(hwnd) = win32_hwnd(window) {
        return unsafe { windows_sys::Win32::UI::WindowsAndMessaging::IsWindowVisible(hwnd) != 0 };
    }
    true
}

/// Synchronizes `HKCU\Software\Microsoft\Windows\CurrentVersion\Run\Posthaze` with `enabled`.
pub(crate) fn sync_windows_startup_registry(enabled: bool) -> Result<(), String> {
    #[cfg(target_os = "windows")]
    {
        use std::os::windows::ffi::OsStrExt as _;
        use windows_sys::Win32::Foundation::ERROR_FILE_NOT_FOUND;
        use windows_sys::Win32::System::Registry::{
            HKEY, HKEY_CURRENT_USER, KEY_SET_VALUE, REG_SZ, RegCloseKey, RegCreateKeyW,
            RegDeleteValueW, RegOpenKeyExW, RegSetValueExW,
        };

        let subkey: Vec<u16> = "Software\\Microsoft\\Windows\\CurrentVersion\\Run\0"
            .encode_utf16()
            .collect();
        let value_name: Vec<u16> = "Posthaze\0".encode_utf16().collect();

        if enabled {
            let exe = std::env::current_exe()
                .map_err(|err| format!("Could not resolve executable path: {err}"))?;
            let command = format_startup_command(&exe);
            let wide_data: Vec<u16> = std::ffi::OsStr::new(&command)
                .encode_wide()
                .chain(std::iter::once(0))
                .collect();
            let byte_len = u32::try_from(wide_data.len() * std::mem::size_of::<u16>())
                .map_err(|_| "Startup command path is too long".to_string())?;

            let mut hkey: HKEY = std::ptr::null_mut();
            let status =
                unsafe { RegCreateKeyW(HKEY_CURRENT_USER, subkey.as_ptr(), &mut hkey) };
            if status != 0 {
                return Err(format!("Could not open Run registry key (Win32 error {status})"));
            }
            let set_status = unsafe {
                RegSetValueExW(
                    hkey,
                    value_name.as_ptr(),
                    0,
                    REG_SZ,
                    wide_data.as_ptr().cast::<u8>(),
                    byte_len,
                )
            };
            unsafe { RegCloseKey(hkey) };
            if set_status != 0 {
                return Err(format!(
                    "Could not write Posthaze startup entry (Win32 error {set_status})"
                ));
            }
            Ok(())
        } else {
            let mut hkey: HKEY = std::ptr::null_mut();
            let status = unsafe {
                RegOpenKeyExW(
                    HKEY_CURRENT_USER,
                    subkey.as_ptr(),
                    0,
                    KEY_SET_VALUE,
                    &mut hkey,
                )
            };
            if status == ERROR_FILE_NOT_FOUND {
                return Ok(());
            }
            if status != 0 {
                return Err(format!("Could not open Run registry key (Win32 error {status})"));
            }
            let del_status = unsafe { RegDeleteValueW(hkey, value_name.as_ptr()) };
            unsafe { RegCloseKey(hkey) };
            if del_status != 0 && del_status != ERROR_FILE_NOT_FOUND {
                return Err(format!(
                    "Could not remove Posthaze startup entry (Win32 error {del_status})"
                ));
            }
            Ok(())
        }
    }
    #[cfg(not(target_os = "windows"))]
    {
        let _ = enabled;
        Ok(())
    }
}

/// Opens a downloaded file using the OS default application (`ShellExecuteW` on Windows).
pub(crate) fn open_file_in_shell(path: &Path) -> Result<(), String> {
    if !path.exists() {
        return Err(format!("File not found on disk: {}", path.display()));
    }
    #[cfg(target_os = "windows")]
    {
        use std::os::windows::ffi::OsStrExt as _;
        use windows_sys::Win32::UI::Shell::ShellExecuteW;
        use windows_sys::Win32::UI::WindowsAndMessaging::SW_SHOWNORMAL;

        let verb: Vec<u16> = "open\0".encode_utf16().collect();
        let file: Vec<u16> = path
            .as_os_str()
            .encode_wide()
            .chain(std::iter::once(0))
            .collect();
        let res = unsafe {
            ShellExecuteW(
                std::ptr::null_mut(),
                verb.as_ptr(),
                file.as_ptr(),
                std::ptr::null(),
                std::ptr::null(),
                SW_SHOWNORMAL,
            )
        };
        if (res as isize) <= 32 {
            return Err(format!(
                "Could not open file (ShellExecuteW error {})",
                res as isize
            ));
        }
        Ok(())
    }
    #[cfg(not(target_os = "windows"))]
    {
        let _ = path;
        Err("Open File is only supported on Windows".to_string())
    }
}

/// Opens an `http://` or `https://` URL in the user's default web browser via `ShellExecuteW`.
pub(crate) fn open_url_in_browser(url: &str) -> Result<(), String> {
    if !url.starts_with("https://") && !url.starts_with("http://") {
        return Err(format!("Refusing to open non-HTTP URL: {url}"));
    }
    #[cfg(target_os = "windows")]
    {
        use std::os::windows::ffi::OsStrExt as _;
        use windows_sys::Win32::UI::Shell::ShellExecuteW;
        use windows_sys::Win32::UI::WindowsAndMessaging::SW_SHOWNORMAL;

        let verb: Vec<u16> = "open\0".encode_utf16().collect();
        let target: Vec<u16> = std::ffi::OsStr::new(url)
            .encode_wide()
            .chain(std::iter::once(0))
            .collect();
        let res = unsafe {
            ShellExecuteW(
                std::ptr::null_mut(),
                verb.as_ptr(),
                target.as_ptr(),
                std::ptr::null(),
                std::ptr::null(),
                SW_SHOWNORMAL,
            )
        };
        if (res as isize) <= 32 {
            return Err(format!(
                "Could not open URL (ShellExecuteW error {})",
                res as isize
            ));
        }
        Ok(())
    }
    #[cfg(not(target_os = "windows"))]
    {
        let _ = url;
        Err("Open URL is only supported on Windows".to_string())
    }
}

/// Reveals a downloaded file or its parent directory in the OS file manager.
pub(crate) fn open_folder_in_shell(path: &Path) -> Result<(), String> {
    if path.exists() {
        spawn_explorer(format!("/select,{}", path.display()), "open folder")
    } else if let Some(parent) = path.parent().filter(|p| p.exists()) {
        spawn_explorer(parent, "open folder")
    } else {
        Err(format!("Folder not found: {}", path.display()))
    }
}

/// Ensures `dir` exists and opens it directly in Windows Explorer.
pub(crate) fn open_directory_in_shell(dir: &Path) -> Result<(), String> {
    std::fs::create_dir_all(dir)
        .map_err(|err| format!("Could not prepare folder {}: {err}", dir.display()))?;
    spawn_explorer(dir, "open folder")
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

#[cfg(test)]
mod tests {
    use std::path::Path;

    use super::format_startup_command;

    #[test]
    fn formats_startup_registry_command_with_minimized_flag() {
        let cmd = format_startup_command(Path::new(r"C:\Users\Test\Posthaze\posthaze-ui.exe"));
        assert_eq!(cmd, "\"C:\\Users\\Test\\Posthaze\\posthaze-ui.exe\" --minimized");
    }
}
