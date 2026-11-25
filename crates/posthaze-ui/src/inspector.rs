//! Bottom docked Selected Download Inspector with Graduated Interval Byte-Map Ruler.

use std::time::Duration;

use gpui_kit::assets::IconName;
use gpui_kit::component::Icon;
use gpui_kit::prelude::FluentBuilder;
use gpui_kit::*;
use posthaze_engine::{DownloadId, DownloadState, SegmentProgress};

use crate::grid_cells::{
    BadgeTone, CellBorder, GridTone, grid_badge, grid_cell_button, grid_progress_track,
    grid_section_header, grid_status_dot,
};
use crate::motion::RowMotionSample;
use crate::theme::*;
use crate::view_model::{DownloadRowView, format_bytes, normalize_byte_map_segments};

pub(crate) struct InspectorCallbacks<F1, F2, F3> {
    pub on_restart_fresh: F1,
    pub on_delete: F2,
    pub on_toggle_collapse: F3,
}

pub(crate) fn render_inspector<F1, F2, F3>(
    selected: Option<&DownloadRowView>,
    segments: &[SegmentProgress],
    height: Pixels,
    collapsed: bool,
    motion: RowMotionSample,
    callbacks: InspectorCallbacks<F1, F2, F3>,
) -> AnyElement
where
    F1: Fn(DownloadId, &mut Window, &mut App) + 'static,
    F2: Fn(DownloadId, bool, &mut Window, &mut App) + 'static,
    F3: Fn(&mut Window, &mut App) + 'static,
{
    let on_toggle_collapse = callbacks.on_toggle_collapse;

    if collapsed {
        let summary = match selected {
            Some(row) => {
                let cp_ratio = (row.checkpoint_ratio() * 100.0).clamp(0.0, 100.0);
                format!(
                    "{} · Checkpoint: {} ({cp_ratio:.1}%)",
                    row.filename,
                    format_bytes(row.checkpointed_bytes)
                )
            }
            None => "No download selected".to_string(),
        };

        return div()
            .h(px(28.0))
            .w_full()
            .bg(graphite_bg())
            .border_t_1()
            .border_color(graphite_border())
            .flex()
            .items_center()
            .justify_between()
            .child(
                div()
                    .flex_1()
                    .h_full()
                    .flex()
                    .items_center()
                    .overflow_hidden()
                    .child(
                        div()
                            .h_full()
                            .px_3()
                            .bg(graphite_elevated())
                            .border_r_1()
                            .border_color(graphite_border())
                            .flex()
                            .items_center()
                            .font_family(MONO_FONT)
                            .text_size(TYPE_HEADER)
                            .font_weight(FontWeight::SEMIBOLD)
                            .text_color(silver_muted())
                            .child("Inspector"),
                    )
                    .child(
                        div()
                            .flex_1()
                            .px_3()
                            .font_family(MONO_FONT)
                            .text_size(TYPE_MICRO)
                            .text_color(silver_muted())
                            .truncate()
                            .child(summary),
                    ),
            )
            .child(grid_cell_button(
                "inspector-expand",
                Some(IconName::PanelBottomOpen),
                Some("Expand"),
                Some("Expand Inspector"),
                GridTone::Graphite,
                CellBorder::Left,
                true,
                move |_, window, cx| on_toggle_collapse(window, cx),
            ))
            .into_any_element();
    }

    let Some(row) = selected else {
        return div()
            .h(height)
            .w_full()
            .bg(graphite_bg())
            .border_t_1()
            .border_color(graphite_border())
            .flex()
            .flex_col()
            .child(
                grid_section_header("Byte-map inspector", GridTone::Graphite).child(
                    grid_cell_button(
                        "inspector-collapse-empty",
                        Some(IconName::PanelBottomClose),
                        None,
                        Some("Collapse Inspector"),
                        GridTone::Graphite,
                        CellBorder::Left,
                        true,
                        move |_, window, cx| on_toggle_collapse(window, cx),
                    ),
                ),
            )
            .child(
                div()
                    .flex_1()
                    .flex()
                    .items_center()
                    .justify_center()
                    .child(
                        div()
                            .font_family(MONO_FONT)
                            .text_size(TYPE_BODY)
                            .line_height(px(18.0))
                            .text_color(silver_muted())
                            .child("Select a download above to inspect its byte-map progress and segments"),
                    ),
            )
            .into_any_element();
    };

    let normalized = normalize_byte_map_segments(segments, row.total_bytes, row.state);
    let total_label = row
        .total_bytes
        .map(format_bytes)
        .unwrap_or_else(|| "STREAM".to_string());
    let cp_ratio = (row.checkpoint_ratio() * 100.0).clamp(0.0, 100.0);
    let inflight_delta = row
        .bytes_downloaded
        .saturating_sub(row.checkpointed_bytes);

    let id_restart = row.id.clone();
    let id_delete = row.id.clone();

    div()
        .h(height)
        .w_full()
        .bg(graphite_bg())
        .border_t_1()
        .border_color(graphite_border())
        .px_4()
        .py_2p5()
        .flex()
        .flex_col()
        .gap_2()
        .overflow_hidden()
        // Header strip: ID pill + filename + URL + stamped HTTP validator badges
        .child(
            div()
                .flex()
                .items_center()
                .justify_between()
                .gap_3()
                .pb_2()
                .border_b_1()
                .border_color(graphite_border())
                .child(
                    div()
                        .flex_1()
                        .flex()
                        .items_center()
                        .gap_2()
                        .overflow_hidden()
                        .child(grid_badge(
                            row.id.as_str().chars().take(8).collect::<String>(),
                            BadgeTone::Muted,
                        ))
                        .child(
                            div()
                                .text_size(TYPE_BODY)
                                .line_height(px(18.0))
                                .font_weight(FontWeight::SEMIBOLD)
                                .text_color(silver_text())
                                .truncate()
                                .child(row.filename.clone()),
                        )
                        .child(
                            div()
                                .flex_1()
                                .font_family(MONO_FONT)
                                .text_size(TYPE_MICRO)
                                .line_height(px(16.0))
                                .text_color(silver_muted())
                                .truncate()
                                .child(row.final_url.as_deref().unwrap_or(&row.url).to_string()),
                        ),
                )
                .child(
                    div()
                        .flex()
                        .items_center()
                        .gap_1p5()
                        .child(grid_badge(
                            if row.supports_ranges {
                                "206 MULTI-SEGMENT"
                            } else {
                                "200 SINGLE-STREAM"
                            },
                            BadgeTone::Neutral,
                        ))
                        .when(row.sync_on_complete, |el| {
                            el.child(grid_badge("SYNC-ON-COMPLETE", BadgeTone::Neutral))
                        })
                        .when_some(row.etag.clone(), |el, etag| {
                            el.child(grid_badge(
                                format!("ETag {}", truncate_badge(&etag, 16)),
                                BadgeTone::Neutral,
                            ))
                        })
                        .when_some(row.last_modified.clone(), |el, lm| {
                            el.child(grid_badge(
                                format!("LM {}", truncate_badge(&lm, 18)),
                                BadgeTone::Neutral,
                            ))
                        })
                        .child(
                            div().h(px(26.0)).child(grid_cell_button(
                                "inspector-collapse",
                                Some(IconName::PanelBottomClose),
                                None,
                                Some("Collapse Inspector"),
                                GridTone::Graphite,
                                CellBorder::Both,
                                true,
                                move |_, window, cx| on_toggle_collapse(window, cx),
                            )),
                        ),
                ),
        )
        // Diagnostic & Errata Strip for Failed Downloads
        .when(row.state == DownloadState::Failed, |el| {
            let diag = crate::errors::describe_error(
                row.error_message.as_deref().unwrap_or(""),
                row.is_resource_changed,
            );
            let msg = format!("{}: {}", diag.headline, diag.detail);
            let tip_full = match diag.tip {
                Some(t) => format!("{msg}\n{t}"),
                None => msg.clone(),
            };
            el.child(
                div()
                    .h(px(32.0))
                    .pl_3()
                    .bg(errata_surface_bg())
                    .border_1()
                    .border_color(errata_vermilion())
                    .flex()
                    .items_center()
                    .justify_between()
                    .gap_2()
                    .child(
                        div()
                            .id("errata-diagnostic-text")
                            .flex_1()
                            .flex()
                            .items_center()
                            .gap_2()
                            .min_w_0()
                            .tooltip(move |window, cx| {
                                gpui_kit::component::tooltip::Tooltip::new(tip_full.clone())
                                    .build(window, cx)
                            })
                            .child(
                                Icon::new(IconName::TriangleAlert)
                                    .size(px(14.0))
                                    .text_color(errata_vermilion()),
                            )
                            .child(
                                div()
                                    .font_family(MONO_FONT)
                                    .text_size(TYPE_BODY)
                                    .line_height(px(18.0))
                                    .font_weight(FontWeight::MEDIUM)
                                    .text_color(silver_text())
                                    .truncate()
                                    .child(msg),
                            ),
                    )
                    .child(
                        div()
                            .h_full()
                            .flex()
                            .items_center()
                            .when(row.is_resource_changed, |actions| {
                                actions.child(grid_cell_button(
                                    "errata-restart-fresh",
                                    Some(IconName::RotateCcw),
                                    Some("Restart fresh"),
                                    Some("Restart Fresh"),
                                    GridTone::Primary,
                                    CellBorder::Left,
                                    true,
                                    move |_, window, cx| {
                                        (callbacks.on_restart_fresh)(
                                            id_restart.clone(),
                                            window,
                                            cx,
                                        );
                                    },
                                ))
                            })
                            .child(grid_cell_button(
                                "errata-delete",
                                Some(IconName::Trash),
                                Some("Delete"),
                                Some("Remove / Recycle (Shift+Click: Permanent)"),
                                GridTone::Danger,
                                CellBorder::Left,
                                true,
                                move |ev, window, cx| {
                                    (callbacks.on_delete)(
                                        id_delete.clone(),
                                        ev.modifiers().shift,
                                        window,
                                        cx,
                                    );
                                },
                            )),
                    ),
            )
        })
        // Signature Move: Graduated Interval Byte-Map Ruler
        .child(
            div()
                .flex()
                .flex_col()
                .gap_1()
                .pb_2()
                .border_b_1()
                .border_color(graphite_border())
                .child(
                    div()
                        .flex()
                        .items_end()
                        .font_family(MONO_FONT)
                        .text_size(TYPE_HEADER)
                        .line_height(px(14.0))
                        .text_color(silver_muted())
                        .child(ruler_quarter_tick("0% (0 B)", false))
                        .child(ruler_quarter_tick(
                            row.total_bytes
                                .map(|t| format!("25% ({})", format_bytes(t / 4)))
                                .unwrap_or_else(|| "25%".to_string()),
                            true,
                        ))
                        .child(ruler_quarter_tick(
                            row.total_bytes
                                .map(|t| format!("50% ({})", format_bytes(t / 2)))
                                .unwrap_or_else(|| "50%".to_string()),
                            true,
                        ))
                        .child(
                            div()
                                .flex_1()
                                .flex()
                                .justify_between()
                                .border_l_1()
                                .border_color(graphite_border())
                                .pl_1()
                                .child(
                                    row.total_bytes
                                        .map(|t| format!("75% ({})", format_bytes((t * 3) / 4)))
                                        .unwrap_or_else(|| "75%".to_string()),
                                )
                                .child(format!("100% ({total_label})")),
                        ),
                )
                .child(
                    grid_progress_track(
                        px(22.0),
                        Some(if motion.completion_flash > 0.05 {
                            halide_emerald().opacity(0.35 + 0.65 * motion.completion_flash)
                        } else if motion.split_flash > 0.05 {
                            selenium_amber().opacity(0.35 + 0.65 * motion.split_flash)
                        } else {
                            graphite_border()
                        }),
                    )
                        .children(normalized.into_iter().enumerate().map(|(idx, seg)| {
                            let is_active = row.state == DownloadState::Active;
                            let inflight_el = div()
                                .h_full()
                                .w(relative(seg.inflight_fill_ratio))
                                .bg(selenium_amber());
                            div()
                                .h_full()
                                .w(relative(seg.width_ratio))
                                .when(idx > 0, |el| {
                                    el.border_l_1().border_color(if motion.split_flash > 0.05 {
                                        selenium_amber().opacity(motion.split_flash)
                                    } else {
                                        graphite_bg()
                                    })
                                })
                                .flex()
                                .overflow_hidden()
                                .child(
                                    div()
                                        .h_full()
                                        .w(relative(seg.checkpoint_fill_ratio))
                                        .bg(if seg.completed && row.state == DownloadState::Completed {
                                            halide_emerald()
                                        } else if row.state == DownloadState::Failed {
                                            errata_vermilion()
                                        } else {
                                            cyanotype_blue()
                                        }),
                                )
                                .child(if is_active && seg.inflight_fill_ratio > 0.0 {
                                    inflight_el
                                        .with_animation(
                                            SharedString::from(format!("insp-seg-inflight-{idx}")),
                                            Animation::new(Duration::from_millis(1400))
                                                .repeat()
                                                .with_easing(pulsating_between(0.72, 1.0)),
                                            |el, delta| el.opacity(delta),
                                        )
                                        .into_any_element()
                                } else {
                                    inflight_el.into_any_element()
                                })
                        })),
                )
                .child(
                    div()
                        .flex()
                        .items_center()
                        .justify_between()
                        .font_family(MONO_FONT)
                        .text_size(TYPE_HEADER)
                        .line_height(px(14.0))
                        .text_color(silver_muted())
                        .child(
                            div()
                                .flex()
                                .items_center()
                                .gap_4()
                                .child(
                                    div()
                                        .flex()
                                        .items_center()
                                        .gap_1p5()
                                        .child(grid_status_dot(cyanotype_blue(), 9.0, false))
                                        .child("Disk Checkpoint (Writer-Ack)"),
                                )
                                .child(
                                    div()
                                        .flex()
                                        .items_center()
                                        .gap_1p5()
                                        .child(grid_status_dot(selenium_amber(), 9.0, false))
                                        .child("In-Flight Network Cursor"),
                                ),
                        )
                        .child(
                            div().text_color(silver_text()).child(format!(
                                "Checkpoint: {} ({cp_ratio:.1}%) · In-Flight: +{}",
                                format_bytes(row.checkpointed_bytes),
                                format_bytes(inflight_delta),
                            )),
                        ),
                ),
        )
        // Compact Tabular Segment Grid
        .child(
            div()
                .id("inspector-segments-scroll")
                .flex_1()
                .overflow_y_scroll()
                .flex()
                .flex_wrap()
                .gap_1p5()
                .children(segments.iter().map(|seg| {
                    let span = seg
                        .end
                        .map(|e| e.saturating_sub(seg.start))
                        .unwrap_or(0);
                    let cp_pct = if seg.completed {
                        100
                    } else {
                        (seg.checkpointed_cursor.saturating_sub(seg.start) * 100)
                            .checked_div(span)
                            .unwrap_or(0)
                            .min(100)
                    };
                    let (seg_status, seg_color) = if seg.completed {
                        ("DONE", halide_emerald())
                    } else if seg.cursor > seg.start && row.state == DownloadState::Active {
                        ("RECV", selenium_amber())
                    } else {
                        ("WAIT", silver_muted())
                    };
                    div()
                        .min_w(px(240.0))
                        .flex_1()
                        .px_2p5()
                        .py_1()
                        .bg(graphite_elevated())
                        .border_1()
                        .border_color(graphite_border())
                        .flex()
                        .items_center()
                        .justify_between()
                        .font_family(MONO_FONT)
                        .text_size(TYPE_HEADER)
                        .line_height(px(15.0))
                        .text_color(silver_muted())
                        .child(
                            div()
                                .flex()
                                .items_center()
                                .gap_1p5()
                                .child(
                                    div()
                                        .font_weight(FontWeight::SEMIBOLD)
                                        .text_color(silver_text())
                                        .child(format!("#{:02}", seg.id)),
                                )
                                .child(format!(
                                    "[{}..{})",
                                    format_bytes(seg.start),
                                    seg.end.map(format_bytes).unwrap_or_else(|| "∞".to_string())
                                )),
                        )
                        .child(
                            div()
                                .flex()
                                .items_center()
                                .gap_2()
                                .child(div().text_color(silver_text()).child(format!("{cp_pct}%")))
                                .child(if seg_status == "RECV" {
                                    div()
                                        .font_weight(FontWeight::SEMIBOLD)
                                        .text_color(seg_color)
                                        .child(seg_status)
                                        .with_animation(
                                            SharedString::from(format!("insp-seg-recv-{}", seg.id)),
                                            Animation::new(Duration::from_millis(1400))
                                                .repeat()
                                                .with_easing(pulsating_between(0.55, 1.0)),
                                            |el, delta| el.opacity(delta),
                                        )
                                        .into_any_element()
                                } else {
                                    div()
                                        .font_weight(FontWeight::SEMIBOLD)
                                        .text_color(seg_color)
                                        .child(seg_status)
                                        .into_any_element()
                                }),
                        )
                })),
        )
        .into_any_element()
}

fn ruler_quarter_tick(label: impl Into<SharedString>, left_tick: bool) -> impl IntoElement {
    div()
        .flex_1()
        .when(left_tick, |el| el.border_l_1().border_color(graphite_border()))
        .pl_1()
        .child(label.into())
}

fn truncate_badge(s: &str, max_chars: usize) -> String {
    if s.chars().count() <= max_chars {
        s.to_string()
    } else {
        let mut out: String = s.chars().take(max_chars.saturating_sub(1)).collect();
        out.push('…');
        out
    }
}
