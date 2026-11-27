//! Cross-platform Windows 11 DWM Mica backdrop support, preference listener,
//! and multi-window theme/backdrop synchronization.
//!
//! All `windows-sys` and `raw-window-handle` calls are gated behind
//! `#[cfg(all(target_os = "windows", feature = "mica"))]`. Non-Windows targets
//! and `--no-default-features` Windows builds expose identical public signatures
//! as safe opaque stubs.

use gpui_kit::{App, Subscription, Window, WindowAppearance};

use crate::palette::{PaletteTone, ThemeBackdrop, ThemeBadge, ThemeCatalog, ThemePalette};
use crate::theme::apply_theme_with_accent;

/// Minimum Windows build number supporting `DWMWA_SYSTEMBACKDROP_TYPE` (Windows 11 22H2).
pub const MIN_MICA_WINDOWS_BUILD: u32 = 22621;

/// Reason why native Windows 11 Mica translucency is unavailable and the UI
/// has fallen back to a calibrated opaque palette.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum MicaFallbackReason {
    /// OS is non-Windows, `mica` feature is disabled, or Windows build `< 22621`.
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
    /// Returns a human-readable status note describing why Mica fell back to opaque.
    pub const fn status_note(self) -> &'static str {
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
pub struct MicaProbeInputs {
    /// Detected Windows build number (`None` on non-Windows or when unavailable).
    pub windows_build: Option<u32>,
    /// Whether `GPUI_DISABLE_DIRECT_COMPOSITION` is enabled.
    pub direct_composition_disabled: bool,
    /// Whether Windows High Contrast mode is active.
    pub high_contrast_active: bool,
    /// Whether Windows Transparency Effects are enabled.
    pub transparency_enabled: bool,
    /// Whether a prior DWM backdrop call failed with a negative `HRESULT`.
    pub dwm_rejected: bool,
}

/// Evaluates Mica availability in strict priority order.
pub fn evaluate_mica_availability(inputs: MicaProbeInputs) -> Result<(), MicaFallbackReason> {
    if inputs
        .windows_build
        .is_none_or(|build| build < MIN_MICA_WINDOWS_BUILD)
    {
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
pub fn is_direct_composition_disabled_value(value: Option<&str>) -> bool {
    matches!(value, Some(v) if v == "1" || v.eq_ignore_ascii_case("true"))
}

/// Maps Windows `AppsUseLightTheme` registry DWORD value to GPUI's [`WindowAppearance`].
pub fn parse_apps_use_light_theme(value: Option<u32>) -> WindowAppearance {
    match value {
        Some(v) if v != 0 => WindowAppearance::Light,
        _ => WindowAppearance::Dark,
    }
}

/// Formats the Win32 window class name for a Mica preference listener instance.
#[cfg_attr(not(all(target_os = "windows", feature = "mica")), allow(dead_code))]
fn listener_class_name(id: u64) -> String {
    if id == 0 {
        "ChronophotoMicaPrefListener\0".to_string()
    } else {
        format!("ChronophotoMicaPrefListener-{id}\0")
    }
}

#[cfg(all(target_os = "windows", feature = "mica"))]
mod imp {
    use std::sync::atomic::{AtomicBool, AtomicU64, Ordering};
    use std::sync::{OnceLock, mpsc};
    use std::thread::JoinHandle;

    use gpui_kit::{Window, WindowAppearance, WindowBackgroundAppearance};

    use super::{
        MIN_MICA_WINDOWS_BUILD, MicaFallbackReason, MicaProbeInputs, evaluate_mica_availability,
        is_direct_composition_disabled_value, listener_class_name, parse_apps_use_light_theme,
    };
    use crate::palette::{PaletteTone, ThemeBackdrop};

    const PERSONALIZE_SUBKEY: &str =
        "Software\\Microsoft\\Windows\\CurrentVersion\\Themes\\Personalize\0";
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

    static DWM_REJECTED: AtomicBool = AtomicBool::new(false);
    static NEXT_LISTENER_ID: AtomicU64 = AtomicU64::new(0);

    fn win32_hwnd(window: &Window) -> Option<windows_sys::Win32::Foundation::HWND> {
        use raw_window_handle::{HasWindowHandle, RawWindowHandle};
        let handle = HasWindowHandle::window_handle(window).ok()?;
        match handle.as_raw() {
            RawWindowHandle::Win32(h) => {
                Some(h.hwnd.get() as windows_sys::Win32::Foundation::HWND)
            }
            _ => None,
        }
    }

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

    fn is_transparency_enabled() -> bool {
        read_personalize_dword("EnableTransparency").is_none_or(|data| data != 0)
    }

    pub(super) fn initial_os_appearance() -> WindowAppearance {
        parse_apps_use_light_theme(read_personalize_dword("AppsUseLightTheme"))
    }

    pub(super) fn mica_availability() -> Result<(), MicaFallbackReason> {
        let env_val = std::env::var("GPUI_DISABLE_DIRECT_COMPOSITION").ok();
        evaluate_mica_availability(MicaProbeInputs {
            windows_build: windows_build_number(),
            direct_composition_disabled: is_direct_composition_disabled_value(env_val.as_deref()),
            high_contrast_active: is_high_contrast_active(),
            transparency_enabled: is_transparency_enabled(),
            dwm_rejected: DWM_REJECTED.load(Ordering::Relaxed),
        })
    }

    fn set_dwm_dark_mode(hwnd: windows_sys::Win32::Foundation::HWND, is_dark: bool) -> i32 {
        use windows_sys::Win32::Graphics::Dwm::{
            DWMWA_USE_IMMERSIVE_DARK_MODE, DwmSetWindowAttribute,
        };

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

    fn set_dwm_backdrop_type(
        hwnd: windows_sys::Win32::Foundation::HWND,
        backdrop_type: i32,
    ) -> i32 {
        use windows_sys::Win32::Graphics::Dwm::{
            DWMWA_SYSTEMBACKDROP_TYPE, DwmSetWindowAttribute,
        };

        unsafe {
            DwmSetWindowAttribute(
                hwnd,
                DWMWA_SYSTEMBACKDROP_TYPE as u32,
                (&backdrop_type as *const i32).cast::<std::ffi::c_void>(),
                std::mem::size_of::<i32>() as u32,
            )
        }
    }

    fn check_dwm_hr(hr: i32, op: &'static str, window: &Window) -> Result<(), String> {
        if hr < 0 {
            DWM_REJECTED.store(true, Ordering::Relaxed);
            window.set_background_appearance(WindowBackgroundAppearance::Opaque);
            return Err(format!("{op} failed (HRESULT 0x{:08X})", hr as u32));
        }
        Ok(())
    }

    pub(super) fn apply_window_backdrop(
        window: &Window,
        backdrop: ThemeBackdrop,
        tone: PaletteTone,
    ) -> Result<(), String> {
        use windows_sys::Win32::Graphics::Dwm::{
            DWMSBT_MAINWINDOW, DWMSBT_NONE, DWMSBT_TABBEDWINDOW,
        };

        window.set_background_appearance(backdrop.to_window_background());

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
                )?;
                check_dwm_hr(
                    set_dwm_backdrop_type(hwnd, backdrop_value),
                    "DWMWA_SYSTEMBACKDROP_TYPE",
                    window,
                )?;
            }
            ThemeBackdrop::Opaque => {
                if windows_build_number().is_some_and(|build| build >= MIN_MICA_WINDOWS_BUILD) {
                    let _ = set_dwm_backdrop_type(hwnd, DWMSBT_NONE);
                }
                let _ = set_dwm_dark_mode(hwnd, is_dark);
            }
        }

        Ok(())
    }

    pub(super) fn assert_window_dark_mode(window: &Window, is_dark: bool) {
        if let Some(hwnd) = win32_hwnd(window) {
            let _ = set_dwm_dark_mode(hwnd, is_dark);
        }
    }

    std::thread_local! {
        static MICA_PREF_CALLBACK: std::cell::RefCell<Option<Box<dyn Fn()>>> =
            const { std::cell::RefCell::new(None) };
    }

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

    pub(super) struct ListenerInner {
        thread_id: u32,
        join_handle: JoinHandle<()>,
    }

    impl ListenerInner {
        pub(super) fn shutdown(self) {
            use windows_sys::Win32::UI::WindowsAndMessaging::{PostThreadMessageW, WM_QUIT};
            unsafe {
                PostThreadMessageW(self.thread_id, WM_QUIT, 0, 0);
            }
            if std::thread::current().id() != self.join_handle.thread().id() {
                let _ = self.join_handle.join();
            }
        }
    }

    pub(super) fn spawn_mica_preference_listener<F>(on_change: F) -> Option<ListenerInner>
    where
        F: Fn() + Send + 'static,
    {
        let listener_id = NEXT_LISTENER_ID.fetch_add(1, Ordering::Relaxed);
        let class_name_utf8 = listener_class_name(listener_id);
        let (tx, rx) = mpsc::sync_channel::<u32>(1);

        let join_handle = std::thread::Builder::new()
            .name(format!("chronophoto-mica-pref-listener-{listener_id}"))
            .spawn(move || {
                use windows_sys::Win32::Foundation::{
                    CloseHandle, ERROR_SUCCESS, HANDLE, WAIT_OBJECT_0,
                };
                use windows_sys::Win32::System::Registry::{
                    HKEY, HKEY_CURRENT_USER, KEY_NOTIFY, REG_NOTIFY_CHANGE_LAST_SET, RegCloseKey,
                    RegNotifyChangeKeyValue, RegOpenKeyExW,
                };
                use windows_sys::Win32::System::Threading::{
                    CreateEventW, GetCurrentThreadId, INFINITE,
                };
                use windows_sys::Win32::UI::WindowsAndMessaging::{
                    CreateWindowExW, DestroyWindow, DispatchMessageW, GetMessageW, MSG,
                    MsgWaitForMultipleObjects, PM_REMOVE, PeekMessageW, QS_ALLINPUT,
                    RegisterClassW, TranslateMessage, UnregisterClassW, WM_QUIT, WNDCLASSW,
                    WS_OVERLAPPED,
                };

                MICA_PREF_CALLBACK.with(|slot| {
                    *slot.borrow_mut() = Some(Box::new(on_change));
                });

                let class_name: Vec<u16> = class_name_utf8.encode_utf16().collect();
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
                let class_atom = unsafe { RegisterClassW(&wc) };

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

                let mut msg: MSG = unsafe { std::mem::zeroed() };
                // Ensure the thread message queue exists before publishing the thread ID.
                unsafe {
                    PeekMessageW(&mut msg, std::ptr::null_mut(), 0, 0, 0);
                }
                let _ = tx.send(unsafe { GetCurrentThreadId() });

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
                if class_atom != 0 {
                    unsafe { UnregisterClassW(class_name.as_ptr(), std::ptr::null_mut()) };
                }
                MICA_PREF_CALLBACK.with(|slot| {
                    *slot.borrow_mut() = None;
                });
            })
            .ok()?;

        let thread_id = rx.recv().ok()?;
        Some(ListenerInner {
            thread_id,
            join_handle,
        })
    }
}

#[cfg(not(all(target_os = "windows", feature = "mica")))]
mod imp {
    use gpui_kit::{Window, WindowAppearance, WindowBackgroundAppearance};

    use super::{MicaFallbackReason, parse_apps_use_light_theme};
    use crate::palette::{PaletteTone, ThemeBackdrop};

    pub(super) struct ListenerInner;

    impl ListenerInner {
        pub(super) fn shutdown(self) {}
    }

    pub(super) fn initial_os_appearance() -> WindowAppearance {
        parse_apps_use_light_theme(None)
    }

    pub(super) fn mica_availability() -> Result<(), MicaFallbackReason> {
        Err(MicaFallbackReason::UnsupportedOs)
    }

    pub(super) fn apply_window_backdrop(
        window: &Window,
        backdrop: ThemeBackdrop,
        _tone: PaletteTone,
    ) -> Result<(), String> {
        window.set_background_appearance(WindowBackgroundAppearance::Opaque);
        match backdrop {
            ThemeBackdrop::Opaque => Ok(()),
            ThemeBackdrop::Mica | ThemeBackdrop::MicaAlt => {
                Err("Mica backdrop is unavailable without Windows mica support".to_string())
            }
        }
    }

    pub(super) fn assert_window_dark_mode(_window: &Window, _is_dark: bool) {}

    pub(super) fn spawn_mica_preference_listener<F>(_on_change: F) -> Option<ListenerInner>
    where
        F: Fn() + Send + 'static,
    {
        None
    }
}

/// RAII handle for a background Mica preference listener thread.
///
/// Dropping this handle posts `WM_QUIT` to the listener thread and waits for it
/// to exit. Call [`MicaPreferenceHandle::detach`] to keep the listener running
/// for the lifetime of the process.
pub struct MicaPreferenceHandle {
    inner: Option<imp::ListenerInner>,
}

impl MicaPreferenceHandle {
    /// Detaches the listener so it continues running for the lifetime of the process.
    pub fn detach(mut self) {
        self.inner = None;
    }
}

impl Drop for MicaPreferenceHandle {
    fn drop(&mut self) {
        if let Some(inner) = self.inner.take() {
            inner.shutdown();
        }
    }
}

/// Queries the initial OS appearance before any GPUI window is open so `Mica (System)`
/// opens on frame 1 with the correct Dark/Light polarity.
pub fn initial_os_appearance() -> WindowAppearance {
    imp::initial_os_appearance()
}

/// Checks whether native Windows 11 Mica translucency is currently available.
pub fn mica_availability() -> Result<(), MicaFallbackReason> {
    imp::mica_availability()
}

/// Applies the requested window backdrop (`Opaque`, `Mica`, or `MicaAlt`) and
/// immersive dark-mode polarity to a GPUI window.
pub fn apply_window_backdrop(
    window: &Window,
    backdrop: ThemeBackdrop,
    tone: PaletteTone,
) -> Result<(), String> {
    imp::apply_window_backdrop(window, backdrop, tone)
}

/// Re-asserts Win32 `DWMWA_USE_IMMERSIVE_DARK_MODE` on a GPUI window.
///
/// Intended to be called in a deferred task (`window.defer(...)`) after GPUI's
/// `WM_SETTINGCHANGE` handler runs so forced-polarity Mica themes keep their
/// DWM dark-mode bit.
pub fn assert_window_dark_mode(window: &Window, is_dark: bool) {
    imp::assert_window_dark_mode(window, is_dark);
}

/// Spawns a background listener that invokes `on_change` whenever Windows
/// Transparency Effects or High Contrast settings change.
///
/// On non-Windows targets or when the `mica` feature is disabled, returns a
/// no-op [`MicaPreferenceHandle`].
pub fn spawn_mica_preference_listener<F>(on_change: F) -> MicaPreferenceHandle
where
    F: Fn() + Send + 'static,
{
    MicaPreferenceHandle {
        inner: imp::spawn_mica_preference_listener(on_change),
    }
}

/// Returns `true` when `theme_id` forces a specific Mica dark/light polarity
/// (`"mica-dark"` or `"mica-light"`) rather than following the OS appearance.
pub fn is_forced_mica_polarity(theme_id: &str) -> bool {
    let catalog = ThemeCatalog::new();
    let preset = catalog.find_theme_preset(theme_id);
    preset.is_mica && preset.badge != ThemeBadge::Auto
}

fn apply_palette_to_all_windows(
    palette: ThemePalette,
    current_window: &mut Option<&mut Window>,
    cx: &mut App,
) -> bool {
    let mut any_failed = false;

    let current_handle = if let Some(win) = current_window.as_deref_mut() {
        if apply_window_backdrop(win, palette.backdrop, palette.tone).is_err() {
            any_failed = true;
        }
        win.refresh();
        Some(win.window_handle())
    } else {
        None
    };

    for handle in cx.windows() {
        if current_handle == Some(handle) {
            continue;
        }
        let _ = handle.update(cx, |_, win, _| {
            if apply_window_backdrop(win, palette.backdrop, palette.tone).is_err() {
                any_failed = true;
            }
            win.refresh();
        });
    }

    any_failed
}

fn sync_theme_and_backdrops_inner(
    theme_id: &str,
    accent_override: Option<&str>,
    mut current_window: Option<&mut Window>,
    cx: &mut App,
) -> ThemePalette {
    let os_appearance = if let Some(win) = current_window.as_deref() {
        win.appearance()
    } else {
        cx.windows()
            .into_iter()
            .find_map(|handle| handle.update(cx, |_, win, _| win.appearance()).ok())
            .unwrap_or_else(initial_os_appearance)
    };

    let mica_available = mica_availability().is_ok();
    let mut palette =
        apply_theme_with_accent(theme_id, accent_override, os_appearance, mica_available, cx);

    let any_failed = apply_palette_to_all_windows(palette, &mut current_window, cx);
    if any_failed && palette.backdrop != ThemeBackdrop::Opaque {
        palette = apply_theme_with_accent(theme_id, accent_override, os_appearance, false, cx);
        let _ = apply_palette_to_all_windows(palette, &mut current_window, cx);
    }

    palette
}

/// Synchronizes the active [`ThemePalette`], `ChronoTheme`, GPUI Kit tokens, and
/// DWM backdrops across every open window.
pub fn sync_theme_and_backdrops(
    theme_id: &str,
    accent_override: Option<&str>,
    cx: &mut App,
) -> ThemePalette {
    sync_theme_and_backdrops_inner(theme_id, accent_override, None, cx)
}

/// Synchronizes the active [`ThemePalette`], `ChronoTheme`, GPUI Kit tokens, and
/// DWM backdrops across the currently borrowed `window` and all other open windows.
pub fn sync_theme_and_backdrops_in_window(
    theme_id: &str,
    accent_override: Option<&str>,
    window: &mut Window,
    cx: &mut App,
) -> ThemePalette {
    sync_theme_and_backdrops_inner(theme_id, accent_override, Some(window), cx)
}

/// Observes OS window appearance changes (`WM_SETTINGCHANGE`) to re-sync
/// `Mica (System)` and defer `assert_window_dark_mode` for forced-polarity Mica themes.
pub fn observe_window_backdrop_sync<F>(window: &mut Window, mut selection_fn: F) -> Subscription
where
    F: FnMut(&mut App) -> Option<(String, Option<String>)> + 'static,
{
    window.observe_window_appearance(move |window, cx| {
        let Some((theme_id, accent_override)) = selection_fn(cx) else {
            return;
        };
        let palette =
            sync_theme_and_backdrops_in_window(&theme_id, accent_override.as_deref(), window, cx);
        if is_forced_mica_polarity(&theme_id) {
            let is_dark = palette.tone == PaletteTone::Dark;
            window.defer(cx, move |window, cx| {
                assert_window_dark_mode(window, is_dark);
                let current_handle = window.window_handle();
                for handle in cx.windows() {
                    if handle == current_handle {
                        continue;
                    }
                    let _ = handle.update(cx, |_, win, _| {
                        assert_window_dark_mode(win, is_dark);
                    });
                }
            });
        }
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn mica_fallback_reason_status_notes_are_non_empty() {
        for reason in [
            MicaFallbackReason::UnsupportedOs,
            MicaFallbackReason::DirectCompositionDisabled,
            MicaFallbackReason::HighContrastActive,
            MicaFallbackReason::TransparencyDisabled,
            MicaFallbackReason::DwmRejected,
        ] {
            assert!(!reason.status_note().is_empty());
        }
    }

    #[test]
    fn evaluate_mica_availability_enforces_strict_priority_order() {
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
        assert_eq!(parse_apps_use_light_theme(Some(0)), WindowAppearance::Dark);
        assert_eq!(
            parse_apps_use_light_theme(Some(1)),
            WindowAppearance::Light
        );
        assert_eq!(
            parse_apps_use_light_theme(Some(42)),
            WindowAppearance::Light
        );
        assert_eq!(parse_apps_use_light_theme(None), WindowAppearance::Dark);
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
    fn listener_class_names_are_unique_and_nul_terminated() {
        let first = listener_class_name(0);
        let second = listener_class_name(1);
        let third = listener_class_name(2);
        assert_eq!(first, "ChronophotoMicaPrefListener\0");
        assert!(second.starts_with("ChronophotoMicaPrefListener-"));
        assert!(second.ends_with('\0'));
        assert_ne!(second, third);
    }

    #[test]
    fn is_forced_mica_polarity_identifies_forced_presets_only() {
        assert!(is_forced_mica_polarity("mica-dark"));
        assert!(is_forced_mica_polarity("mica-light"));
        assert!(!is_forced_mica_polarity("mica-system"));
        assert!(!is_forced_mica_polarity("chronophotographic"));
        assert!(!is_forced_mica_polarity("unknown-preset"));
    }

    #[test]
    fn mica_availability_and_listener_handle_smoke_check() {
        let result = mica_availability();
        #[cfg(not(all(target_os = "windows", feature = "mica")))]
        assert_eq!(result, Err(MicaFallbackReason::UnsupportedOs));
        #[cfg(all(target_os = "windows", feature = "mica"))]
        if let Err(reason) = result {
            assert!(!reason.status_note().is_empty());
        }

        let appearance = initial_os_appearance();
        assert!(matches!(
            appearance,
            WindowAppearance::Dark | WindowAppearance::Light
        ));

        let handle1 = spawn_mica_preference_listener(|| {});
        let handle2 = spawn_mica_preference_listener(|| {});
        drop(handle1);
        drop(handle2);
    }
}
