//! Bottom docked Selected Download Inspector with Graduated Interval Byte-Map Ruler.

use std::time::Duration;

use chronophoto_ui::components::{
    AlertBanner, AlertTone, BadgeTone, CellBorder, GridBadge, GridCellButton, GridProgressTrack,
    GridSectionHeader, GridStatusDot, GridTone,
};
use chronophoto_ui::theme::ChronoAppExt;
use chronophoto_ui::tokens::{TYPE_BODY, TYPE_HEADER, TYPE_MICRO};
use gpui_kit::assets::IconName;
use gpui_kit::component::Theme;
use gpui_kit::prelude::FluentBuilder;
use gpui_kit::*;
use posthaze_engine::{DownloadId, DownloadState, SegmentProgress};

use crate::motion::RowMotionSample;
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
    cx: &App,
    callbacks: InspectorCallbacks<F1, F2, F3>,
) -> AnyElement
where
    F1: Fn(DownloadId, &mut Window, &mut App) + 'static,
    F2: Fn(DownloadId, bool, &mut Window, &mut App) + 'static,
    F3: Fn(&mut Window, &mut App) + 'static,
{
    let palette = cx.chrono().palette;
    let chrome_bg = palette.chrome_bg.hsla();
    let chrome_elevated = palette.chrome_elevated.hsla();
    let chrome_border = palette.chrome_border.hsla();
    let chrome_text = palette.chrome_text.hsla();
    let chrome_muted = palette.chrome_muted.hsla();
    let accent = palette.accent.hsla();
    let info = palette.info.hsla();
    let success = palette.success.hsla();
    let danger = palette.danger.hsla();
    let mono = Theme::global(cx).mono_font_family.clone();

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
            .bg(chrome_bg)
            .border_t_1()
            .border_color(chrome_border)
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
                            .bg(chrome_elevated)
                            .border_r_1()
                            .border_color(chrome_border)
                            .flex()
                            .items_center()
                            .font_family(mono.clone())
                            .text_size(TYPE_HEADER)
                            .font_weight(FontWeight::SEMIBOLD)
                            .text_color(chrome_muted)
                            .child("Inspector"),
                    )
                    .child(
                        div()
                            .flex_1()
                            .px_3()
                            .font_family(mono)
                            .text_size(TYPE_MICRO)
                            .text_color(chrome_muted)
                            .truncate()
                            .child(summary),
                    ),
            )
            .child(
                GridCellButton::new("inspector-expand")
                    .icon(IconName::PanelBottomOpen)
                    .label("Expand")
                    .tooltip("Expand Inspector")
                    .tone(GridTone::Chrome)
                    .border(CellBorder::Left)
                    .compact(true)
                    .on_click(move |_, window, cx| on_toggle_collapse(window, cx)),
            )
            .into_any_element();
    }

    let Some(row) = selected else {
        return div()
            .h(height)
            .w_full()
            .bg(chrome_bg)
            .border_t_1()
            .border_color(chrome_border)
            .flex()
            .flex_col()
            .child(
                GridSectionHeader::new("Byte-map inspector")
                    .tone(GridTone::Chrome)
                    .trailing(
                        GridCellButton::new("inspector-collapse-empty")
                            .icon(IconName::PanelBottomClose)
                            .tooltip("Collapse Inspector")
                            .tone(GridTone::Chrome)
                            .border(CellBorder::Left)
                            .compact(true)
                            .on_click(move |_, window, cx| on_toggle_collapse(window, cx)),
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
                            .font_family(mono)
                            .text_size(TYPE_BODY)
                            .line_height(px(18.0))
                            .text_color(chrome_muted)
                            .child(
                                "Select a download above to inspect its byte-map progress and segments",
                            ),
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
    let inflight_delta = row.bytes_downloaded.saturating_sub(row.checkpointed_bytes);

    let id_restart = row.id.clone();
    let id_delete = row.id.clone();

    div()
        .h(height)
        .w_full()
        .bg(chrome_bg)
        .border_t_1()
        .border_color(chrome_border)
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
                .border_color(chrome_border)
                .child(
                    div()
                        .flex_1()
                        .flex()
                        .items_center()
                        .gap_2()
                        .overflow_hidden()
                        .child(
                            GridBadge::new(row.id.as_str().chars().take(8).collect::<String>())
                                .tone(BadgeTone::Muted),
                        )
                        .child(
                            div()
                                .text_size(TYPE_BODY)
                                .line_height(px(18.0))
                                .font_weight(FontWeight::SEMIBOLD)
                                .text_color(chrome_text)
                                .truncate()
                                .child(row.filename.clone()),
                        )
                        .child(
                            div()
                                .flex_1()
                                .font_family(mono.clone())
                                .text_size(TYPE_MICRO)
                                .line_height(px(16.0))
                                .text_color(chrome_muted)
                                .truncate()
                                .child(row.final_url.as_deref().unwrap_or(&row.url).to_string()),
                        ),
                )
                .child(
                    div()
                        .flex()
                        .items_center()
                        .gap_1p5()
                        .child(
                            GridBadge::new(if row.supports_ranges {
                                "206 MULTI-SEGMENT"
                            } else {
                                "200 SINGLE-STREAM"
                            })
                            .tone(BadgeTone::Neutral),
                        )
                        .when(row.sync_on_complete, |el| {
                            el.child(GridBadge::new("SYNC-ON-COMPLETE").tone(BadgeTone::Neutral))
                        })
                        .when_some(row.etag.clone(), |el, etag| {
                            el.child(
                                GridBadge::new(format!("ETag {}", truncate_badge(&etag, 16)))
                                    .tone(BadgeTone::Neutral),
                            )
                        })
                        .when_some(row.last_modified.clone(), |el, lm| {
                            el.child(
                                GridBadge::new(format!("LM {}", truncate_badge(&lm, 18)))
                                    .tone(BadgeTone::Neutral),
                            )
                        })
                        .child(
                            div().h(px(26.0)).child(
                                GridCellButton::new("inspector-collapse")
                                    .icon(IconName::PanelBottomClose)
                                    .tooltip("Collapse Inspector")
                                    .tone(GridTone::Chrome)
                                    .border(CellBorder::Both)
                                    .compact(true)
                                    .on_click(move |_, window, cx| on_toggle_collapse(window, cx)),
                            ),
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
                AlertBanner::new(msg)
                    .id("errata-diagnostic-text")
                    .tone(AlertTone::Danger)
                    .tooltip(tip_full)
                    .when(row.is_resource_changed, |banner| {
                        banner.action(
                            GridCellButton::new("errata-restart-fresh")
                                .icon(IconName::RotateCcw)
                                .label("Restart fresh")
                                .tooltip("Restart Fresh")
                                .tone(GridTone::Primary)
                                .border(CellBorder::Left)
                                .compact(true)
                                .on_click(move |_, window, cx| {
                                    (callbacks.on_restart_fresh)(id_restart.clone(), window, cx);
                                }),
                        )
                    })
                    .action(
                        GridCellButton::new("errata-delete")
                            .icon(IconName::Trash)
                            .label("Delete")
                            .tooltip("Remove / Recycle (Shift+Click: Permanent)")
                            .tone(GridTone::Danger)
                            .border(CellBorder::Left)
                            .compact(true)
                            .on_click(move |ev, window, cx| {
                                (callbacks.on_delete)(
                                    id_delete.clone(),
                                    ev.modifiers().shift,
                                    window,
                                    cx,
                                );
                            }),
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
                .border_color(chrome_border)
                .child(
                    div()
                        .flex()
                        .items_end()
                        .font_family(mono.clone())
                        .text_size(TYPE_HEADER)
                        .line_height(px(14.0))
                        .text_color(chrome_muted)
                        .child(ruler_quarter_tick("0% (0 B)", false, chrome_border))
                        .child(ruler_quarter_tick(
                            row.total_bytes
                                .map(|t| format!("25% ({})", format_bytes(t / 4)))
                                .unwrap_or_else(|| "25%".to_string()),
                            true,
                            chrome_border,
                        ))
                        .child(ruler_quarter_tick(
                            row.total_bytes
                                .map(|t| format!("50% ({})", format_bytes(t / 2)))
                                .unwrap_or_else(|| "50%".to_string()),
                            true,
                            chrome_border,
                        ))
                        .child(
                            div()
                                .flex_1()
                                .flex()
                                .justify_between()
                                .border_l_1()
                                .border_color(chrome_border)
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
                    GridProgressTrack::new()
                        .height(px(22.0))
                        .border_color(if motion.completion_flash > 0.05 {
                            success.opacity(0.35 + 0.65 * motion.completion_flash)
                        } else if motion.split_flash > 0.05 {
                            accent.opacity(0.35 + 0.65 * motion.split_flash)
                        } else {
                            chrome_border
                        })
                        .children(normalized.into_iter().enumerate().map(|(idx, seg)| {
                            let is_active = row.state == DownloadState::Active;
                            let inflight_el = div()
                                .h_full()
                                .w(relative(seg.inflight_fill_ratio))
                                .bg(accent);
                            div()
                                .h_full()
                                .w(relative(seg.width_ratio))
                                .when(idx > 0, |el| {
                                    el.border_l_1().border_color(if motion.split_flash > 0.05 {
                                        accent.opacity(motion.split_flash)
                                    } else {
                                        chrome_bg
                                    })
                                })
                                .flex()
                                .overflow_hidden()
                                .child(
                                    div()
                                        .h_full()
                                        .w(relative(seg.checkpoint_fill_ratio))
                                        .bg(if seg.completed && row.state == DownloadState::Completed {
                                            success
                                        } else if row.state == DownloadState::Failed {
                                            danger
                                        } else {
                                            info
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
                        .font_family(mono.clone())
                        .text_size(TYPE_HEADER)
                        .line_height(px(14.0))
                        .text_color(chrome_muted)
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
                                        .child(GridStatusDot::new(info).size(9.0).round(false))
                                        .child("Disk Checkpoint (Writer-Ack)"),
                                )
                                .child(
                                    div()
                                        .flex()
                                        .items_center()
                                        .gap_1p5()
                                        .child(GridStatusDot::new(accent).size(9.0).round(false))
                                        .child("In-Flight Network Cursor"),
                                ),
                        )
                        .child(
                            div().text_color(chrome_text).child(format!(
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
                        ("DONE", success)
                    } else if seg.cursor > seg.start && row.state == DownloadState::Active {
                        ("RECV", accent)
                    } else {
                        ("WAIT", chrome_muted)
                    };
                    div()
                        .min_w(px(240.0))
                        .flex_1()
                        .px_2p5()
                        .py_1()
                        .bg(chrome_elevated)
                        .border_1()
                        .border_color(chrome_border)
                        .flex()
                        .items_center()
                        .justify_between()
                        .font_family(mono.clone())
                        .text_size(TYPE_HEADER)
                        .line_height(px(15.0))
                        .text_color(chrome_muted)
                        .child(
                            div()
                                .flex()
                                .items_center()
                                .gap_1p5()
                                .child(
                                    div()
                                        .font_weight(FontWeight::SEMIBOLD)
                                        .text_color(chrome_text)
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
                                .child(div().text_color(chrome_text).child(format!("{cp_pct}%")))
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

fn ruler_quarter_tick(
    label: impl Into<SharedString>,
    left_tick: bool,
    border_col: Hsla,
) -> impl IntoElement {
    div()
        .flex_1()
        .when(left_tick, |el| el.border_l_1().border_color(border_col))
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
