//! Posthaze Download Manager desktop UI powered by GPUI and GPUI Kit.

#![cfg_attr(not(debug_assertions), windows_subsystem = "windows")]

mod assets;
mod bridge;
mod browser_integration;
mod context_menu;
mod curl_parser;
mod diagnostics;
mod grid_cells;
mod inspector;
mod ipc;
mod modals;
mod motion;
mod platform_hooks;
mod preflight_window;
mod settings_modal;
mod sidebar;
mod table;
mod theme;
mod tray;
mod updater;
mod view_model;

use std::path::PathBuf;
use std::time::{Duration, Instant};

use gpui_kit::assets::IconName;
use gpui_kit::component::input::{InputEvent, InputState};
use gpui_kit::component::{Icon, InteractiveElementExt as _, TitleBar};
use gpui_kit::prelude::FluentBuilder;
use gpui_kit::*;
use posthaze_engine::{
    DeleteDefaultAction, DownloadId, DownloadRequest, DownloadState, EngineEvent, SegmentProgress,
    default_data_dir, default_db_path, default_logs_dir,
};

use crate::assets::{BRAND_LOGO_SVG, PosthazeAssets};
use crate::bridge::{EngineBridge, EngineCommand, EngineSnapshot};
use crate::context_menu::{ContextMenuAction, ContextMenuState, render_context_menu};
use crate::curl_parser::parse_url_or_curl;
use crate::grid_cells::{CellBorder, GridTone, grid_cell_button};
use crate::inspector::{InspectorCallbacks, render_inspector};
use crate::modals::{
    ConfirmAction, FileOpModalState, render_confirm_modal, render_file_op_modal,
};
use crate::motion::MotionState;
use crate::platform_hooks::{
    hide_window_to_tray, is_window_visible, open_directory_in_shell, open_file_in_shell,
    open_folder_in_shell, open_url_in_browser, show_window_from_tray, spawn_completion_hook,
    spawn_windows_toast, sync_windows_startup_registry,
};
use crate::preflight_window::PreflightWindow;
use crate::settings_modal::{
    SettingsAction, SettingsModalState, SettingsTab, render_settings_modal,
};
use crate::theme::*;
use crate::updater::UpdateState;
use crate::view_model::{
    COLLAPSED_INSPECTOR_HEIGHT, DEFAULT_INSPECTOR_HEIGHT, DEFAULT_SIDEBAR_WIDTH, DownloadRowView,
    FilterTab, INSPECTOR_MAGNETIC_SNAP_RADIUS, KeyboardAction, KeyboardContext,
    PrimaryDownloadAction, QueueController, RAIL_SIDEBAR_WIDTH, SIDEBAR_MAGNETIC_SNAP_RADIUS,
    SortColumn, SortDirection, UiEffect, apply_magnetic_detent, compute_fifo_ranks,
    compute_layout_profile, derive_display_filename, filter_and_sort_rows_with_query,
    resolve_inspector_drag, resolve_keyboard_action, resolve_sidebar_drag,
};

fn main() {
    diagnostics::init_diagnostics(default_logs_dir());
    let start_minimized = std::env::args().any(|arg| arg == "--minimized");
    if ipc::try_connect_existing_instance(!start_minimized) {
        return;
    }
    let bridge = EngineBridge::open(default_db_path()).expect("failed to initialize engine");

    gpui_kit::application()
        .with_assets(PosthazeAssets)
        .run(move |cx| {
            gpui_kit::init(cx);
            theme::apply_theme("chronophotographic", WindowAppearance::Dark, false, cx);

            let window_options = WindowOptions {
                window_bounds: Some(WindowBounds::Windowed(Bounds::centered(
                    None,
                    size(px(1020.0), px(640.0)),
                    cx,
                ))),
                show: !start_minimized,
                focus: !start_minimized,
                window_min_size: Some(size(px(1020.0), px(640.0))),
                ..TitleBar::window_options()
            };

            let bridge = bridge.clone();
            gpui_kit::open_window(window_options, cx, move |window, cx| {
                cx.new(|cx| PosthazeApp::new(bridge, window, cx))
            })
            .expect("failed to open Posthaze window");
        });
}

#[derive(Clone, Copy)]
enum ActiveResize {
    Sidebar { start_x: Pixels, start_width: f32 },
    Inspector { start_y: Pixels, start_height: f32 },
}

pub(crate) struct PosthazeApp {
    bridge: EngineBridge,
    window_handle: AnyWindowHandle,
    pub(crate) focus_handle: FocusHandle,
    pub(crate) url_input: Entity<InputState>,
    pub(crate) search_input: Entity<InputState>,
    pub(crate) queue: QueueController,
    pub(crate) filter: FilterTab,
    pub(crate) sort_col: SortColumn,
    pub(crate) sort_dir: SortDirection,
    status_banner: Option<String>,
    settings_modal: Option<SettingsModalState>,
    confirm_modal: Option<ConfirmAction>,
    file_op_modal: Option<FileOpModalState>,
    context_menu: Option<ContextMenuState>,
    next_probe_cascade: usize,
    pub(crate) sidebar_width: f32,
    pub(crate) sidebar_collapsed: bool,
    pub(crate) inspector_height: f32,
    pub(crate) inspector_collapsed: bool,
    active_resize: Option<ActiveResize>,
    pub(crate) motion: MotionState,
    motion_loop_running: bool,
    startup_layout_applied: bool,
    pub(crate) table_scroll_handle: UniformListScrollHandle,
    refresh_in_flight: bool,
    refresh_pending: bool,
    last_progress_notify: Instant,
    quitting: bool,
    _tray_handle: Option<tray::TrayHandle>,
    update_state: UpdateState,
    is_portable: bool,
    update_timer_started: bool,
}

impl PosthazeApp {
    fn new(bridge: EngineBridge, window: &mut Window, cx: &mut Context<Self>) -> Self {
        let url_input = cx.new(|cx| {
            InputState::new(window, cx).placeholder("Paste or enter HTTP/HTTPS URL to probe...")
        });
        url_input.update(cx, |state, cx| {
            state.focus(window, cx);
        });

        cx.subscribe_in(&url_input, window, |this, _, ev: &InputEvent, window, cx| {
            if matches!(ev, InputEvent::PressEnter { .. }) {
                this.submit_url_from_input(window, cx);
            }
        })
        .detach();

        let search_input = cx.new(|cx| {
            InputState::new(window, cx).placeholder("Filter by filename or URL...")
        });
        cx.subscribe_in(&search_input, window, |_, _, ev: &InputEvent, _, cx| {
            if matches!(ev, InputEvent::Change) {
                cx.notify();
            }
        })
        .detach();

        let mut app = Self {
            bridge,
            window_handle: window.window_handle(),
            focus_handle: cx.focus_handle(),
            url_input,
            search_input,
            queue: QueueController::default(),
            filter: FilterTab::All,
            sort_col: SortColumn::CreatedAt,
            sort_dir: SortDirection::Descending,
            status_banner: None,
            settings_modal: None,
            confirm_modal: None,
            file_op_modal: None,
            context_menu: None,
            next_probe_cascade: 0,
            sidebar_width: DEFAULT_SIDEBAR_WIDTH,
            sidebar_collapsed: true,
            inspector_height: DEFAULT_INSPECTOR_HEIGHT,
            inspector_collapsed: true,
            active_resize: None,
            motion: MotionState::default(),
            motion_loop_running: false,
            startup_layout_applied: false,
            table_scroll_handle: UniformListScrollHandle::new(),
            refresh_in_flight: false,
            refresh_pending: false,
            last_progress_notify: Instant::now(),
            quitting: false,
            _tray_handle: None,
            update_state: UpdateState::Idle,
            is_portable: updater::is_current_install_portable(),
            update_timer_started: false,
        };
        if let Some((tray_handle, tray_rx)) = tray::init_tray() {
            app._tray_handle = Some(tray_handle);
            app.start_tray_listener(tray_rx, cx);
        }
        let close_entity = cx.entity().downgrade();
        window.on_window_should_close(cx, move |window, cx| {
            let hide_to_tray = close_entity
                .update(cx, |app, _| {
                    !app.quitting && app.queue.settings.ui_close_to_tray
                })
                .unwrap_or(false);
            if hide_to_tray {
                hide_window_to_tray(window);
                return false;
            }
            true
        });

        app.refresh_from_engine(cx);
        app.start_event_listener(cx);
        app.start_ipc_listener(cx);
        app
    }

    fn apply_snapshot(&mut self, snapshot: EngineSnapshot, cx: &mut Context<Self>) {
        let effect = self.queue.apply_snapshot(snapshot);
        if !self.startup_layout_applied {
            self.startup_layout_applied = true;
            if self.queue.settings.ui_start_sidebar_collapsed {
                self.sidebar_collapsed = true;
            }
            if self.queue.settings.auto_check_updates {
                self.check_for_updates(true, cx);
            }
            if !self.update_timer_started {
                self.update_timer_started = true;
                self.start_update_timer(cx);
            }
        }
        theme::apply_theme_with_accent(
            &self.queue.settings.ui_theme,
            self.queue.settings.ui_accent_override.as_deref(),
            WindowAppearance::Dark,
            false,
            cx,
        );
        if let Some(effect) = effect {
            self.execute_ui_effects(vec![effect], cx);
        }
        cx.notify();
    }

    fn execute_ui_effects(&mut self, effects: Vec<UiEffect>, cx: &mut Context<Self>) {
        for effect in effects {
            match effect {
                UiEffect::RefreshFromEngine => self.refresh_from_engine(cx),
                UiEffect::ResumeDownload(id) => self.resume_download(id, cx),
                UiEffect::ShowToast { title, body } => spawn_windows_toast(&title, &body),
                UiEffect::RunCompletionHook { path } => spawn_completion_hook(
                    &self.queue.settings.completion_cmd_path,
                    &self.queue.settings.completion_cmd_args,
                    &path,
                ),
                UiEffect::LoadSegments(id) => self.load_segments_for(id, cx),
            }
        }
    }

    fn refresh_from_engine(&mut self, cx: &mut Context<Self>) {
        if self.refresh_in_flight {
            self.refresh_pending = true;
            return;
        }
        self.refresh_in_flight = true;
        self.refresh_pending = false;
        let rx = self.bridge.load_snapshot();
        cx.spawn(async move |this, cx| {
            let res = rx.await;
            let _ = this.update(cx, |app, cx| {
                app.refresh_in_flight = false;
                if let Ok(Ok(snapshot)) = res {
                    app.apply_snapshot(snapshot, cx);
                }
                if app.refresh_pending {
                    app.refresh_from_engine(cx);
                }
            });
        })
        .detach();
    }

    fn load_segments_for(&self, id: DownloadId, cx: &mut Context<Self>) {
        let rx = self.bridge.load_segments(id);
        cx.spawn(async move |this, cx| {
            if let Ok(Ok((id, segs))) = rx.await {
                let _ = this.update(cx, |app, cx| {
                    app.queue.persisted_segments.insert(id, segs);
                    cx.notify();
                });
            }
        })
        .detach();
    }

    fn start_tray_listener(
        &self,
        mut rx: tokio::sync::mpsc::UnboundedReceiver<tray::TrayAction>,
        cx: &mut Context<Self>,
    ) {
        let window_handle = self.window_handle;
        cx.spawn(async move |this, cx| {
            while let Some(action) = rx.recv().await {
                match action {
                    tray::TrayAction::ToggleShowHide => {
                        let _ = cx.update_window(window_handle, |_, window, _| {
                            if is_window_visible(window) && window.is_window_active() {
                                hide_window_to_tray(window);
                            } else {
                                show_window_from_tray(window);
                            }
                        });
                    }
                    tray::TrayAction::PauseAll => {
                        let _ = this.update(cx, |app, cx| {
                            app.pause_all(cx);
                        });
                    }
                    tray::TrayAction::Quit => {
                        let _ = this.update(cx, |app, cx| {
                            app.quitting = true;
                            cx.quit();
                        });
                    }
                }
            }
        })
        .detach();
    }

    fn start_ipc_listener(&self, cx: &mut Context<Self>) {
        let mut rx = ipc::start_named_pipe_server(self.bridge.runtime());
        let window_handle = self.window_handle;
        let bridge = self.bridge.clone();
        cx.spawn(async move |this, cx| {
            while let Some(cmd) = rx.recv().await {
                match cmd {
                    ipc::PipeCommand::ShowMainWindow => {
                        let _ = cx.update_window(window_handle, |_, window, _| {
                            show_window_from_tray(window);
                        });
                    }
                    ipc::PipeCommand::Enqueue(req) => {
                        let bridge_for_win = bridge.clone();
                        let main_app = this.clone();
                        cx.update(|cx| {
                            let (settings, cascade) = main_app
                                .update(cx, |app, _| {
                                    let idx = app.next_probe_cascade;
                                    app.next_probe_cascade = app.next_probe_cascade.wrapping_add(1);
                                    (app.queue.settings.clone(), idx)
                                })
                                .unwrap_or_default();
                            PreflightWindow::open(
                                bridge_for_win,
                                settings,
                                req.url,
                                req.headers,
                                window_handle,
                                main_app,
                                cascade,
                                cx,
                            );
                        });
                    }
                }
            }
        })
        .detach();
    }

    fn start_event_listener(&self, cx: &mut Context<Self>) {
        let mut events = self.bridge.subscribe_events();
        cx.spawn(async move |this, cx| {
            loop {
                match events.recv().await {
                    Ok(EngineEvent::ProgressUpdated { id, snapshot }) => {
                        let _ = this.update(cx, |app, cx| {
                            let now = Instant::now();
                            if app.motion.on_progress(&id, &snapshot, now) {
                                app.ensure_motion_loop(cx);
                            }
                            app.queue.live_map.insert(id, snapshot);
                            if !app.motion_loop_running
                                && now.duration_since(app.last_progress_notify)
                                    >= Duration::from_millis(33)
                            {
                                app.last_progress_notify = now;
                                cx.notify();
                            }
                        });
                    }
                    Ok(EngineEvent::StateChanged {
                        id,
                        state,
                        error,
                        failure_kind,
                    }) => {
                        let _ = this.update(cx, |app, cx| {
                            let total_bytes = app
                                .queue
                                .records
                                .iter()
                                .find(|r| r.id == id)
                                .and_then(|r| r.content_length);
                            if app
                                .motion
                                .on_state_changed(&id, state, total_bytes, Instant::now())
                            {
                                app.ensure_motion_loop(cx);
                            }
                            let win_active = cx
                                .update_window(app.window_handle, |_, w, _| w.is_window_active())
                                .unwrap_or(false);
                            let effects = app.queue.on_engine_event(
                                EngineEvent::StateChanged {
                                    id,
                                    state,
                                    error,
                                    failure_kind,
                                },
                                win_active,
                            );
                            app.execute_ui_effects(effects, cx);
                        });
                    }
                    Ok(event @ (EngineEvent::QueueUpdated | EngineEvent::SettingsUpdated(_))) => {
                        let _ = this.update(cx, |app, cx| {
                            let effects = app.queue.on_engine_event(event, false);
                            app.execute_ui_effects(effects, cx);
                            cx.notify();
                        });
                    }
                    Err(tokio::sync::broadcast::error::RecvError::Lagged(_)) => {
                        let _ = this.update(cx, |app, cx| {
                            app.refresh_from_engine(cx);
                        });
                    }
                    Err(tokio::sync::broadcast::error::RecvError::Closed) => break,
                }
            }
        })
        .detach();
    }

    fn ensure_motion_loop(&mut self, cx: &mut Context<Self>) {
        if self.motion_loop_running {
            return;
        }
        self.motion_loop_running = true;
        cx.spawn(async move |this, cx| {
            loop {
                cx.background_executor()
                    .timer(Duration::from_millis(16))
                    .await;
                let keep_going = this
                    .update(cx, |app, cx| {
                        if let Some(expected_y) = app.motion.current_table_scroll() {
                            let actual_y = {
                                let inner = app.table_scroll_handle.0.borrow();
                                -f32::from(inner.base_handle.offset().y)
                            };
                            if (actual_y - expected_y).abs() > 2.5 {
                                app.motion.clear_table_scroll();
                            }
                        }
                        let active = app.motion.step(Instant::now());
                        let next_y = app.motion.take_table_scroll_y();
                        if let Some(y) = next_y {
                            let inner = app.table_scroll_handle.0.borrow_mut();
                            let cur = inner.base_handle.offset();
                            inner.base_handle.set_offset(point(cur.x, px(-y)));
                        }
                        if active || next_y.is_some() {
                            cx.notify();
                        }
                        if !active {
                            app.motion_loop_running = false;
                        }
                        active
                    })
                    .unwrap_or(false);
                if !keep_going {
                    break;
                }
            }
        })
        .detach();
    }

    fn submit_url_from_input(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        let raw = self.url_input.read(cx).value().to_string();
        let trimmed = raw.trim();
        if trimmed.is_empty() {
            return;
        }
        let parsed = parse_url_or_curl(trimmed);
        if !parsed.headers.is_empty() {
            let clean_url = parsed.url.clone();
            self.url_input.update(cx, |state, cx| {
                state.set_value(clean_url, window, cx);
            });
        }
        self.begin_probe(parsed.url, parsed.headers, window, cx);
    }

    fn paste_and_probe_from_clipboard(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        if let Some(item) = cx.read_from_clipboard()
            && let Some(text) = item.text()
        {
            let trimmed = text.trim();
            if !trimmed.is_empty() {
                let parsed = parse_url_or_curl(trimmed);
                let clean_url = parsed.url.clone();
                self.url_input.update(cx, |state, cx| {
                    state.set_value(clean_url, window, cx);
                });
                self.begin_probe(parsed.url, parsed.headers, window, cx);
                return;
            }
        }
        self.status_banner = Some("Clipboard does not contain a URL.".to_string());
        cx.notify();
    }

    fn begin_probe(
        &mut self,
        url: String,
        headers: Vec<(String, String)>,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        self.status_banner = None;
        let cascade_idx = self.next_probe_cascade;
        self.next_probe_cascade = self.next_probe_cascade.wrapping_add(1);
        let bridge = self.bridge.clone();
        let settings = self.queue.settings.clone();
        let main_window = window.window_handle();
        let main_app = cx.entity().downgrade();
        PreflightWindow::open(
            bridge,
            settings,
            url,
            headers,
            main_window,
            main_app,
            cascade_idx,
            cx,
        );
        self.url_input.update(cx, |state, cx| {
            state.set_value(String::new(), window, cx);
        });
        cx.notify();
    }

    fn dispatch_download_request(
        &mut self,
        req: DownloadRequest,
        start_immediately: bool,
        cx: &mut Context<Self>,
    ) {
        let cmd = if start_immediately {
            EngineCommand::Start { request: req }
        } else {
            EngineCommand::EnqueuePaused { request: req }
        };
        self.run_command_with(cmd, cx, move |app, created_id| {
            if let Some(id) = created_id
                && (start_immediately || app.queue.selected_id.is_none())
            {
                app.queue.select_single(id);
            }
            app.status_banner = None;
        });
    }

    fn restart_fresh_download(&mut self, id: DownloadId, cx: &mut Context<Self>) {
        self.run_command(EngineCommand::RestartFresh(id), cx);
    }

    fn open_settings_modal(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        self.open_settings_modal_on_tab(SettingsTab::General, window, cx);
    }

    fn open_settings_modal_on_tab(
        &mut self,
        tab: SettingsTab,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        let mut state = SettingsModalState::from_settings(&self.queue.settings, window, cx);
        state.active_tab = tab;
        self.settings_modal = Some(state);
        cx.notify();
    }

    fn check_for_updates(&mut self, background: bool, cx: &mut Context<Self>) {
        if matches!(
            self.update_state,
            UpdateState::Checking | UpdateState::Downloading { .. } | UpdateState::Installing
        ) {
            return;
        }
        if !background {
            self.update_state = UpdateState::Checking;
            cx.notify();
        }
        let runtime = self.bridge.runtime().clone();
        let (tx, rx) = tokio::sync::oneshot::channel();
        runtime.spawn(async move {
            let res = updater::check_latest_release(env!("CARGO_PKG_VERSION")).await;
            let _ = tx.send(res);
        });
        let window_handle = self.window_handle;
        cx.spawn(async move |this, cx| {
            let Ok(res) = rx.await else {
                return;
            };
            let is_active = cx
                .update_window(window_handle, |_, window, _| window.is_window_active())
                .unwrap_or(true);
            let _ = this.update(cx, |app, cx| {
                match res {
                    Ok(Some(info)) => {
                        let ver = info.version.clone();
                        let was_same = app
                            .update_state
                            .available_release()
                            .is_some_and(|prev| prev.version == ver);
                        app.update_state = UpdateState::Available(info);
                        if !was_same && !is_active {
                            spawn_windows_toast(
                                "Posthaze — Update Available",
                                &format!("Posthaze v{ver} is ready to install."),
                            );
                        }
                    }
                    Ok(None) => {
                        if !background {
                            app.update_state = UpdateState::UpToDate;
                        }
                    }
                    Err(err) => {
                        if !background {
                            app.update_state = UpdateState::Failed(err);
                        } else {
                            tracing::debug!("background update check skipped/failed: {err}");
                        }
                    }
                }
                cx.notify();
            });
        })
        .detach();
    }

    fn start_update_timer(&mut self, cx: &mut Context<Self>) {
        let runtime = self.bridge.runtime().clone();
        let (tx, mut rx) = tokio::sync::mpsc::unbounded_channel::<()>();
        runtime.spawn(async move {
            loop {
                tokio::time::sleep(updater::AUTO_CHECK_INTERVAL).await;
                if tx.send(()).is_err() {
                    break;
                }
            }
        });
        cx.spawn(async move |this, cx| {
            while rx.recv().await.is_some() {
                let keep_going = this
                    .update(cx, |app, cx| {
                        if app.queue.settings.auto_check_updates {
                            app.check_for_updates(true, cx);
                        }
                    })
                    .is_ok();
                if !keep_going {
                    break;
                }
            }
        })
        .detach();
    }

    fn install_available_update(&mut self, cx: &mut Context<Self>) {
        if self.is_portable {
            return;
        }
        let Some(info) = self.update_state.available_release().cloned() else {
            return;
        };
        let keys = match updater::embedded_verifying_keys() {
            Ok(keys) => keys,
            Err(err) => {
                self.update_state = UpdateState::Failed(err);
                cx.notify();
                return;
            }
        };
        self.update_state = UpdateState::Downloading {
            info: info.clone(),
            pct: 0,
        };
        cx.notify();

        let runtime = self.bridge.runtime().clone();
        let (pct_tx, mut pct_rx) = tokio::sync::mpsc::unbounded_channel::<u8>();
        let (done_tx, done_rx) = tokio::sync::oneshot::channel();
        let dl_info = info.clone();
        runtime.spawn(async move {
            let res = updater::download_and_verify_update(&dl_info, &keys, move |pct| {
                let _ = pct_tx.send(pct);
            })
            .await;
            let _ = done_tx.send(res);
        });

        let info_for_pct = info.clone();
        cx.spawn(async move |this, cx| {
            while let Some(pct) = pct_rx.recv().await {
                let ok = this
                    .update(cx, |app, cx| {
                        if matches!(app.update_state, UpdateState::Downloading { .. }) {
                            app.update_state = UpdateState::Downloading {
                                info: info_for_pct.clone(),
                                pct,
                            };
                            cx.notify();
                        }
                    })
                    .is_ok();
                if !ok {
                    break;
                }
            }
        })
        .detach();

        cx.spawn(async move |this, cx| {
            let Ok(res) = done_rx.await else {
                return;
            };
            let _ = this.update(cx, |app, cx| match res {
                Ok(setup_path) => {
                    app.update_state = UpdateState::Installing;
                    cx.notify();
                    match updater::launch_silent_installer(&setup_path) {
                        Ok(()) => {
                            app.quitting = true;
                            cx.quit();
                        }
                        Err(err) => {
                            app.update_state = UpdateState::Failed(err);
                            cx.notify();
                        }
                    }
                }
                Err(err) => {
                    app.update_state = UpdateState::Failed(err);
                    cx.notify();
                }
            });
        })
        .detach();
    }

    fn save_settings_modal(&mut self, cx: &mut Context<Self>) {
        let Some(modal) = &mut self.settings_modal else {
            return;
        };
        match modal.apply_to_settings(cx) {
            Ok(new_settings) => {
                let startup_enabled = new_settings.launch_on_startup;
                self.settings_modal = None;
                if let Err(err) = sync_windows_startup_registry(startup_enabled) {
                    self.status_banner = Some(err);
                }
                self.run_command(EngineCommand::UpdateSettings(new_settings), cx);
            }
            Err(err) => {
                modal.error = Some(err);
                cx.notify();
            }
        }
    }

    fn handle_settings_action(
        &mut self,
        action: SettingsAction,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        if matches!(action, SettingsAction::CheckForUpdates) {
            self.check_for_updates(false, cx);
            return;
        }
        if matches!(action, SettingsAction::InstallUpdate) {
            self.install_available_update(cx);
            return;
        }
        if let SettingsAction::OpenReleaseNotes(url) = action {
            if let Err(err) = open_url_in_browser(&url) {
                self.status_banner = Some(err);
                cx.notify();
            }
            return;
        }
        if matches!(action, SettingsAction::RefreshBrowserStatus) {
            if let Some(modal) = &mut self.settings_modal {
                modal.browser_statuses = browser_integration::query_browser_statuses();
                cx.notify();
            }
            return;
        }
        if matches!(action, SettingsAction::RegisterNativeHost) {
            if let Some(modal) = &mut self.settings_modal {
                let ext_id = modal.extension_id_input.read(cx).value().to_string();
                let res = browser_integration::resolve_cli_executable().and_then(|cli_exe| {
                    browser_integration::register_native_host(&default_data_dir(), &cli_exe, &ext_id)
                        .map(|manifest| format!("Registered com.posthaze.host -> {}", manifest.display()))
                });
                modal.integration_feedback = Some(res);
                modal.browser_statuses = browser_integration::query_browser_statuses();
                cx.notify();
            }
            return;
        }
        if matches!(action, SettingsAction::OpenLogsFolder) {
            if let Err(err) = open_directory_in_shell(&default_logs_dir()) {
                self.status_banner = Some(err);
                cx.notify();
            }
            return;
        }
        if matches!(action, SettingsAction::BrowseSaveFolder) {
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
                        let _ = this.update(cx, |app, cx| {
                            if let Some(modal) = &mut app.settings_modal {
                                modal.dir_input.update(cx, |st, cx| {
                                    st.set_value(chosen_str.clone(), window, cx);
                                });
                                modal.draft.dir = chosen_str;
                                cx.notify();
                            }
                        });
                    }
                    window.activate_window();
                });
            })
            .detach();
            return;
        }
        if let Some(modal) = &mut self.settings_modal {
            modal.handle_action(action, window, cx);
            cx.notify();
        }
    }

    fn pause_download(&mut self, id: DownloadId, cx: &mut Context<Self>) {
        self.run_command(EngineCommand::Pause(id), cx);
    }

    fn resume_download(&mut self, id: DownloadId, cx: &mut Context<Self>) {
        self.run_command(EngineCommand::Resume(id), cx);
    }

    fn cancel_download(&mut self, id: DownloadId, cx: &mut Context<Self>) {
        self.run_command(EngineCommand::Cancel(id), cx);
    }

    fn request_delete_download(&mut self, id: DownloadId, filename: String, cx: &mut Context<Self>) {
        match self.queue.settings.delete_action_default {
            DeleteDefaultAction::Ask => {
                self.request_delete_download_with_file(id, filename, false, cx);
            }
            DeleteDefaultAction::Remove => self.delete_download(id, false, cx),
            DeleteDefaultAction::DeleteFile => self.delete_download(id, true, cx),
        }
    }

    fn request_delete_download_with_file(
        &mut self,
        id: DownloadId,
        filename: String,
        delete_file: bool,
        cx: &mut Context<Self>,
    ) {
        self.confirm_modal = Some(ConfirmAction::DeleteDownload {
            id,
            filename,
            delete_file,
        });
        cx.notify();
    }

    pub(crate) fn collapse_inspector_if_open(&mut self, cx: &mut Context<Self>) {
        if !self.inspector_collapsed {
            self.inspector_collapsed = true;
            self.motion
                .animate_inspector(self.inspector_height, COLLAPSED_INSPECTOR_HEIGHT);
            self.ensure_motion_loop(cx);
        }
    }

    pub(crate) fn request_delete_selected(&mut self, cx: &mut Context<Self>) {
        let selected = self.queue.selected_list();
        if selected.len() <= 1 {
            if let Some(id) = selected.into_iter().next()
                && let Some(rec) = self.queue.records.iter().find(|r| r.id == id)
            {
                let filename = derive_display_filename(rec);
                self.request_delete_download(id, filename, cx);
            }
            return;
        }
        match self.queue.settings.delete_action_default {
            DeleteDefaultAction::Ask => {
                self.confirm_modal = Some(ConfirmAction::DeleteMultiple {
                    ids: selected,
                    delete_file: false,
                });
                cx.notify();
            }
            DeleteDefaultAction::Remove => {
                for id in selected {
                    self.delete_download(id, false, cx);
                }
            }
            DeleteDefaultAction::DeleteFile => {
                for id in selected {
                    self.delete_download(id, true, cx);
                }
            }
        }
    }

    pub(crate) fn select_and_reveal_download(&mut self, id: DownloadId, cx: &mut Context<Self>) {
        self.queue.select_single(id.clone());
        self.load_segments_for(id, cx);
        if self.queue.settings.ui_click_row_opens_inspector && self.inspector_collapsed {
            self.inspector_collapsed = false;
            self.motion
                .animate_inspector(COLLAPSED_INSPECTOR_HEIGHT, self.inspector_height);
            self.ensure_motion_loop(cx);
        }
    }

    fn delete_download(&mut self, id: DownloadId, delete_file: bool, cx: &mut Context<Self>) {
        self.run_command(EngineCommand::Delete { id, delete_file }, cx);
    }

    fn request_clear_completed(&mut self, cx: &mut Context<Self>) {
        let count = self.queue.clear_completed_ids().len();
        if count == 0 {
            return;
        }
        self.confirm_modal = Some(ConfirmAction::ClearCompleted { count });
        cx.notify();
    }

    fn execute_confirmed_action(&mut self, cx: &mut Context<Self>) {
        let Some(action) = self.confirm_modal.take() else {
            return;
        };
        match action {
            ConfirmAction::DeleteDownload {
                id, delete_file, ..
            } => self.delete_download(id, delete_file, cx),
            ConfirmAction::DeleteMultiple { ids, delete_file } => {
                for id in ids {
                    self.delete_download(id, delete_file, cx);
                }
            }
            ConfirmAction::ClearCompleted { .. } => self.clear_completed(cx),
            ConfirmAction::OverwriteCollision {
                request,
                start_immediately,
                ..
            } => self.dispatch_download_request(*request, start_immediately, cx),
        }
    }

    fn open_downloaded_file(&mut self, output_path: Option<PathBuf>, cx: &mut Context<Self>) {
        let Some(path) = output_path else {
            self.status_banner = Some("No output path recorded for this download.".to_string());
            cx.notify();
            return;
        };
        if let Err(err) = open_file_in_shell(&path) {
            self.status_banner = Some(err);
            cx.notify();
        }
    }

    fn execute_context_menu_action(
        &mut self,
        id: DownloadId,
        action: ContextMenuAction,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        self.context_menu = None;
        let record = self.queue.records.iter().find(|r| r.id == id).cloned();

        match action {
            ContextMenuAction::Pause => self.pause_download(id, cx),
            ContextMenuAction::Resume => self.resume_download(id, cx),
            ContextMenuAction::RestartFresh => self.restart_fresh_download(id, cx),
            ContextMenuAction::Cancel => self.cancel_download(id, cx),
            ContextMenuAction::OpenFile => {
                self.open_downloaded_file(record.and_then(|r| r.output_path), cx);
            }
            ContextMenuAction::ShowInFolder => {
                self.open_download_folder(record.and_then(|r| r.output_path), cx);
            }
            ContextMenuAction::CopyUrl => {
                if let Some(rec) = record {
                    let url = rec.final_url.unwrap_or(rec.url);
                    cx.write_to_clipboard(ClipboardItem::new_string(url));
                }
                cx.notify();
            }
            ContextMenuAction::InspectByteMap => {
                self.queue.selected_id = Some(id.clone());
                self.load_segments_for(id, cx);
                if self.inspector_collapsed {
                    self.inspector_collapsed = false;
                    self.motion
                        .animate_inspector(COLLAPSED_INSPECTOR_HEIGHT, self.inspector_height);
                    self.ensure_motion_loop(cx);
                }
                cx.notify();
            }
            ContextMenuAction::FileIntegrity => {
                let filename = record
                    .as_ref()
                    .map(derive_display_filename)
                    .unwrap_or_else(|| "download".to_string());
                self.file_op_modal = Some(FileOpModalState::Integrity {
                    filename,
                    digest: None,
                    error: None,
                });
                cx.notify();
                let rx = self.bridge.compute_blake3(id);
                cx.spawn(async move |this, cx| {
                    let res = rx.await;
                    let _ = this.update(cx, |app, cx| {
                        if let Some(FileOpModalState::Integrity { digest, error, .. }) =
                            &mut app.file_op_modal
                        {
                            match res {
                                Ok(Ok(hash)) => *digest = Some(hash),
                                Ok(Err(err)) => *error = Some(err.to_string()),
                                Err(_) => *error = Some("Engine bridge closed".to_string()),
                            }
                            cx.notify();
                        }
                    });
                })
                .detach();
            }
            ContextMenuAction::RenameFile => {
                let current_name = record
                    .as_ref()
                    .map(derive_display_filename)
                    .unwrap_or_default();
                let input = cx.new(|cx| {
                    let mut st = InputState::new(window, cx).placeholder("New filename");
                    st.set_value(current_name, window, cx);
                    st
                });
                self.file_op_modal = Some(FileOpModalState::Rename {
                    id,
                    input,
                    error: None,
                });
                cx.notify();
            }
            ContextMenuAction::MoveTo => {
                let current_dir = record
                    .as_ref()
                    .and_then(|r| r.output_path.as_ref())
                    .and_then(|p| p.parent())
                    .map(|p| p.to_string_lossy().to_string())
                    .unwrap_or_else(|| {
                        self.queue.settings.default_output_dir.to_string_lossy().to_string()
                    });
                let input = cx.new(|cx| {
                    let mut st = InputState::new(window, cx).placeholder("Target directory");
                    st.set_value(current_dir, window, cx);
                    st
                });
                self.file_op_modal = Some(FileOpModalState::Move {
                    id,
                    input,
                    error: None,
                });
                cx.notify();
            }
            ContextMenuAction::RemoveFromList => {
                let filename = record
                    .as_ref()
                    .map(derive_display_filename)
                    .unwrap_or_else(|| "download".to_string());
                self.request_delete_download_with_file(id, filename, false, cx);
            }
            ContextMenuAction::DeleteFileAndEntry => {
                let filename = record
                    .as_ref()
                    .map(derive_display_filename)
                    .unwrap_or_else(|| "download".to_string());
                self.request_delete_download_with_file(id, filename, true, cx);
            }
        }
    }

    fn execute_file_op_confirm(&mut self, cx: &mut Context<Self>) {
        let Some(modal) = &mut self.file_op_modal else {
            return;
        };
        match modal {
            FileOpModalState::Rename { id, input, error } => {
                let new_name = input.read(cx).value().trim().to_string();
                if new_name.is_empty() {
                    *error = Some("Filename cannot be empty.".to_string());
                    cx.notify();
                    return;
                }
                let id = id.clone();
                self.file_op_modal = None;
                self.run_command(EngineCommand::RenameFile { id, new_name }, cx);
            }
            FileOpModalState::Move { id, input, error } => {
                let dir_str = input.read(cx).value().trim().to_string();
                if dir_str.is_empty() {
                    *error = Some("Destination directory cannot be empty.".to_string());
                    cx.notify();
                    return;
                }
                let id = id.clone();
                let target_dir = PathBuf::from(dir_str);
                self.file_op_modal = None;
                self.run_command(EngineCommand::MoveFile { id, target_dir }, cx);
            }
            FileOpModalState::Integrity { .. } => {
                self.file_op_modal = None;
                cx.notify();
            }
        }
    }

    fn open_download_folder(&mut self, output_path: Option<PathBuf>, cx: &mut Context<Self>) {
        let Some(path) = output_path else {
            self.status_banner = Some("No output path recorded for this download.".to_string());
            cx.notify();
            return;
        };
        if let Err(err) = open_folder_in_shell(&path) {
            self.status_banner = Some(err);
            cx.notify();
        }
    }

    fn resume_all(&mut self, cx: &mut Context<Self>) {
        let ids = self.queue.resume_all_ids();
        self.run_command(EngineCommand::ResumeMany(ids), cx);
    }

    fn pause_all(&mut self, cx: &mut Context<Self>) {
        let ids = self.queue.pause_all_ids();
        self.run_command(EngineCommand::PauseMany(ids), cx);
    }

    fn clear_completed(&mut self, cx: &mut Context<Self>) {
        let ids = self.queue.clear_completed_ids();
        self.run_command(
            EngineCommand::DeleteMany {
                ids,
                delete_file: false,
            },
            cx,
        );
    }

    fn run_command(&self, command: EngineCommand, cx: &mut Context<Self>) {
        self.run_command_with(command, cx, |_, _| {});
    }

    fn run_command_with<F>(
        &self,
        command: EngineCommand,
        cx: &mut Context<Self>,
        on_success: F,
    ) where
        F: FnOnce(&mut Self, Option<DownloadId>) + 'static,
    {
        let rx = self.bridge.execute(command);
        cx.spawn(async move |this, cx| {
            let result = rx.await;
            let _ = this.update(cx, |app, cx| match result {
                Ok(Ok(outcome)) => {
                    on_success(app, outcome.created_id);
                    app.apply_snapshot(outcome.snapshot, cx);
                }
                Ok(Err(err)) => {
                    app.status_banner = Some(err.to_string());
                    cx.notify();
                }
                Err(_) => {
                    app.status_banner = Some("Engine bridge channel closed".to_string());
                    cx.notify();
                }
            });
        })
        .detach();
    }

    fn toggle_sort(&mut self, col: SortColumn, cx: &mut Context<Self>) {
        if self.sort_col == col {
            self.sort_dir = self.sort_dir.toggle();
        } else {
            self.sort_col = col;
            self.sort_dir = SortDirection::Ascending;
        }
        cx.notify();
    }
}

impl Render for PosthazeApp {
    fn render(&mut self, window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let zoom_rem = 16.0 * (self.queue.settings.ui_zoom_percent.clamp(80, 150) as f32) / 100.0;
        window.set_rem_size(px(zoom_rem));
        let viewport = window.viewport_size();
        let target_left_width = if self.sidebar_collapsed {
            RAIL_SIDEBAR_WIDTH
        } else {
            self.sidebar_width
        };
        let left_width = self.motion.current_sidebar_width(target_left_width);
        let right_width = (f32::from(viewport.width) - left_width - 7.0).max(0.0);
        let profile = compute_layout_profile(
            right_width,
            f32::from(viewport.height),
            self.inspector_height,
        )
        .with_settings(&self.queue.settings);
        let target_inspector_height = if self.inspector_collapsed {
            COLLAPSED_INSPECTOR_HEIGHT
        } else {
            profile.effective_inspector_height
        };
        let animated_inspector_height = self
            .motion
            .current_inspector_height(target_inspector_height);
        let render_inspector_collapsed = self.inspector_collapsed
            && (animated_inspector_height - COLLAPSED_INSPECTOR_HEIGHT).abs() <= 1.0;
        let ranks = compute_fifo_ranks(&self.queue.records, &self.queue.live_map);
        let all_rows: Vec<DownloadRowView> = self
            .queue
            .records
            .iter()
            .map(|rec| {
                DownloadRowView::from_record(
                    rec,
                    self.queue.live_map.get(&rec.id),
                    self.queue.segment_counts.get(&rec.id).copied().unwrap_or(0),
                    ranks.get(&rec.id).copied(),
                )
            })
            .collect();

        let total_speed: u64 = all_rows
            .iter()
            .filter(|r| r.state == DownloadState::Active)
            .map(|r| r.bytes_per_sec)
            .sum();
        let active_slots = all_rows
            .iter()
            .filter(|r| matches!(r.state, DownloadState::Active | DownloadState::Probing))
            .count();

        let search_query = self.search_input.read(cx).value();
        let visible_rows = filter_and_sort_rows_with_query(
            &all_rows,
            self.filter,
            &search_query,
            self.sort_col,
            self.sort_dir,
        );
        let is_empty_queue = all_rows.is_empty();

        let selected_row = self
            .queue
            .selected_id
            .as_ref()
            .and_then(|id| all_rows.iter().find(|r| &r.id == id));
        let selected_segments: Vec<SegmentProgress> = self
            .queue
            .selected_id
            .as_ref()
            .and_then(|id| {
                self.queue.live_map
                    .get(id)
                    .filter(|s| !s.segments.is_empty())
                    .map(|s| s.segments.clone())
                    .or_else(|| self.queue.persisted_segments.get(id).cloned())
            })
            .unwrap_or_default();

        let inspector_motion = selected_row
            .map(|r| self.motion.sample(&r.id, r.total_bytes, Instant::now()))
            .unwrap_or_default();
        let entity_restart = cx.entity().clone();
        let entity_delete = cx.entity().clone();
        let entity_inspector_toggle = cx.entity().clone();
        let inspector = render_inspector(
            selected_row,
            &selected_segments,
            px(animated_inspector_height),
            render_inspector_collapsed,
            inspector_motion,
            InspectorCallbacks {
                on_restart_fresh: move |id: DownloadId, _: &mut Window, cx: &mut App| {
                    entity_restart.update(cx, |app, cx| app.restart_fresh_download(id, cx));
                },
                on_delete: move |id: DownloadId, _: &mut Window, cx: &mut App| {
                    entity_delete.update(cx, |app, cx| {
                        let filename = app
                            .queue
                            .records
                            .iter()
                            .find(|r| r.id == id)
                            .map(derive_display_filename)
                            .unwrap_or_else(|| "download".to_string());
                        app.request_delete_download(id, filename, cx);
                    });
                },
                on_toggle_collapse: move |_: &mut Window, cx: &mut App| {
                    entity_inspector_toggle.update(cx, |app, cx| {
                        let from = if app.inspector_collapsed {
                            COLLAPSED_INSPECTOR_HEIGHT
                        } else {
                            app.inspector_height
                        };
                        app.inspector_collapsed = !app.inspector_collapsed;
                        let to = if app.inspector_collapsed {
                            COLLAPSED_INSPECTOR_HEIGHT
                        } else {
                            app.inspector_height
                        };
                        app.motion.animate_inspector(from, to);
                        app.ensure_motion_loop(cx);
                        cx.notify();
                    });
                },
            },
        );

        let settings_overlay = self.settings_modal.as_ref().map(|modal| {
            let e_action = cx.entity().clone();
            let e_cancel = cx.entity().clone();
            let e_save = cx.entity().clone();
            render_settings_modal(
                modal,
                &self.update_state,
                self.is_portable,
                move |action, window, cx| {
                    e_action.update(cx, |app, cx| app.handle_settings_action(action, window, cx));
                },
                move |window, cx| {
                    e_cancel.update(cx, |app, cx| {
                        app.settings_modal = None;
                        theme::apply_theme_with_accent(
                            &app.queue.settings.ui_theme,
                            app.queue.settings.ui_accent_override.as_deref(),
                            window.appearance(),
                            false,
                            cx,
                        );
                        cx.notify();
                    });
                },
                move |_, cx| {
                    e_save.update(cx, |app, cx| app.save_settings_modal(cx));
                },
            )
        });

        let file_op_overlay = self.file_op_modal.as_ref().map(|modal| {
            let e_cancel = cx.entity().clone();
            let e_confirm = cx.entity().clone();
            let e_copy = cx.entity().clone();
            render_file_op_modal(
                modal,
                move |_, cx| {
                    e_cancel.update(cx, |app, cx| {
                        app.file_op_modal = None;
                        cx.notify();
                    });
                },
                move |_, cx| {
                    e_confirm.update(cx, |app, cx| app.execute_file_op_confirm(cx));
                },
                move |hash, _, cx| {
                    cx.write_to_clipboard(ClipboardItem::new_string(hash));
                    e_copy.update(cx, |app, cx| {
                        app.status_banner = Some("BLAKE3 digest copied to clipboard.".to_string());
                        cx.notify();
                    });
                },
            )
        });

        let confirm_overlay = self.confirm_modal.as_ref().map(|action| {
            let e_cancel = cx.entity().clone();
            let e_confirm = cx.entity().clone();
            render_confirm_modal(
                action,
                move |_, cx| {
                    e_cancel.update(cx, |app, cx| {
                        app.confirm_modal = None;
                        cx.notify();
                    });
                },
                move |_, cx| {
                    e_confirm.update(cx, |app, cx| app.execute_confirmed_action(cx));
                },
            )
        });

        let context_menu_overlay = self.context_menu.as_ref().and_then(|menu| {
            let row = all_rows.iter().find(|r| r.id == menu.download_id)?;
            let e_close = cx.entity().clone();
            let e_action = cx.entity().clone();
            Some(render_context_menu(
                row,
                menu.anchor,
                viewport,
                move |_, cx| {
                    e_close.update(cx, |app, cx| {
                        app.context_menu = None;
                        cx.notify();
                    });
                },
                move |id, action, window, cx| {
                    e_action.update(cx, |app, cx| {
                        app.execute_context_menu_action(id, action, window, cx);
                    });
                },
            ))
        });

        let active_resize = self.active_resize;
        let resizing_sidebar = matches!(active_resize, Some(ActiveResize::Sidebar { .. }));
        let resizing_inspector = matches!(active_resize, Some(ActiveResize::Inspector { .. }));

        let visible_ids: std::sync::Arc<Vec<DownloadId>> =
            std::sync::Arc::new(visible_rows.iter().map(|r| r.id.clone()).collect());
        let visible_ids_kbd = visible_ids.clone();
        let total_count = all_rows.len();

        div()
            .relative()
            .flex()
            .flex_col()
            .size_full()
            .bg(paper_bg())
            .text_color(carbon_text())
            .track_focus(&self.focus_handle)
            .on_key_down(cx.listener(move |app, event: &KeyDownEvent, window, cx| {
                let key = event.keystroke.key.as_str();
                let kb_ctx = KeyboardContext {
                    has_confirm_modal: app.confirm_modal.is_some(),
                    has_preflight_resolved: false,
                    has_any_preflight: false,
                    has_settings_modal: app.settings_modal.is_some(),
                    has_file_op_modal: app.file_op_modal.is_some(),
                    url_input_focused: app.url_input.read(cx).focus_handle(cx).is_focused(window)
                        || app.search_input.read(cx).focus_handle(cx).is_focused(window),
                };

                let ctrl_or_cmd =
                    event.keystroke.modifiers.control || event.keystroke.modifiers.platform;
                let shift = event.keystroke.modifiers.shift;
                if ctrl_or_cmd && key == "q" {
                    app.quitting = true;
                    cx.quit();
                    return;
                }
                if ctrl_or_cmd
                    && key == "a"
                    && !kb_ctx.has_confirm_modal
                    && !kb_ctx.has_any_preflight
                    && !kb_ctx.has_settings_modal
                    && !kb_ctx.has_file_op_modal
                    && !kb_ctx.url_input_focused
                {
                    app.queue.select_all(&visible_ids_kbd);
                    if let Some(lead) = app.queue.selected_id.clone() {
                        app.load_segments_for(lead, cx);
                    }
                    cx.notify();
                    return;
                }

                match resolve_keyboard_action(key, kb_ctx) {
                    KeyboardAction::DismissOverlays => {
                        let mut changed = false;
                        if app.confirm_modal.take().is_some() {
                            changed = true;
                        }
                        if app.settings_modal.take().is_some() {
                            theme::apply_theme_with_accent(
                                &app.queue.settings.ui_theme,
                                app.queue.settings.ui_accent_override.as_deref(),
                                window.appearance(),
                                false,
                                cx,
                            );
                            changed = true;
                        }
                        if app.file_op_modal.take().is_some() {
                            changed = true;
                        }
                        if app.context_menu.take().is_some() {
                            changed = true;
                        }
                        if changed {
                            cx.notify();
                        }
                    }
                    KeyboardAction::ConfirmActiveModal => {
                        if app.confirm_modal.is_some() {
                            app.execute_confirmed_action(cx);
                        } else if app.settings_modal.is_some() {
                            app.save_settings_modal(cx);
                        } else if app.file_op_modal.is_some() {
                            app.execute_file_op_confirm(cx);
                        }
                    }
                    KeyboardAction::StepSelection(delta) => {
                        if let Some(next_id) =
                            app.queue
                                .step_selection_extending(&visible_ids_kbd, delta, shift)
                        {
                            if let Some(idx) = visible_ids_kbd.iter().position(|id| id == &next_id) {
                                let row_height = app.queue.settings.ui_table_density.row_height_px();
                                let (scroll_top, viewport_h, max_scroll) = {
                                    let inner = app.table_scroll_handle.0.borrow();
                                    let top = -f32::from(inner.base_handle.offset().y);
                                    let vh = f32::from(inner.base_handle.bounds().size.height);
                                    let max_s = f32::from(inner.base_handle.max_offset().y);
                                    (top, vh, max_s)
                                };
                                if viewport_h > 1.0 && max_scroll > 0.5 {
                                    let row_top = idx as f32 * row_height;
                                    let row_bottom = row_top + row_height;
                                    let target_top = if row_top < scroll_top {
                                        Some(row_top.clamp(0.0, max_scroll))
                                    } else if row_bottom > scroll_top + viewport_h {
                                        Some((row_bottom - viewport_h).clamp(0.0, max_scroll))
                                    } else {
                                        None
                                    };
                                    if let Some(target) = target_top {
                                        app.motion.animate_table_scroll(scroll_top, target);
                                        app.ensure_motion_loop(cx);
                                    }
                                }
                            }
                            app.load_segments_for(next_id, cx);
                            cx.notify();
                        }
                    }
                    KeyboardAction::TriggerPrimaryAction => {
                        for id in app.queue.selected_list() {
                            if let Some(action) = app.queue.primary_action_for(&id) {
                                match action {
                                    PrimaryDownloadAction::Pause(id) => app.pause_download(id, cx),
                                    PrimaryDownloadAction::Resume(id) => app.resume_download(id, cx),
                                    PrimaryDownloadAction::RestartFresh(id) => {
                                        app.restart_fresh_download(id, cx)
                                    }
                                }
                            }
                        }
                    }
                    KeyboardAction::RequestDeleteSelected => {
                        app.request_delete_selected(cx);
                    }
                    KeyboardAction::None => {}
                }
            }))
            .when(resizing_sidebar, |el| el.cursor_col_resize())
            .when(resizing_inspector, |el| el.cursor_row_resize())
            .on_mouse_move(cx.listener(|app, event: &MouseMoveEvent, _, cx| {
                let Some(resize) = app.active_resize else {
                    return;
                };
                if event.pressed_button != Some(MouseButton::Left) {
                    app.active_resize = None;
                    cx.notify();
                    return;
                }
                match resize {
                    ActiveResize::Sidebar {
                        start_x,
                        start_width,
                    } => {
                        let delta = f32::from(event.position.x - start_x);
                        let (detented, did_snap) = apply_magnetic_detent(
                            start_width + delta,
                            DEFAULT_SIDEBAR_WIDTH,
                            SIDEBAR_MAGNETIC_SNAP_RADIUS,
                        );
                        let (collapsed, expanded_width) =
                            resolve_sidebar_drag(detented, app.sidebar_width);
                        let was_collapsed = app.sidebar_collapsed;
                        let width_changed = (expanded_width - app.sidebar_width).abs() > 0.5;
                        app.sidebar_collapsed = collapsed;
                        app.sidebar_width = expanded_width;
                        app.motion.set_sidebar_immediate();
                        if (did_snap && width_changed) || (collapsed != was_collapsed) {
                            app.motion.trigger_sash_detent(Instant::now());
                            app.ensure_motion_loop(cx);
                        }
                    }
                    ActiveResize::Inspector {
                        start_y,
                        start_height,
                    } => {
                        let delta = f32::from(start_y - event.position.y);
                        let (detented, did_snap) = apply_magnetic_detent(
                            start_height + delta,
                            DEFAULT_INSPECTOR_HEIGHT,
                            INSPECTOR_MAGNETIC_SNAP_RADIUS,
                        );
                        let (collapsed, expanded_height) =
                            resolve_inspector_drag(detented, app.inspector_height);
                        let was_collapsed = app.inspector_collapsed;
                        let height_changed = (expanded_height - app.inspector_height).abs() > 0.5;
                        app.inspector_collapsed = collapsed;
                        app.inspector_height = expanded_height;
                        app.motion.set_inspector_immediate();
                        if (did_snap && height_changed) || (collapsed != was_collapsed) {
                            app.motion.trigger_sash_detent(Instant::now());
                            app.ensure_motion_loop(cx);
                        }
                    }
                }
                cx.notify();
            }))
            .on_mouse_up(
                MouseButton::Left,
                cx.listener(|app, _, _, cx| {
                    if app.active_resize.take().is_some() {
                        cx.notify();
                    }
                }),
            )
            .child(self.render_titlebar(active_slots, total_speed, total_count, cx))
            .child(
                div()
                    .flex_1()
                    .min_h_0()
                    .flex()
                    .overflow_hidden()
                    // Left Navigation & Filter Rail (52px icon rail when collapsed, 180-380px when expanded)
                    .child(self.render_sidebar(&all_rows, total_speed, active_slots, cx))
                    .child(
                        div()
                            .id("sidebar-resize")
                            .w(px(7.0))
                            .h_full()
                            .flex_shrink_0()
                            .flex()
                            .justify_center()
                            .cursor_col_resize()
                            .hover(|s| s.bg(rgba(0x2A282555)))
                            .on_mouse_down(
                                MouseButton::Left,
                                cx.listener(|app, ev: &MouseDownEvent, _, cx| {
                                    cx.stop_propagation();
                                    let start_width = if app.sidebar_collapsed {
                                        RAIL_SIDEBAR_WIDTH
                                    } else {
                                        app.sidebar_width
                                    };
                                    app.active_resize = Some(ActiveResize::Sidebar {
                                        start_x: ev.position.x,
                                        start_width,
                                    });
                                    cx.notify();
                                }),
                            )
                            .on_double_click(cx.listener(|app, _, _, cx| {
                                let from = if app.sidebar_collapsed {
                                    RAIL_SIDEBAR_WIDTH
                                } else {
                                    app.sidebar_width
                                };
                                app.sidebar_collapsed = false;
                                app.sidebar_width = DEFAULT_SIDEBAR_WIDTH;
                                app.motion.animate_sidebar(from, DEFAULT_SIDEBAR_WIDTH);
                                app.motion.trigger_sash_detent(Instant::now());
                                app.ensure_motion_loop(cx);
                                cx.notify();
                            }))
                            .relative()
                            .child({
                                let detent_flash = self.motion.sash_detent_flash(Instant::now());
                                let is_snapped = !self.sidebar_collapsed
                                    && (self.sidebar_width - DEFAULT_SIDEBAR_WIDTH).abs() < 0.5;
                                div()
                                    .w(px(if resizing_sidebar || detent_flash > 0.05 {
                                        2.0
                                    } else {
                                        1.0
                                    }))
                                    .h_full()
                                    .bg(if detent_flash > 0.05 || (resizing_sidebar && is_snapped) {
                                        cyanotype_blue()
                                    } else if resizing_sidebar {
                                        selenium_amber()
                                    } else {
                                        graphite_border()
                                    })
                            })
                            .when(resizing_sidebar, |el| {
                                let is_snapped = !self.sidebar_collapsed
                                    && (self.sidebar_width - DEFAULT_SIDEBAR_WIDTH).abs() < 0.5;
                                let label = if self.sidebar_collapsed {
                                    format!("RAIL {:.0} px", RAIL_SIDEBAR_WIDTH)
                                } else if is_snapped {
                                    format!("{:.0} px · DEFAULT", self.sidebar_width)
                                } else {
                                    format!("{:.0} px", self.sidebar_width)
                                };
                                el.child(
                                    div()
                                        .absolute()
                                        .left(px(10.0))
                                        .top(px(14.0))
                                        .px_2()
                                        .py_0p5()
                                        .rounded(px(3.0))
                                        .bg(graphite_bg())
                                        .border_1()
                                        .border_color(if is_snapped {
                                            cyanotype_blue()
                                        } else {
                                            selenium_amber()
                                        })
                                        .font_family(MONO_FONT)
                                        .text_size(TYPE_HEADER)
                                        .text_color(silver_text())
                                        .whitespace_nowrap()
                                        .child(label),
                                )
                            }),
                    )
                    // Main Right Column: Top Toolbar + Center Table + Collapsible & Resizable Bottom Inspector
                    .child(
                        div()
                            .flex()
                            .flex_col()
                            .flex_1()
                            .min_w_0()
                            .h_full()
                            .overflow_hidden()
                            .child(self.render_toolbar(profile, cx))
                            .when_some(self.status_banner.clone(), |el, msg| {
                                el.child(
                                    div()
                                        .px_4()
                                        .py_1p5()
                                        .bg(errata_surface_bg())
                                        .border_b_1()
                                        .border_color(errata_vermilion())
                                        .flex()
                                        .items_center()
                                        .justify_between()
                                        .gap_3()
                                        .child(
                                            div()
                                                .flex()
                                                .items_center()
                                                .gap_2()
                                                .min_w_0()
                                                .child(
                                                    Icon::new(IconName::TriangleAlert)
                                                        .size(px(14.0))
                                                        .text_color(errata_vermilion()),
                                                )
                                                .child(
                                                    div()
                                                        .font_family(MONO_FONT)
                                                        .text_size(TYPE_BODY)
                                                        .text_color(silver_text())
                                                        .truncate()
                                                        .child(msg),
                                                ),
                                        )
                                        .child(
                                            div().h(px(26.0)).child(grid_cell_button(
                                                "dismiss-status-banner",
                                                Some(IconName::X),
                                                None,
                                                Some("Dismiss"),
                                                GridTone::Danger,
                                                CellBorder::Left,
                                                true,
                                                cx.listener(|app, _, _, cx| {
                                                    app.status_banner = None;
                                                    cx.notify();
                                                }),
                                            )),
                                        ),
                                )
                            })
                            .child(self.render_table(
                                visible_rows,
                                visible_ids,
                                is_empty_queue,
                                profile,
                                cx,
                            ))
                            .child(
                                div()
                                    .id("inspector-resize")
                                    .h(px(7.0))
                                    .w_full()
                                    .flex_shrink_0()
                                    .flex()
                                    .items_center()
                                    .cursor_row_resize()
                                    .hover(|s| s.bg(rgba(0x2A282555)))
                                    .on_mouse_down(
                                        MouseButton::Left,
                                        cx.listener(|app, ev: &MouseDownEvent, _, cx| {
                                            cx.stop_propagation();
                                            let start_height = if app.inspector_collapsed {
                                                COLLAPSED_INSPECTOR_HEIGHT
                                            } else {
                                                app.inspector_height
                                            };
                                            app.active_resize = Some(ActiveResize::Inspector {
                                                start_y: ev.position.y,
                                                start_height,
                                            });
                                            cx.notify();
                                        }),
                                    )
                                    .on_double_click(cx.listener(|app, _, _, cx| {
                                        let from = if app.inspector_collapsed {
                                            COLLAPSED_INSPECTOR_HEIGHT
                                        } else {
                                            app.inspector_height
                                        };
                                        app.inspector_collapsed = false;
                                        app.inspector_height = DEFAULT_INSPECTOR_HEIGHT;
                                        app.motion.animate_inspector(from, DEFAULT_INSPECTOR_HEIGHT);
                                        app.motion.trigger_sash_detent(Instant::now());
                                        app.ensure_motion_loop(cx);
                                        cx.notify();
                                    }))
                                    .relative()
                                    .child({
                                        let detent_flash =
                                            self.motion.sash_detent_flash(Instant::now());
                                        let is_snapped = !self.inspector_collapsed
                                            && (self.inspector_height - DEFAULT_INSPECTOR_HEIGHT)
                                                .abs()
                                                < 0.5;
                                        div()
                                            .h(px(if resizing_inspector || detent_flash > 0.05 {
                                                2.0
                                            } else {
                                                1.0
                                            }))
                                            .w_full()
                                            .bg(if detent_flash > 0.05
                                                || (resizing_inspector && is_snapped)
                                            {
                                                cyanotype_blue()
                                            } else if resizing_inspector {
                                                selenium_amber()
                                            } else {
                                                graphite_border()
                                            })
                                    })
                                    .when(resizing_inspector, |el| {
                                        let is_snapped = !self.inspector_collapsed
                                            && (self.inspector_height - DEFAULT_INSPECTOR_HEIGHT)
                                                .abs()
                                                < 0.5;
                                        let label = if self.inspector_collapsed {
                                            format!("COLLAPSED {:.0} px", COLLAPSED_INSPECTOR_HEIGHT)
                                        } else if is_snapped {
                                            format!("{:.0} px · DEFAULT", self.inspector_height)
                                        } else {
                                            format!("{:.0} px", self.inspector_height)
                                        };
                                        el.child(
                                            div()
                                                .absolute()
                                                .right(px(16.0))
                                                .top(px(-24.0))
                                                .px_2()
                                                .py_0p5()
                                                .rounded(px(3.0))
                                                .bg(graphite_bg())
                                                .border_1()
                                                .border_color(if is_snapped {
                                                    cyanotype_blue()
                                                } else {
                                                    selenium_amber()
                                                })
                                                .font_family(MONO_FONT)
                                                .text_size(TYPE_HEADER)
                                                .text_color(silver_text())
                                                .whitespace_nowrap()
                                                .child(label),
                                        )
                                    }),
                            )
                            .child(inspector),
                    ),
            )
            .when_some(context_menu_overlay, |el, overlay| el.child(overlay))
            .when_some(settings_overlay, |el, overlay| el.child(overlay))
            .when_some(file_op_overlay, |el, overlay| el.child(overlay))
            .when_some(confirm_overlay, |el, overlay| el.child(overlay))
    }
}

impl PosthazeApp {
    fn render_titlebar(
        &self,
        active_slots: usize,
        total_speed: u64,
        total_count: usize,
        cx: &mut Context<Self>,
    ) -> impl IntoElement {
        let update_chip = self.update_state.available_release().map(|info| {
            let label = format!("UPDATE v{}", info.version);
            div()
                .id("titlebar-update-chip")
                .h_full()
                .px_2p5()
                .border_r_1()
                .border_color(graphite_border())
                .bg(selenium_amber())
                .hover(|s| s.bg(active_theme().accent_hover.hsla()))
                .cursor_pointer()
                .flex()
                .items_center()
                .gap_1()
                .font_family(MONO_FONT)
                .text_size(TYPE_MICRO)
                .font_weight(FontWeight::SEMIBOLD)
                .text_color(active_theme().accent_foreground.hsla())
                .on_click(cx.listener(|app, _, window, cx| {
                    app.open_settings_modal_on_tab(SettingsTab::Updates, window, cx);
                }))
                .child(Icon::new(IconName::ArrowDown).size(px(11.0)))
                .child(label)
        });

        TitleBar::new()
            .bg(graphite_bg())
            .border_color(graphite_border())
            .child(
                div()
                    .h_full()
                    .flex()
                    .items_center()
                    // Brand cell
                    .child(
                        div()
                            .h_full()
                            .pr_3()
                            .border_r_1()
                            .border_color(graphite_border())
                            .flex()
                            .items_center()
                            .gap_2()
                            .child(
                                svg()
                                    .path(BRAND_LOGO_SVG)
                                    .w(px(15.0))
                                    .h(px(13.0))
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
                    // Version cell
                    .child(
                        div()
                            .h_full()
                            .px_2p5()
                            .border_r_1()
                            .border_color(graphite_border())
                            .flex()
                            .items_center()
                            .font_family(MONO_FONT)
                            .text_size(TYPE_HEADER)
                            .text_color(silver_muted())
                            .child(concat!("v", env!("CARGO_PKG_VERSION"), " beta")),
                    )
                    // Update available chip
                    .when_some(update_chip, |el, chip| el.child(chip))
                    // Engine / Queue telemetry cell
                    .when(self.queue.settings.ui_show_titlebar_telemetry, |el| {
                        el.child(
                            div()
                                .h_full()
                                .px_3()
                                .border_r_1()
                                .border_color(graphite_border())
                                .flex()
                                .items_center()
                                .gap_2()
                                .font_family(MONO_FONT)
                                .text_size(TYPE_HEADER)
                                .text_color(silver_muted())
                                .child(div().w(px(6.0)).h(px(6.0)).bg(if active_slots > 0 {
                                    selenium_amber()
                                } else {
                                    graphite_border()
                                }))
                                .child(if active_slots > 0 {
                                    format!(
                                        "ACTIVE {active_slots}/{} · {}",
                                        self.queue.settings.max_concurrent_downloads,
                                        view_model::format_speed(total_speed)
                                    )
                                } else {
                                    format!("IDLE · {total_count} QUEUED")
                                }),
                        )
                    }),
            )
    }
}
