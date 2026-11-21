//! Platform-specific OS helpers for file/folder shell launches, Windows notifications,
//! Win32 DWM Mica backdrops, and post-completion hooks.

use std::path::Path;
#[cfg(target_os = "windows")]
use std::collections::HashMap;
#[cfg(target_os = "windows")]
use std::sync::atomic::{AtomicBool, Ordering};
#[cfg(target_os = "windows")]
use std::sync::{Mutex, OnceLock};

use crate::theme::{PaletteTone, ThemeBackdrop};

/// Minimum Windows build number supporting `DWMWA_SYSTEMBACKDROP_TYPE` (Windows 11 22H2).
const MIN_MICA_WINDOWS_BUILD: u32 = 22621;

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

/// Reason why native Windows 11 Mica translucency is unavailable and Posthaze
/// has fallen back to its calibrated opaque palette.
#[allow(dead_code)]
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum MicaFallbackReason {
    /// OS is non-Windows or Windows build `< 22621` (older than Windows 11 22H2).
    UnsupportedOs,
    /// `GPUI_DISABLE_DIRECT_COMPOSITION` is enabled (`"1"` or `"true"`).
    DirectCompositionDisabled,
    /// Windows High Contrast mode (`SPI_GETHIGHCONTRAST` / `HCF_HIGHCONTRASTON`) is active.
    HighContrastActive,
    /// Windows Transparency Effects (`EnableTransparency == 0`) are turned off.
    TransparencyDisabled,
    /// A Win32 DWM call returned a failure `HRESULT` (`< 0`).
    DwmRejected,
}

impl MicaFallbackReason {
    /// Returns the user-facing status note displayed below the theme grid in `Settings > Interface`.
    #[allow(dead_code)]
    pub(crate) const fn status_note(self) -> &'static str {
        match self {
            Self::UnsupportedOs => {
                "Native Mica translucency requires Windows 11 22H2 or newer — using calibrated opaque fallback."
            }
            Self::DirectCompositionDisabled => {
                "DirectComposition is disabled (`GPUI_DISABLE_DIRECT_COMPOSITION=1`) — using calibrated opaque fallback."
            }
            Self::HighContrastActive => {
                "Windows High Contrast mode is active — using calibrated opaque fallback."
            }
            Self::TransparencyDisabled => {
                "Windows Transparency Effects are turned off — using calibrated opaque fallback."
            }
            Self::DwmRejected => {
                "Windows DWM declined the Mica backdrop — using calibrated opaque fallback."
            }
        }
    }
}

/// Pure snapshot of OS and runtime inputs used to evaluate Mica availability.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
struct MicaProbeInputs {
    windows_build: Option<u32>,
    direct_composition_disabled: bool,
    high_contrast_active: bool,
    transparency_enabled: bool,
    dwm_rejected: bool,
}

/// Evaluates Mica availability in strict spec priority order.
#[allow(dead_code)]
fn evaluate_mica_availability(inputs: MicaProbeInputs) -> Result<(), MicaFallbackReason> {
    if inputs.windows_build.is_none_or(|build| build < MIN_MICA_WINDOWS_BUILD) {
        return Err(MicaFallbackReason::UnsupportedOs);
    }
    if inputs.direct_composition_disabled {
        return Err(MicaFallbackReason::DirectCompositionDisabled);
    }
    if inputs.high_contrast_active {
        return Err(MicaFallbackReason::HighContrastActive);
    }
    if !inputs.transparency_enabled {
        return Err(MicaFallbackReason::TransparencyDisabled);
    }
    if inputs.dwm_rejected {
        return Err(MicaFallbackReason::DwmRejected);
    }
    Ok(())
}

/// Parses the `GPUI_DISABLE_DIRECT_COMPOSITION` environment variable value.
#[allow(dead_code)]
fn is_direct_composition_disabled_value(value: Option<&str>) -> bool {
    matches!(value, Some(v) if v == "1" || v.eq_ignore_ascii_case("true"))
}

#[cfg(target_os = "windows")]
static DWM_REJECTED: AtomicBool = AtomicBool::new(false);

#[cfg(target_os = "windows")]
fn last_window_backdrops() -> &'static Mutex<HashMap<isize, (ThemeBackdrop, PaletteTone)>> {
    static MAP: OnceLock<Mutex<HashMap<isize, (ThemeBackdrop, PaletteTone)>>> = OnceLock::new();
    MAP.get_or_init(|| Mutex::new(HashMap::new()))
}

#[cfg(target_os = "windows")]
fn windows_build_number() -> Option<u32> {
    use windows_sys::Wdk::System::SystemServices::RtlGetVersion;
    use windows_sys::Win32::System::SystemInformation::OSVERSIONINFOW;

    static BUILD: OnceLock<Option<u32>> = OnceLock::new();
    *BUILD.get_or_init(|| {
        let mut info: OSVERSIONINFOW = unsafe { std::mem::zeroed() };
        info.dwOSVersionInfoSize = std::mem::size_of::<OSVERSIONINFOW>() as u32;
        let status = unsafe { RtlGetVersion(&mut info) };
        if status == 0 {
            Some(info.dwBuildNumber)
        } else {
            None
        }
    })
}

#[cfg(target_os = "windows")]
fn is_high_contrast_active() -> bool {
    use windows_sys::Win32::UI::Accessibility::{HCF_HIGHCONTRASTON, HIGHCONTRASTW};
    use windows_sys::Win32::UI::WindowsAndMessaging::{
        SPI_GETHIGHCONTRAST, SystemParametersInfoW,
    };

    let mut hc: HIGHCONTRASTW = unsafe { std::mem::zeroed() };
    hc.cbSize = std::mem::size_of::<HIGHCONTRASTW>() as u32;
    let ok = unsafe {
        SystemParametersInfoW(
            SPI_GETHIGHCONTRAST,
            hc.cbSize,
            (&mut hc as *mut HIGHCONTRASTW).cast::<std::ffi::c_void>(),
            0,
        )
    };
    ok != 0 && (hc.dwFlags & HCF_HIGHCONTRASTON) != 0
}

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

#[cfg(target_os = "windows")]
fn is_transparency_enabled() -> bool {
    // Missing key or value defaults to enabled on Windows.
    read_personalize_dword("EnableTransparency").is_none_or(|data| data != 0)
}

/// Maps Windows `AppsUseLightTheme` registry DWORD value to GPUI's [`gpui_kit::WindowAppearance`].
fn parse_apps_use_light_theme(value: Option<u32>) -> gpui_kit::WindowAppearance {
    match value {
        Some(v) if v != 0 => gpui_kit::WindowAppearance::Light,
        _ => gpui_kit::WindowAppearance::Dark,
    }
}

/// Queries the initial OS appearance before any GPUI window is open so `Mica (System)`
/// opens on frame 1 with the correct Dark/Light polarity and DWM backdrop.
pub(crate) fn initial_os_appearance() -> gpui_kit::WindowAppearance {
    #[cfg(target_os = "windows")]
    {
        parse_apps_use_light_theme(read_personalize_dword("AppsUseLightTheme"))
    }
    #[cfg(not(target_os = "windows"))]
    {
        parse_apps_use_light_theme(None)
    }
}

/// Checks whether native Windows 11 Mica translucency is currently available.
#[allow(dead_code)]
pub(crate) fn mica_availability() -> Result<(), MicaFallbackReason> {
    #[cfg(target_os = "windows")]
    {
        let env_val = std::env::var("GPUI_DISABLE_DIRECT_COMPOSITION").ok();
        evaluate_mica_availability(MicaProbeInputs {
            windows_build: windows_build_number(),
            direct_composition_disabled: is_direct_composition_disabled_value(env_val.as_deref()),
            high_contrast_active: is_high_contrast_active(),
            transparency_enabled: is_transparency_enabled(),
            dwm_rejected: DWM_REJECTED.load(Ordering::Relaxed),
        })
    }
    #[cfg(not(target_os = "windows"))]
    {
        evaluate_mica_availability(MicaProbeInputs {
            windows_build: None,
            direct_composition_disabled: false,
            high_contrast_active: false,
            transparency_enabled: false,
            dwm_rejected: false,
        })
    }
}

#[cfg(target_os = "windows")]
fn set_dwm_dark_mode(hwnd: windows_sys::Win32::Foundation::HWND, is_dark: bool) -> i32 {
    use windows_sys::Win32::Graphics::Dwm::{DWMWA_USE_IMMERSIVE_DARK_MODE, DwmSetWindowAttribute};

    let dark: i32 = i32::from(is_dark);
    unsafe {
        DwmSetWindowAttribute(
            hwnd,
            DWMWA_USE_IMMERSIVE_DARK_MODE as u32,
            (&dark as *const i32).cast::<std::ffi::c_void>(),
            std::mem::size_of::<i32>() as u32,
        )
    }
}

#[cfg(target_os = "windows")]
fn set_dwm_backdrop_type(hwnd: windows_sys::Win32::Foundation::HWND, backdrop_type: i32) -> i32 {
    use windows_sys::Win32::Graphics::Dwm::{DWMWA_SYSTEMBACKDROP_TYPE, DwmSetWindowAttribute};

    unsafe {
        DwmSetWindowAttribute(
            hwnd,
            DWMWA_SYSTEMBACKDROP_TYPE as u32,
            (&backdrop_type as *const i32).cast::<std::ffi::c_void>(),
            std::mem::size_of::<i32>() as u32,
        )
    }
}

#[cfg(target_os = "windows")]
fn extend_dwm_frame(hwnd: windows_sys::Win32::Foundation::HWND, margin: i32) -> i32 {
    use windows_sys::Win32::Graphics::Dwm::DwmExtendFrameIntoClientArea;
    use windows_sys::Win32::UI::Controls::MARGINS;

    let margins = MARGINS {
        cxLeftWidth: margin,
        cxRightWidth: margin,
        cyTopHeight: margin,
        cyBottomHeight: margin,
    };
    unsafe { DwmExtendFrameIntoClientArea(hwnd, &margins) }
}

#[cfg(target_os = "windows")]
fn check_dwm_hr(
    hr: i32,
    op: &'static str,
    window: &gpui_kit::Window,
    hwnd: windows_sys::Win32::Foundation::HWND,
    reset_margins_on_err: bool,
) -> Result<(), String> {
    if hr < 0 {
        tracing::warn!(
            hr = format_args!("0x{:08X}", hr as u32),
            op,
            "Win32 DWM call failed while enabling Mica"
        );
        if reset_margins_on_err {
            let _ = extend_dwm_frame(hwnd, 0);
        }
        DWM_REJECTED.store(true, Ordering::Relaxed);
        window.set_background_appearance(gpui_kit::WindowBackgroundAppearance::Opaque);
        return Err(format!("{op} failed (HRESULT 0x{:08X})", hr as u32));
    }
    Ok(())
}

/// Applies the requested window backdrop (`Opaque`, `Mica`, or `MicaAlt`) and
/// immersive dark-mode polarity to a GPUI window.
pub(crate) fn apply_window_backdrop(
    window: &gpui_kit::Window,
    backdrop: ThemeBackdrop,
    tone: PaletteTone,
) -> Result<(), String> {
    window.set_background_appearance(backdrop.to_window_background());

    #[cfg(target_os = "windows")]
    {
        use windows_sys::Win32::Graphics::Dwm::{
            DWMSBT_MAINWINDOW, DWMSBT_NONE, DWMSBT_TABBEDWINDOW,
        };

        let Some(hwnd) = win32_hwnd(window) else {
            return Ok(());
        };
        let is_dark = tone == PaletteTone::Dark;

        match backdrop {
            ThemeBackdrop::Mica | ThemeBackdrop::MicaAlt => {
                let backdrop_value = match backdrop {
                    ThemeBackdrop::Mica => DWMSBT_MAINWINDOW,
                    ThemeBackdrop::MicaAlt => DWMSBT_TABBEDWINDOW,
                    ThemeBackdrop::Opaque => unreachable!(),
                };

                check_dwm_hr(
                    set_dwm_dark_mode(hwnd, is_dark),
                    "DWMWA_USE_IMMERSIVE_DARK_MODE",
                    window,
                    hwnd,
                    false,
                )?;
                check_dwm_hr(
                    extend_dwm_frame(hwnd, -1),
                    "DwmExtendFrameIntoClientArea",
                    window,
                    hwnd,
                    false,
                )?;
                check_dwm_hr(
                    set_dwm_backdrop_type(hwnd, backdrop_value),
                    "DWMWA_SYSTEMBACKDROP_TYPE",
                    window,
                    hwnd,
                    true,
                )?;
            }
            ThemeBackdrop::Opaque => {
                if windows_build_number().is_some_and(|build| build >= MIN_MICA_WINDOWS_BUILD) {
                    let hr_backdrop = set_dwm_backdrop_type(hwnd, DWMSBT_NONE);
                    if hr_backdrop < 0 {
                        tracing::warn!(
                            hr = format_args!("0x{:08X}", hr_backdrop as u32),
                            "DWMWA_SYSTEMBACKDROP_TYPE(DWMSBT_NONE) failed"
                        );
                    }
                }
                let hr_margins = extend_dwm_frame(hwnd, 0);
                if hr_margins < 0 {
                    tracing::warn!(
                        hr = format_args!("0x{:08X}", hr_margins as u32),
                        "DwmExtendFrameIntoClientArea(0) failed"
                    );
                }
                let hr_dark = set_dwm_dark_mode(hwnd, is_dark);
                if hr_dark < 0 {
                    tracing::warn!(
                        hr = format_args!("0x{:08X}", hr_dark as u32),
                        is_dark,
                        "DWMWA_USE_IMMERSIVE_DARK_MODE failed"
                    );
                }
            }
        }

        if let Ok(mut map) = last_window_backdrops().lock() {
            map.insert(hwnd as isize, (backdrop, tone));
        }
        Ok(())
    }

    #[cfg(not(target_os = "windows"))]
    {
        let _ = tone;
        Ok(())
    }
}

/// Re-asserts Win32 `DWMWA_USE_IMMERSIVE_DARK_MODE` on a GPUI window.
///
/// Intended to be called in a deferred task (`window.defer(...)`) after GPUI's
/// `WM_SETTINGCHANGE` (`ImmersiveColorSet`) handler runs so forced-polarity Mica
/// themes (`Mica Dark` / `Mica Light`) keep their DWM dark-mode bit.
#[allow(dead_code)]
pub(crate) fn assert_window_dark_mode(window: &gpui_kit::Window, is_dark: bool) {
    #[cfg(target_os = "windows")]
    if let Some(hwnd) = win32_hwnd(window) {
        let hr = set_dwm_dark_mode(hwnd, is_dark);
        if hr < 0 {
            tracing::warn!(
                hr = format_args!("0x{:08X}", hr as u32),
                is_dark,
                "Deferred DWMWA_USE_IMMERSIVE_DARK_MODE assertion failed"
            );
        }
    }
    #[cfg(not(target_os = "windows"))]
    let _ = (window, is_dark);
}

#[cfg(target_os = "windows")]
std::thread_local! {
    static MICA_PREF_CALLBACK: std::cell::RefCell<Option<Box<dyn Fn()>>> =
        const { std::cell::RefCell::new(None) };
}

#[cfg(target_os = "windows")]
unsafe extern "system" fn mica_pref_wnd_proc(
    hwnd: windows_sys::Win32::Foundation::HWND,
    msg: u32,
    wparam: windows_sys::Win32::Foundation::WPARAM,
    lparam: windows_sys::Win32::Foundation::LPARAM,
) -> windows_sys::Win32::Foundation::LRESULT {
    use windows_sys::Win32::UI::WindowsAndMessaging::{
        DefWindowProcW, WM_SETTINGCHANGE, WM_SYSCOLORCHANGE, WM_THEMECHANGED,
    };

    if matches!(msg, WM_SETTINGCHANGE | WM_THEMECHANGED | WM_SYSCOLORCHANGE) {
        MICA_PREF_CALLBACK.with(|slot| {
            if let Some(cb) = slot.borrow().as_ref() {
                cb();
            }
        });
    }
    unsafe { DefWindowProcW(hwnd, msg, wparam, lparam) }
}

/// Spawns a lightweight, zero-polling background listener that invokes `on_change`
/// whenever Windows Transparency Effects (`EnableTransparency`) or High Contrast
/// (`SPI_SETHIGHCONTRAST` / `WM_THEMECHANGED` / `WM_SYSCOLORCHANGE`) change.
#[allow(dead_code)]
pub(crate) fn spawn_mica_preference_listener<F>(on_change: F)
where
    F: Fn() + Send + 'static,
{
    #[cfg(target_os = "windows")]
    {
        let _ = std::thread::Builder::new()
            .name("posthaze-mica-pref-listener".to_string())
            .spawn(move || {
                use windows_sys::Win32::Foundation::{
                    CloseHandle, ERROR_SUCCESS, HANDLE, WAIT_OBJECT_0,
                };
                use windows_sys::Win32::System::Registry::{
                    HKEY, HKEY_CURRENT_USER, KEY_NOTIFY, REG_NOTIFY_CHANGE_LAST_SET, RegCloseKey,
                    RegNotifyChangeKeyValue, RegOpenKeyExW,
                };
                use windows_sys::Win32::System::Threading::{CreateEventW, INFINITE};
                use windows_sys::Win32::UI::WindowsAndMessaging::{
                    CreateWindowExW, DestroyWindow, DispatchMessageW, GetMessageW, MSG,
                    MsgWaitForMultipleObjects, PM_REMOVE, PeekMessageW, QS_ALLINPUT,
                    RegisterClassW, TranslateMessage, WM_QUIT, WNDCLASSW, WS_OVERLAPPED,
                };

                MICA_PREF_CALLBACK.with(|slot| {
                    *slot.borrow_mut() = Some(Box::new(on_change));
                });

                let class_name: Vec<u16> = "PosthazeMicaPrefListener\0".encode_utf16().collect();
                let wc = WNDCLASSW {
                    style: 0,
                    lpfnWndProc: Some(mica_pref_wnd_proc),
                    cbClsExtra: 0,
                    cbWndExtra: 0,
                    hInstance: std::ptr::null_mut(),
                    hIcon: std::ptr::null_mut(),
                    hCursor: std::ptr::null_mut(),
                    hbrBackground: std::ptr::null_mut(),
                    lpszMenuName: std::ptr::null(),
                    lpszClassName: class_name.as_ptr(),
                };
                unsafe {
                    RegisterClassW(&wc);
                }

                // Must be a hidden top-level WS_OVERLAPPED window (never HWND_MESSAGE,
                // which does not receive HWND_BROADCAST WM_SETTINGCHANGE/WM_THEMECHANGED).
                let hwnd = unsafe {
                    CreateWindowExW(
                        0,
                        class_name.as_ptr(),
                        class_name.as_ptr(),
                        WS_OVERLAPPED,
                        0,
                        0,
                        0,
                        0,
                        std::ptr::null_mut(),
                        std::ptr::null_mut(),
                        std::ptr::null_mut(),
                        std::ptr::null(),
                    )
                };

                let mut hkey: HKEY = std::ptr::null_mut();
                let reg_opened = unsafe {
                    RegOpenKeyExW(
                        HKEY_CURRENT_USER,
                        PERSONALIZE_SUBKEY_UTF16.as_ptr(),
                        0,
                        KEY_NOTIFY,
                        &mut hkey,
                    )
                } == ERROR_SUCCESS;

                let reg_event: HANDLE = if reg_opened {
                    unsafe { CreateEventW(std::ptr::null(), 0, 0, std::ptr::null()) }
                } else {
                    std::ptr::null_mut()
                };

                let arm_reg_watch = |key: HKEY, event: HANDLE| -> bool {
                    if key.is_null() || event.is_null() {
                        return false;
                    }
                    unsafe {
                        RegNotifyChangeKeyValue(key, 0, REG_NOTIFY_CHANGE_LAST_SET, event, 1)
                            == ERROR_SUCCESS
                    }
                };

                let mut reg_watch_active = arm_reg_watch(hkey, reg_event);
                let mut msg: MSG = unsafe { std::mem::zeroed() };

                if reg_watch_active {
                    let handles = [reg_event];
                    loop {
                        let wait = unsafe {
                            MsgWaitForMultipleObjects(
                                handles.len() as u32,
                                handles.as_ptr(),
                                0,
                                INFINITE,
                                QS_ALLINPUT,
                            )
                        };
                        if wait == WAIT_OBJECT_0 {
                            reg_watch_active = arm_reg_watch(hkey, reg_event);
                            MICA_PREF_CALLBACK.with(|slot| {
                                if let Some(cb) = slot.borrow().as_ref() {
                                    cb();
                                }
                            });
                            if !reg_watch_active {
                                break;
                            }
                        } else if wait == WAIT_OBJECT_0 + handles.len() as u32 {
                            let mut quit = false;
                            while unsafe {
                                PeekMessageW(&mut msg, std::ptr::null_mut(), 0, 0, PM_REMOVE)
                            } != 0
                            {
                                if msg.message == WM_QUIT {
                                    quit = true;
                                    break;
                                }
                                unsafe {
                                    TranslateMessage(&msg);
                                    DispatchMessageW(&msg);
                                }
                            }
                            if quit {
                                break;
                            }
                        } else {
                            break;
                        }
                    }
                } else if !hwnd.is_null() {
                    while unsafe { GetMessageW(&mut msg, std::ptr::null_mut(), 0, 0) } > 0 {
                        unsafe {
                            TranslateMessage(&msg);
                            DispatchMessageW(&msg);
                        }
                    }
                }

                if !reg_event.is_null() {
                    unsafe { CloseHandle(reg_event) };
                }
                if reg_opened && !hkey.is_null() {
                    unsafe { RegCloseKey(hkey) };
                }
                if !hwnd.is_null() {
                    unsafe { DestroyWindow(hwnd) };
                }
                MICA_PREF_CALLBACK.with(|slot| {
                    *slot.borrow_mut() = None;
                });
            });
    }
    #[cfg(not(target_os = "windows"))]
    let _ = on_change;
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
/// re-applies any previously configured DWM backdrop so restoring from `SW_HIDE`
/// never loses Mica state.
pub(crate) fn show_window_from_tray(window: &gpui_kit::Window) {
    #[cfg(target_os = "windows")]
    let saved_backdrop = if let Some(hwnd) = win32_hwnd(window) {
        unsafe {
            windows_sys::Win32::UI::WindowsAndMessaging::ShowWindowAsync(
                hwnd,
                windows_sys::Win32::UI::WindowsAndMessaging::SW_SHOW,
            );
        }
        last_window_backdrops()
            .lock()
            .ok()
            .and_then(|map| map.get(&(hwnd as isize)).copied())
    } else {
        None
    };
    window.activate_window();
    #[cfg(target_os = "windows")]
    if let Some((backdrop, tone)) = saved_backdrop {
        let _ = apply_window_backdrop(window, backdrop, tone);
    }
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

    use super::{
        MicaFallbackReason, MicaProbeInputs, evaluate_mica_availability, format_startup_command,
        initial_os_appearance, is_direct_composition_disabled_value, parse_apps_use_light_theme,
    };

    #[test]
    fn formats_startup_registry_command_with_minimized_flag() {
        let cmd = format_startup_command(Path::new(r"C:\Users\Test\Posthaze\posthaze-ui.exe"));
        assert_eq!(cmd, "\"C:\\Users\\Test\\Posthaze\\posthaze-ui.exe\" --minimized");
    }

    #[test]
    fn mica_fallback_reason_status_notes_match_spec() {
        assert_eq!(
            MicaFallbackReason::UnsupportedOs.status_note(),
            "Native Mica translucency requires Windows 11 22H2 or newer — using calibrated opaque fallback."
        );
        assert_eq!(
            MicaFallbackReason::DirectCompositionDisabled.status_note(),
            "DirectComposition is disabled (`GPUI_DISABLE_DIRECT_COMPOSITION=1`) — using calibrated opaque fallback."
        );
        assert_eq!(
            MicaFallbackReason::HighContrastActive.status_note(),
            "Windows High Contrast mode is active — using calibrated opaque fallback."
        );
        assert_eq!(
            MicaFallbackReason::TransparencyDisabled.status_note(),
            "Windows Transparency Effects are turned off — using calibrated opaque fallback."
        );
        assert_eq!(
            MicaFallbackReason::DwmRejected.status_note(),
            "Windows DWM declined the Mica backdrop — using calibrated opaque fallback."
        );
    }

    #[test]
    fn evaluate_mica_availability_enforces_strict_priority_order() {
        // All failing at once -> UnsupportedOs wins (priority 1)
        assert_eq!(
            evaluate_mica_availability(MicaProbeInputs {
                windows_build: Some(22000),
                direct_composition_disabled: true,
                high_contrast_active: true,
                transparency_enabled: false,
                dwm_rejected: true,
            }),
            Err(MicaFallbackReason::UnsupportedOs)
        );
        // Non-Windows (None) -> UnsupportedOs
        assert_eq!(
            evaluate_mica_availability(MicaProbeInputs {
                windows_build: None,
                direct_composition_disabled: false,
                high_contrast_active: false,
                transparency_enabled: true,
                dwm_rejected: false,
            }),
            Err(MicaFallbackReason::UnsupportedOs)
        );
        // Build 22621+ -> DirectCompositionDisabled wins (priority 2)
        assert_eq!(
            evaluate_mica_availability(MicaProbeInputs {
                windows_build: Some(22621),
                direct_composition_disabled: true,
                high_contrast_active: true,
                transparency_enabled: false,
                dwm_rejected: true,
            }),
            Err(MicaFallbackReason::DirectCompositionDisabled)
        );
        // HighContrastActive wins (priority 3)
        assert_eq!(
            evaluate_mica_availability(MicaProbeInputs {
                windows_build: Some(22631),
                direct_composition_disabled: false,
                high_contrast_active: true,
                transparency_enabled: false,
                dwm_rejected: true,
            }),
            Err(MicaFallbackReason::HighContrastActive)
        );
        // TransparencyDisabled wins (priority 4)
        assert_eq!(
            evaluate_mica_availability(MicaProbeInputs {
                windows_build: Some(26100),
                direct_composition_disabled: false,
                high_contrast_active: false,
                transparency_enabled: false,
                dwm_rejected: true,
            }),
            Err(MicaFallbackReason::TransparencyDisabled)
        );
        // DwmRejected wins (priority 5)
        assert_eq!(
            evaluate_mica_availability(MicaProbeInputs {
                windows_build: Some(26100),
                direct_composition_disabled: false,
                high_contrast_active: false,
                transparency_enabled: true,
                dwm_rejected: true,
            }),
            Err(MicaFallbackReason::DwmRejected)
        );
        // All clear -> Ok(())
        assert_eq!(
            evaluate_mica_availability(MicaProbeInputs {
                windows_build: Some(22621),
                direct_composition_disabled: false,
                high_contrast_active: false,
                transparency_enabled: true,
                dwm_rejected: false,
            }),
            Ok(())
        );
    }

    #[test]
    fn apps_use_light_theme_maps_dword_to_window_appearance() {
        assert_eq!(
            parse_apps_use_light_theme(Some(0)),
            gpui_kit::WindowAppearance::Dark
        );
        assert_eq!(
            parse_apps_use_light_theme(Some(1)),
            gpui_kit::WindowAppearance::Light
        );
        assert_eq!(
            parse_apps_use_light_theme(Some(42)),
            gpui_kit::WindowAppearance::Light
        );
        assert_eq!(
            parse_apps_use_light_theme(None),
            gpui_kit::WindowAppearance::Dark
        );
    }

    #[test]
    fn initial_os_appearance_smoke_check_returns_dark_or_light() {
        let appearance = initial_os_appearance();
        assert!(matches!(
            appearance,
            gpui_kit::WindowAppearance::Dark | gpui_kit::WindowAppearance::Light
        ));
    }

    #[test]
    fn direct_composition_env_parser_recognizes_disabled_values() {
        assert!(is_direct_composition_disabled_value(Some("1")));
        assert!(is_direct_composition_disabled_value(Some("true")));
        assert!(is_direct_composition_disabled_value(Some("TRUE")));
        assert!(!is_direct_composition_disabled_value(Some("0")));
        assert!(!is_direct_composition_disabled_value(Some("false")));
        assert!(!is_direct_composition_disabled_value(Some("")));
        assert!(!is_direct_composition_disabled_value(None));
    }

    #[test]
    fn mica_availability_smoke_check_runs_cleanly() {
        let result = super::mica_availability();
        if let Err(reason) = result {
            assert!(!reason.status_note().is_empty());
        }
    }
}
