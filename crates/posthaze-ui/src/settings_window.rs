//! Independent, single-instance GPUI window for application and engine Settings.

use gpui_kit::component::input::InputEvent;
use gpui_kit::component::TitleBar;
use gpui_kit::*;
use posthaze_engine::{DownloadCategory, Settings};

use crate::PosthazeApp;
use crate::assets::BRAND_LOGO_SVG;
use crate::bridge::EngineBridge;
use crate::browser_integration::{
    query_browser_statuses, register_native_host, resolve_cli_executable,
};
use crate::platform_hooks::{open_directory_in_shell, open_url_in_browser};
use crate::settings_modal::{
    SETTINGS_SIDEBAR_WIDTH, SettingsAction, SettingsModalState, SettingsTab,
    render_settings_window_body,
};
use crate::theme::*;
use crate::updater::UpdateState;

const SETTINGS_WINDOW_WIDTH: f32 = 840.0;
const SETTINGS_WINDOW_HEIGHT: f32 = 580.0;
const SETTINGS_WINDOW_MIN_WIDTH: f32 = 700.0;
const SETTINGS_WINDOW_MIN_HEIGHT: f32 = 480.0;

/// Single-instance window handle and weak entity pair for [`SettingsWindow`].
#[derive(Clone)]
pub(crate) struct SettingsWindowHandle {
    pub(crate) window: AnyWindowHandle,
    pub(crate) entity: WeakEntity<SettingsWindow>,
}

/// Standalone resizable OS window hosting the multi-tab `Settings` editor.
pub(crate) struct SettingsWindow {
    bridge: EngineBridge,
    state: SettingsModalState,
    update_state: UpdateState,
    is_portable: bool,
    main_app: WeakEntity<PosthazeApp>,
    focus_handle: FocusHandle,
    last_zoom_percent: Option<u16>,
    _appearance_subscription: Subscription,
}

impl SettingsWindow {
    /// Opens the standalone `SettingsWindow` centered on screen with custom `TitleBar` chrome.
    #[allow(clippy::too_many_arguments)]
    pub(crate) fn open(
        bridge: EngineBridge,
        settings: Settings,
        categories: Vec<DownloadCategory>,
        update_state: UpdateState,
        is_portable: bool,
        initial_tab: SettingsTab,
        main_app: WeakEntity<PosthazeApp>,
        cx: &mut App,
    ) -> Option<SettingsWindowHandle> {
        let bounds = Bounds::centered(
            None,
            size(px(SETTINGS_WINDOW_WIDTH), px(SETTINGS_WINDOW_HEIGHT)),
            cx,
        );
        let active_palette = active_theme();
        let main_app_for_fallback = main_app.clone();
        let fallback_theme = settings.ui_theme.clone();
        let fallback_accent = settings.ui_accent_override.clone();

        let options = WindowOptions {
            window_bounds: Some(WindowBounds::Windowed(bounds)),
            window_background: active_palette.backdrop.to_window_background(),
            focus: true,
            show: true,
            kind: WindowKind::Normal,
            is_resizable: true,
            window_min_size: Some(size(
                px(SETTINGS_WINDOW_MIN_WIDTH),
                px(SETTINGS_WINDOW_MIN_HEIGHT),
            )),
            ..TitleBar::window_options()
        };

        let Ok((handle, entity)) = gpui_kit::open_window(options, cx, move |window, cx| {
            let entity = cx.new(|cx| {
                Self::new(
                    bridge,
                    settings,
                    categories,
                    update_state,
                    is_portable,
                    initial_tab,
                    main_app,
                    window,
                    cx,
                )
            });
            window.activate_window();
            entity
        }) else {
            return None;
        };

        let backdrop_failed = handle
            .update(cx, |_, window, _| {
                let res = crate::platform_hooks::apply_window_backdrop(
                    window,
                    active_palette.backdrop,
                    active_palette.tone,
                );
                window.activate_window();
                res.is_err()
            })
            .unwrap_or(false);
        if backdrop_failed && active_palette.backdrop != ThemeBackdrop::Opaque {
            let (active_theme_id, active_accent) = main_app_for_fallback
                .update(cx, |app, _| app.active_theme_selection())
                .unwrap_or((fallback_theme, fallback_accent));
            crate::sync_theme_and_backdrops(&active_theme_id, active_accent.as_deref(), cx);
        }

        Some(SettingsWindowHandle {
            window: handle,
            entity: entity.downgrade(),
        })
    }

    #[allow(clippy::too_many_arguments)]
    fn new(
        bridge: EngineBridge,
        settings: Settings,
        categories: Vec<DownloadCategory>,
        update_state: UpdateState,
        is_portable: bool,
        initial_tab: SettingsTab,
        main_app: WeakEntity<PosthazeApp>,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) -> Self {
        let focus_handle = cx.focus_handle();
        focus_handle.focus(window, cx);

        let mut state = SettingsModalState::from_settings(&settings, categories, window, cx);
        state.active_tab = initial_tab;

        cx.subscribe_in(
            &state.cat_extensions_input,
            window,
            |win, input, ev: &InputEvent, window, cx| match ev {
                InputEvent::PressEnter { .. } => {
                    win.state.commit_extension_input(window, cx);
                    cx.notify();
                }
                InputEvent::Change => {
                    let raw = input.read(cx).value();
                    if raw.contains([',', ';', ' ', '\n', '\r']) {
                        win.state.commit_extension_input(window, cx);
                    }
                    cx.notify();
                }
                _ => {}
            },
        )
        .detach();

        cx.subscribe_in(
            &state.cat_patterns_input,
            window,
            |win, input, ev: &InputEvent, window, cx| match ev {
                InputEvent::PressEnter { .. } => {
                    win.state.commit_pattern_input(window, cx);
                    cx.notify();
                }
                InputEvent::Change => {
                    let raw = input.read(cx).value();
                    if raw.contains([',', ';', ' ', '\n', '\r']) {
                        win.state.commit_pattern_input(window, cx);
                    }
                    cx.notify();
                }
                _ => {}
            },
        )
        .detach();

        for input in [
            &state.dir_input,
            &state.cat_filter_input,
            &state.cat_rule_test_input,
            &state.cat_name_input,
            &state.cat_dir_input,
        ] {
            cx.subscribe_in(input, window, |_, _, ev: &InputEvent, _, cx| {
                if matches!(ev, InputEvent::Change) {
                    cx.notify();
                }
            })
            .detach();
        }

        let main_app_for_appearance = main_app.clone();
        let _appearance_subscription = window.observe_window_appearance(move |window, cx| {
            let Ok((theme_id, _)) =
                main_app_for_appearance.update(cx, |app, _| app.active_theme_selection())
            else {
                return;
            };
            if crate::is_forced_mica_polarity(&theme_id) {
                let is_dark = active_theme().tone == PaletteTone::Dark;
                window.defer(cx, move |window, _| {
                    crate::platform_hooks::assert_window_dark_mode(window, is_dark);
                });
            }
        });

        let main_app_for_close = main_app.clone();
        window.on_window_should_close(cx, move |_window, cx| {
            revert_preview_and_clear_handle(&main_app_for_close, cx);
            true
        });

        Self {
            bridge,
            state,
            update_state,
            is_portable,
            main_app,
            focus_handle,
            last_zoom_percent: None,
            _appearance_subscription,
        }
    }

    /// Switches the active section tab cleanly when re-focusing an already open `SettingsWindow`.
    pub(crate) fn select_tab(&mut self, tab: SettingsTab, cx: &mut Context<Self>) {
        self.state.active_tab = tab;
        cx.notify();
    }

    /// Updates cached `UpdateState` mirrored from `PosthazeApp` while background/manual checks run.
    pub(crate) fn set_update_state(&mut self, update_state: UpdateState, cx: &mut Context<Self>) {
        self.update_state = update_state;
        cx.notify();
    }

    /// Synchronizes background category updates (for example, remembered folders).
    pub(crate) fn sync_categories(
        &mut self,
        categories: Vec<DownloadCategory>,
        cx: &mut Context<Self>,
    ) {
        if !categories.is_empty() {
            self.state.categories = categories;
            cx.notify();
        }
    }

    fn handle_action(&mut self, action: SettingsAction, window: &mut Window, cx: &mut Context<Self>) {
        match action {
            SettingsAction::CheckForUpdates => {
                let _ = self.main_app.update(cx, |app, cx| {
                    app.check_for_updates(false, cx);
                });
            }
            SettingsAction::InstallUpdate => {
                let _ = self.main_app.update(cx, |app, cx| {
                    app.install_available_update(cx);
                });
            }
            SettingsAction::OpenReleaseNotes(url) => {
                let _ = self.main_app.update(cx, |app, cx| {
                    app.run_shell_open(move || open_url_in_browser(&url), cx);
                });
            }
            SettingsAction::RefreshBrowserStatus => {
                self.state.browser_statuses = query_browser_statuses();
                cx.notify();
            }
            SettingsAction::RegisterNativeHost => {
                let ext_id = self.state.extension_id_input.read(cx).value().to_string();
                let res = resolve_cli_executable().and_then(|cli_exe| {
                    register_native_host(&posthaze_engine::default_data_dir(), &cli_exe, &ext_id)
                        .map(|manifest| format!("Registered com.posthaze.host -> {}", manifest.display()))
                });
                self.state.integration_feedback = Some(res);
                self.state.browser_statuses = query_browser_statuses();
                cx.notify();
            }
            SettingsAction::OpenLogsFolder => {
                let _ = self.main_app.update(cx, |app, cx| {
                    app.run_shell_open(
                        || open_directory_in_shell(&posthaze_engine::default_logs_dir()),
                        cx,
                    );
                });
            }
            SettingsAction::ClearRecentFolders => {
                let rx = self.bridge.clear_folder_history();
                cx.spawn(async move |this, cx| {
                    let res = rx.await;
                    let _ = this.update(cx, |win, cx| {
                        match res {
                            Ok(Ok(())) => {
                                win.state.system_feedback =
                                    Some(Ok("Cleared saved destination folder history.".to_string()));
                            }
                            Ok(Err(err)) => {
                                win.state.system_feedback =
                                    Some(Err(format!("Failed to clear folder history: {err}")));
                            }
                            Err(_) => {}
                        }
                        cx.notify();
                    });
                })
                .detach();
            }
            SettingsAction::BrowseSaveFolder => {
                let rx = cx.prompt_for_paths(PathPromptOptions {
                    files: false,
                    directories: true,
                    multiple: false,
                    prompt: Some("Select Default Download Folder".into()),
                });
                let window_handle = window.window_handle();
                cx.spawn(async move |this, cx| {
                    let res = rx.await;
                    let _ = cx.update_window(window_handle, |_, window, cx| {
                        if let Ok(Ok(Some(paths))) = res
                            && let Some(chosen) = paths.into_iter().next()
                        {
                            let chosen_str = chosen.to_string_lossy().to_string();
                            let _ = this.update(cx, |win, cx| {
                                win.state.dir_input.update(cx, |st, cx| {
                                    st.set_value(chosen_str.clone(), window, cx);
                                });
                                win.state.draft.dir = chosen_str;
                                cx.notify();
                            });
                        }
                        window.activate_window();
                    });
                })
                .detach();
            }
            SettingsAction::BrowseCategoryFolder => {
                let rx = cx.prompt_for_paths(PathPromptOptions {
                    files: false,
                    directories: true,
                    multiple: false,
                    prompt: Some("Select Category Download Folder".into()),
                });
                let window_handle = window.window_handle();
                cx.spawn(async move |this, cx| {
                    let res = rx.await;
                    let _ = cx.update_window(window_handle, |_, window, cx| {
                        if let Ok(Ok(Some(paths))) = res
                            && let Some(chosen) = paths.into_iter().next()
                        {
                            let chosen_str = chosen.to_string_lossy().to_string();
                            let _ = this.update(cx, |win, cx| {
                                win.state.cat_dir_input.update(cx, |st, cx| {
                                    st.set_value(chosen_str.clone(), window, cx);
                                });
                                win.state.sync_selected_category_from_inputs(cx);
                                cx.notify();
                            });
                        }
                        window.activate_window();
                    });
                })
                .detach();
            }
            other => {
                let updates_preview = matches!(
                    other,
                    SettingsAction::SetTheme(_)
                        | SettingsAction::SetAccentOverride(_)
                        | SettingsAction::ResetToDefaults
                );
                self.state.handle_action(other, window, cx);
                if updates_preview {
                    let preview = (
                        self.state.draft.ui_theme.clone(),
                        self.state.draft.ui_accent_override.clone(),
                    );
                    let _ = self.main_app.update(cx, |app, _| {
                        app.set_settings_theme_preview(Some(preview));
                    });
                }
                cx.notify();
            }
        }
    }

    fn apply_settings(&mut self, close_after: bool, window: &mut Window, cx: &mut Context<Self>) {
        self.state.commit_extension_input(window, cx);
        self.state.commit_pattern_input(window, cx);
        match (
            self.state.apply_to_settings(cx),
            self.state.apply_categories(cx),
        ) {
            (Ok(new_settings), Ok(new_categories)) => {
                self.state.error = None;
                let main_app = self.main_app.clone();
                let _ = main_app.update(cx, |app, cx| {
                    if close_after {
                        app.clear_settings_window();
                    }
                    app.commit_saved_settings(new_settings, new_categories, cx);
                });
                if close_after {
                    window.remove_window();
                } else {
                    cx.notify();
                }
            }
            (Err(err), _) | (_, Err(err)) => {
                self.state.error = Some(err);
                cx.notify();
            }
        }
    }

    fn cancel_and_close(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        revert_preview_and_clear_handle(&self.main_app, cx);
        window.remove_window();
    }

    fn render_titlebar(&self) -> impl IntoElement {
        TitleBar::new()
            .pl_0()
            .bg(titlebar_bg())
            .border_color(graphite_border())
            .child(
                div()
                    .w(px(SETTINGS_SIDEBAR_WIDTH))
                    .flex_shrink_0()
                    .h_full()
                    .pl(px(12.0))
                    .border_r_1()
                    .border_color(graphite_border())
                    .flex()
                    .items_center()
                    .child(
                        div()
                            .h_full()
                            .pr(px(10.0))
                            .border_r_1()
                            .border_color(graphite_border())
                            .flex()
                            .items_center()
                            .gap(px(8.0))
                            .child(
                                svg()
                                    .path(BRAND_LOGO_SVG)
                                    .size(px(22.0))
                                    .text_color(selenium_amber()),
                            )
                            .child(
                                div()
                                    .font_family(MONO_FONT)
                                    .text_size(TYPE_MICRO)
                                    .font_weight(FontWeight::SEMIBOLD)
                                    .text_color(silver_text())
                                    .child("POSTHAZE"),
                            ),
                    )
                    .child(
                        div()
                            .flex_1()
                            .min_w_0()
                            .h_full()
                            .px(px(10.0))
                            .flex()
                            .items_center()
                            .font_family(MONO_FONT)
                            .text_size(TYPE_HEADER)
                            .font_weight(FontWeight::SEMIBOLD)
                            .text_color(silver_text())
                            .truncate()
                            .child("SETTINGS"),
                    ),
            )
    }
}

fn revert_preview_and_clear_handle(main_app: &WeakEntity<PosthazeApp>, cx: &mut App) {
    let saved = main_app
        .update(cx, |app, _| {
            app.set_settings_theme_preview(None);
            app.clear_settings_window();
            app.active_theme_selection()
        })
        .ok();
    if let Some((theme_id, accent)) = saved {
        crate::sync_theme_and_backdrops(&theme_id, accent.as_deref(), cx);
    }
}

impl Render for SettingsWindow {
    fn render(&mut self, window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let zoom_percent = self.state.draft.ui_zoom_percent.clamp(80, 150);
        if self.last_zoom_percent != Some(zoom_percent) {
            self.last_zoom_percent = Some(zoom_percent);
            let zoom_rem = 16.0 * (zoom_percent as f32) / 100.0;
            window.set_rem_size(px(zoom_rem));
        }

        let e_action = cx.entity().clone();
        let e_cancel = cx.entity().clone();
        let e_apply = cx.entity().clone();
        let e_save = cx.entity().clone();

        div()
            .flex()
            .flex_col()
            .size_full()
            .text_color(silver_text())
            .track_focus(&self.focus_handle)
            .on_key_down(cx.listener(|win, event: &KeyDownEvent, window, cx| {
                let key = event.keystroke.key.as_str();
                let ctrl_or_cmd =
                    event.keystroke.modifiers.control || event.keystroke.modifiers.platform;
                if ctrl_or_cmd && key == "q" {
                    let _ = win.main_app.update(cx, |app, cx| {
                        app.quitting = true;
                        cx.quit();
                    });
                    return;
                }
                if key == "escape" {
                    win.cancel_and_close(window, cx);
                } else if key == "enter" {
                    let in_inline_input = [
                        &win.state.cat_extensions_input,
                        &win.state.cat_patterns_input,
                        &win.state.cat_filter_input,
                        &win.state.cat_rule_test_input,
                    ]
                    .iter()
                    .any(|inp| inp.read(cx).focus_handle(cx).is_focused(window));
                    if !in_inline_input {
                        win.apply_settings(true, window, cx);
                    }
                }
            }))
            .child(self.render_titlebar())
            .child(render_settings_window_body(
                &self.state,
                &self.update_state,
                self.is_portable,
                cx,
                move |action, window, cx| {
                    e_action.update(cx, |win, cx| win.handle_action(action, window, cx));
                },
                move |window, cx| {
                    e_cancel.update(cx, |win, cx| win.cancel_and_close(window, cx));
                },
                move |window, cx| {
                    e_apply.update(cx, |win, cx| win.apply_settings(false, window, cx));
                },
                move |window, cx| {
                    e_save.update(cx, |win, cx| win.apply_settings(true, window, cx));
                },
            ))
    }
}
