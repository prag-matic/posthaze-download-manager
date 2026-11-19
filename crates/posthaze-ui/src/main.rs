//! Posthaze Download Manager desktop UI powered by GPUI and GPUI Kit.

mod bridge;
mod inspector;
mod modals;
mod theme;
mod view_model;

use std::collections::HashMap;
use std::path::PathBuf;

use gpui_kit::assets::{AllAssets, IconName};
use gpui_kit::component::button::{Button, ButtonVariants};
use gpui_kit::component::input::{Input, InputEvent, InputState};
use gpui_kit::component::{
    Icon, InteractiveElementExt as _, Sizable, Theme, ThemeMode, TitleBar,
};
use gpui_kit::prelude::FluentBuilder;
use gpui_kit::*;
use posthaze_engine::{
    DownloadId, DownloadRecord, DownloadRequest, DownloadState, EngineEvent, ProgressSnapshot,
    SegmentProgress, Settings,
};

use crate::bridge::{EngineBridge, default_db_path};
use crate::inspector::{InspectorCallbacks, render_inspector};
use crate::modals::{
    PreflightModalState, SettingsModalState, render_preflight_modal, render_settings_modal,
};
use crate::theme::*;
use crate::view_model::{
    DEFAULT_INSPECTOR_HEIGHT, DEFAULT_SIDEBAR_WIDTH, DownloadRowView, FilterTab,
    RAIL_SIDEBAR_WIDTH, SortColumn, SortDirection, clamp_inspector_height, compute_fifo_ranks,
    filter_and_sort_rows, format_bytes, format_eta, format_speed, parse_settings_inputs,
    resolve_sidebar_drag, state_icon,
};

const COL_STATE: f32 = 104.0;
const COL_PROGRESS: f32 = 176.0;
const COL_SPEED: f32 = 90.0;
const COL_ETA: f32 = 64.0;
const COL_SEGS: f32 = 48.0;
const COL_ACTIONS: f32 = 148.0;

fn main() {
    let bridge = EngineBridge::open(default_db_path()).expect("failed to initialize engine");

    gpui_kit::application()
        .with_assets(AllAssets)
        .run(move |cx| {
            gpui_kit::init(cx);
            Theme::change(ThemeMode::Dark, None, cx);
            {
                let theme = Theme::global_mut(cx);
                theme.radius = px(4.0);
                theme.radius_lg = px(6.0);
                theme.background = graphite_bg();
                theme.foreground = silver_text();
                theme.border = paper_border();
                theme.input = graphite_bg();
                theme.muted = graphite_elevated();
                theme.muted_foreground = silver_muted();
                theme.secondary = graphite_elevated();
                theme.secondary_hover = paper_hover();
                theme.secondary_active = paper_selected();
                theme.secondary_foreground = silver_text();
                theme.primary = selenium_amber();
                theme.primary_hover = rgb(0xD97724).into();
                theme.primary_active = rgb(0xC2661B).into();
                theme.primary_foreground = graphite_bg();
                theme.button_primary = selenium_amber();
                theme.button_primary_hover = rgb(0xD97724).into();
                theme.button_primary_active = rgb(0xC2661B).into();
                theme.button_primary_foreground = graphite_bg();
                theme.button_secondary = graphite_elevated();
                theme.button_secondary_hover = paper_hover();
                theme.button_secondary_active = paper_selected();
                theme.button_secondary_foreground = silver_text();
                theme.button_hover = paper_hover();
                theme.button_active = paper_selected();
                theme.button_foreground = silver_text();
                theme.ring = rgb(0x524E48).into();
                theme.title_bar = graphite_bg();
                theme.title_bar_border = graphite_border();
                theme.tokens.primary.background = selenium_amber().into();
                theme.tokens.primary_hover.background = rgb(0xD97724).into();
                theme.tokens.primary_active.background = rgb(0xC2661B).into();
                theme.tokens.button_primary.background = selenium_amber().into();
                theme.tokens.button_primary_hover.background = rgb(0xD97724).into();
                theme.tokens.button_primary_active.background = rgb(0xC2661B).into();
                theme.tokens.button_secondary.background = graphite_elevated().into();
                theme.tokens.button_secondary_hover.background = paper_hover().into();
                theme.tokens.button_secondary_active.background = paper_selected().into();
                theme.tokens.button.background = graphite_elevated().into();
                theme.tokens.button_hover.background = paper_hover().into();
                theme.tokens.button_active.background = paper_selected().into();
            }

            let window_options = WindowOptions {
                window_bounds: Some(WindowBounds::Windowed(Bounds::centered(
                    None,
                    size(px(1440.0), px(900.0)),
                    cx,
                ))),
                window_min_size: Some(size(px(960.0), px(640.0))),
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

struct PosthazeApp {
    bridge: EngineBridge,
    url_input: Entity<InputState>,
    records: Vec<DownloadRecord>,
    live_map: HashMap<DownloadId, ProgressSnapshot>,
    persisted_segments: HashMap<DownloadId, Vec<SegmentProgress>>,
    settings: Settings,
    filter: FilterTab,
    sort_col: SortColumn,
    sort_dir: SortDirection,
    selected_id: Option<DownloadId>,
    status_banner: Option<String>,
    preflight_modal: Option<PreflightModalState>,
    settings_modal: Option<SettingsModalState>,
    sidebar_width: f32,
    sidebar_collapsed: bool,
    inspector_height: f32,
    inspector_collapsed: bool,
    active_resize: Option<ActiveResize>,
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

        let app = Self {
            bridge,
            url_input,
            records: Vec::new(),
            live_map: HashMap::new(),
            persisted_segments: HashMap::new(),
            settings: Settings::default(),
            filter: FilterTab::All,
            sort_col: SortColumn::CreatedAt,
            sort_dir: SortDirection::Descending,
            selected_id: None,
            status_banner: None,
            preflight_modal: None,
            settings_modal: None,
            sidebar_width: DEFAULT_SIDEBAR_WIDTH,
            sidebar_collapsed: false,
            inspector_height: DEFAULT_INSPECTOR_HEIGHT,
            inspector_collapsed: false,
            active_resize: None,
        };

        app.refresh_from_engine(cx);
        app.start_event_listener(cx);
        app
    }

    fn refresh_from_engine(&self, cx: &mut Context<Self>) {
        let selected = self.selected_id.clone();
        let rx = self.bridge.dispatch(move |engine| async move {
            let records = engine.list_downloads().await;
            let settings = engine.get_settings().await;
            let segments = if let Some(id) = selected.as_ref() {
                engine.list_segments(id).await.ok().map(|s| (id.clone(), s))
            } else {
                None
            };
            (records, settings, segments)
        });
        cx.spawn(async move |this, cx| {
            if let Ok((Ok(records), Ok(settings), segments)) = rx.await {
                let _ = this.update(cx, |app, cx| {
                    app.records = records;
                    app.settings = settings;
                    if let Some((id, segs)) = segments {
                        app.persisted_segments.insert(id, segs);
                    }
                    if app.selected_id.is_none() {
                        app.selected_id = app.records.first().map(|r| r.id.clone());
                        if let Some(id) = app.selected_id.clone() {
                            app.load_segments_for(id, cx);
                        }
                    }
                    cx.notify();
                });
            }
        })
        .detach();
    }

    fn load_segments_for(&self, id: DownloadId, cx: &mut Context<Self>) {
        let rx = self
            .bridge
            .dispatch(move |engine| async move { engine.list_segments(&id).await.map(|s| (id, s)) });
        cx.spawn(async move |this, cx| {
            if let Ok(Ok((id, segs))) = rx.await {
                let _ = this.update(cx, |app, cx| {
                    app.persisted_segments.insert(id, segs);
                    cx.notify();
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
                            app.live_map.insert(id, snapshot);
                            cx.notify();
                        });
                    }
                    Ok(EngineEvent::StateChanged { id, state, .. }) => {
                        let _ = this.update(cx, |app, cx| {
                            if state != DownloadState::Active {
                                app.live_map.remove(&id);
                            }
                            app.refresh_from_engine(cx);
                        });
                    }
                    Ok(EngineEvent::QueueUpdated) => {
                        let _ = this.update(cx, |app, cx| {
                            app.refresh_from_engine(cx);
                        });
                    }
                    Ok(EngineEvent::SettingsUpdated(settings)) => {
                        let _ = this.update(cx, |app, cx| {
                            app.settings = settings;
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

    fn submit_url_from_input(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        let raw = self.url_input.read(cx).value().to_string();
        let trimmed = raw.trim().to_string();
        if trimmed.is_empty() {
            return;
        }
        self.begin_probe(trimmed, window, cx);
    }

    fn paste_and_probe_from_clipboard(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        if let Some(item) = cx.read_from_clipboard()
            && let Some(text) = item.text()
        {
            let trimmed = text.trim().to_string();
            if !trimmed.is_empty() {
                self.url_input.update(cx, |state, cx| {
                    state.set_value(trimmed.clone(), window, cx);
                });
                self.begin_probe(trimmed, window, cx);
                return;
            }
        }
        self.status_banner = Some("Clipboard does not contain a URL.".to_string());
        cx.notify();
    }

    fn begin_probe(&mut self, url: String, window: &mut Window, cx: &mut Context<Self>) {
        self.status_banner = None;
        self.preflight_modal = Some(PreflightModalState::Probing { url: url.clone() });
        cx.notify();

        let out_dir = self.settings.default_output_dir.clone();
        let default_workers = self.settings.max_workers.clamp(1, 32);
        let window_handle = window.window_handle();
        let probe_url = url.clone();
        let rx = self
            .bridge
            .dispatch(move |engine| async move { engine.probe(probe_url, out_dir).await });

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
                            let initial_name = info
                                .disambiguated_output_path
                                .file_name()
                                .and_then(|n| n.to_str())
                                .unwrap_or(&info.suggested_filename)
                                .to_string();
                            let initial_dir = info
                                .disambiguated_output_path
                                .parent()
                                .map(|p| p.to_string_lossy().to_string())
                                .unwrap_or_else(|| {
                                    app.settings.default_output_dir.to_string_lossy().to_string()
                                });
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
                            let workers = if info.supports_ranges {
                                default_workers
                            } else {
                                1
                            };
                            app.preflight_modal = Some(PreflightModalState::Resolved {
                                info,
                                filename_input,
                                dir_input,
                                workers,
                            });
                        }
                        Ok(Err(err)) => {
                            app.preflight_modal = Some(PreflightModalState::Error {
                                url,
                                message: err.to_string(),
                            });
                        }
                        Err(_) => {
                            app.preflight_modal = Some(PreflightModalState::Error {
                                url,
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

    fn confirm_preflight(&mut self, focus_new: bool, window: &mut Window, cx: &mut Context<Self>) {
        let Some(PreflightModalState::Resolved {
            info,
            filename_input,
            dir_input,
            workers,
        }) = self.preflight_modal.take()
        else {
            return;
        };

        let filename = filename_input.read(cx).value().trim().to_string();
        let dir_str = dir_input.read(cx).value().trim().to_string();
        let out_dir = if dir_str.is_empty() {
            self.settings.default_output_dir.clone()
        } else {
            PathBuf::from(dir_str)
        };

        let mut req = DownloadRequest::new(info.original_url, &out_dir);
        if !filename.is_empty() {
            req.output_path = Some(out_dir.join(filename));
        }
        req.max_workers = if info.supports_ranges { workers } else { 1 };
        req.sync_on_complete = self.settings.sync_on_complete;

        self.url_input.update(cx, |state, cx| {
            state.set_value(String::new(), window, cx);
        });

        let rx = self
            .bridge
            .dispatch(move |engine| async move { engine.start(req).await.map(|h| h.id().clone()) });
        cx.spawn(async move |this, cx| {
            let res = rx.await;
            let _ = this.update(cx, |app, cx| {
                match res {
                    Ok(Ok(id)) => {
                        if focus_new || app.selected_id.is_none() {
                            app.selected_id = Some(id);
                        }
                        app.status_banner = None;
                        app.refresh_from_engine(cx);
                    }
                    Ok(Err(err)) => {
                        app.status_banner = Some(format!("Failed to start download: {err}"));
                    }
                    Err(_) => {
                        app.status_banner = Some("Engine bridge channel closed".to_string());
                    }
                }
                cx.notify();
            });
        })
        .detach();
    }

    fn restart_fresh_download(&mut self, id: DownloadId, cx: &mut Context<Self>) {
        let rx = self
            .bridge
            .dispatch(move |engine| async move { engine.restart_fresh(&id).await.map(|_| ()) });
        self.run_unit_action(rx, cx);
    }

    fn open_settings_modal(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        self.settings_modal = Some(SettingsModalState::from_settings(&self.settings, window, cx));
        cx.notify();
    }

    fn save_settings_modal(&mut self, cx: &mut Context<Self>) {
        let Some(modal) = &mut self.settings_modal else {
            return;
        };
        let dir = modal.dir_input.read(cx).value().to_string();
        let workers = modal.workers_input.read(cx).value().to_string();
        let concurrent = modal.concurrent_input.read(cx).value().to_string();
        let sync = modal.sync_on_complete;

        match parse_settings_inputs(&dir, &workers, &concurrent, sync) {
            Ok(new_settings) => {
                self.settings_modal = None;
                let rx = self
                    .bridge
                    .dispatch(move |engine| async move { engine.update_settings(new_settings).await });
                self.run_unit_action(rx, cx);
            }
            Err(err) => {
                modal.error = Some(err);
                cx.notify();
            }
        }
    }

    fn pause_download(&mut self, id: DownloadId, cx: &mut Context<Self>) {
        let rx = self
            .bridge
            .dispatch(move |engine| async move { engine.pause(&id).await });
        self.run_unit_action(rx, cx);
    }

    fn resume_download(&mut self, id: DownloadId, cx: &mut Context<Self>) {
        let rx = self
            .bridge
            .dispatch(move |engine| async move { engine.resume(&id).await.map(|_| ()) });
        self.run_unit_action(rx, cx);
    }

    fn cancel_download(&mut self, id: DownloadId, cx: &mut Context<Self>) {
        let rx = self
            .bridge
            .dispatch(move |engine| async move { engine.cancel(&id).await });
        self.run_unit_action(rx, cx);
    }

    fn delete_download(&mut self, id: DownloadId, cx: &mut Context<Self>) {
        let rx = self
            .bridge
            .dispatch(move |engine| async move { engine.delete(&id, false).await });
        self.run_unit_action(rx, cx);
    }

    fn resume_all(&mut self, cx: &mut Context<Self>) {
        let ids: Vec<DownloadId> = self
            .records
            .iter()
            .filter(|r| matches!(r.state, DownloadState::Paused | DownloadState::Failed))
            .map(|r| r.id.clone())
            .collect();
        let rx = self.bridge.dispatch(move |engine| async move {
            for id in ids {
                let _ = engine.resume(&id).await;
            }
            Ok(())
        });
        self.run_unit_action(rx, cx);
    }

    fn pause_all(&mut self, cx: &mut Context<Self>) {
        let ids: Vec<DownloadId> = self
            .records
            .iter()
            .filter(|r| {
                matches!(
                    self.live_map.get(&r.id).map_or(r.state, |s| s.state),
                    DownloadState::Active | DownloadState::Queued | DownloadState::Probing
                )
            })
            .map(|r| r.id.clone())
            .collect();
        let rx = self.bridge.dispatch(move |engine| async move {
            for id in ids {
                let _ = engine.pause(&id).await;
            }
            Ok(())
        });
        self.run_unit_action(rx, cx);
    }

    fn clear_completed(&mut self, cx: &mut Context<Self>) {
        let ids: Vec<DownloadId> = self
            .records
            .iter()
            .filter(|r| matches!(r.state, DownloadState::Completed | DownloadState::Cancelled))
            .map(|r| r.id.clone())
            .collect();
        let rx = self.bridge.dispatch(move |engine| async move {
            for id in ids {
                let _ = engine.delete(&id, false).await;
            }
            Ok(())
        });
        self.run_unit_action(rx, cx);
    }

    fn run_unit_action(
        &self,
        rx: tokio::sync::oneshot::Receiver<Result<(), posthaze_engine::EngineError>>,
        cx: &mut Context<Self>,
    ) {
        cx.spawn(async move |this, cx| {
            let result = rx.await;
            let _ = this.update(cx, |app, cx| {
                match result {
                    Ok(Ok(())) => app.refresh_from_engine(cx),
                    Ok(Err(err)) => {
                        app.status_banner = Some(err.to_string());
                        cx.notify();
                    }
                    Err(_) => {
                        app.status_banner = Some("Engine bridge channel closed".to_string());
                        cx.notify();
                    }
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
    fn render(&mut self, _: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let ranks = compute_fifo_ranks(&self.records, &self.live_map);
        let all_rows: Vec<DownloadRowView> = self
            .records
            .iter()
            .map(|rec| {
                DownloadRowView::from_record(
                    rec,
                    self.live_map.get(&rec.id),
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

        let visible_rows = filter_and_sort_rows(
            all_rows.clone(),
            self.filter,
            self.sort_col,
            self.sort_dir,
        );
        let is_empty_queue = all_rows.is_empty();

        let selected_row = self
            .selected_id
            .as_ref()
            .and_then(|id| all_rows.iter().find(|r| &r.id == id));
        let selected_segments: Vec<SegmentProgress> = self
            .selected_id
            .as_ref()
            .and_then(|id| {
                self.live_map
                    .get(id)
                    .filter(|s| !s.segments.is_empty())
                    .map(|s| s.segments.clone())
                    .or_else(|| self.persisted_segments.get(id).cloned())
            })
            .unwrap_or_default();

        let entity_restart = cx.entity().clone();
        let entity_delete = cx.entity().clone();
        let entity_inspector_toggle = cx.entity().clone();
        let inspector = render_inspector(
            selected_row,
            &selected_segments,
            px(self.inspector_height),
            self.inspector_collapsed,
            InspectorCallbacks {
                on_restart_fresh: move |id: DownloadId, _: &mut Window, cx: &mut App| {
                    entity_restart.update(cx, |app, cx| app.restart_fresh_download(id, cx));
                },
                on_delete: move |id: DownloadId, _: &mut Window, cx: &mut App| {
                    entity_delete.update(cx, |app, cx| app.delete_download(id, cx));
                },
                on_toggle_collapse: move |_: &mut Window, cx: &mut App| {
                    entity_inspector_toggle.update(cx, |app, cx| {
                        app.inspector_collapsed = !app.inspector_collapsed;
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
            render_preflight_modal(
                modal,
                move |_, cx| {
                    e_close.update(cx, |app, cx| {
                        app.preflight_modal = None;
                        cx.notify();
                    });
                },
                move |url, window, cx| {
                    e_retry.update(cx, |app, cx| app.begin_probe(url, window, cx));
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
            )
        });

        let settings_overlay = self.settings_modal.as_ref().map(|modal| {
            let e_sync = cx.entity().clone();
            let e_cancel = cx.entity().clone();
            let e_save = cx.entity().clone();
            render_settings_modal(
                modal,
                move |_, cx| {
                    e_sync.update(cx, |app, cx| {
                        if let Some(m) = &mut app.settings_modal {
                            m.sync_on_complete = !m.sync_on_complete;
                            cx.notify();
                        }
                    });
                },
                move |_, cx| {
                    e_cancel.update(cx, |app, cx| {
                        app.settings_modal = None;
                        cx.notify();
                    });
                },
                move |_, cx| {
                    e_save.update(cx, |app, cx| app.save_settings_modal(cx));
                },
            )
        });

        let active_resize = self.active_resize;
        let resizing_sidebar = matches!(active_resize, Some(ActiveResize::Sidebar { .. }));
        let resizing_inspector = matches!(active_resize, Some(ActiveResize::Inspector { .. }));

        div()
            .relative()
            .flex()
            .flex_col()
            .size_full()
            .bg(paper_bg())
            .text_color(carbon_text())
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
                        let (collapsed, expanded_width) =
                            resolve_sidebar_drag(start_width + delta, app.sidebar_width);
                        app.sidebar_collapsed = collapsed;
                        app.sidebar_width = expanded_width;
                    }
                    ActiveResize::Inspector {
                        start_y,
                        start_height,
                    } => {
                        let delta = f32::from(start_y - event.position.y);
                        app.inspector_height = clamp_inspector_height(start_height + delta);
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
                                app.sidebar_collapsed = false;
                                app.sidebar_width = DEFAULT_SIDEBAR_WIDTH;
                                cx.notify();
                            }))
                            .child(
                                div()
                                    .w(px(if resizing_sidebar { 2.0 } else { 1.0 }))
                                    .h_full()
                                    .bg(if resizing_sidebar {
                                        selenium_amber()
                                    } else {
                                        graphite_border()
                                    }),
                            ),
                    )
                    // Main Right Column: Top Toolbar + Center Table + Collapsible & Resizable Bottom Inspector
                    .child(
                        div()
                            .flex()
                            .flex_col()
                            .flex_1()
                            .h_full()
                            .overflow_hidden()
                            .child(self.render_toolbar(cx))
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
                            .child(self.render_table(&visible_rows, is_empty_queue, cx))
                            .when(!self.inspector_collapsed, |el| {
                                el.child(
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
                                                app.active_resize = Some(ActiveResize::Inspector {
                                                    start_y: ev.position.y,
                                                    start_height: app.inspector_height,
                                                });
                                                cx.notify();
                                            }),
                                        )
                                        .on_double_click(cx.listener(|app, _, _, cx| {
                                            app.inspector_height = DEFAULT_INSPECTOR_HEIGHT;
                                            cx.notify();
                                        }))
                                        .child(
                                            div()
                                                .h(px(if resizing_inspector { 2.0 } else { 1.0 }))
                                                .w_full()
                                                .bg(if resizing_inspector {
                                                    selenium_amber()
                                                } else {
                                                    graphite_border()
                                                }),
                                        ),
                                )
                            })
                            .child(inspector),
                    ),
            )
            .when_some(preflight_overlay, |el, overlay| el.child(overlay))
            .when_some(settings_overlay, |el, overlay| el.child(overlay))
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
                        div()
                            .w(px(14.0))
                            .h(px(14.0))
                            .border_1()
                            .border_color(silver_muted())
                            .flex()
                            .items_center()
                            .justify_center()
                            .child(div().w(px(6.0)).h(px(6.0)).bg(selenium_amber())),
                    )
                    .child(
                        div()
                            .font_family(MONO_FONT)
                            .text_size(TYPE_MICRO)
                            .font_weight(FontWeight::SEMIBOLD)
                            .text_color(silver_text())
                            .child("POSTHAZE // v0.3.0-m3"),
                    )
                    .child(div().text_color(graphite_border()).child("|"))
                    .child(
                        div()
                            .font_family(MONO_FONT)
                            .text_size(TYPE_HEADER)
                            .text_color(silver_muted())
                            .child("GPUI-KIT 0.10.0 · TOKIO-BRIDGE"),
                    ),
            )
    }
    fn render_sidebar(
        &self,
        all_rows: &[DownloadRowView],
        total_speed: u64,
        active_slots: usize,
        cx: &mut Context<Self>,
    ) -> impl IntoElement {
        let tabs = [
            FilterTab::All,
            FilterTab::Active,
            FilterTab::Queued,
            FilterTab::Paused,
            FilterTab::Completed,
            FilterTab::Failed,
        ];

        let sidebar_width = self.sidebar_width;

        if self.sidebar_collapsed {
            return div()
                .relative()
                .w(px(RAIL_SIDEBAR_WIDTH))
                .h_full()
                .flex()
                .flex_col()
                .justify_between()
                .items_center()
                .bg(graphite_bg())
                .py_2p5()
                .child(
                    div()
                        .w_full()
                        .flex()
                        .flex_col()
                        .items_center()
                        .gap_3()
                        .child(
                            div()
                                .w_full()
                                .pb_2()
                                .border_b_1()
                                .border_color(graphite_border())
                                .flex()
                                .justify_center()
                                .child(
                                    Button::new("sidebar-expand")
                                        .xsmall()
                                        .ghost()
                                        .icon(IconName::PanelLeftOpen)
                                        .tooltip("Expand Sidebar")
                                        .on_click(cx.listener(|app, _, _, cx| {
                                            app.sidebar_collapsed = false;
                                            cx.notify();
                                        })),
                                ),
                        )
                        .child(
                            div().w_full().flex().flex_col().gap_1().children(
                                tabs.into_iter().map(|tab| {
                                    let count =
                                        all_rows.iter().filter(|r| tab.matches(r.state)).count();
                                    let selected = self.filter == tab;
                                    let (tab_icon, tab_color) = match tab {
                                        FilterTab::All => (IconName::Folder, silver_text()),
                                        FilterTab::Active => (IconName::ArrowDown, selenium_amber()),
                                        FilterTab::Queued => (IconName::Clock, silver_muted()),
                                        FilterTab::Paused => (IconName::Pause, silver_muted()),
                                        FilterTab::Completed => {
                                            (IconName::CircleCheck, halide_emerald())
                                        }
                                        FilterTab::Failed => {
                                            (IconName::TriangleAlert, errata_vermilion())
                                        }
                                    };
                                    div()
                                        .id(SharedString::from(format!("rail-{}", tab.label())))
                                        .w_full()
                                        .py_2()
                                        .flex()
                                        .items_center()
                                        .justify_center()
                                        .cursor_pointer()
                                        .bg(if selected {
                                            paper_hover()
                                        } else {
                                            graphite_bg()
                                        })
                                        .border_l_2()
                                        .border_color(if selected {
                                            selenium_amber()
                                        } else {
                                            graphite_bg()
                                        })
                                        .hover(|s| s.bg(graphite_elevated()))
                                        .on_click(cx.listener(move |app, _, _, cx| {
                                            app.filter = tab;
                                            cx.notify();
                                        }))
                                        .child(
                                            Button::new(SharedString::from(format!(
                                                "rail-btn-{}",
                                                tab.label()
                                            )))
                                            .xsmall()
                                            .ghost()
                                            .icon(tab_icon)
                                            .text_color(tab_color)
                                            .tooltip(format!("{} ({count})", tab.label()))
                                            .on_click(cx.listener(move |app, _, _, cx| {
                                                app.filter = tab;
                                                cx.notify();
                                            })),
                                        )
                                }),
                            ),
                        ),
                )
                .child(
                    div()
                        .w_full()
                        .pt_2p5()
                        .border_t_1()
                        .border_color(graphite_border())
                        .flex()
                        .flex_col()
                        .items_center()
                        .gap_2()
                        .child(div().w(px(6.0)).h(px(6.0)).bg(if active_slots > 0 {
                            selenium_amber()
                        } else {
                            graphite_border()
                        }))
                        .child(
                            Button::new("open-settings")
                                .small()
                                .ghost()
                                .icon(IconName::Settings)
                                .tooltip("Engine Settings")
                                .on_click(cx.listener(|app, _, window, cx| {
                                    app.open_settings_modal(window, cx);
                                })),
                        ),
                )
                .into_any_element();
        }

        div()
            .relative()
            .w(px(sidebar_width))
            .h_full()
            .flex()
            .flex_col()
            .justify_between()
            .bg(graphite_bg())
            .p_3()
            .child(
                div()
                    .flex()
                    .flex_col()
                    .gap_3()
                    // Compact Queue Header (branding lives in TitleBar)
                    .child(
                        div()
                            .pb_2()
                            .border_b_1()
                            .border_color(graphite_border())
                            .flex()
                            .items_center()
                            .justify_between()
                            .child(
                                div()
                                    .font_family(MONO_FONT)
                                    .text_size(TYPE_HEADER)
                                    .font_weight(FontWeight::SEMIBOLD)
                                    .text_color(silver_muted())
                                    .child("QUEUE FILTERS"),
                            )
                            .child(
                                Button::new("sidebar-collapse")
                                    .xsmall()
                                    .ghost()
                                    .icon(IconName::PanelLeftClose)
                                    .tooltip("Collapse to Rail")
                                    .on_click(cx.listener(|app, _, _, cx| {
                                        app.sidebar_collapsed = true;
                                        cx.notify();
                                    })),
                            ),
                    )
                    // Filter Rail
                    .child(
                        div().flex().flex_col().gap_1().children(tabs.into_iter().map(
                            |tab| {
                                let count =
                                    all_rows.iter().filter(|r| tab.matches(r.state)).count();
                                let selected = self.filter == tab;
                                let (tab_icon, tab_color) = match tab {
                                    FilterTab::All => (IconName::Folder, silver_text()),
                                    FilterTab::Active => (IconName::ArrowDown, selenium_amber()),
                                    FilterTab::Queued => (IconName::Clock, silver_muted()),
                                    FilterTab::Paused => (IconName::Pause, silver_muted()),
                                    FilterTab::Completed => (IconName::CircleCheck, halide_emerald()),
                                    FilterTab::Failed => (IconName::TriangleAlert, errata_vermilion()),
                                };
                                div()
                                    .id(SharedString::from(format!("filter-{}", tab.label())))
                                    .flex()
                                    .items_center()
                                    .justify_between()
                                    .px_2p5()
                                    .py_1p5()
                                    .cursor_pointer()
                                    .bg(if selected {
                                        paper_hover()
                                    } else {
                                        graphite_bg()
                                    })
                                    .border_l_2()
                                    .border_color(if selected {
                                        selenium_amber()
                                    } else {
                                        graphite_bg()
                                    })
                                    .hover(|s| s.bg(graphite_elevated()))
                                    .text_size(px(12.5))
                                    .line_height(px(18.0))
                                    .font_weight(if selected {
                                        FontWeight::SEMIBOLD
                                    } else {
                                        FontWeight::NORMAL
                                    })
                                    .text_color(if selected {
                                        silver_text()
                                    } else {
                                        silver_muted()
                                    })
                                    .on_click(cx.listener(move |app, _, _, cx| {
                                        app.filter = tab;
                                        cx.notify();
                                    }))
                                    .child(
                                        div()
                                            .flex()
                                            .items_center()
                                            .gap_2()
                                            .child(
                                                Icon::new(tab_icon)
                                                    .size(px(13.0))
                                                    .text_color(tab_color),
                                            )
                                            .child(tab.label()),
                                    )
                                    .child(
                                        div()
                                            .px_1p5()
                                            .bg(graphite_bg())
                                            .border_1()
                                            .border_color(graphite_border())
                                            .font_family(MONO_FONT)
                                            .text_size(TYPE_MICRO)
                                            .text_color(silver_text())
                                            .child(count.to_string()),
                                    )
                            },
                        )),
                    ),
            )
            // Global Telemetry Readout
            .child(
                div()
                    .flex()
                    .flex_col()
                    .gap_2()
                    .pt_3()
                    .border_t_1()
                    .border_color(graphite_border())
                    .child(
                        div()
                            .p_2p5()
                            .bg(graphite_elevated())
                            .border_1()
                            .border_color(graphite_border())
                            .flex()
                            .flex_col()
                            .gap_1()
                            .child(
                                div()
                                    .flex()
                                    .items_center()
                                    .justify_between()
                                    .child(
                                        div()
                                            .font_family(MONO_FONT)
                                            .text_size(TYPE_HEADER)
                                            .line_height(px(14.0))
                                            .font_weight(FontWeight::SEMIBOLD)
                                            .text_color(silver_muted())
                                            .child("ACTIVE THROUGHPUT"),
                                    )
                                    .child(div().w(px(6.0)).h(px(6.0)).bg(if active_slots > 0 {
                                        selenium_amber()
                                    } else {
                                        graphite_border()
                                    })),
                            )
                            .child(
                                div()
                                    .font_family(MONO_FONT)
                                    .text_size(TYPE_DISPLAY)
                                    .line_height(px(22.0))
                                    .font_weight(FontWeight::BOLD)
                                    .text_color(silver_text())
                                    .child(format_speed(total_speed)),
                            )
                            .child(
                                div()
                                    .font_family(MONO_FONT)
                                    .text_size(TYPE_MICRO)
                                    .line_height(px(14.0))
                                    .text_color(silver_muted())
                                    .child(format!(
                                        "{:.1} Mbps",
                                        (total_speed as f64 * 8.0) / 1_000_000.0
                                    )),
                            )
                            .child(
                                div()
                                    .pt_1p5()
                                    .mt_1()
                                    .border_t_1()
                                    .border_color(graphite_border())
                                    .flex()
                                    .flex_col()
                                    .gap_1()
                                    .child(
                                        div()
                                            .flex()
                                            .items_center()
                                            .justify_between()
                                            .font_family(MONO_FONT)
                                            .text_size(TYPE_MICRO)
                                            .line_height(px(14.0))
                                            .child(
                                                div()
                                                    .text_color(silver_muted())
                                                    .child("QUEUE SLOTS"),
                                            )
                                            .child(
                                                div()
                                                    .font_weight(FontWeight::SEMIBOLD)
                                                    .text_color(silver_text())
                                                    .child(format!(
                                                        "{active_slots} / {}",
                                                        self.settings.max_concurrent_downloads
                                                    )),
                                            ),
                                    )
                                    .child(
                                        div().flex().gap_1().children(
                                            (0..self.settings.max_concurrent_downloads).map(|i| {
                                                div().h(px(5.0)).flex_1().bg(if i < active_slots {
                                                    selenium_amber()
                                                } else {
                                                    graphite_border()
                                                })
                                            }),
                                        ),
                                    ),
                            ),
                    )
                    .child(
                        Button::new("open-settings")
                            .small()
                            .icon(IconName::Settings)
                            .label("Engine Settings")
                            .on_click(cx.listener(|app, _, window, cx| {
                                app.open_settings_modal(window, cx);
                            })),
                    ),
            )
            .into_any_element()
    }

    fn render_toolbar(&self, cx: &mut Context<Self>) -> impl IntoElement {
        div()
            .h(px(54.0))
            .w_full()
            .flex()
            .items_center()
            .gap_2()
            .px_4()
            .bg(paper_bg())
            .border_b_1()
            .border_color(paper_border())
            .child(
                div()
                    .flex_1()
                    .h(px(32.0))
                    .pl_2p5()
                    .pr_1()
                    .bg(graphite_bg())
                    .border_1()
                    .border_color(paper_border())
                    .rounded(px(4.0))
                    .flex()
                    .items_center()
                    .gap_1p5()
                    .child(
                        div()
                            .flex_1()
                            .font_family(MONO_FONT)
                            .text_size(TYPE_BODY)
                            .child(Input::new(&self.url_input).appearance(false)),
                    )
                    .child(
                        Button::new("paste-probe")
                            .xsmall()
                            .primary()
                            .icon(IconName::ClipboardPaste)
                            .tooltip("Paste & Probe")
                            .on_click(cx.listener(|app, _, window, cx| {
                                app.paste_and_probe_from_clipboard(window, cx);
                            })),
                    ),
            )
            .child(
                Button::new("resume-all")
                    .small()
                    .ghost()
                    .icon(IconName::Play)
                    .label("Resume All")
                    .on_click(cx.listener(|app, _, _, cx| {
                        app.resume_all(cx);
                    })),
            )
            .child(
                Button::new("pause-all")
                    .small()
                    .ghost()
                    .icon(IconName::Pause)
                    .label("Pause All")
                    .on_click(cx.listener(|app, _, _, cx| {
                        app.pause_all(cx);
                    })),
            )
            .child(
                Button::new("clear-completed")
                    .small()
                    .ghost()
                    .icon(IconName::Trash)
                    .label("Clear Completed")
                    .on_click(cx.listener(|app, _, _, cx| {
                        app.clear_completed(cx);
                    })),
            )
    }

    fn render_table(
        &self,
        rows: &[DownloadRowView],
        is_empty_queue: bool,
        cx: &mut Context<Self>,
    ) -> impl IntoElement {
        if rows.is_empty() {
            return div()
                .flex_1()
                .flex()
                .flex_col()
                .items_center()
                .justify_center()
                .gap_3()
                .bg(paper_bg())
                .child(
                    Icon::new(IconName::Download)
                        .size(px(28.0))
                        .text_color(carbon_muted()),
                )
                .child(
                    div()
                        .text_size(TYPE_DISPLAY)
                        .line_height(px(24.0))
                        .font_weight(FontWeight::BOLD)
                        .text_color(carbon_text())
                        .child(if is_empty_queue {
                            "No Downloads in Queue"
                        } else {
                            "No Matching Downloads"
                        }),
                )
                .child(
                    div()
                        .max_w(px(440.0))
                        .text_center()
                        .text_size(TYPE_BODY)
                        .line_height(px(18.0))
                        .text_color(carbon_muted())
                        .child(if is_empty_queue {
                            "Enter an HTTP/HTTPS URL above or paste directly from clipboard."
                        } else {
                            "Switch queue filter in the left rail or paste a new URL."
                        }),
                )
                .when(is_empty_queue, |el| {
                    el.child(
                        Button::new("empty-paste")
                            .primary()
                            .icon(IconName::ClipboardPaste)
                            .label("Paste URL from Clipboard & Probe")
                            .on_click(cx.listener(|app, _, window, cx| {
                                app.paste_and_probe_from_clipboard(window, cx);
                            })),
                    )
                })
                .into_any_element();
        }

        let mut row_elements = Vec::with_capacity(rows.len());
        for row in rows {
            row_elements.push(self.render_row(row, cx));
        }

        div()
            .flex_1()
            .flex()
            .flex_col()
            .overflow_hidden()
            .bg(paper_bg())
            // Table Header (shared column constants with render_row)
            .child(
                div()
                    .h(px(28.0))
                    .flex()
                    .items_center()
                    .px_4()
                    .bg(table_header_bg())
                    .border_b_1()
                    .border_color(paper_border())
                    .font_family(MONO_FONT)
                    .text_size(TYPE_HEADER)
                    .line_height(px(14.0))
                    .font_weight(FontWeight::SEMIBOLD)
                    .text_color(carbon_muted())
                    .child(self.header_cell("STATE", Some(COL_STATE), SortColumn::State, cx))
                    .child(self.header_cell("FILENAME", None, SortColumn::Filename, cx))
                    .child(self.header_cell("PROGRESS", Some(COL_PROGRESS), SortColumn::Progress, cx))
                    .child(self.header_cell("SPEED", Some(COL_SPEED), SortColumn::Speed, cx))
                    .child(
                        div()
                            .w(px(COL_ETA))
                            .h_full()
                            .flex()
                            .items_center()
                            .px_2()
                            .border_r_1()
                            .border_color(paper_border())
                            .child("ETA"),
                    )
                    .child(
                        div()
                            .w(px(COL_SEGS))
                            .h_full()
                            .flex()
                            .items_center()
                            .justify_center()
                            .border_r_1()
                            .border_color(paper_border())
                            .child("SEGS"),
                    )
                    .child(
                        div()
                            .w(px(COL_ACTIONS))
                            .h_full()
                            .flex()
                            .items_center()
                            .justify_end()
                            .child("ACTIONS"),
                    ),
            )
            // Scrollable Rows
            .child(
                div()
                    .id("downloads-table-scroll")
                    .flex_1()
                    .overflow_y_scroll()
                    .children(row_elements),
            )
            .into_any_element()
    }

    fn header_cell(
        &self,
        title: &'static str,
        width: Option<f32>,
        col: SortColumn,
        cx: &mut Context<Self>,
    ) -> impl IntoElement + use<> {
        let sort_icon = if self.sort_col == col {
            Some(match self.sort_dir {
                SortDirection::Ascending => IconName::ChevronUp,
                SortDirection::Descending => IconName::ChevronDown,
            })
        } else {
            None
        };
        let base = div()
            .id(SharedString::from(format!("hdr-{title}")))
            .h_full()
            .flex()
            .items_center()
            .gap_1()
            .pr_2()
            .border_r_1()
            .border_color(paper_border())
            .cursor_pointer()
            .hover(|s| s.text_color(carbon_text()))
            .on_click(cx.listener(move |app, _, _, cx| {
                app.toggle_sort(col, cx);
            }))
            .child(title)
            .when_some(sort_icon, |el, icon| {
                el.child(Icon::new(icon).size(px(12.0)).text_color(selenium_amber()))
            });
        match width {
            Some(w) => base.w(px(w)).into_any_element(),
            None => base.flex_1().min_w(px(120.0)).px_2().into_any_element(),
        }
    }

    fn render_row(&self, row: &DownloadRowView, cx: &mut Context<Self>) -> impl IntoElement + use<> {
        let is_selected = self.selected_id.as_ref() == Some(&row.id);
        let (icon_name, label) = state_icon(row.state);
        let state_color = match row.state {
            DownloadState::Active => selenium_amber(),
            DownloadState::Probing | DownloadState::Queued => cyanotype_blue(),
            DownloadState::Completed => halide_emerald(),
            DownloadState::Failed => errata_vermilion(),
            DownloadState::Paused | DownloadState::Cancelled => carbon_muted(),
        };

        let cp_pct = (row.checkpoint_ratio() * 100.0).clamp(0.0, 100.0);
        let total_pct = (row.total_ratio() * 100.0).clamp(0.0, 100.0);
        let inflight_pct = (total_pct - cp_pct).max(0.0);

        let id_select = row.id.clone();
        let id_pause = row.id.clone();
        let id_resume = row.id.clone();
        let id_cancel = row.id.clone();
        let id_delete = row.id.clone();

        div()
            .id(SharedString::from(format!("row-{}", row.id.as_str())))
            .h(px(38.0))
            .flex()
            .items_center()
            .pr_4()
            .border_b_1()
            .border_color(paper_border())
            .bg(if is_selected {
                paper_selected()
            } else {
                paper_bg()
            })
            .hover(|s| s.bg(paper_hover()))
            .cursor_pointer()
            .on_click(cx.listener(move |app, _, _, cx| {
                app.selected_id = Some(id_select.clone());
                app.load_segments_for(id_select.clone(), cx);
                cx.notify();
            }))
            // 3px selection accent bar (Selenium Amber)
            .child(
                div()
                    .w(px(3.0))
                    .h_full()
                    .mr_3()
                    .bg(if is_selected {
                        selenium_amber()
                    } else {
                        paper_bg()
                    }),
            )
            // State + FIFO rank badge
            .child(
                div()
                    .w(px(COL_STATE))
                    .h_full()
                    .pr_2()
                    .border_r_1()
                    .border_color(paper_border())
                    .flex()
                    .items_center()
                    .gap_1p5()
                    .font_family(MONO_FONT)
                    .text_size(TYPE_MICRO)
                    .line_height(px(16.0))
                    .font_weight(FontWeight::SEMIBOLD)
                    .text_color(state_color)
                    .child(Icon::new(icon_name).size(px(13.0)).text_color(state_color))
                    .child(if let Some(rank) = row.fifo_rank {
                        div()
                            .px_1()
                            .bg(graphite_elevated())
                            .border_1()
                            .border_color(graphite_border())
                            .text_color(silver_text())
                            .child(if rank == 1 {
                                "#1 NEXT".to_string()
                            } else {
                                format!("#{rank}")
                            })
                            .into_any_element()
                    } else {
                        div().child(label).into_any_element()
                    }),
            )
            // Filename
            .child(
                div()
                    .flex_1()
                    .min_w(px(120.0))
                    .h_full()
                    .px_2()
                    .border_r_1()
                    .border_color(paper_border())
                    .flex()
                    .items_center()
                    .text_size(TYPE_ROW_PRIMARY)
                    .line_height(px(18.0))
                    .font_weight(FontWeight::MEDIUM)
                    .text_color(carbon_text())
                    .truncate()
                    .child(row.filename.clone()),
            )
            // Dual-tone progress bar + byte/percent readout
            .child(
                div()
                    .w(px(COL_PROGRESS))
                    .h_full()
                    .px_2p5()
                    .border_r_1()
                    .border_color(paper_border())
                    .flex()
                    .flex_col()
                    .justify_center()
                    .gap_1()
                    .child(
                        div()
                            .flex()
                            .items_center()
                            .justify_between()
                            .font_family(MONO_FONT)
                            .text_size(TYPE_MICRO)
                            .line_height(px(13.0))
                            .child(
                                div()
                                    .text_color(carbon_muted())
                                    .child(match row.total_bytes {
                                        Some(total) => format!(
                                            "{} / {}",
                                            format_bytes(row.bytes_downloaded),
                                            format_bytes(total)
                                        ),
                                        None => format!("{} / Stream", format_bytes(row.bytes_downloaded)),
                                    }),
                            )
                            .child(
                                div()
                                    .font_weight(FontWeight::SEMIBOLD)
                                    .text_color(carbon_text())
                                    .child(if row.total_bytes.is_some() {
                                        format!("{total_pct:.1}%")
                                    } else {
                                        "200".to_string()
                                    }),
                            ),
                    )
                    .child(
                        div()
                            .h(px(6.0))
                            .w_full()
                            .bg(graphite_bg())
                            .flex()
                            .overflow_hidden()
                            .child(
                                div()
                                    .h_full()
                                    .w(relative(cp_pct / 100.0))
                                    .bg(if row.state == DownloadState::Completed {
                                        halide_emerald()
                                    } else if row.state == DownloadState::Failed {
                                        errata_vermilion()
                                    } else {
                                        cyanotype_blue()
                                    }),
                            )
                            .child(
                                div()
                                    .h_full()
                                    .w(relative(inflight_pct / 100.0))
                                    .bg(selenium_amber()),
                            ),
                    ),
            )
            // Speed
            .child(
                div()
                    .w(px(COL_SPEED))
                    .h_full()
                    .px_2()
                    .border_r_1()
                    .border_color(paper_border())
                    .flex()
                    .items_center()
                    .font_family(MONO_FONT)
                    .text_size(TYPE_BODY)
                    .line_height(px(16.0))
                    .text_color(carbon_text())
                    .child(format_speed(row.bytes_per_sec)),
            )
            // ETA
            .child(
                div()
                    .w(px(COL_ETA))
                    .h_full()
                    .px_2()
                    .border_r_1()
                    .border_color(paper_border())
                    .flex()
                    .items_center()
                    .font_family(MONO_FONT)
                    .text_size(TYPE_BODY)
                    .line_height(px(16.0))
                    .text_color(carbon_muted())
                    .child(format_eta(row.eta_secs)),
            )
            // Segments
            .child(
                div()
                    .w(px(COL_SEGS))
                    .h_full()
                    .border_r_1()
                    .border_color(paper_border())
                    .flex()
                    .items_center()
                    .justify_center()
                    .font_family(MONO_FONT)
                    .text_size(TYPE_BODY)
                    .line_height(px(16.0))
                    .text_color(carbon_muted())
                    .child(if row.segment_count == 0 {
                        "—".to_string()
                    } else {
                        row.segment_count.to_string()
                    }),
            )
            // Inline Actions
            .child(
                div()
                    .w(px(COL_ACTIONS))
                    .h_full()
                    .pl_2()
                    .flex()
                    .items_center()
                    .justify_end()
                    .gap_1()
                    .when(
                        matches!(
                            row.state,
                            DownloadState::Active | DownloadState::Queued | DownloadState::Probing
                        ),
                        |el| {
                            el.child(
                                Button::new(SharedString::from(format!(
                                    "pause-{}",
                                    row.id.as_str()
                                )))
                                .xsmall()
                                .ghost()
                                .icon(IconName::Pause)
                                .tooltip("Pause")
                                .on_click(cx.listener(move |app, _, _, cx| {
                                    app.pause_download(id_pause.clone(), cx);
                                })),
                            )
                        },
                    )
                    .when(
                        matches!(row.state, DownloadState::Paused | DownloadState::Failed),
                        |el| {
                            let id_action = id_resume.clone();
                            if row.is_resource_changed {
                                el.child(
                                    Button::new(SharedString::from(format!(
                                        "restart-{}",
                                        row.id.as_str()
                                    )))
                                    .xsmall()
                                    .primary()
                                    .icon(IconName::RotateCcw)
                                    .label("Restart")
                                    .tooltip("Restart Fresh")
                                    .on_click(cx.listener(move |app, _, _, cx| {
                                        app.restart_fresh_download(id_action.clone(), cx);
                                    })),
                                )
                            } else {
                                el.child(
                                    Button::new(SharedString::from(format!(
                                        "resume-{}",
                                        row.id.as_str()
                                    )))
                                    .xsmall()
                                    .ghost()
                                    .icon(IconName::Play)
                                    .tooltip("Resume")
                                    .on_click(cx.listener(move |app, _, _, cx| {
                                        app.resume_download(id_action.clone(), cx);
                                    })),
                                )
                            }
                        },
                    )
                    .when(
                        !matches!(row.state, DownloadState::Completed | DownloadState::Cancelled),
                        |el| {
                            el.child(
                                Button::new(SharedString::from(format!(
                                    "cancel-{}",
                                    row.id.as_str()
                                )))
                                .xsmall()
                                .ghost()
                                .icon(IconName::X)
                                .tooltip("Cancel")
                                .on_click(cx.listener(move |app, _, _, cx| {
                                    app.cancel_download(id_cancel.clone(), cx);
                                })),
                            )
                        },
                    )
                    .child(
                        Button::new(SharedString::from(format!("del-{}", row.id.as_str())))
                            .xsmall()
                            .ghost()
                            .icon(IconName::Trash)
                            .tooltip("Delete")
                            .on_click(cx.listener(move |app, _, _, cx| {
                                app.delete_download(id_delete.clone(), cx);
                            })),
                    ),
            )
    }
}
