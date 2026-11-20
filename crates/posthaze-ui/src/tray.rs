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

/// Creates the Windows System Tray icon (`Show/Hide`, `Pause All`, `Quit`) and returns a
/// [`TrayHandle`] paired with an async receiver of [`TrayAction`]s.
#[cfg(target_os = "windows")]
pub(crate) fn init_tray() -> Option<(TrayHandle, mpsc::UnboundedReceiver<TrayAction>)> {
    use tray_icon::menu::{Menu, MenuEvent, MenuItem, PredefinedMenuItem};
    use tray_icon::{Icon, MouseButton, MouseButtonState, TrayIconBuilder, TrayIconEvent};

    let icon = Icon::from_resource(1, Some((32, 32)))
        .or_else(|_| Icon::from_rgba(vec![229, 138, 53, 255], 1, 1))
        .ok()?;

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
    std::thread::Builder::new()
        .name("posthaze-tray-events".to_string())
        .spawn(move || {
            let menu_rx = MenuEvent::receiver();
            let tray_rx = TrayIconEvent::receiver();
            loop {
                while let Ok(ev) = menu_rx.try_recv() {
                    let action = if ev.id == show_id {
                        Some(TrayAction::ToggleShowHide)
                    } else if ev.id == pause_id {
                        Some(TrayAction::PauseAll)
                    } else if ev.id == quit_id {
                        Some(TrayAction::Quit)
                    } else {
                        None
                    };
                    if let Some(act) = action
                        && tx_menu.send(act).is_err()
                    {
                        return;
                    }
                }
                while let Ok(ev) = tray_rx.try_recv() {
                    if let TrayIconEvent::Click {
                        button: MouseButton::Left,
                        button_state: MouseButtonState::Up,
                        ..
                    } = ev
                        && tx_menu.send(TrayAction::ToggleShowHide).is_err()
                    {
                        return;
                    }
                }
                std::thread::sleep(std::time::Duration::from_millis(50));
            }
        })
        .ok();

    Some((TrayHandle { _tray: tray }, rx))
}

/// No-op tray initialization on non-Windows targets.
#[cfg(not(target_os = "windows"))]
pub(crate) fn init_tray() -> Option<(TrayHandle, mpsc::UnboundedReceiver<TrayAction>)> {
    None
}
