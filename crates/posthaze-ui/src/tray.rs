//! Windows System Tray icon and menu event bridge for `posthaze-ui`.

use tokio::sync::mpsc;

/// Actions triggered from the Windows System Tray icon or context menu.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum TrayAction {
    /// Toggle visibility / foreground activation of the main Posthaze window.
    ToggleShowHide,
    /// Pause all `Active` `Download`s in the `Queue`.
    PauseAll,
    /// Exit the application cleanly regardless of Close-to-Tray.
    Quit,
}

/// Keeps the underlying OS tray icon alive while dropped only on full application exit.
#[cfg(target_os = "windows")]
pub(crate) struct TrayHandle {
    _tray: tray_icon::TrayIcon,
}

/// Placeholder tray handle on non-Windows targets.
#[cfg(not(target_os = "windows"))]
pub(crate) struct TrayHandle;

#[cfg(target_os = "windows")]
fn load_tray_icon() -> Option<tray_icon::Icon> {
    use image::imageops::FilterType;
    use tray_icon::Icon;

    const TRAY_ICON_PNG: &[u8] = include_bytes!("../../../assets/posthaze_logo_no_back.png");
    if let Ok(img) = image::load_from_memory(TRAY_ICON_PNG) {
        let resized = img.resize_exact(32, 32, FilterType::Lanczos3).to_rgba8();
        let (w, h) = resized.dimensions();
        if let Ok(icon) = Icon::from_rgba(resized.into_raw(), w, h) {
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

    let icon = load_tray_icon()?;

    let show_item = MenuItem::new("Show / Hide Posthaze", true, None);
    let pause_item = MenuItem::new("Pause All Downloads", true, None);
    let quit_item = MenuItem::new("Quit Posthaze", true, None);

    let show_id = show_item.id().clone();
    let pause_id = pause_item.id().clone();
    let quit_id = quit_item.id().clone();

    let menu = Menu::new();
    let _ = menu.append(&show_item);
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

    Some((TrayHandle { _tray: tray }, rx))
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
        assert!(load_tray_icon().is_some());
    }
}
