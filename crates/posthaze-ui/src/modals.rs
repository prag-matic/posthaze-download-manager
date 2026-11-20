//! Pre-Flight Probe Confirmation Modal and Persisted Settings Modal.

use std::time::Duration;

use gpui_kit::assets::IconName;
use gpui_kit::component::input::{Input, InputState};
use gpui_kit::component::Icon;
use gpui_kit::prelude::FluentBuilder;
use gpui_kit::*;
use posthaze_engine::{DownloadId, DownloadRequest, PreflightInfo};

use crate::curl_parser::mask_header_summary;
use crate::grid_cells::{
    CellBorder, GridTone, grid_cell_button, grid_kv_row, grid_modal_footer,
};
use crate::theme::*;
use crate::view_model::format_bytes;

/// Active confirmation prompt for destructive or collision-override queue/record actions.
pub(crate) enum ConfirmAction {
    DeleteDownload {
        id: DownloadId,
        filename: String,
        delete_file: bool,
    },
    DeleteMultiple {
        ids: Vec<DownloadId>,
        delete_file: bool,
    },
    ClearCompleted { count: usize },
    OverwriteCollision {
        request: Box<DownloadRequest>,
        start_immediately: bool,
        filename: String,
    },
}

/// Active tab inside the resolved Pre-Flight Add Download window.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum PreflightTab {
    General,
    Options,
}

/// Splits `filename` into `(stem, Some(".ext"))`, ignoring leading dotfiles with no extra extension.
pub(crate) fn split_filename_extension(filename: &str) -> (String, Option<String>) {
    let trimmed = filename.trim();
    if let Some(dot) = trimmed.rfind('.')
        && dot > 0
        && dot + 1 < trimmed.len()
    {
        return (trimmed[..dot].to_string(), Some(trimmed[dot..].to_string()));
    }
    (trimmed.to_string(), None)
}

/// Active modal for pre-flight URL probing and confirmation.
pub(crate) enum PreflightModalState {
    Probing {
        url: String,
        headers: Vec<(String, String)>,
    },
    Error {
        url: String,
        headers: Vec<(String, String)>,
        message: String,
    },
    Resolved {
        info: Box<PreflightInfo>,
        filename_input: Entity<InputState>,
        dir_input: Entity<InputState>,
        cookie_input: Entity<InputState>,
        referer_input: Entity<InputState>,
        user_agent_input: Entity<InputState>,
        headers: Vec<(String, String)>,
        active_tab: PreflightTab,
        extension_locked: bool,
        locked_extension: Option<String>,
        workers: usize,
    },
}

/// Active modal for completed-file operations (`Rename`, `Move`, `File Integrity`).
pub(crate) enum FileOpModalState {
    Rename {
        id: DownloadId,
        input: Entity<InputState>,
        error: Option<String>,
    },
    Move {
        id: DownloadId,
        input: Entity<InputState>,
        error: Option<String>,
    },
    Integrity {
        filename: String,
        digest: Option<String>,
        error: Option<String>,
    },
}

#[allow(clippy::too_many_arguments)]
pub(crate) fn render_preflight_card<F1, F2, F3, F4, F5, F6, F7>(
    state: &PreflightModalState,
    show_add_paused: bool,
    on_close: F1,
    on_retry: F2,
    on_worker_delta: F3,
    on_confirm: F4,
    on_edit_url: F5,
    on_browse_dir: F6,
    on_toggle_ext_lock: F7,
) -> AnyElement
where
    F1: Fn(&mut Window, &mut App) + Clone + 'static,
    F2: Fn(String, Vec<(String, String)>, &mut Window, &mut App) + 'static,
    F3: Fn(isize, &mut Window, &mut App) + Clone + 'static,
    F4: Fn(bool, &mut Window, &mut App) + Clone + 'static,
    F5: Fn(String, &mut Window, &mut App) + 'static,
    F6: Fn(&mut Window, &mut App) + 'static,
    F7: Fn(&mut Window, &mut App) + 'static,
{
    match state {
        PreflightModalState::Probing { url, headers } => div()
            .w_full()
            .flex_1()
            .min_h_0()
            .bg(graphite_bg())
            .flex()
            .flex_col()
            .child(
                div()
                    .w_full()
                    .h(px(3.0))
                    .bg(byte_track_bg())
                    .overflow_hidden()
                    .child(
                        div()
                            .w(px(120.0))
                            .h_full()
                            .bg(selenium_amber())
                            .with_animation(
                                "preflight-shutter-sweep",
                                Animation::new(Duration::from_millis(1050))
                                    .repeat()
                                    .with_easing(ease_in_out),
                                |el, delta| el.ml(px(-40.0 + delta * 460.0)),
                            ),
                    ),
            )
            .child(
                div()
                    .w_full()
                    .flex_1()
                    .min_h_0()
                    .p_3()
                    .flex()
                    .flex_col()
                    .gap_2()
                    .child(
                        div()
                            .flex()
                            .items_center()
                            .justify_between()
                            .child(
                                div()
                                    .text_size(px(14.0))
                                    .line_height(px(18.0))
                                    .font_weight(FontWeight::BOLD)
                                    .text_color(silver_text())
                                    .child("Probing Target"),
                            )
                            .child(
                                div()
                                    .flex()
                                    .items_center()
                                    .gap_1p5()
                                    .child(
                                        div()
                                            .flex()
                                            .items_center()
                                            .child(
                                                Icon::new(IconName::LoaderCircle)
                                                    .size(px(13.0))
                                                    .text_color(selenium_amber()),
                                            )
                                            .with_animation(
                                                "preflight-probing-pulse",
                                                Animation::new(Duration::from_millis(900))
                                                    .repeat()
                                                    .with_easing(pulsating_between(0.45, 1.0)),
                                                |el, delta| el.opacity(delta),
                                            ),
                                    )
                                    .child(
                                        div()
                                            .font_family(MONO_FONT)
                                            .text_size(TYPE_MICRO)
                                            .text_color(selenium_amber())
                                            .child("HEAD / RANGE"),
                                    ),
                            ),
                    )
                    .child(
                        div()
                            .font_family(MONO_FONT)
                            .text_size(TYPE_MICRO)
                            .line_height(px(15.0))
                            .text_color(silver_muted())
                            .truncate()
                            .child(url.clone()),
                    )
                    .when(!headers.is_empty(), |el| {
                        el.child(
                            div()
                                .font_family(MONO_FONT)
                                .text_size(TYPE_MICRO)
                                .text_color(silver_muted())
                                .child(format!("Sending {} custom header(s)", headers.len())),
                        )
                    }),
            )
            .child(grid_modal_footer(GridTone::Graphite).child(grid_cell_button(
                "probe-cancel",
                Some(IconName::X),
                Some("CANCEL"),
                None,
                GridTone::Graphite,
                CellBorder::Left,
                false,
                move |_, window, cx| on_close(window, cx),
            ))),
        PreflightModalState::Error {
            url,
            headers,
            message,
        } => {
            let url_retry = url.clone();
            let headers_retry = headers.clone();
            let url_edit = url.clone();
            let is_forbidden = message.contains("403");
            div()
                .w_full()
                .flex_1()
                .min_h_0()
                .bg(graphite_bg())
                .border_t_2()
                .border_color(errata_vermilion())
                .flex()
                .flex_col()
                .child(
                    div()
                        .w_full()
                        .flex_1()
                        .min_h_0()
                        .p_3()
                        .flex()
                        .flex_col()
                        .gap_2()
                        .child(
                    div()
                        .flex()
                        .items_center()
                        .gap_1p5()
                        .child(
                            Icon::new(IconName::TriangleAlert)
                                .size(px(14.0))
                                .text_color(errata_vermilion()),
                        )
                        .child(
                            div()
                                .text_size(px(14.0))
                                .line_height(px(18.0))
                                .font_weight(FontWeight::SEMIBOLD)
                                .text_color(errata_vermilion())
                                .child("Could Not Reach File"),
                        ),
                )
                .child(
                    div()
                        .font_family(MONO_FONT)
                        .text_size(TYPE_BODY)
                        .line_height(px(16.0))
                        .text_color(silver_text())
                        .child(message.clone()),
                )
                .child(
                    div()
                        .font_family(MONO_FONT)
                        .text_size(TYPE_MICRO)
                        .line_height(px(14.0))
                        .text_color(silver_muted())
                        .truncate()
                        .child(url.clone()),
                )
                .when(is_forbidden, |el| {
                    el.child(
                        div()
                            .p_2()
                            .bg(graphite_elevated())
                            .border_1()
                            .border_color(graphite_border())
                            .rounded(px(3.0))
                            .text_size(TYPE_MICRO)
                            .line_height(px(14.0))
                            .text_color(silver_muted())
                            .child("Tip: Use DevTools -> Copy as cURL if this site requires a browser session."),
                    )
                }),
                )
                .child(
                    grid_modal_footer(GridTone::Graphite)
                        .child(grid_cell_button(
                            "probe-edit",
                            Some(IconName::Clipboard),
                            Some("EDIT URL"),
                            None,
                            GridTone::Graphite,
                            CellBorder::Left,
                            false,
                            move |_, window, cx| {
                                on_edit_url(url_edit.clone(), window, cx);
                            },
                        ))
                        .child(grid_cell_button(
                            "probe-close",
                            Some(IconName::X),
                            Some("CLOSE"),
                            None,
                            GridTone::Graphite,
                            CellBorder::Left,
                            false,
                            move |_, window, cx| on_close(window, cx),
                        ))
                        .child(grid_cell_button(
                            "probe-retry",
                            Some(IconName::RefreshCw),
                            Some("RETRY"),
                            None,
                            GridTone::Primary,
                            CellBorder::Left,
                            false,
                            move |_, window, cx| {
                                on_retry(
                                    url_retry.clone(),
                                    headers_retry.clone(),
                                    window,
                                    cx,
                                );
                            },
                        )),
                )
        }
        PreflightModalState::Resolved {
            info,
            filename_input,
            dir_input,
            cookie_input,
            referer_input,
            user_agent_input,
            headers,
            active_tab,
            extension_locked,
            locked_extension,
            workers,
        } => {
            let size_label = info
                .content_length
                .map(format_bytes)
                .unwrap_or_else(|| "Unknown".to_string());
            let proto_label = if info.supports_ranges {
                format!("HTTP 206 · {size_label}")
            } else {
                format!("HTTP 200 · {size_label}")
            };
            let segment_count = if info.supports_ranges {
                (*workers).clamp(1, 32)
            } else {
                1
            };
            let segment_meta = if info.supports_ranges {
                match info.content_length {
                    Some(total) if total > 0 => {
                        format!("{}/seg", format_bytes((total / segment_count as u64).max(1)))
                    }
                    _ => "Dynamic split".to_string(),
                }
            } else {
                "No Range".to_string()
            };
            let confirm_now = on_confirm.clone();
            let confirm_queue = on_confirm;
            let header_summary = (!headers.is_empty()).then(|| mask_header_summary(headers));
            let ext_is_locked = *extension_locked;
            let ext_has_value = locked_extension.is_some();
            let ext_badge_label = if ext_is_locked {
                locked_extension
                    .as_deref()
                    .map(|e| e.to_ascii_uppercase())
                    .unwrap_or_else(|| "LOCKED".to_string())
            } else {
                "UNLOCKED".to_string()
            };
            let ext_tooltip = if ext_is_locked {
                "Extension locked — click to unlock and edit full filename"
            } else {
                "Extension unlocked — click to lock extension"
            };

            div()
                .w_full()
                .flex_1()
                .min_h_0()
                .bg(graphite_bg())
                .flex()
                .flex_col()
                .child(
                    div()
                        .w_full()
                        .flex_1()
                        .min_h_0()
                        .p_3()
                        .flex()
                        .flex_col()
                        .gap_1p5()
                        .child(
                    div()
                        .flex()
                        .items_center()
                        .justify_between()
                        .gap_2()
                        .child(
                            div()
                                .text_size(px(14.0))
                                .line_height(px(18.0))
                                .font_weight(FontWeight::SEMIBOLD)
                                .text_color(silver_text())
                                .child("Add Download"),
                        )
                        .child(
                            div()
                                .px_1p5()
                                .py_0p5()
                                .rounded(px(3.0))
                                .bg(graphite_elevated())
                                .border_1()
                                .border_color(graphite_border())
                                .font_family(MONO_FONT)
                                .text_size(TYPE_MICRO)
                                .line_height(px(13.0))
                                .font_weight(FontWeight::MEDIUM)
                                .text_color(if info.supports_ranges {
                                    silver_text()
                                } else {
                                    silver_muted()
                                })
                                .child(proto_label),
                        ),
                )
                .child(
                    div()
                        .font_family(MONO_FONT)
                        .text_size(TYPE_MICRO)
                        .line_height(px(14.0))
                        .text_color(silver_muted())
                        .truncate()
                        .child(info.final_url.clone()),
                )
                .when(*active_tab == PreflightTab::General, |el| {
                    el.child(compact_input_row_with_action(
                        "FILE",
                        filename_input,
                        Some(
                            div()
                                .id("pf-ext-lock")
                                .h_full()
                                .px_2()
                                .border_l_1()
                                .border_color(graphite_border())
                                .flex()
                                .items_center()
                                .justify_center()
                                .font_family(MONO_FONT)
                                .text_size(TYPE_MICRO)
                                .font_weight(FontWeight::SEMIBOLD)
                                .cursor_pointer()
                                .when(ext_is_locked && ext_has_value, |el| {
                                    el.bg(selenium_amber())
                                        .text_color(rgb(active_theme().accent_foreground))
                                        .hover(|s| s.opacity(0.9))
                                })
                                .when(!(ext_is_locked && ext_has_value), |el| {
                                    el.bg(graphite_elevated())
                                        .text_color(silver_muted())
                                        .hover(|s| {
                                            s.bg(selenium_amber())
                                                .text_color(rgb(active_theme().accent_foreground))
                                        })
                                })
                                .tooltip(move |window, cx| {
                                    gpui_kit::component::tooltip::Tooltip::new(ext_tooltip)
                                        .build(window, cx)
                                })
                                .on_click(move |_, window, cx| on_toggle_ext_lock(window, cx))
                                .child(ext_badge_label.clone())
                                .into_any_element(),
                        ),
                    ))
                    .child(compact_input_row_with_action(
                        "SAVE TO",
                        dir_input,
                        Some(
                            grid_cell_button(
                                "pf-browse-dir",
                                Some(IconName::FolderOpen),
                                None,
                                Some("Choose destination folder"),
                                GridTone::Graphite,
                                CellBorder::Left,
                                true,
                                move |_, window, cx| on_browse_dir(window, cx),
                            )
                            .into_any_element(),
                        ),
                    ))
                })
                .when(*active_tab == PreflightTab::Options, |el| {
                    el.child(
                        div()
                            .w_full()
                            .bg(byte_track_bg())
                            .border_1()
                            .border_color(graphite_border())
                            .flex()
                            .flex_col()
                            .child(
                                div()
                                    .h(px(18.0))
                                    .px_1p5()
                                    .bg(graphite_elevated())
                                    .border_b_1()
                                    .border_color(graphite_border())
                                    .flex()
                                    .items_center()
                                    .justify_between()
                                    .font_family(MONO_FONT)
                                    .text_size(TYPE_MICRO)
                                    .child(
                                        div().text_color(silver_muted()).child(
                                            if info.supports_ranges {
                                                format!("{segment_count} PARALLEL SEGMENTS")
                                            } else {
                                                "SINGLE STREAM".to_string()
                                            },
                                        ),
                                    )
                                    .child(
                                        div()
                                            .text_color(if info.supports_ranges {
                                                selenium_amber()
                                            } else {
                                                silver_muted()
                                            })
                                            .child(segment_meta),
                                    ),
                            )
                            .child(
                                div()
                                    .w_full()
                                    .h(px(7.0))
                                    .flex()
                                    .items_stretch()
                                    .children((0..segment_count).map(|idx| {
                                        let fill = if !info.supports_ranges {
                                            silver_muted().opacity(0.45)
                                        } else if idx == 0 {
                                            selenium_amber()
                                        } else {
                                            cyanotype_blue().opacity(0.85)
                                        };
                                        div()
                                            .flex_1()
                                            .h_full()
                                            .bg(fill)
                                            .when(idx > 0, |el| {
                                                el.border_l_1().border_color(byte_track_bg())
                                            })
                                    }))
                                    .with_animation(
                                        "preflight-interval-reveal",
                                        Animation::new(Duration::from_millis(180))
                                            .with_easing(ease_out_quint()),
                                        |el, delta| el.opacity(0.4 + 0.6 * delta),
                                    ),
                            ),
                    )
                    .child(
                        div()
                            .flex()
                            .items_center()
                            .justify_between()
                            .gap_2()
                            .child(
                                div()
                                    .flex()
                                    .items_center()
                                    .gap_1p5()
                                    .child(
                                        div()
                                            .font_family(MONO_FONT)
                                            .text_size(TYPE_MICRO)
                                            .text_color(silver_muted())
                                            .child("REQUEST HEADERS"),
                                    )
                                    .when_some(header_summary, |el, summary| {
                                        el.child(
                                            div()
                                                .id("preflight-headers-badge")
                                                .px_1p5()
                                                .py_0p5()
                                                .rounded(px(3.0))
                                                .bg(graphite_elevated())
                                                .border_1()
                                                .border_color(graphite_border())
                                                .font_family(MONO_FONT)
                                                .text_size(TYPE_MICRO)
                                                .text_color(selenium_amber())
                                                .tooltip(move |window, cx| {
                                                    gpui_kit::component::tooltip::Tooltip::new(
                                                        summary.clone(),
                                                    )
                                                    .build(window, cx)
                                                })
                                                .child("cURL"),
                                        )
                                    }),
                            )
                            .child(
                                div()
                                    .flex()
                                    .items_center()
                                    .gap_2()
                                    .child(
                                        div()
                                            .font_family(MONO_FONT)
                                            .text_size(TYPE_MICRO)
                                            .text_color(silver_muted())
                                            .child(if info.supports_ranges {
                                                "WORKERS"
                                            } else {
                                                "WORKERS (1)"
                                            }),
                                    )
                                    .child(
                                        div()
                                            .h(px(26.0))
                                            .border_1()
                                            .border_color(graphite_border())
                                            .flex()
                                            .items_center()
                                            .when(info.supports_ranges, |el| {
                                                let dec = on_worker_delta.clone();
                                                el.child(grid_cell_button(
                                                    "w-dec",
                                                    Some(IconName::Minus),
                                                    None,
                                                    Some("Decrease workers"),
                                                    GridTone::Graphite,
                                                    CellBorder::Right,
                                                    true,
                                                    move |_, window, cx| dec(-1, window, cx),
                                                ))
                                            })
                                            .child(
                                                div()
                                                    .w(px(28.0))
                                                    .text_center()
                                                    .font_family(MONO_FONT)
                                                    .text_size(TYPE_BODY)
                                                    .text_color(silver_text())
                                                    .child(workers.to_string()),
                                            )
                                            .when(info.supports_ranges, |el| {
                                                el.child(grid_cell_button(
                                                    "w-inc",
                                                    Some(IconName::Plus),
                                                    None,
                                                    Some("Increase workers"),
                                                    GridTone::Graphite,
                                                    CellBorder::Left,
                                                    true,
                                                    move |_, window, cx| {
                                                        on_worker_delta(1, window, cx)
                                                    },
                                                ))
                                            }),
                                    ),
                            ),
                    )
                    .child(
                        div()
                            .p_2()
                            .bg(graphite_elevated())
                            .border_1()
                            .border_color(graphite_border())
                            .rounded(px(3.0))
                            .flex()
                            .flex_col()
                            .gap_1()
                            .child(compact_input_row("COOKIE", cookie_input))
                            .child(compact_input_row("REFERER", referer_input))
                            .child(compact_input_row("AGENT", user_agent_input)),
                    )
                }),
                )
                .child(
                    grid_modal_footer(GridTone::Graphite)
                        .child(grid_cell_button(
                            "pf-cancel",
                            Some(IconName::X),
                            Some("CANCEL"),
                            None,
                            GridTone::Graphite,
                            CellBorder::Left,
                            false,
                            move |_, window, cx| on_close(window, cx),
                        ))
                        .when(show_add_paused, |el| {
                            el.child(grid_cell_button(
                                "pf-queue",
                                Some(IconName::Clock),
                                Some("ADD PAUSED"),
                                Some("Save to list in Paused state without starting transfer"),
                                GridTone::Graphite,
                                CellBorder::Left,
                                false,
                                move |_, window, cx| confirm_queue(false, window, cx),
                            ))
                        })
                        .child(grid_cell_button(
                            "pf-now",
                            Some(IconName::Download),
                            Some("DOWNLOAD NOW"),
                            None,
                            GridTone::Primary,
                            CellBorder::Left,
                            false,
                            move |_, window, cx| confirm_now(true, window, cx),
                        )),
                )
        }
    }
    .into_any_element()
}

pub(crate) fn render_file_op_modal<F1, F2, F3>(
    state: &FileOpModalState,
    on_cancel: F1,
    on_confirm: F2,
    on_copy_hash: F3,
) -> AnyElement
where
    F1: Fn(&mut Window, &mut App) + Clone + 'static,
    F2: Fn(&mut Window, &mut App) + 'static,
    F3: Fn(String, &mut Window, &mut App) + 'static,
{
    let on_backdrop_close = on_cancel.clone();
    let card = match state {
        FileOpModalState::Rename { input, error, .. } => file_op_form_card(
            "Rename Completed File",
            "New Filename",
            "Rename",
            IconName::Check,
            px(460.0),
            input,
            error.as_deref(),
            on_cancel,
            on_confirm,
        ),
        FileOpModalState::Move { input, error, .. } => file_op_form_card(
            "Move Completed File",
            "Destination Directory",
            "Move File",
            IconName::FolderOpen,
            px(480.0),
            input,
            error.as_deref(),
            on_cancel,
            on_confirm,
        ),
        FileOpModalState::Integrity {
            filename,
            digest,
            error,
        } => div()
            .w(px(520.0))
            .p_5()
            .bg(graphite_bg())
            .border_1()
            .border_color(graphite_border())
            .rounded(px(6.0))
            .flex()
            .flex_col()
            .gap_3()
            .child(
                div()
                    .text_size(TYPE_HEADING)
                    .font_weight(FontWeight::SEMIBOLD)
                    .text_color(silver_text())
                    .child("File Integrity (BLAKE3)"),
            )
            .child(
                div()
                    .font_family(MONO_FONT)
                    .text_size(TYPE_MICRO)
                    .text_color(silver_muted())
                    .truncate()
                    .child(filename.clone()),
            )
            .when_some(error.clone(), |el, err| {
                el.child(
                    div()
                        .font_family(MONO_FONT)
                        .text_size(TYPE_MICRO)
                        .text_color(errata_vermilion())
                        .child(err),
                )
            })
            .child(
                div()
                    .p_2p5()
                    .bg(table_header_bg())
                    .border_1()
                    .border_color(graphite_border())
                    .rounded(px(4.0))
                    .font_family(MONO_FONT)
                    .text_size(TYPE_MICRO)
                    .text_color(if digest.is_some() {
                        halide_emerald()
                    } else {
                        selenium_amber()
                    })
                    .child(
                        digest
                            .clone()
                            .unwrap_or_else(|| "Computing BLAKE3 digest...".to_string()),
                    ),
            )
            .child(
                grid_modal_footer(GridTone::Graphite)
                    .when_some(digest.clone(), |el, hash| {
                        el.child(grid_cell_button(
                            "hash-copy",
                            Some(IconName::Copy),
                            Some("COPY BLAKE3"),
                            None,
                            GridTone::Graphite,
                            CellBorder::Left,
                            false,
                            move |_, window, cx| {
                                on_copy_hash(hash.clone(), window, cx);
                            },
                        ))
                    })
                    .child(grid_cell_button(
                        "hash-close",
                        Some(IconName::Check),
                        Some("CLOSE"),
                        None,
                        GridTone::Primary,
                        CellBorder::Left,
                        false,
                        move |_, window, cx| on_cancel(window, cx),
                    )),
            ),
    };

    modal_backdrop(card, on_backdrop_close)
}

pub(crate) fn render_confirm_modal<F1, F2>(
    action: &ConfirmAction,
    on_cancel: F1,
    on_confirm: F2,
) -> AnyElement
where
    F1: Fn(&mut Window, &mut App) + Clone + 'static,
    F2: Fn(&mut Window, &mut App) + 'static,
{
    let (title, detail, confirm_label) = match action {
        ConfirmAction::DeleteDownload {
            filename,
            delete_file: true,
            ..
        } => (
            "Delete File & Entry",
            format!(
                "Permanently delete \"{filename}\" from disk and remove it from the list? This cannot be undone."
            ),
            "Delete File",
        ),
        ConfirmAction::DeleteDownload {
            filename,
            delete_file: false,
            ..
        } => (
            "Remove Download",
            format!(
                "Remove \"{filename}\" from the list? Partial progress will be discarded; completed files on disk are kept."
            ),
            "Remove",
        ),
        ConfirmAction::DeleteMultiple {
            ids,
            delete_file: true,
        } => (
            "Delete Selected Files",
            format!(
                "Permanently delete {} selected download(s) from disk and remove them from the list? This cannot be undone.",
                ids.len()
            ),
            "Delete Files",
        ),
        ConfirmAction::DeleteMultiple {
            ids,
            delete_file: false,
        } => (
            "Remove Selected Downloads",
            format!(
                "Remove {} selected download(s) from the list? Partial progress will be discarded; completed files on disk are kept.",
                ids.len()
            ),
            "Remove All",
        ),
        ConfirmAction::ClearCompleted { count } => (
            "Clear Finished Downloads",
            format!(
                "Remove {count} completed or cancelled entry(s) from the list? Downloaded files on disk are kept."
            ),
            "Clear List",
        ),
        ConfirmAction::OverwriteCollision { filename, .. } => (
            "File Already Exists",
            format!(
                "\"{filename}\" already exists in the destination directory. Overwrite the existing file?"
            ),
            "Overwrite",
        ),
    };

    let on_backdrop_close = on_cancel.clone();
    let card = div()
        .w(px(440.0))
        .p_5()
        .bg(graphite_bg())
        .border_1()
        .border_color(errata_vermilion())
        .rounded(px(6.0))
        .flex()
        .flex_col()
        .gap_3()
        .child(
            div()
                .flex()
                .items_center()
                .gap_2()
                .child(
                    Icon::new(IconName::TriangleAlert)
                        .size(px(16.0))
                        .text_color(errata_vermilion()),
                )
                .child(
                    div()
                        .text_size(TYPE_HEADING)
                        .line_height(px(22.0))
                        .font_weight(FontWeight::BOLD)
                        .text_color(silver_text())
                        .child(title),
                ),
        )
        .child(
            div()
                .text_size(TYPE_BODY)
                .line_height(px(18.0))
                .text_color(silver_muted())
                .child(detail),
        )
        .child(
            grid_modal_footer(GridTone::Graphite)
                .child(grid_cell_button(
                    "confirm-cancel",
                    Some(IconName::X),
                    Some("CANCEL"),
                    None,
                    GridTone::Graphite,
                    CellBorder::Left,
                    false,
                    move |_, window, cx| on_cancel(window, cx),
                ))
                .child(grid_cell_button(
                    "confirm-proceed",
                    Some(IconName::Trash),
                    Some(confirm_label),
                    None,
                    GridTone::Danger,
                    CellBorder::Left,
                    false,
                    move |_, window, cx| on_confirm(window, cx),
                )),
        );

    modal_backdrop(card, on_backdrop_close)
}

#[allow(clippy::too_many_arguments)]
fn file_op_form_card<F1, F2>(
    title: &'static str,
    input_label: &'static str,
    confirm_label: &'static str,
    confirm_icon: IconName,
    width: Pixels,
    input: &Entity<InputState>,
    error: Option<&str>,
    on_cancel: F1,
    on_confirm: F2,
) -> Div
where
    F1: Fn(&mut Window, &mut App) + 'static,
    F2: Fn(&mut Window, &mut App) + 'static,
{
    div()
        .w(width)
        .p_5()
        .bg(graphite_bg())
        .border_1()
        .border_color(graphite_border())
        .rounded(px(6.0))
        .flex()
        .flex_col()
        .gap_3()
        .child(
            div()
                .text_size(TYPE_HEADING)
                .font_weight(FontWeight::SEMIBOLD)
                .text_color(silver_text())
                .child(title),
        )
        .when_some(error.map(str::to_string), |el, err| {
            el.child(
                div()
                    .font_family(MONO_FONT)
                    .text_size(TYPE_MICRO)
                    .text_color(errata_vermilion())
                    .child(err),
            )
        })
        .child(labeled_input(input_label, input))
        .child(
            grid_modal_footer(GridTone::Graphite)
                .child(grid_cell_button(
                    "fileop-cancel",
                    Some(IconName::X),
                    Some("CANCEL"),
                    None,
                    GridTone::Graphite,
                    CellBorder::Left,
                    false,
                    move |_, window, cx| on_cancel(window, cx),
                ))
                .child(grid_cell_button(
                    "fileop-save",
                    Some(confirm_icon),
                    Some(confirm_label),
                    None,
                    GridTone::Primary,
                    CellBorder::Left,
                    false,
                    move |_, window, cx| on_confirm(window, cx),
                )),
        )
}

fn compact_input_row(label: &'static str, state: &Entity<InputState>) -> impl IntoElement {
    compact_input_row_with_action(label, state, None)
}

fn compact_input_row_with_action(
    label: &'static str,
    state: &Entity<InputState>,
    trailing_action: Option<AnyElement>,
) -> impl IntoElement {
    div()
        .w_full()
        .h(px(30.0))
        .bg(graphite_bg())
        .border_1()
        .border_color(graphite_border())
        .flex()
        .items_center()
        .child(
            div()
                .w(px(68.0))
                .h_full()
                .px_2()
                .bg(graphite_elevated())
                .border_r_1()
                .border_color(graphite_border())
                .flex()
                .items_center()
                .font_family(MONO_FONT)
                .text_size(TYPE_MICRO)
                .text_color(silver_muted())
                .child(label),
        )
        .child(
            div()
                .flex_1()
                .min_w_0()
                .px_1p5()
                .font_family(MONO_FONT)
                .text_size(TYPE_BODY)
                .child(Input::new(state).appearance(false)),
        )
        .when_some(trailing_action, |el, action| el.child(action))
}

pub(crate) fn labeled_input(label: &'static str, state: &Entity<InputState>) -> impl IntoElement {
    div().border_t_1().border_color(graphite_border()).child(
        grid_kv_row(
            label,
            160.0,
            div()
                .w_full()
                .font_family(MONO_FONT)
                .text_size(TYPE_BODY)
                .child(Input::new(state).appearance(false)),
            GridTone::Graphite,
        ),
    )
}

pub(crate) fn modal_backdrop<F>(card: impl IntoElement, on_close: F) -> AnyElement
where
    F: Fn(&mut Window, &mut App) + 'static,
{
    div()
        .id("modal-backdrop")
        .absolute()
        .inset_0()
        .occlude()
        .p_4()
        .bg(rgba(0x141413AA))
        .flex()
        .items_center()
        .justify_center()
        .on_mouse_down(MouseButton::Right, |_, _, cx| cx.stop_propagation())
        .on_scroll(|_, _, cx| cx.stop_propagation())
        .on_click(move |_, window, cx| on_close(window, cx))
        .child(
            div()
                .id("modal-card-stop")
                .max_w_full()
                .max_h_full()
                .on_mouse_down(MouseButton::Left, |_, _, cx| cx.stop_propagation())
                .on_mouse_down(MouseButton::Right, |_, _, cx| cx.stop_propagation())
                .on_click(|_, _, cx| cx.stop_propagation())
                .child(card)
                .with_animation(
                    "modal-card-enter",
                    Animation::new(Duration::from_millis(140)).with_easing(ease_out_quint()),
                    |el, delta| el.mt(px((1.0 - delta) * 8.0)).opacity(0.65 + 0.35 * delta),
                ),
        )
        .into_any_element()
}

#[cfg(test)]
mod tests {
    use super::split_filename_extension;

    #[test]
    fn splits_standard_and_edge_case_extensions() {
        assert_eq!(
            split_filename_extension("Silksong_v1.0.2832.rar"),
            ("Silksong_v1.0.2832".to_string(), Some(".rar".to_string()))
        );
        assert_eq!(
            split_filename_extension("README"),
            ("README".to_string(), None)
        );
        assert_eq!(
            split_filename_extension(".gitignore"),
            (".gitignore".to_string(), None)
        );
        assert_eq!(
            split_filename_extension("archive."),
            ("archive.".to_string(), None)
        );
    }
}
