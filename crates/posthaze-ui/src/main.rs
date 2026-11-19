//! Posthaze Download Manager desktop UI powered by GPUI and GPUI Kit.

mod bridge;
mod theme;
mod view_model;

use std::collections::HashMap;
use std::time::Duration;

use gpui_kit::component::Sizable;
use gpui_kit::component::button::{Button, ButtonVariants};
use gpui_kit::component::input::{Input, InputEvent, InputState};
use gpui_kit::prelude::FluentBuilder;
use gpui_kit::*;
use posthaze_engine::{
    DownloadId, DownloadRecord, DownloadRequest, DownloadState, EngineEvent, ProgressSnapshot,
    Settings,
};

use crate::bridge::{EngineBridge, default_db_path};
use crate::theme::*;
use crate::view_model::{
    DownloadRowView, FilterTab, SortColumn, SortDirection, compute_fifo_ranks,
    filter_and_sort_rows, format_bytes, format_eta, format_speed, state_glyph,
};

const COL_STATE: f32 = 104.0;
const COL_PROGRESS: f32 = 176.0;
const COL_SPEED: f32 = 90.0;
const COL_ETA: f32 = 64.0;
const COL_SEGS: f32 = 48.0;
const COL_ACTIONS: f32 = 160.0;

fn main() {
    let bridge = EngineBridge::open(default_db_path()).expect("failed to initialize engine");

    gpui_kit::application().run(move |cx| {
        gpui_kit::init(cx);

        let window_options = WindowOptions {
            window_bounds: Some(WindowBounds::Windowed(Bounds::centered(
                None,
                size(px(1440.0), px(900.0)),
                cx,
            ))),
            window_min_size: Some(size(px(960.0), px(640.0))),
            titlebar: Some(TitlebarOptions {
                title: Some("Posthaze Download Manager".into()),
                ..Default::default()
            }),
            ..Default::default()
        };

        let bridge = bridge.clone();
        gpui_kit::open_window(window_options, cx, move |window, cx| {
            cx.new(|cx| PosthazeApp::new(bridge, window, cx))
        })
        .expect("failed to open Posthaze window");
    });
}

struct PosthazeApp {
    bridge: EngineBridge,
    url_input: Entity<InputState>,
    records: Vec<DownloadRecord>,
    live_map: HashMap<DownloadId, ProgressSnapshot>,
    settings: Settings,
    filter: FilterTab,
    sort_col: SortColumn,
    sort_dir: SortDirection,
    selected_id: Option<DownloadId>,
    status_banner: Option<String>,
}

impl PosthazeApp {
    fn new(bridge: EngineBridge, window: &mut Window, cx: &mut Context<Self>) -> Self {
        let url_input = cx.new(|cx| {
            InputState::new(window, cx).placeholder("Paste or enter HTTP/HTTPS URL to probe...")
        });
        url_input.update(cx, |state, cx| {
            state.focus(window, cx);
        });

        cx.subscribe(&url_input, |this, _, ev: &InputEvent, cx| {
            if matches!(ev, InputEvent::PressEnter { .. }) {
                this.submit_url_from_input(cx);
            }
        })
        .detach();

        let app = Self {
            bridge,
            url_input,
            records: Vec::new(),
            live_map: HashMap::new(),
            settings: Settings::default(),
            filter: FilterTab::All,
            sort_col: SortColumn::CreatedAt,
            sort_dir: SortDirection::Descending,
            selected_id: None,
            status_banner: None,
        };

        app.refresh_from_engine(cx);
        app.start_event_listener(cx);
        app
    }

    fn refresh_from_engine(&self, cx: &mut Context<Self>) {
        let rx = self.bridge.dispatch(|engine| async move {
            let records = engine.list_downloads().await;
            let settings = engine.get_settings().await;
            (records, settings)
        });
        cx.spawn(async move |this, cx| {
            if let Ok((Ok(records), Ok(settings))) = rx.await {
                let _ = this.update(cx, |app, cx| {
                    app.records = records;
                    app.settings = settings;
                    if app.selected_id.is_none() {
                        app.selected_id = app.records.first().map(|r| r.id.clone());
                    }
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
                    Ok(EngineEvent::StateChanged { .. }) | Ok(EngineEvent::QueueUpdated) => {
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

    fn submit_url_from_input(&mut self, cx: &mut Context<Self>) {
        let raw = self.url_input.read(cx).value().to_string();
        let trimmed = raw.trim().to_string();
        if trimmed.is_empty() {
            return;
        }
        self.start_url(trimmed, cx);
    }

    fn paste_and_probe_from_clipboard(&mut self, cx: &mut Context<Self>) {
        if let Some(item) = cx.read_from_clipboard()
            && let Some(text) = item.text()
        {
            let trimmed = text.trim().to_string();
            if !trimmed.is_empty() {
                self.start_url(trimmed, cx);
                return;
            }
        }
        self.status_banner = Some("Clipboard does not contain a URL.".to_string());
        cx.notify();
    }

    fn start_url(&mut self, url: String, cx: &mut Context<Self>) {
        let out_dir = self.settings.default_output_dir.clone();
        let max_workers = self.settings.max_workers;
        let sync = self.settings.sync_on_complete;
        let rx = self.bridge.dispatch(move |engine| async move {
            let mut req = DownloadRequest::new(url, out_dir);
            req.max_workers = max_workers;
            req.sync_on_complete = sync;
            engine.start(req).await.map(|h| h.id().clone())
        });
        cx.spawn(async move |this, cx| {
            if let Ok(res) = rx.await {
                let _ = this.update(cx, |app, cx| {
                    match res {
                        Ok(id) => {
                            app.selected_id = Some(id);
                            app.status_banner = None;
                            app.refresh_from_engine(cx);
                        }
                        Err(err) => {
                            app.status_banner = Some(format!("Failed to start download: {err}"));
                        }
                    }
                    cx.notify();
                });
            }
        })
        .detach();
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

        div()
            .flex()
            .size_full()
            .bg(paper_bg())
            .text_color(carbon_text())
            // Left Navigation & Filter Rail (220px, #181715)
            .child(self.render_sidebar(&all_rows, total_speed, active_slots, cx))
            // Main Right Column: Top Toolbar + Center Table + Bottom Inspector Anchor
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
                                .py_2()
                                .bg(errata_vermilion())
                                .text_color(silver_text())
                                .text_xs()
                                .child(msg),
                        )
                    })
                    .child(self.render_table(&visible_rows, is_empty_queue, cx))
                    .child(self.render_inspector_summary(&all_rows)),
            )
    }
}

impl PosthazeApp {
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

        div()
            .w(px(220.0))
            .h_full()
            .flex()
            .flex_col()
            .justify_between()
            .bg(graphite_bg())
            .border_r_1()
            .border_color(graphite_border())
            .p_3()
            .child(
                div()
                    .flex()
                    .flex_col()
                    .gap_4()
                    // Brand Plate Header
                    .child(
                        div()
                            .pb_3()
                            .border_b_1()
                            .border_color(graphite_border())
                            .child(
                                div()
                                    .text_sm()
                                    .font_weight(FontWeight::BOLD)
                                    .text_color(silver_text())
                                    .child("POSTHAZE"),
                            )
                            .child(
                                div()
                                    .text_xs()
                                    .text_color(silver_muted())
                                    .child("INTERVAL DOWNLOADER"),
                            ),
                    )
                    // Filter Rail
                    .child(
                        div().flex().flex_col().gap_1().children(tabs.into_iter().map(
                            |tab| {
                                let count =
                                    all_rows.iter().filter(|r| tab.matches(r.state)).count();
                                let selected = self.filter == tab;
                                div()
                                    .id(SharedString::from(format!("filter-{}", tab.label())))
                                    .flex()
                                    .items_center()
                                    .justify_between()
                                    .px_2p5()
                                    .py_1p5()
                                    .rounded(px(4.0))
                                    .cursor_pointer()
                                    .bg(if selected {
                                        graphite_elevated()
                                    } else {
                                        graphite_bg()
                                    })
                                    .border_1()
                                    .border_color(if selected {
                                        selenium_amber()
                                    } else {
                                        graphite_bg()
                                    })
                                    .text_xs()
                                    .text_color(if selected {
                                        silver_text()
                                    } else {
                                        silver_muted()
                                    })
                                    .on_click(cx.listener(move |app, _, _, cx| {
                                        app.filter = tab;
                                        cx.notify();
                                    }))
                                    .child(tab.label())
                                    .child(
                                        div()
                                            .font_family("Consolas")
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
                            .rounded(px(4.0))
                            .bg(graphite_elevated())
                            .border_1()
                            .border_color(graphite_border())
                            .flex()
                            .flex_col()
                            .gap_1()
                            .child(
                                div()
                                    .text_xs()
                                    .text_color(silver_muted())
                                    .child("THROUGHPUT"),
                            )
                            .child(
                                div()
                                    .font_family("Consolas")
                                    .text_sm()
                                    .font_weight(FontWeight::BOLD)
                                    .text_color(selenium_amber())
                                    .child(format_speed(total_speed)),
                            )
                            .child(
                                div()
                                    .font_family("Consolas")
                                    .text_xs()
                                    .text_color(silver_muted())
                                    .child(format!(
                                        "SLOTS {active_slots} / {}",
                                        self.settings.max_concurrent_downloads
                                    )),
                            ),
                    )
                    .child(
                        Button::new("open-settings")
                            .small()
                            .label("Settings")
                            .on_click(cx.listener(|app, _, _, cx| {
                                app.status_banner = Some(format!(
                                    "Settings: {} workers · {} concurrent · {}",
                                    app.settings.max_workers,
                                    app.settings.max_concurrent_downloads,
                                    app.settings.default_output_dir.display()
                                ));
                                cx.notify();
                            })),
                    ),
            )
    }

    fn render_toolbar(&self, cx: &mut Context<Self>) -> impl IntoElement {
        div()
            .h(px(56.0))
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
                    .font_family("Consolas")
                    .child(Input::new(&self.url_input)),
            )
            .child(
                Button::new("paste-probe")
                    .primary()
                    .label("Paste & Probe")
                    .on_click(cx.listener(|app, _, _, cx| {
                        app.paste_and_probe_from_clipboard(cx);
                    })),
            )
            .child(
                Button::new("resume-all")
                    .label("Resume All")
                    .on_click(cx.listener(|app, _, _, cx| {
                        app.resume_all(cx);
                    })),
            )
            .child(
                Button::new("pause-all")
                    .label("Pause All")
                    .on_click(cx.listener(|app, _, _, cx| {
                        app.pause_all(cx);
                    })),
            )
            .child(
                Button::new("clear-completed")
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
                    div()
                        .text_base()
                        .font_weight(FontWeight::SEMIBOLD)
                        .text_color(carbon_text())
                        .child(if is_empty_queue {
                            "No Downloads in Queue"
                        } else {
                            "No Matching Downloads"
                        }),
                )
                .child(
                    div()
                        .text_xs()
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
                            .label("Paste URL from Clipboard & Probe")
                            .on_click(cx.listener(|app, _, _, cx| {
                                app.paste_and_probe_from_clipboard(cx);
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
                    .h(px(32.0))
                    .flex()
                    .items_center()
                    .px_4()
                    .border_b_1()
                    .border_color(paper_border())
                    .text_xs()
                    .font_weight(FontWeight::SEMIBOLD)
                    .text_color(carbon_muted())
                    .child(self.header_cell("STATE", Some(COL_STATE), SortColumn::State, cx))
                    .child(self.header_cell("FILENAME", None, SortColumn::Filename, cx))
                    .child(self.header_cell("PROGRESS", Some(COL_PROGRESS), SortColumn::Progress, cx))
                    .child(self.header_cell("SPEED", Some(COL_SPEED), SortColumn::Speed, cx))
                    .child(div().w(px(COL_ETA)).child("ETA"))
                    .child(div().w(px(COL_SEGS)).child("SEGS"))
                    .child(div().w(px(COL_ACTIONS)).child("ACTIONS")),
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
        let indicator = if self.sort_col == col {
            match self.sort_dir {
                SortDirection::Ascending => " ▲",
                SortDirection::Descending => " ▼",
            }
        } else {
            ""
        };
        let base = div()
            .id(SharedString::from(format!("hdr-{title}")))
            .cursor_pointer()
            .on_click(cx.listener(move |app, _, _, cx| {
                app.toggle_sort(col, cx);
            }))
            .child(format!("{title}{indicator}"));
        match width {
            Some(w) => base.w(px(w)).into_any_element(),
            None => base.flex_1().min_w(px(120.0)).into_any_element(),
        }
    }

    fn render_row(&self, row: &DownloadRowView, cx: &mut Context<Self>) -> impl IntoElement + use<> {
        let is_selected = self.selected_id.as_ref() == Some(&row.id);
        let (glyph, label) = state_glyph(row.state);
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
            .h(px(36.0))
            .flex()
            .items_center()
            .px_4()
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
                cx.notify();
            }))
            // State + FIFO rank badge
            .child(
                div()
                    .w(px(COL_STATE))
                    .flex()
                    .items_center()
                    .gap_1p5()
                    .font_family("Consolas")
                    .text_xs()
                    .text_color(state_color)
                    .child(glyph)
                    .child(if let Some(rank) = row.fifo_rank {
                        if rank == 1 {
                            "#1 NEXT".to_string()
                        } else {
                            format!("#{rank}")
                        }
                    } else {
                        label.to_string()
                    }),
            )
            // Filename
            .child(
                div()
                    .flex_1()
                    .min_w(px(120.0))
                    .pr_2()
                    .text_xs()
                    .font_weight(FontWeight::MEDIUM)
                    .truncate()
                    .child(row.filename.clone()),
            )
            // Dual-tone progress bar + byte readout
            .child(
                div()
                    .w(px(COL_PROGRESS))
                    .pr_3()
                    .flex()
                    .flex_col()
                    .gap_1()
                    .child(
                        div()
                            .h(px(6.0))
                            .w_full()
                            .bg(paper_border())
                            .flex()
                            .overflow_hidden()
                            .child(div().h_full().w(relative(cp_pct / 100.0)).bg(cyanotype_blue()))
                            .child(
                                div()
                                    .h_full()
                                    .w(relative(inflight_pct / 100.0))
                                    .bg(selenium_amber()),
                            ),
                    )
                    .child(
                        div()
                            .font_family("Consolas")
                            .text_xs()
                            .text_color(carbon_muted())
                            .child(match row.total_bytes {
                                Some(total) => format!(
                                    "{} / {}",
                                    format_bytes(row.bytes_downloaded),
                                    format_bytes(total)
                                ),
                                None => format_bytes(row.bytes_downloaded),
                            }),
                    ),
            )
            // Speed
            .child(
                div()
                    .w(px(COL_SPEED))
                    .font_family("Consolas")
                    .text_xs()
                    .child(format_speed(row.bytes_per_sec)),
            )
            // ETA
            .child(
                div()
                    .w(px(COL_ETA))
                    .font_family("Consolas")
                    .text_xs()
                    .text_color(carbon_muted())
                    .child(format_eta(row.eta_secs)),
            )
            // Segments
            .child(
                div()
                    .w(px(COL_SEGS))
                    .font_family("Consolas")
                    .text_xs()
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
                    .flex()
                    .items_center()
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
                                .label("Pause")
                                .on_click(cx.listener(move |app, _, _, cx| {
                                    app.pause_download(id_pause.clone(), cx);
                                })),
                            )
                        },
                    )
                    .when(
                        matches!(row.state, DownloadState::Paused | DownloadState::Failed),
                        |el| {
                            el.child(
                                Button::new(SharedString::from(format!(
                                    "resume-{}",
                                    row.id.as_str()
                                )))
                                .xsmall()
                                .label("Resume")
                                .on_click(cx.listener(move |app, _, _, cx| {
                                    app.resume_download(id_resume.clone(), cx);
                                })),
                            )
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
                                .label("Cancel")
                                .on_click(cx.listener(move |app, _, _, cx| {
                                    app.cancel_download(id_cancel.clone(), cx);
                                })),
                            )
                        },
                    )
                    .child(
                        Button::new(SharedString::from(format!("del-{}", row.id.as_str())))
                            .xsmall()
                            .label("Delete")
                            .on_click(cx.listener(move |app, _, _, cx| {
                                app.delete_download(id_delete.clone(), cx);
                            })),
                    ),
            )
    }

    fn render_inspector_summary(&self, all_rows: &[DownloadRowView]) -> impl IntoElement {
        let selected = self
            .selected_id
            .as_ref()
            .and_then(|id| all_rows.iter().find(|r| &r.id == id));

        div()
            .h(px(140.0))
            .w_full()
            .bg(graphite_bg())
            .border_t_1()
            .border_color(graphite_border())
            .p_4()
            .flex()
            .flex_col()
            .justify_between()
            .child(match selected {
                Some(row) => div()
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
                                    .text_sm()
                                    .font_weight(FontWeight::SEMIBOLD)
                                    .text_color(silver_text())
                                    .child(row.filename.clone()),
                            )
                            .child(
                                div()
                                    .font_family("Consolas")
                                    .text_xs()
                                    .text_color(silver_muted())
                                    .child(format!(
                                        "{} | {} workers",
                                        if row.supports_ranges {
                                            "206 MULTI-SEGMENT"
                                        } else {
                                            "200 SINGLE-STREAM"
                                        },
                                        row.active_workers
                                    )),
                            ),
                    )
                    .child(
                        div()
                            .font_family("Consolas")
                            .text_xs()
                            .text_color(silver_muted())
                            .truncate()
                            .child(row.final_url.as_deref().unwrap_or(&row.url).to_string()),
                    )
                    .into_any_element(),
                None => div()
                    .text_xs()
                    .text_color(silver_muted())
                    .child("Select a download to inspect segment checkpoints and HTTP validators.")
                    .into_any_element(),
            })
            .child(
                div()
                    .font_family("Consolas")
                    .text_xs()
                    .text_color(silver_muted())
                    .child(format!(
                        "Checkpoint legend: Blue = flushed on disk · Amber = in-flight · 90ms state snap ({:?})",
                        Duration::from_millis(90)
                    )),
            )
    }
}
