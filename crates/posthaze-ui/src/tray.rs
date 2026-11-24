//! Windows System Tray icon and menu event bridge for `posthaze-ui`.

use tokio::sync::mpsc;

/// Actions triggered from the Windows System Tray icon or context menu.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum TrayAction {
    /// Toggle visibility / foreground activation of the main Posthaze window.
    ToggleShowHide,
    /// Open or foreground the standalone `SettingsWindow`.
    OpenSettings,
    /// Pause all `Active` `Download`s in the `Queue`.
    PauseAll,
    /// Exit the application cleanly regardless of Close-to-Tray.
    Quit,
}

/// Keeps the underlying OS tray icon alive while dropped only on full application exit.
#[cfg(target_os = "windows")]
pub(crate) struct TrayHandle {
    _tray: tray_icon::TrayIcon,
    last_light_taskbar: bool,
}

/// Placeholder tray handle on non-Windows targets.
#[cfg(not(target_os = "windows"))]
pub(crate) struct TrayHandle;

impl TrayHandle {
    /// Updates the tray icon polarity if Windows' `SystemUsesLightTheme` setting changed.
    pub(crate) fn sync_system_theme(&mut self) {
        #[cfg(target_os = "windows")]
        {
            let light_taskbar = crate::platform_hooks::system_uses_light_theme();
            if light_taskbar != self.last_light_taskbar {
                if let Some(icon) = load_tray_icon(light_taskbar) {
                    let _ = self._tray.set_icon(Some(icon));
                }
                self.last_light_taskbar = light_taskbar;
            }
        }
    }
}

#[cfg(target_os = "windows")]
fn load_tray_icon(light_taskbar: bool) -> Option<tray_icon::Icon> {
    use image::imageops::FilterType;
    use tray_icon::Icon;

    // Dark ink on light taskbars; light ink on dark taskbars.
    const TRAY_ICON_LIGHT_PNG: &[u8] =
        include_bytes!("../../../assets/Logo Light/Logo Light 32x32.png");
    const TRAY_ICON_DARK_PNG: &[u8] =
        include_bytes!("../../../assets/Logo Dark/Logo Dark 32x32.png");

    let bytes = if light_taskbar {
        TRAY_ICON_DARK_PNG
    } else {
        TRAY_ICON_LIGHT_PNG
    };
    if let Ok(img) = image::load_from_memory(bytes) {
        let rgba = if img.width() == 32 && img.height() == 32 {
            img.to_rgba8()
        } else {
            img.resize_exact(32, 32, FilterType::Lanczos3).to_rgba8()
        };
        let (w, h) = rgba.dimensions();
        if let Ok(icon) = Icon::from_rgba(rgba.into_raw(), w, h) {
            return Some(icon);
        }
    }
    Icon::from_resource(1, Some((32, 32))).ok()
}

/// Creates the Windows System Tray icon (`Show/Hide`, `Pause All`, `Quit`) and returns a
/// [`TrayHandle`] paired with an async receiver of [`TrayAction`]s.
#[cfg(target_os = "windows")]
pub(crate) fn init_tray() -> Option<(TrayHandle, mpsc::UnboundedReceiver<TrayAction>)> {
    use tray_icon::menu::{Menu, MenuEvent, MenuItem, PredefinedMenuItem};
    use tray_icon::{MouseButton, MouseButtonState, TrayIconBuilder, TrayIconEvent};

    let light_taskbar = crate::platform_hooks::system_uses_light_theme();
    let icon = load_tray_icon(light_taskbar)?;

    let show_item = MenuItem::new("Show / Hide Posthaze", true, None);
    let settings_item = MenuItem::new("Settings...", true, None);
    let pause_item = MenuItem::new("Pause All Downloads", true, None);
    let quit_item = MenuItem::new("Quit Posthaze", true, None);

    let show_id = show_item.id().clone();
    let settings_id = settings_item.id().clone();
    let pause_id = pause_item.id().clone();
    let quit_id = quit_item.id().clone();

    let menu = Menu::new();
    let _ = menu.append(&show_item);
    let _ = menu.append(&settings_item);
    let _ = menu.append(&pause_item);
    let _ = menu.append(&PredefinedMenuItem::separator());
    let _ = menu.append(&quit_item);

    let tray = TrayIconBuilder::new()
        .with_menu(Box::new(menu))
        .with_tooltip("Posthaze Download Manager")
        .with_icon(icon)
        .build()
        .ok()?;

    let (tx, rx) = mpsc::unbounded_channel();

    let tx_menu = tx.clone();
    MenuEvent::set_event_handler(Some(move |ev: MenuEvent| {
        let action = if ev.id == show_id {
            Some(TrayAction::ToggleShowHide)
        } else if ev.id == settings_id {
            Some(TrayAction::OpenSettings)
        } else if ev.id == pause_id {
            Some(TrayAction::PauseAll)
        } else if ev.id == quit_id {
            Some(TrayAction::Quit)
        } else {
            None
        };
        if let Some(act) = action {
            let _ = tx_menu.send(act);
        }
    }));

    TrayIconEvent::set_event_handler(Some(move |ev: TrayIconEvent| {
        if let TrayIconEvent::Click {
            button: MouseButton::Left,
            button_state: MouseButtonState::Up,
            ..
        } = ev
        {
            let _ = tx.send(TrayAction::ToggleShowHide);
        }
    }));

    Some((
        TrayHandle {
            _tray: tray,
            last_light_taskbar: light_taskbar,
        },
        rx,
    ))
}

/// No-op tray initialization on non-Windows targets.
#[cfg(not(target_os = "windows"))]
pub(crate) fn init_tray() -> Option<(TrayHandle, mpsc::UnboundedReceiver<TrayAction>)> {
    None
}

#[cfg(all(test, target_os = "windows"))]
mod tests {
    use super::load_tray_icon;

    #[test]
    fn loads_embedded_tray_icon_rgba() {
        assert!(load_tray_icon(false).is_some());
        assert!(load_tray_icon(true).is_some());
    }
}
