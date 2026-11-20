//! Top toolbar and Download queue table/row views for [PosthazeApp].

use std::sync::Arc;
use std::time::{Duration, Instant};

use gpui_kit::assets::IconName;
use gpui_kit::component::button::{Button, ButtonVariants};
use gpui_kit::component::input::Input;
use gpui_kit::component::tooltip::Tooltip;
use gpui_kit::component::{Icon, InteractiveElementExt as _, Sizable};
use gpui_kit::prelude::FluentBuilder;
use gpui_kit::*;
use posthaze_engine::DownloadState;

use crate::PosthazeApp;
use crate::assets::BRAND_LOGO_SVG;
use crate::context_menu::ContextMenuState;
use crate::motion::RowMotionSample;
use crate::theme::*;
use crate::view_model::{
    COLLAPSED_INSPECTOR_HEIGHT, DownloadRowView, FilterTab, LayoutProfile, SortColumn,
    SortDirection, format_bytes, format_eta, format_speed, state_icon,
};

const COL_STATE: f32 = 104.0;
const COL_SPEED: f32 = 90.0;
const COL_ETA: f32 = 64.0;
const COL_SEGS: f32 = 48.0;
const COL_ACTIONS: f32 = 148.0;

impl PosthazeApp {
    pub(crate) fn render_toolbar(&self, profile: LayoutProfile, cx: &mut Context<Self>) -> impl IntoElement {
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
                    .when(!profile.compact_toolbar, |b| b.label("Resume All"))
                    .tooltip("Resume All")
                    .on_click(cx.listener(|app, _, _, cx| {
                        app.resume_all(cx);
                    })),
            )
            .child(
                Button::new("pause-all")
                    .small()
                    .ghost()
                    .icon(IconName::Pause)
                    .when(!profile.compact_toolbar, |b| b.label("Pause All"))
                    .tooltip("Pause All")
                    .on_click(cx.listener(|app, _, _, cx| {
                        app.pause_all(cx);
                    })),
            )
            .child(
                Button::new("clear-completed")
                    .small()
                    .ghost()
                    .icon(IconName::Trash)
                    .when(!profile.compact_toolbar, |b| b.label("Clear Completed"))
                    .tooltip("Clear Completed")
                    .on_click(cx.listener(|app, _, _, cx| {
                        app.request_clear_completed(cx);
                    })),
            )
    }

    pub(crate) fn render_table(
        &self,
        rows: Vec<DownloadRowView>,
        is_empty_queue: bool,
        profile: LayoutProfile,
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
                .child(if is_empty_queue {
                    div()
                        .w(px(44.0))
                        .h(px(44.0))
                        .rounded(px(10.0))
                        .bg(graphite_bg())
                        .border_1()
                        .border_color(graphite_border())
                        .flex()
                        .items_center()
                        .justify_center()
                        .child(
                            svg()
                                .path(BRAND_LOGO_SVG)
                                .w(px(26.0))
                                .h(px(23.0))
                                .text_color(selenium_amber()),
                        )
                        .into_any_element()
                } else {
                    Icon::new(IconName::Download)
                        .size(px(28.0))
                        .text_color(carbon_muted())
                        .into_any_element()
                })
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
                .when(!is_empty_queue, |el| {
                    el.child(
                        Button::new("empty-show-all")
                            .small()
                            .icon(IconName::Folder)
                            .label("Show All Downloads")
                            .on_click(cx.listener(|app, _, _, cx| {
                                app.filter = FilterTab::All;
                                cx.notify();
                            })),
                    )
                })
                .into_any_element();
        }

        let visible_rows = Arc::new(rows);
        let row_count = visible_rows.len();
        let entity = cx.entity().clone();
        let scroll_handle = self.table_scroll_handle.clone();

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
                    .child(self.header_cell(
                        "PROGRESS",
                        Some(profile.progress_col_width),
                        SortColumn::Progress,
                        cx,
                    ))
                    .when(profile.show_speed, |el| {
                        el.child(self.header_cell("SPEED", Some(COL_SPEED), SortColumn::Speed, cx))
                    })
                    .when(profile.show_eta, |el| {
                        el.child(
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
                    })
                    .when(profile.show_segs, |el| {
                        el.child(
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
                    })
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
            // Virtualized Scrollable Rows
            .child(
                uniform_list("downloads-table-scroll", row_count, move |range, _window, cx| {
                    entity.update(cx, |app, cx| {
                        range
                            .map(|idx| app.render_row(&visible_rows[idx], profile, cx).into_any_element())
                            .collect::<Vec<_>>()
                    })
                })
                .flex_1()
                .size_full()
                .track_scroll(&scroll_handle),
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
            Some(w) if col == SortColumn::State => base.w(px(w)).into_any_element(),
            Some(w) => base.w(px(w)).pl_2().into_any_element(),
            None => base.flex_1().min_w(px(120.0)).px_2().into_any_element(),
        }
    }

    fn render_row(
        &self,
        row: &DownloadRowView,
        profile: LayoutProfile,
        cx: &mut Context<Self>,
    ) -> impl IntoElement + use<> {
        let is_selected = self.queue.selected_id.as_ref() == Some(&row.id);
        let (icon_name, label) = state_icon(row.state);
        let state_color = match row.state {
            DownloadState::Active => selenium_amber(),
            DownloadState::Probing | DownloadState::Queued => cyanotype_blue(),
            DownloadState::Completed => halide_emerald(),
            DownloadState::Failed => errata_vermilion(),
            DownloadState::Paused | DownloadState::Cancelled => carbon_muted(),
        };

        let motion_sample: RowMotionSample =
            self.motion.sample(&row.id, row.total_bytes, Instant::now());
        let cp_pct = (motion_sample
            .checkpoint_ratio
            .unwrap_or_else(|| row.checkpoint_ratio())
            * 100.0)
            .clamp(0.0, 100.0);
        let total_pct = (motion_sample
            .downloaded_ratio
            .unwrap_or_else(|| row.total_ratio())
            * 100.0)
            .clamp(0.0, 100.0);
        let inflight_pct = (total_pct - cp_pct).max(0.0);

        let id_select = row.id.clone();
        let id_pause = row.id.clone();
        let id_resume = row.id.clone();
        let id_cancel = row.id.clone();
        let id_delete = row.id.clone();
        let filename_del = row.filename.clone();
        let output_path = row.output_path.clone();
        let open_path = row.output_path.clone();
        let row_state = row.state;

        let failed_tooltip = (row.state == DownloadState::Failed)
            .then(|| row.error_message.clone())
            .flatten();

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
                app.queue.selected_id = Some(id_select.clone());
                app.load_segments_for(id_select.clone(), cx);
                if app.queue.settings.ui_click_row_opens_inspector && app.inspector_collapsed {
                    app.inspector_collapsed = false;
                    app.motion
                        .animate_inspector(COLLAPSED_INSPECTOR_HEIGHT, app.inspector_height);
                    app.ensure_motion_loop(cx);
                }
                cx.notify();
            }))
            .on_double_click(cx.listener(move |app, _, _, cx| {
                if row_state == DownloadState::Completed {
                    app.open_downloaded_file(open_path.clone(), cx);
                }
            }))
            .on_mouse_down(
                MouseButton::Right,
                cx.listener({
                    let id_ctx = row.id.clone();
                    move |app, ev: &MouseDownEvent, _, cx| {
                        cx.stop_propagation();
                        app.queue.selected_id = Some(id_ctx.clone());
                        app.load_segments_for(id_ctx.clone(), cx);
                        app.context_menu = Some(ContextMenuState {
                            download_id: id_ctx.clone(),
                            anchor: ev.position,
                        });
                        cx.notify();
                    }
                }),
            )
            // 3px selection accent bar + 13px margin = 16px left offset matching header px_4
            .child(
                div()
                    .w(px(3.0))
                    .flex_shrink_0()
                    .h_full()
                    .mr(px(13.0))
                    .bg(if is_selected {
                        selenium_amber()
                    } else {
                        paper_bg()
                    }),
            )
            // State + FIFO rank badge
            .child(
                div()
                    .id(SharedString::from(format!("state-{}", row.id.as_str())))
                    .w(px(COL_STATE))
                    .flex_shrink_0()
                    .h_full()
                    .pr_2()
                    .border_r_1()
                    .border_color(paper_border())
                    .flex()
                    .items_center()
                    .overflow_hidden()
                    .gap_1p5()
                    .font_family(MONO_FONT)
                    .text_size(TYPE_MICRO)
                    .line_height(px(16.0))
                    .font_weight(FontWeight::SEMIBOLD)
                    .text_color(state_color)
                    .when_some(failed_tooltip, |el, msg| {
                        el.tooltip(move |window, cx| Tooltip::new(msg.clone()).build(window, cx))
                    })
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
                    .min_w_0()
                    .h_full()
                    .px_2()
                    .border_r_1()
                    .border_color(paper_border())
                    .flex()
                    .items_center()
                    .overflow_hidden()
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
                    .w(px(profile.progress_col_width))
                    .flex_shrink_0()
                    .h_full()
                    .px_2p5()
                    .border_r_1()
                    .border_color(paper_border())
                    .flex()
                    .flex_col()
                    .justify_center()
                    .overflow_hidden()
                    .gap_1()
                    .child(
                        div()
                            .flex()
                            .items_center()
                            .justify_between()
                            .gap_2()
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
                                        "STREAM".to_string()
                                    }),
                            ),
                    )
                    .child(
                        div()
                            .h(px(6.0))
                            .w_full()
                            .bg(graphite_bg())
                            .when(motion_sample.completion_flash > 0.05, |el| {
                                el.border_1()
                                    .border_color(halide_emerald().opacity(motion_sample.completion_flash))
                            })
                            .when(
                                motion_sample.completion_flash <= 0.05
                                    && motion_sample.split_flash > 0.05,
                                |el| {
                                    el.border_1()
                                        .border_color(selenium_amber().opacity(motion_sample.split_flash))
                                },
                            )
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
                            .child({
                                let inflight_bar = div()
                                    .h_full()
                                    .w(relative(inflight_pct / 100.0))
                                    .bg(selenium_amber());
                                if row.state == DownloadState::Active && inflight_pct > 0.0 {
                                    inflight_bar
                                        .with_animation(
                                            SharedString::from(format!(
                                                "row-inflight-pulse-{}",
                                                row.id.as_str()
                                            )),
                                            Animation::new(Duration::from_millis(1400))
                                                .repeat()
                                                .with_easing(pulsating_between(0.72, 1.0)),
                                            |el, delta| el.opacity(delta),
                                        )
                                        .into_any_element()
                                } else {
                                    inflight_bar.into_any_element()
                                }
                            }),
                    ),
            )
            // Speed
            .when(profile.show_speed, |el| {
                el.child(
                    div()
                        .w(px(COL_SPEED))
                        .flex_shrink_0()
                        .h_full()
                        .px_2()
                        .border_r_1()
                        .border_color(paper_border())
                        .flex()
                        .items_center()
                        .overflow_hidden()
                        .font_family(MONO_FONT)
                        .text_size(TYPE_BODY)
                        .line_height(px(16.0))
                        .text_color(carbon_text())
                        .child(format_speed(row.bytes_per_sec)),
                )
            })
            // ETA
            .when(profile.show_eta, |el| {
                el.child(
                    div()
                        .w(px(COL_ETA))
                        .flex_shrink_0()
                        .h_full()
                        .px_2()
                        .border_r_1()
                        .border_color(paper_border())
                        .flex()
                        .items_center()
                        .overflow_hidden()
                        .font_family(MONO_FONT)
                        .text_size(TYPE_BODY)
                        .line_height(px(16.0))
                        .text_color(carbon_muted())
                        .child(format_eta(row.eta_secs)),
                )
            })
            // Segments
            .when(profile.show_segs, |el| {
                el.child(
                    div()
                        .w(px(COL_SEGS))
                        .flex_shrink_0()
                        .h_full()
                        .border_r_1()
                        .border_color(paper_border())
                        .flex()
                        .items_center()
                        .justify_center()
                        .overflow_hidden()
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
            })
            // Inline Actions
            .child(
                div()
                    .id(SharedString::from(format!("actions-{}", row.id.as_str())))
                    .w(px(COL_ACTIONS))
                    .flex_shrink_0()
                    .h_full()
                    .pl_2()
                    .flex()
                    .items_center()
                    .justify_end()
                    .overflow_hidden()
                    .gap_1()
                    .on_click(|_, _, cx| cx.stop_propagation())
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
                        matches!(
                            row.state,
                            DownloadState::Active | DownloadState::Queued | DownloadState::Probing
                        ),
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
                    .when(row.state == DownloadState::Completed, |el| {
                        el.child(
                            Button::new(SharedString::from(format!("open-dir-{}", row.id.as_str())))
                                .xsmall()
                                .ghost()
                                .icon(IconName::FolderOpen)
                                .tooltip("Open Folder")
                                .on_click(cx.listener(move |app, _, _, cx| {
                                    app.open_download_folder(output_path.clone(), cx);
                                })),
                        )
                    })
                    .child(
                        Button::new(SharedString::from(format!("del-{}", row.id.as_str())))
                            .xsmall()
                            .ghost()
                            .icon(IconName::Trash)
                            .tooltip("Delete")
                            .on_click(cx.listener(move |app, _, _, cx| {
                                app.request_delete_download(
                                    id_delete.clone(),
                                    filename_del.clone(),
                                    cx,
                                );
                            })),
                    ),
            )
    }
}
