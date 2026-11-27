//! App-specific OS helpers for file/folder shell launches, Windows tray/taskbar
//! integration, startup registry, EcoQoS, power requests, and post-completion hooks.

use std::path::Path;

use chronophoto_ui::palette::{PaletteTone, ThemeBackdrop};

/// UTF-16 null-terminated registry subkey for Windows Personalize theme settings.
#[cfg(target_os = "windows")]
const PERSONALIZE_SUBKEY: &str =
    "Software\\Microsoft\\Windows\\CurrentVersion\\Themes\\Personalize\0";
#[cfg(target_os = "windows")]
const PERSONALIZE_SUBKEY_UTF16: [u16; PERSONALIZE_SUBKEY.len()] = {
    let bytes = PERSONALIZE_SUBKEY.as_bytes();
    let mut buf = [0u16; PERSONALIZE_SUBKEY.len()];
    let mut i = 0;
    while i < bytes.len() {
        buf[i] = bytes[i] as u16;
        i += 1;
    }
    buf
};

#[cfg(target_os = "windows")]
fn read_personalize_dword(value_name: &str) -> Option<u32> {
    use windows_sys::Win32::Foundation::ERROR_SUCCESS;
    use windows_sys::Win32::System::Registry::{
        HKEY_CURRENT_USER, RRF_RT_REG_DWORD, RegGetValueW,
    };

    let wide_name: Vec<u16> = value_name
        .encode_utf16()
        .chain(std::iter::once(0))
        .collect();
    let mut data: u32 = 0;
    let mut data_size = std::mem::size_of::<u32>() as u32;
    let status = unsafe {
        RegGetValueW(
            HKEY_CURRENT_USER,
            PERSONALIZE_SUBKEY_UTF16.as_ptr(),
            wide_name.as_ptr(),
            RRF_RT_REG_DWORD,
            std::ptr::null_mut(),
            (&mut data as *mut u32).cast::<std::ffi::c_void>(),
            &mut data_size,
        )
    };
    (status == ERROR_SUCCESS).then_some(data)
}

/// Returns `true` when the Windows taskbar / system shell is configured for Light mode
/// (`HKCU\...\Personalize\SystemUsesLightTheme != 0`), defaulting to `false` (dark).
pub(crate) fn system_uses_light_theme() -> bool {
    #[cfg(target_os = "windows")]
    {
        read_personalize_dword("SystemUsesLightTheme").is_some_and(|v| v != 0)
    }
    #[cfg(not(target_os = "windows"))]
    {
        false
    }
}

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

/// Restores a hidden/minimized GPUI window to the taskbar, activates it, and
/// re-applies the active DWM backdrop so restoring from `SW_HIDE` never loses
/// Mica state.
pub(crate) fn show_window_from_tray(
    window: &gpui_kit::Window,
    backdrop: ThemeBackdrop,
    tone: PaletteTone,
) {
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
    let _ = chronophoto_ui::platform::apply_window_backdrop(window, backdrop, tone);
}

/// Returns whether the GPUI window is currently visible (not hidden to tray).
pub(crate) fn is_window_visible(window: &gpui_kit::Window) -> bool {
    #[cfg(target_os = "windows")]
    if let Some(hwnd) = win32_hwnd(window) {
        return unsafe { windows_sys::Win32::UI::WindowsAndMessaging::IsWindowVisible(hwnd) != 0 };
    }
    true
}

/// Toggles Windows 11 EcoQoS (`PROCESS_POWER_THROTTLING_EXECUTION_SPEED`) for the process.
///
/// Intended to be enabled only while hidden in the System Tray with zero active downloads.
pub(crate) fn set_process_eco_qos(enabled: bool) {
    #[cfg(target_os = "windows")]
    {
        use windows_sys::Win32::System::Threading::{
            GetCurrentProcess, PROCESS_POWER_THROTTLING_CURRENT_VERSION,
            PROCESS_POWER_THROTTLING_EXECUTION_SPEED, PROCESS_POWER_THROTTLING_STATE,
            ProcessPowerThrottling, SetProcessInformation,
        };

        let state = PROCESS_POWER_THROTTLING_STATE {
            Version: PROCESS_POWER_THROTTLING_CURRENT_VERSION,
            ControlMask: PROCESS_POWER_THROTTLING_EXECUTION_SPEED,
            StateMask: if enabled {
                PROCESS_POWER_THROTTLING_EXECUTION_SPEED
            } else {
                0
            },
        };
        // SAFETY: `state` is a valid initialized `PROCESS_POWER_THROTTLING_STATE` for the current process.
        unsafe {
            let _ = SetProcessInformation(
                GetCurrentProcess(),
                ProcessPowerThrottling,
                (&raw const state).cast(),
                std::mem::size_of::<PROCESS_POWER_THROTTLING_STATE>() as u32,
            );
        }
    }
    #[cfg(not(target_os = "windows"))]
    {
        let _ = enabled;
    }
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

/// Runs `ShellExecuteW` inside a COM STA apartment so shell extensions (e.g. `.zip` folders)
/// work reliably when invoked off the GPUI main thread.
#[cfg(target_os = "windows")]
fn shell_execute_open(target_wide: &[u16]) -> isize {
    use windows_sys::Win32::System::Com::{
        COINIT_APARTMENTTHREADED, COINIT_DISABLE_OLE1DDE, CoInitializeEx, CoUninitialize,
    };
    use windows_sys::Win32::UI::Shell::ShellExecuteW;
    use windows_sys::Win32::UI::WindowsAndMessaging::SW_SHOWNORMAL;

    let verb: Vec<u16> = "open\0".encode_utf16().collect();
    // SAFETY: `CoInitializeEx` initializes COM on the current worker thread; if it succeeds
    // (`S_OK` or `S_FALSE`), we pair it with `CoUninitialize` after `ShellExecuteW`.
    unsafe {
        let hr = CoInitializeEx(
            std::ptr::null(),
            (COINIT_APARTMENTTHREADED | COINIT_DISABLE_OLE1DDE) as u32,
        );
        let res = ShellExecuteW(
            std::ptr::null_mut(),
            verb.as_ptr(),
            target_wide.as_ptr(),
            std::ptr::null(),
            std::ptr::null(),
            SW_SHOWNORMAL,
        );
        if hr >= 0 {
            CoUninitialize();
        }
        res as isize
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

        let file: Vec<u16> = path
            .as_os_str()
            .encode_wide()
            .chain(std::iter::once(0))
            .collect();
        let code = shell_execute_open(&file);
        if code <= 32 {
            return Err(format!("Could not open file (ShellExecuteW error {code})"));
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

        let target: Vec<u16> = std::ffi::OsStr::new(url)
            .encode_wide()
            .chain(std::iter::once(0))
            .collect();
        let code = shell_execute_open(&target);
        if code <= 32 {
            return Err(format!("Could not open URL (ShellExecuteW error {code})"));
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
            use std::os::windows::process::CommandExt as _;
            const CREATE_NO_WINDOW: u32 = 0x0800_0000;

            let full_cmd = format!("\"{exe}\" {args_line}");
            let _ = std::process::Command::new("cmd.exe")
                .args(["/C", &full_cmd])
                .creation_flags(CREATE_NO_WINDOW)
                .spawn();
        }
        #[cfg(not(target_os = "windows"))]
        let _ = (exe, args_line);
    });
}

/// Aggregate progress state reflected onto the Windows taskbar button (`ITaskbarList3`).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum TaskbarProgressState {
    None,
    Indeterminate,
    Normal { completed: u64, total: u64 },
}

/// Prevents Windows idle sleep (`PowerRequestSystemRequired`) while downloads are active,
/// while still allowing the display to turn off and lock normally.
pub(crate) fn set_keep_system_awake(enabled: bool) {
    #[cfg(target_os = "windows")]
    win32_power::set_keep_awake(enabled);
    #[cfg(not(target_os = "windows"))]
    let _ = enabled;
}

/// Updates the Windows taskbar progress bar on `window` via `ITaskbarList3`.
pub(crate) fn sync_taskbar_progress(window: &gpui_kit::Window, state: TaskbarProgressState) {
    #[cfg(target_os = "windows")]
    win32_taskbar::sync(window, state);
    #[cfg(not(target_os = "windows"))]
    let _ = (window, state);
}

#[cfg(target_os = "windows")]
mod win32_power {
    use std::sync::{Mutex, OnceLock};

    use windows_sys::Win32::Foundation::{CloseHandle, HANDLE, INVALID_HANDLE_VALUE};
    use windows_sys::Win32::System::Power::{
        PowerClearRequest, PowerCreateRequest, PowerRequestSystemRequired, PowerSetRequest,
    };
    use windows_sys::Win32::System::Threading::{
        POWER_REQUEST_CONTEXT_SIMPLE_STRING, REASON_CONTEXT, REASON_CONTEXT_0,
    };

    /// `POWER_REQUEST_CONTEXT_VERSION` (`0`) from `<winnt.h>`.
    const POWER_REQUEST_CONTEXT_VERSION: u32 = 0;

    struct PowerGuard {
        handle: isize,
        active: bool,
    }

    impl Drop for PowerGuard {
        fn drop(&mut self) {
            if self.handle != 0 {
                let raw = self.handle as HANDLE;
                if self.active {
                    // SAFETY: `raw` is a valid power request handle created by `PowerCreateRequest`.
                    unsafe {
                        let _ = PowerClearRequest(raw, PowerRequestSystemRequired);
                    }
                }
                // SAFETY: `raw` is owned by `PowerGuard` and closed once.
                unsafe {
                    let _ = CloseHandle(raw);
                }
            }
        }
    }

    fn guard_cell() -> &'static Mutex<PowerGuard> {
        static CELL: OnceLock<Mutex<PowerGuard>> = OnceLock::new();
        CELL.get_or_init(|| {
            let reason: Vec<u16> = "Posthaze is downloading\0".encode_utf16().collect();
            let mut ctx = REASON_CONTEXT {
                Version: POWER_REQUEST_CONTEXT_VERSION,
                Flags: POWER_REQUEST_CONTEXT_SIMPLE_STRING,
                Reason: REASON_CONTEXT_0 {
                    SimpleReasonString: reason.as_ptr().cast_mut(),
                },
            };
            // SAFETY: `ctx` points to a valid NUL-terminated UTF-16 string for the duration of the call.
            let handle = unsafe { PowerCreateRequest(&raw mut ctx) };
            let raw_int = if handle == INVALID_HANDLE_VALUE || handle.is_null() {
                0
            } else {
                handle as isize
            };
            Mutex::new(PowerGuard {
                handle: raw_int,
                active: false,
            })
        })
    }

    pub(super) fn set_keep_awake(enabled: bool) {
        let Ok(mut guard) = guard_cell().lock() else {
            return;
        };
        if guard.handle == 0 || guard.active == enabled {
            return;
        }
        let raw = guard.handle as HANDLE;
        // SAFETY: `raw` was returned by `PowerCreateRequest` and remains valid for the process lifetime.
        let ok = unsafe {
            if enabled {
                PowerSetRequest(raw, PowerRequestSystemRequired)
            } else {
                PowerClearRequest(raw, PowerRequestSystemRequired)
            }
        };
        if ok != 0 {
            guard.active = enabled;
        }
    }
}

#[cfg(target_os = "windows")]
mod win32_taskbar {
    use std::cell::RefCell;
    use std::ffi::c_void;

    use raw_window_handle::{HasWindowHandle, RawWindowHandle};
    use windows::Win32::Foundation::HWND;
    use windows::Win32::System::Com::{
        CLSCTX_ALL, COINIT_APARTMENTTHREADED, CoCreateInstance, CoInitializeEx,
    };
    use windows::Win32::UI::Shell::{
        ITaskbarList3, TBPF_INDETERMINATE, TBPF_NOPROGRESS, TBPF_NORMAL, TaskbarList,
    };

    use super::TaskbarProgressState;

    thread_local! {
        static TASKBAR: RefCell<Option<ITaskbarList3>> = const { RefCell::new(None) };
    }

    fn with_taskbar(f: impl FnOnce(&ITaskbarList3)) {
        TASKBAR.with(|slot| {
            let mut borrow = slot.borrow_mut();
            if borrow.is_none() {
                // SAFETY: Called on the GPUI UI thread; `CoInitializeEx` is idempotent on STA threads.
                unsafe {
                    let _ = CoInitializeEx(None, COINIT_APARTMENTTHREADED);
                    if let Ok(list) =
                        CoCreateInstance::<_, ITaskbarList3>(&TaskbarList, None, CLSCTX_ALL)
                        && list.HrInit().is_ok()
                    {
                        *borrow = Some(list);
                    }
                }
            }
            if let Some(list) = borrow.as_ref() {
                f(list);
            }
        });
    }

    pub(super) fn sync(window: &gpui_kit::Window, state: TaskbarProgressState) {
        let Ok(handle) = HasWindowHandle::window_handle(window) else {
            return;
        };
        let RawWindowHandle::Win32(h) = handle.as_raw() else {
            return;
        };
        let hwnd = HWND(h.hwnd.get() as *mut c_void);

        with_taskbar(|list| unsafe {
            match state {
                TaskbarProgressState::None => {
                    let _ = list.SetProgressState(hwnd, TBPF_NOPROGRESS);
                }
                TaskbarProgressState::Indeterminate => {
                    let _ = list.SetProgressState(hwnd, TBPF_INDETERMINATE);
                }
                TaskbarProgressState::Normal { completed, total } => {
                    let _ = list.SetProgressState(hwnd, TBPF_NORMAL);
                    let _ = list.SetProgressValue(hwnd, completed, total.max(1));
                }
            }
        });
    }
}

#[cfg(test)]
mod tests {
    use std::path::Path;

    use super::format_startup_command;

    #[test]
    fn formats_startup_registry_command_with_minimized_flag() {
        let cmd = format_startup_command(Path::new(r"C:\Users\Test\Posthaze\posthaze-ui.exe"));
        assert_eq!(
            cmd,
            "\"C:\\Users\\Test\\Posthaze\\posthaze-ui.exe\" --minimized"
        );
    }
}
