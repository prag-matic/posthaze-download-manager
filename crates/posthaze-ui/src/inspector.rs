//! Bottom docked Selected Download Inspector with Graduated Interval Byte-Map Ruler.

use gpui_kit::assets::IconName;
use gpui_kit::component::button::{Button, ButtonVariants};
use gpui_kit::component::{Icon, Sizable};
use gpui_kit::prelude::FluentBuilder;
use gpui_kit::*;
use posthaze_engine::{DownloadId, DownloadState, SegmentProgress};

use crate::theme::*;
use crate::view_model::{DownloadRowView, format_bytes, normalize_byte_map_segments};

pub(crate) struct InspectorCallbacks<F1, F2> {
    pub on_restart_fresh: F1,
    pub on_delete: F2,
}

pub(crate) fn render_inspector<F1, F2>(
    selected: Option<&DownloadRowView>,
    segments: &[SegmentProgress],
    callbacks: InspectorCallbacks<F1, F2>,
) -> AnyElement
where
    F1: Fn(DownloadId, &mut Window, &mut App) + 'static,
    F2: Fn(DownloadId, &mut Window, &mut App) + 'static,
{
    let Some(row) = selected else {
        return div()
            .h(px(240.0))
            .w_full()
            .bg(graphite_bg())
            .border_t_1()
            .border_color(graphite_border())
            .p_4()
            .flex()
            .items_center()
            .justify_center()
            .child(
                div()
                    .font_family(MONO_FONT)
                    .text_size(TYPE_BODY)
                    .line_height(px(18.0))
                    .text_color(silver_muted())
                    .child("Select a Download row to inspect its Graduated Interval Byte-Map Ruler and HTTP Validators."),
            )
            .into_any_element();
    };

    let normalized = normalize_byte_map_segments(segments, row.total_bytes, row.state);
    let total_label = row
        .total_bytes
        .map(format_bytes)
        .unwrap_or_else(|| "STREAM".to_string());

    let id_restart = row.id.clone();
    let id_delete = row.id.clone();

    div()
        .h(px(240.0))
        .w_full()
        .bg(graphite_bg())
        .border_t_1()
        .border_color(graphite_border())
        .px_4()
        .py_3()
        .flex()
        .flex_col()
        .gap_2()
        .overflow_hidden()
        // Header strip: filename + stamped HTTP validator badges
        .child(
            div()
                .flex()
                .items_center()
                .justify_between()
                .gap_2()
                .child(
                    div()
                        .flex_1()
                        .flex()
                        .flex_col()
                        .overflow_hidden()
                        .child(
                            div()
                                .text_size(TYPE_HEADING)
                                .line_height(px(20.0))
                                .font_weight(FontWeight::SEMIBOLD)
                                .text_color(silver_text())
                                .truncate()
                                .child(row.filename.clone()),
                        )
                        .child(
                            div()
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
                        .child(validator_badge(
                            if row.supports_ranges {
                                "206 MULTI-SEGMENT"
                            } else {
                                "200 SINGLE-STREAM"
                            },
                            if row.supports_ranges {
                                cyanotype_blue()
                            } else {
                                graphite_elevated()
                            },
                        ))
                        .when(row.sync_on_complete, |el| {
                            el.child(validator_badge("SYNC-ON-COMPLETE", graphite_elevated()))
                        })
                        .when_some(row.etag.clone(), |el, etag| {
                            el.child(validator_badge(
                                format!("ETag {}", truncate_badge(&etag, 16)),
                                graphite_elevated(),
                            ))
                        })
                        .when_some(row.last_modified.clone(), |el, lm| {
                            el.child(validator_badge(
                                format!("LM {}", truncate_badge(&lm, 18)),
                                graphite_elevated(),
                            ))
                        }),
                ),
        )
        // Errata Recovery Strip on ResourceChanged
        .when(row.is_resource_changed, |el| {
            el.child(
                div()
                    .px_3()
                    .py_1p5()
                    .rounded(px(4.0))
                    .bg(errata_vermilion())
                    .flex()
                    .items_center()
                    .justify_between()
                    .gap_2()
                    .child(
                        div()
                            .flex()
                            .items_center()
                            .gap_2()
                            .child(
                                Icon::new(IconName::TriangleAlert)
                                    .size(px(14.0))
                                    .text_color(silver_text()),
                            )
                            .child(
                                div()
                                    .font_family(MONO_FONT)
                                    .text_size(TYPE_BODY)
                                    .line_height(px(18.0))
                                    .font_weight(FontWeight::MEDIUM)
                                    .text_color(silver_text())
                                    .child("ERRATA: Remote resource changed since pause — .part preserved safely."),
                            ),
                    )
                    .child(
                        div()
                            .flex()
                            .items_center()
                            .gap_1p5()
                            .child(
                                Button::new("errata-restart-fresh")
                                    .xsmall()
                                    .primary()
                                    .icon(IconName::RotateCcw)
                                    .label("Restart Fresh")
                                    .on_click(move |_, window, cx| {
                                        (callbacks.on_restart_fresh)(id_restart.clone(), window, cx);
                                    }),
                            )
                            .child(
                                Button::new("errata-delete")
                                    .xsmall()
                                    .icon(IconName::Trash)
                                    .label("Delete")
                                    .on_click(move |_, window, cx| {
                                        (callbacks.on_delete)(id_delete.clone(), window, cx);
                                    }),
                            ),
                    ),
            )
        })
        // Signature Move: Graduated Interval Byte-Map Ruler
        .child(
            div()
                .flex()
                .flex_col()
                .gap_1()
                .child(
                    div()
                        .flex()
                        .items_end()
                        .font_family(MONO_FONT)
                        .text_size(TYPE_MICRO)
                        .line_height(px(14.0))
                        .text_color(silver_muted())
                        .child(ruler_quarter_tick("0 B (0%)", true))
                        .child(ruler_quarter_tick("25%", true))
                        .child(ruler_quarter_tick("50%", true))
                        .child(
                            div()
                                .flex_1()
                                .flex()
                                .justify_between()
                                .border_l_1()
                                .border_color(graphite_border())
                                .pl_1()
                                .child("75%")
                                .child(format!("{total_label} (100%)")),
                        ),
                )
                .child(
                    div()
                        .h(px(18.0))
                        .w_full()
                        .bg(graphite_elevated())
                        .border_1()
                        .border_color(graphite_border())
                        .flex()
                        .overflow_hidden()
                        .children(normalized.into_iter().enumerate().map(|(idx, seg)| {
                            div()
                                .h_full()
                                .w(relative(seg.width_ratio))
                                .when(idx > 0, |el| el.border_l_1().border_color(graphite_bg()))
                                .flex()
                                .overflow_hidden()
                                .child(
                                    div()
                                        .h_full()
                                        .w(relative(seg.checkpoint_fill_ratio))
                                        .bg(if seg.completed && row.state == DownloadState::Completed {
                                            halide_emerald()
                                        } else {
                                            cyanotype_blue()
                                        }),
                                )
                                .child(
                                    div()
                                        .h_full()
                                        .w(relative(seg.inflight_fill_ratio))
                                        .bg(selenium_amber()),
                                )
                        })),
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
                .gap_x_4()
                .gap_y_1()
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
                    let seg_status = if seg.completed {
                        "DONE"
                    } else if seg.cursor > seg.start {
                        "ACTIVE"
                    } else {
                        "WAIT"
                    };
                    div()
                        .w(px(300.0))
                        .flex()
                        .items_center()
                        .justify_between()
                        .font_family(MONO_FONT)
                        .text_size(TYPE_MICRO)
                        .line_height(px(16.0))
                        .text_color(silver_muted())
                        .child(format!(
                            "#{:02} [{}..{})",
                            seg.id,
                            format_bytes(seg.start),
                            seg.end.map(format_bytes).unwrap_or_else(|| "∞".to_string())
                        ))
                        .child(
                            div()
                                .text_color(if seg.completed {
                                    halide_emerald()
                                } else {
                                    silver_text()
                                })
                                .child(format!("{cp_pct}% · {seg_status}")),
                        )
                })),
        )
        .into_any_element()
}

fn ruler_quarter_tick(label: &'static str, left_tick: bool) -> impl IntoElement {
    div()
        .flex_1()
        .when(left_tick, |el| el.border_l_1().border_color(graphite_border()))
        .pl_1()
        .child(label)
}

fn validator_badge(label: impl Into<SharedString>, bg_color: Hsla) -> impl IntoElement {
    div()
        .px_2()
        .py_0p5()
        .rounded(px(3.0))
        .bg(bg_color)
        .border_1()
        .border_color(graphite_border())
        .font_family(MONO_FONT)
        .text_size(TYPE_MICRO)
        .line_height(px(15.0))
        .font_weight(FontWeight::MEDIUM)
        .text_color(silver_text())
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
