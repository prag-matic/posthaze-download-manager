//! Posthaze Download Manager desktop UI powered by GPUI and GPUI Kit.

mod assets;
mod bridge;
mod context_menu;
mod curl_parser;
mod inspector;
mod ipc;
mod modals;
mod motion;
mod platform_hooks;
mod settings_modal;
mod sidebar;
mod table;
mod theme;
mod view_model;

use std::path::PathBuf;
use std::time::{Duration, Instant};

use gpui_kit::assets::IconName;
use gpui_kit::component::button::{Button, ButtonVariants};
use gpui_kit::component::input::{InputEvent, InputState};
use gpui_kit::component::{Icon, InteractiveElementExt as _, Sizable, TitleBar};
use gpui_kit::prelude::FluentBuilder;
use gpui_kit::*;
use posthaze_engine::{
    CollisionPolicy, DeleteDefaultAction, DownloadId, DownloadRequest, DownloadState, EngineEvent,
    SegmentProgress, categorize_subfolder, default_db_path, resolve_target_path,
};

use crate::assets::{BRAND_LOGO_SVG, PosthazeAssets};
use crate::bridge::{EngineBridge, EngineCommand, EngineSnapshot};
use crate::context_menu::{ContextMenuAction, ContextMenuState, render_context_menu};
use crate::curl_parser::{find_header_value, parse_url_or_curl, set_or_remove_header};
use crate::inspector::{InspectorCallbacks, render_inspector};
use crate::modals::{
    ConfirmAction, FileOpModalState, PreflightModalState, render_confirm_modal,
    render_file_op_modal, render_preflight_modal,
};
use crate::motion::MotionState;
use crate::platform_hooks::{
    open_file_in_shell, open_folder_in_shell, spawn_completion_hook, spawn_windows_toast,
};
use crate::settings_modal::{SettingsAction, SettingsModalState, render_settings_modal};
use crate::theme::*;
use crate::view_model::{
    COLLAPSED_INSPECTOR_HEIGHT, DEFAULT_INSPECTOR_HEIGHT, DEFAULT_SIDEBAR_WIDTH, DownloadRowView,
    FilterTab, INSPECTOR_MAGNETIC_SNAP_RADIUS, KeyboardAction, KeyboardContext,
    PrimaryDownloadAction, QueueController, RAIL_SIDEBAR_WIDTH, SIDEBAR_MAGNETIC_SNAP_RADIUS,
    SortColumn, SortDirection, UiEffect, apply_magnetic_detent, compute_fifo_ranks,
    compute_layout_profile, derive_display_filename, filter_and_sort_rows_with_query,
    resolve_inspector_drag, resolve_keyboard_action, resolve_sidebar_drag,
};

fn main() {
    let bridge = EngineBridge::open(default_db_path()).expect("failed to initialize engine");

    gpui_kit::application()
        .with_assets(PosthazeAssets)
        .run(move |cx| {
            gpui_kit::init(cx);
            theme::apply_theme("chronophotographic", None, cx);

            let window_options = WindowOptions {
                window_bounds: Some(WindowBounds::Windowed(Bounds::centered(
                    None,
                    size(px(1020.0), px(640.0)),
                    cx,
                ))),
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
    preflight_modal: Option<PreflightModalState>,
    settings_modal: Option<SettingsModalState>,
    confirm_modal: Option<ConfirmAction>,
    file_op_modal: Option<FileOpModalState>,
    context_menu: Option<ContextMenuState>,
    pub(crate) sidebar_width: f32,
    pub(crate) sidebar_collapsed: bool,
    pub(crate) inspector_height: f32,
    pub(crate) inspector_collapsed: bool,
    active_resize: Option<ActiveResize>,
    pub(crate) motion: MotionState,
    motion_loop_running: bool,
    pub(crate) table_scroll_handle: UniformListScrollHandle,
    refresh_in_flight: bool,
    refresh_pending: bool,
    last_progress_notify: Instant,
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
            preflight_modal: None,
            settings_modal: None,
            confirm_modal: None,
            file_op_modal: None,
            context_menu: None,
            sidebar_width: DEFAULT_SIDEBAR_WIDTH,
            sidebar_collapsed: true,
            inspector_height: DEFAULT_INSPECTOR_HEIGHT,
            inspector_collapsed: true,
            active_resize: None,
            motion: MotionState::default(),
            motion_loop_running: false,
            table_scroll_handle: UniformListScrollHandle::new(),
            refresh_in_flight: false,
            refresh_pending: false,
            last_progress_notify: Instant::now(),
        };

        app.refresh_from_engine(cx);
        app.start_event_listener(cx);
        app.start_ipc_listener(cx);
        app
    }

    fn apply_snapshot(&mut self, snapshot: EngineSnapshot, cx: &mut Context<Self>) {
        let effect = self.queue.apply_snapshot(snapshot);
        theme::apply_theme(&self.queue.settings.ui_theme, None, cx);
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

    fn start_ipc_listener(&self, cx: &mut Context<Self>) {
        let mut rx = ipc::start_named_pipe_server(self.bridge.runtime());
        let window_handle = self.window_handle;
        cx.spawn(async move |this, cx| {
            while let Some(req) = rx.recv().await {
                let _ = cx.update_window(window_handle, |_, window, cx| {
                    let _ = this.update(cx, |app, cx| {
                        app.url_input.update(cx, |state, cx| {
                            state.set_value(req.url.clone(), window, cx);
                        });
                        app.begin_probe(req.url, req.headers, window, cx);
                    });
                });
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
        self.preflight_modal = Some(PreflightModalState::Probing {
            url: url.clone(),
            headers: headers.clone(),
        });
        cx.notify();

        let out_dir = self.queue.settings.default_output_dir.clone();
        let default_workers = self.queue.settings.max_workers.clamp(1, 32);
        let window_handle = window.window_handle();
        let probe_headers = headers.clone();
        let rx = self.bridge.probe(url.clone(), out_dir, probe_headers);

        cx.spawn(async move |this, cx| {
            let res = rx.await;
            let _ = cx.update_window(window_handle, |_, window, cx| {
                let _ = this.update(cx, |app, cx| {
                    // Only apply if user has not closed the probing modal.
                    if !matches!(app.preflight_modal, Some(PreflightModalState::Probing { .. })) {
                        return;
                    }
                    match res {
                        Ok(Ok(info)) => {
                            let initial_name =
                                if app.queue.settings.collision_policy == CollisionPolicy::Rename {
                                    info.disambiguated_output_path
                                        .file_name()
                                        .and_then(|n| n.to_str())
                                        .unwrap_or(&info.suggested_filename)
                                        .to_string()
                                } else {
                                    info.suggested_filename.clone()
                                };
                            let base_dir = info
                                .disambiguated_output_path
                                .parent()
                                .map(PathBuf::from)
                                .unwrap_or_else(|| app.queue.settings.default_output_dir.clone());
                            let categorized_dir = if app.queue.settings.auto_categorize_folders {
                                if let Some(sub) = categorize_subfolder(&initial_name) {
                                    base_dir.join(sub)
                                } else {
                                    base_dir
                                }
                            } else {
                                base_dir
                            };
                            let initial_dir = categorized_dir.to_string_lossy().to_string();
                            let filename_input = cx.new(|cx| {
                                let mut st = InputState::new(window, cx);
                                st.set_value(initial_name, window, cx);
                                st
                            });
                            let dir_input = cx.new(|cx| {
                                let mut st = InputState::new(window, cx);
                                st.set_value(initial_dir, window, cx);
                                st
                            });
                            let initial_cookie = find_header_value(&headers, "Cookie").to_string();
                            let initial_referer = find_header_value(&headers, "Referer").to_string();
                            let initial_ua = find_header_value(&headers, "User-Agent").to_string();
                            let cookie_input = cx.new(|cx| {
                                let mut st = InputState::new(window, cx)
                                    .placeholder("Optional Cookie header...");
                                st.set_value(initial_cookie, window, cx);
                                st
                            });
                            let referer_input = cx.new(|cx| {
                                let mut st = InputState::new(window, cx)
                                    .placeholder("Optional Referer URL...");
                                st.set_value(initial_referer, window, cx);
                                st
                            });
                            let user_agent_input = cx.new(|cx| {
                                let mut st = InputState::new(window, cx)
                                    .placeholder("Optional User-Agent override...");
                                st.set_value(initial_ua, window, cx);
                                st
                            });
                            let workers = if info.supports_ranges {
                                default_workers
                            } else {
                                1
                            };
                            app.preflight_modal = Some(PreflightModalState::Resolved {
                                info: Box::new(info),
                                filename_input,
                                dir_input,
                                cookie_input,
                                referer_input,
                                user_agent_input,
                                headers,
                                editing_headers: false,
                                workers,
                            });
                        }
                        Ok(Err(err)) => {
                            app.preflight_modal = Some(PreflightModalState::Error {
                                url,
                                headers,
                                message: err.to_string(),
                            });
                        }
                        Err(_) => {
                            app.preflight_modal = Some(PreflightModalState::Error {
                                url,
                                headers,
                                message: "Engine bridge channel closed".to_string(),
                            });
                        }
                    }
                    cx.notify();
                });
            });
        })
        .detach();
    }

    fn confirm_preflight(
        &mut self,
        start_immediately: bool,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        let Some(PreflightModalState::Resolved {
            info,
            filename_input,
            dir_input,
            cookie_input,
            referer_input,
            user_agent_input,
            mut headers,
            workers,
            ..
        }) = self.preflight_modal.take()
        else {
            return;
        };

        let filename = filename_input.read(cx).value().trim().to_string();
        let dir_str = dir_input.read(cx).value().trim().to_string();
        let cookie_val = cookie_input.read(cx).value().to_string();
        let referer_val = referer_input.read(cx).value().to_string();
        let ua_val = user_agent_input.read(cx).value().to_string();
        set_or_remove_header(&mut headers, "Cookie", &cookie_val);
        set_or_remove_header(&mut headers, "Referer", &referer_val);
        set_or_remove_header(&mut headers, "User-Agent", &ua_val);

        let out_dir = if dir_str.is_empty() {
            self.queue.settings.default_output_dir.clone()
        } else {
            PathBuf::from(dir_str)
        };

        let mut req = DownloadRequest::new(info.original_url, &out_dir);
        let mut ask_overwrite_name: Option<String> = None;
        if !filename.is_empty() {
            let resolution =
                resolve_target_path(&out_dir, &filename, self.queue.settings.collision_policy);
            if resolution.needs_overwrite_prompt {
                ask_overwrite_name = Some(filename.clone());
            }
            req.output_path = Some(resolution.path);
        }
        req.max_workers = if info.supports_ranges { workers } else { 1 };
        req.sync_on_complete = self.queue.settings.sync_on_complete;
        req.headers = headers;

        self.url_input.update(cx, |state, cx| {
            state.set_value(String::new(), window, cx);
        });

        if let Some(collision_name) = ask_overwrite_name {
            self.confirm_modal = Some(ConfirmAction::OverwriteCollision {
                request: Box::new(req),
                start_immediately,
                filename: collision_name,
            });
            cx.notify();
            return;
        }

        if self.queue.settings.notify_on_added {
            let allow_notify = !self.queue.settings.notify_only_inactive || !window.is_window_active();
            if allow_notify {
                spawn_windows_toast(
                    "Posthaze — Download Added",
                    &format!("Queued {}", if filename.is_empty() { "download" } else { &filename }),
                );
            }
        }

        self.dispatch_download_request(req, start_immediately, cx);
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
        self.settings_modal = Some(SettingsModalState::from_settings(&self.queue.settings, window, cx));
        cx.notify();
    }

    fn save_settings_modal(&mut self, cx: &mut Context<Self>) {
        let Some(modal) = &mut self.settings_modal else {
            return;
        };
        match modal.apply_to_settings(cx) {
            Ok(new_settings) => {
                self.settings_modal = None;
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
        );
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

        let preflight_overlay = self.preflight_modal.as_ref().map(|modal| {
            let e_close = cx.entity().clone();
            let e_retry = cx.entity().clone();
            let e_delta = cx.entity().clone();
            let e_confirm = cx.entity().clone();
            let e_edit = cx.entity().clone();
            let e_toggle_headers = cx.entity().clone();
            render_preflight_modal(
                modal,
                self.queue.settings.ui_show_add_paused,
                move |_, cx| {
                    e_close.update(cx, |app, cx| {
                        app.preflight_modal = None;
                        cx.notify();
                    });
                },
                move |url, headers, window, cx| {
                    e_retry.update(cx, |app, cx| app.begin_probe(url, headers, window, cx));
                },
                move |delta, _, cx| {
                    e_delta.update(cx, |app, cx| {
                        if let Some(PreflightModalState::Resolved { workers, .. }) =
                            &mut app.preflight_modal
                        {
                            *workers = (*workers as isize + delta).clamp(1, 32) as usize;
                            cx.notify();
                        }
                    });
                },
                move |focus_new, window, cx| {
                    e_confirm.update(cx, |app, cx| app.confirm_preflight(focus_new, window, cx));
                },
                move |url, window, cx| {
                    e_edit.update(cx, |app, cx| {
                        app.preflight_modal = None;
                        app.url_input.update(cx, |st, cx| {
                            st.set_value(url, window, cx);
                            st.focus(window, cx);
                        });
                        cx.notify();
                    });
                },
                move |_, cx| {
                    e_toggle_headers.update(cx, |app, cx| {
                        if let Some(PreflightModalState::Resolved {
                            editing_headers, ..
                        }) = &mut app.preflight_modal
                        {
                            *editing_headers = !*editing_headers;
                            cx.notify();
                        }
                    });
                },
            )
        });

        let settings_overlay = self.settings_modal.as_ref().map(|modal| {
            let e_action = cx.entity().clone();
            let e_cancel = cx.entity().clone();
            let e_save = cx.entity().clone();
            render_settings_modal(
                modal,
                move |action, window, cx| {
                    e_action.update(cx, |app, cx| app.handle_settings_action(action, window, cx));
                },
                move |window, cx| {
                    e_cancel.update(cx, |app, cx| {
                        app.settings_modal = None;
                        theme::apply_theme(&app.queue.settings.ui_theme, Some(window), cx);
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

        let visible_ids: Vec<DownloadId> = visible_rows.iter().map(|r| r.id.clone()).collect();

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
                    has_preflight_resolved: matches!(
                        app.preflight_modal,
                        Some(PreflightModalState::Resolved { .. })
                    ),
                    has_any_preflight: app.preflight_modal.is_some(),
                    has_settings_modal: app.settings_modal.is_some(),
                    has_file_op_modal: app.file_op_modal.is_some(),
                    url_input_focused: app.url_input.read(cx).focus_handle(cx).is_focused(window)
                        || app.search_input.read(cx).focus_handle(cx).is_focused(window),
                };

                let ctrl_or_cmd =
                    event.keystroke.modifiers.control || event.keystroke.modifiers.platform;
                let shift = event.keystroke.modifiers.shift;
                if ctrl_or_cmd
                    && key == "a"
                    && !kb_ctx.has_confirm_modal
                    && !kb_ctx.has_any_preflight
                    && !kb_ctx.has_settings_modal
                    && !kb_ctx.has_file_op_modal
                    && !kb_ctx.url_input_focused
                {
                    app.queue.select_all(&visible_ids);
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
                        if app.preflight_modal.take().is_some() {
                            changed = true;
                        }
                        if app.settings_modal.take().is_some() {
                            theme::apply_theme(&app.queue.settings.ui_theme, Some(window), cx);
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
                        } else if matches!(
                            app.preflight_modal,
                            Some(PreflightModalState::Resolved { .. })
                        ) {
                            app.confirm_preflight(true, window, cx);
                        } else if app.settings_modal.is_some() {
                            app.save_settings_modal(cx);
                        } else if app.file_op_modal.is_some() {
                            app.execute_file_op_confirm(cx);
                        }
                    }
                    KeyboardAction::StepSelection(delta) => {
                        if let Some(next_id) =
                            app.queue
                                .step_selection_extending(&visible_ids, delta, shift)
                        {
                            if let Some(idx) = visible_ids.iter().position(|id| id == &next_id) {
                                const ROW_HEIGHT: f32 = 38.0;
                                let (scroll_top, viewport_h, max_scroll) = {
                                    let inner = app.table_scroll_handle.0.borrow();
                                    let top = -f32::from(inner.base_handle.offset().y);
                                    let vh = f32::from(inner.base_handle.bounds().size.height);
                                    let max_s = f32::from(inner.base_handle.max_offset().y);
                                    (top, vh, max_s)
                                };
                                if viewport_h > 1.0 && max_scroll > 0.5 {
                                    let row_top = idx as f32 * ROW_HEIGHT;
                                    let row_bottom = row_top + ROW_HEIGHT;
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
            .child(self.render_titlebar())
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
                                        .bg(rgba(0xEF444424))
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
                                            Button::new("dismiss-status-banner")
                                                .xsmall()
                                                .ghost()
                                                .icon(IconName::X)
                                                .tooltip("Dismiss")
                                                .on_click(cx.listener(|app, _, _, cx| {
                                                    app.status_banner = None;
                                                    cx.notify();
                                                })),
                                        ),
                                )
                            })
                            .child(self.render_table(visible_rows, is_empty_queue, profile, cx))
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
            .when_some(preflight_overlay, |el, overlay| el.child(overlay))
            .when_some(settings_overlay, |el, overlay| el.child(overlay))
            .when_some(file_op_overlay, |el, overlay| el.child(overlay))
            .when_some(confirm_overlay, |el, overlay| el.child(overlay))
    }
}

impl PosthazeApp {
    fn render_titlebar(&self) -> impl IntoElement {
        TitleBar::new()
            .bg(graphite_bg())
            .border_color(graphite_border())
            .child(
                div()
                    .flex()
                    .items_center()
                    .gap_2()
                    .child(
                        svg()
                            .path(BRAND_LOGO_SVG)
                            .w(px(16.0))
                            .h(px(14.0))
                            .text_color(selenium_amber()),
                    )
                    .child(
                        div()
                            .font_family(MONO_FONT)
                            .text_size(TYPE_MICRO)
                            .font_weight(FontWeight::SEMIBOLD)
                            .text_color(silver_text())
                            .child("POSTHAZE"),
                    )
                    .child(
                        div()
                            .font_family(MONO_FONT)
                            .text_size(TYPE_HEADER)
                            .text_color(silver_muted())
                            .child("v0.3.0"),
                    ),
            )
    }
}
