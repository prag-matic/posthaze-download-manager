//! Pre-Flight Probe Confirmation Modal and Persisted Settings Modal.

use std::path::{Path, PathBuf};
use std::time::Duration;

use chronophoto_ui::components::{
    BadgeTone, CellBorder, GridBadge, GridCellButton, GridCheckOptionRow, GridCompactInputRow,
    GridInputBox, GridKvRow, GridModalFooter, GridProgressTrack, GridStatusDot, GridTone,
    ModalBackdrop, ModalPlate,
};
use chronophoto_ui::theme::ChronoAppExt;
use chronophoto_ui::tokens::{
    RADIUS_BADGE, RADIUS_CONTROL, TYPE_BODY, TYPE_HEADING, TYPE_MICRO,
};
use gpui_kit::assets::IconName;
use gpui_kit::component::input::InputState;
use gpui_kit::component::{Icon, Theme};
use gpui_kit::prelude::FluentBuilder;
use gpui_kit::*;
use posthaze_engine::{
    ChecksumAlgorithm, DeleteFileMode, DownloadCategory, DownloadId, DownloadRequest,
    DownloadState, FileChecksums, PreflightInfo, effective_category_dir, parse_expected_checksum,
};

use crate::curl_parser::mask_header_summary;
use crate::errors::describe_error;
use crate::view_model::format_bytes;

/// Action dispatched from the duplicate-URL banner inside Pre-Flight.
#[derive(Debug, Clone)]
pub(crate) enum DuplicateBannerAction {
    ShowExisting(DownloadId),
    ResumeExisting(DownloadId),
    OpenFile(PathBuf),
}

/// Existing queue entry matching the URL currently opened in Pre-Flight.
#[derive(Debug, Clone)]
pub(crate) struct DuplicateMatch {
    pub id: DownloadId,
    pub state: DownloadState,
    pub filename: String,
    pub output_path: Option<PathBuf>,
    pub file_exists: bool,
    pub bytes_downloaded: u64,
    pub content_length: Option<u64>,
}

/// Active confirmation prompt for destructive, collision-override, or duplicate-transfer actions.
pub(crate) enum ConfirmAction {
    DeleteDownload {
        id: DownloadId,
        filename: String,
        file_mode: Option<DeleteFileMode>,
        has_disk_file: bool,
        allow_mode_switch: bool,
    },
    DeleteMultiple {
        ids: Vec<DownloadId>,
        file_mode: Option<DeleteFileMode>,
        has_disk_file: bool,
        allow_mode_switch: bool,
    },
    ClearCompleted { count: usize },
    OverwriteCollision {
        request: Box<DownloadRequest>,
        start_immediately: bool,
        filename: String,
    },
    DuplicateOngoing {
        existing_id: DownloadId,
        existing_state: DownloadState,
        existing_filename: String,
        pending_request: Box<DownloadRequest>,
        start_immediately: bool,
        disambiguated_path: PathBuf,
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
#[allow(clippy::large_enum_variant)]
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
        default_dir: Box<PathBuf>,
        dir_dropdown_open: bool,
        categories: Vec<DownloadCategory>,
        selected_category_id: String,
        auto_matched_category_id: String,
        category_dropdown_open: bool,
        duplicate_match: Option<Box<DuplicateMatch>>,
        recent_folders: Vec<PathBuf>,
    },
}

/// Active modal for completed-file operations (`Rename`, `Move`, `Verify Checksum`).
pub(crate) enum FileOpModalState {
    Rename {
        id: DownloadId,
        input: Entity<InputState>,
        error: Option<String>,
    },
    Move {
        id: DownloadId,
        input: Entity<InputState>,
        default_dir: PathBuf,
        recent_folders: Vec<PathBuf>,
        dropdown_open: bool,
        error: Option<String>,
    },
    Integrity {
        filename: String,
        expected_input: Entity<InputState>,
        checksums: Option<FileChecksums>,
        server_digest: Option<(ChecksumAlgorithm, String)>,
        error: Option<String>,
    },
}

#[allow(clippy::too_many_arguments)]
pub(crate) fn render_preflight_card<F1, F2, F3, F4, F5, F6, F7, F8, F9, F10, F11, F12>(
    state: &PreflightModalState,
    cx: &App,
    show_add_paused: bool,
    auto_categorize_enabled: bool,
    on_close: F1,
    on_retry: F2,
    on_worker_delta: F3,
    on_confirm: F4,
    on_edit_url: F5,
    on_browse_dir: F6,
    on_toggle_ext_lock: F7,
    on_toggle_dir_dropdown: F8,
    on_pick_recent_dir: F9,
    on_open_duplicate: F10,
    on_toggle_category_dropdown: F11,
    on_pick_category: F12,
) -> AnyElement
where
    F1: Fn(&mut Window, &mut App) + Clone + 'static,
    F2: Fn(String, Vec<(String, String)>, &mut Window, &mut App) + 'static,
    F3: Fn(isize, &mut Window, &mut App) + Clone + 'static,
    F4: Fn(bool, &mut Window, &mut App) + Clone + 'static,
    F5: Fn(String, &mut Window, &mut App) + 'static,
    F6: Fn(&mut Window, &mut App) + Clone + 'static,
    F7: Fn(&mut Window, &mut App) + 'static,
    F8: Fn(&mut Window, &mut App) + 'static,
    F9: Fn(PathBuf, &mut Window, &mut App) + Clone + 'static,
    F10: Fn(DuplicateBannerAction, &mut Window, &mut App) + Clone + 'static,
    F11: Fn(&mut Window, &mut App) + 'static,
    F12: Fn(String, &mut Window, &mut App) + Clone + 'static,
{
    let palette = cx.chrono().palette;
    let chrome_bg = palette.chrome_bg.hsla();
    let chrome_elevated = palette.chrome_elevated.hsla();
    let chrome_border = palette.chrome_border.hsla();
    let chrome_text = palette.chrome_text.hsla();
    let chrome_muted = palette.chrome_muted.hsla();
    let surface_bg = palette.surface_bg.hsla();
    let surface_hover = palette.surface_hover.hsla();
    let input_bg = palette.input_bg.hsla();
    let track_bg = palette.track_bg.hsla();
    let accent = palette.accent.hsla();
    let info_color = palette.info.hsla();
    let success = palette.success.hsla();
    let danger = palette.danger.hsla();
    let mono = Theme::global(cx).mono_font_family.clone();

    match state {
        PreflightModalState::Probing { url, headers } => div()
            .w_full()
            .flex_1()
            .min_h_0()
            .flex()
            .flex_col()
            .child(
                div()
                    .w_full()
                    .h(px(3.0))
                    .bg(track_bg)
                    .overflow_hidden()
                    .child(
                        div()
                            .w(px(120.0))
                            .h_full()
                            .bg(accent)
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
                    .bg(surface_bg)
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
                                    .font_weight(FontWeight::SEMIBOLD)
                                    .text_color(chrome_text)
                                    .child("Probing target"),
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
                                                    .size(px(14.0))
                                                    .text_color(accent),
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
                                            .font_family(mono.clone())
                                            .text_size(TYPE_MICRO)
                                            .text_color(accent)
                                            .child("HEAD / RANGE"),
                                    ),
                            ),
                    )
                    .child(
                        div()
                            .font_family(mono.clone())
                            .text_size(TYPE_MICRO)
                            .line_height(px(15.0))
                            .text_color(chrome_muted)
                            .truncate()
                            .child(url.clone()),
                    )
                    .when(!headers.is_empty(), |el| {
                        el.child(
                            div()
                                .font_family(mono)
                                .text_size(TYPE_MICRO)
                                .text_color(chrome_muted)
                                .child(format!("Sending {} custom header(s)", headers.len())),
                        )
                    }),
            )
            .child(
                div().bg(chrome_bg).child(
                    GridModalFooter::new()
                        .tone(GridTone::Chrome)
                        .child(
                            GridCellButton::new("probe-cancel")
                                .icon(IconName::X)
                                .label("Cancel")
                                .tone(GridTone::Chrome)
                                .border(CellBorder::Left)
                                .on_click(move |_, window, cx| on_close(window, cx)),
                        ),
                ),
            ),
        PreflightModalState::Error {
            url,
            headers,
            message,
        } => {
            let url_retry = url.clone();
            let headers_retry = headers.clone();
            let url_edit = url.clone();
            let diag = describe_error(message, false);
            div()
                .w_full()
                .flex_1()
                .min_h_0()
                .border_t_2()
                .border_color(danger)
                .flex()
                .flex_col()
                .child(
                    div()
                        .w_full()
                        .flex_1()
                        .min_h_0()
                        .p_3()
                        .bg(surface_bg)
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
                                        .text_color(danger),
                                )
                                .child(
                                    div()
                                        .text_size(px(14.0))
                                        .line_height(px(18.0))
                                        .font_weight(FontWeight::SEMIBOLD)
                                        .text_color(danger)
                                        .child(diag.headline),
                                ),
                        )
                        .child(
                            div()
                                .font_family(mono.clone())
                                .text_size(TYPE_BODY)
                                .line_height(px(16.0))
                                .text_color(chrome_text)
                                .child(diag.detail),
                        )
                        .child(
                            div()
                                .font_family(mono)
                                .text_size(TYPE_MICRO)
                                .line_height(px(14.0))
                                .text_color(chrome_muted)
                                .truncate()
                                .child(url.clone()),
                        )
                        .when_some(diag.tip, |el, tip| {
                            el.child(
                                div()
                                    .p_2()
                                    .bg(chrome_elevated)
                                    .border_1()
                                    .border_color(chrome_border)
                                    .rounded(RADIUS_BADGE)
                                    .text_size(TYPE_MICRO)
                                    .line_height(px(14.0))
                                    .text_color(chrome_muted)
                                    .child(tip),
                            )
                        }),
                )
                .child(
                    div().bg(chrome_bg).child(
                        GridModalFooter::new()
                            .tone(GridTone::Chrome)
                            .child(
                                GridCellButton::new("probe-edit")
                                    .icon(IconName::Clipboard)
                                    .label("Edit URL")
                                    .tone(GridTone::Chrome)
                                    .border(CellBorder::Left)
                                    .on_click(move |_, window, cx| {
                                        on_edit_url(url_edit.clone(), window, cx);
                                    }),
                            )
                            .child(
                                GridCellButton::new("probe-close")
                                    .icon(IconName::X)
                                    .label("CLOSE")
                                    .tone(GridTone::Chrome)
                                    .border(CellBorder::Left)
                                    .on_click(move |_, window, cx| on_close(window, cx)),
                            )
                            .child(
                                GridCellButton::new("probe-retry")
                                    .icon(IconName::RefreshCw)
                                    .label("RETRY")
                                    .tone(GridTone::Primary)
                                    .border(CellBorder::Left)
                                    .on_click(move |_, window, cx| {
                                        on_retry(
                                            url_retry.clone(),
                                            headers_retry.clone(),
                                            window,
                                            cx,
                                        );
                                    }),
                            ),
                    ),
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
            default_dir,
            dir_dropdown_open,
            categories,
            selected_category_id,
            auto_matched_category_id,
            category_dropdown_open,
            duplicate_match,
            recent_folders,
        } => {
            let current_dir_text = dir_input.read(cx).value();
            let active_category = categories
                .iter()
                .find(|c| c.id == *selected_category_id)
                .or_else(|| categories.first());
            let active_cat_name = active_category
                .map(|c| c.name.clone())
                .unwrap_or_else(|| "General".to_string());
            let is_auto_matched =
                auto_categorize_enabled && selected_category_id == auto_matched_category_id;
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
                .flex()
                .flex_col()
                .child(
                    div()
                        .w_full()
                        .flex_1()
                        .min_h_0()
                        .px_4()
                        .py_3()
                        .bg(surface_bg)
                        .flex()
                        .flex_col()
                .when(*active_tab == PreflightTab::General, |el| {
                    el.child(
                        div()
                            .flex()
                            .flex_col()
                            .gap_0p5()
                            .mb_3()
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
                                            .text_color(chrome_text)
                                            .child("Add Download"),
                                    )
                                    .child(
                                        GridBadge::new(proto_label).tone(if info.supports_ranges {
                                            BadgeTone::Neutral
                                        } else {
                                            BadgeTone::Muted
                                        }),
                                    ),
                            )
                            .child(
                                div()
                                    .font_family(mono.clone())
                                    .text_size(TYPE_MICRO)
                                    .line_height(px(14.0))
                                    .text_color(chrome_muted)
                                    .truncate()
                                    .child(info.final_url.clone()),
                            ),
                    )
                    .child(
                        div()
                            .w_full()
                            .flex()
                            .flex_col()
                            .gap_1p5()
                            .child(compact_input_row_with_action(
                                "FILE",
                                filename_input,
                                Some(
                                    div()
                                        .id("pf-ext-lock")
                                        .h_full()
                                        .px_2p5()
                                        .border_l_1()
                                        .border_color(chrome_border)
                                        .flex()
                                        .items_center()
                                        .justify_center()
                                        .font_family(mono.clone())
                                        .text_size(TYPE_MICRO)
                                        .font_weight(FontWeight::MEDIUM)
                                        .cursor_pointer()
                                        .when(ext_is_locked && ext_has_value, |el| {
                                            el.bg(chrome_elevated)
                                                .text_color(accent)
                                                .hover(move |s| s.bg(surface_hover))
                                        })
                                        .when(!(ext_is_locked && ext_has_value), |el| {
                                            el.bg(chrome_bg)
                                                .text_color(chrome_muted)
                                                .hover(move |s| {
                                                    s.bg(surface_hover).text_color(chrome_text)
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
                            .child(
                                div()
                                    .w_full()
                                    .flex()
                                    .flex_col()
                                    .child(
                                        div()
                                            .id("pf-category-row")
                                            .h(px(30.0))
                                            .w_full()
                                            .bg(input_bg)
                                            .border_1()
                                            .border_color(chrome_border)
                                            .flex()
                                            .items_center()
                                            .overflow_hidden()
                                            .child(
                                                div()
                                                    .w(px(68.0))
                                                    .flex_shrink_0()
                                                    .h_full()
                                                    .px_2()
                                                    .bg(chrome_elevated)
                                                    .border_r_1()
                                                    .border_color(chrome_border)
                                                    .flex()
                                                    .items_center()
                                                    .font_family(mono.clone())
                                                    .text_size(TYPE_MICRO)
                                                    .text_color(chrome_muted)
                                                    .child("CATEGORY"),
                                            )
                                            .child(
                                                div()
                                                    .id("pf-cat-trigger")
                                                    .flex_1()
                                                    .min_w_0()
                                                    .h_full()
                                                    .px_2()
                                                    .flex()
                                                    .items_center()
                                                    .justify_between()
                                                    .gap_2()
                                                    .cursor_pointer()
                                                    .hover(move |s| s.bg(surface_hover))
                                                    .on_click(move |_, window, cx| {
                                                        on_toggle_category_dropdown(window, cx)
                                                    })
                                                    .child(
                                                        div()
                                                            .flex()
                                                            .items_center()
                                                            .gap_2()
                                                            .min_w_0()
                                                            .child(
                                                                div()
                                                                    .text_size(TYPE_BODY)
                                                                    .font_weight(FontWeight::MEDIUM)
                                                                    .text_color(chrome_text)
                                                                    .truncate()
                                                                    .child(active_cat_name),
                                                            )
                                                            .when(is_auto_matched, |el| {
                                                                el.child(
                                                                    GridBadge::new("AUTO")
                                                                        .tone(BadgeTone::Accent),
                                                                )
                                                            }),
                                                    )
                                                    .child(
                                                        Icon::new(if *category_dropdown_open {
                                                            IconName::ChevronUp
                                                        } else {
                                                            IconName::ChevronDown
                                                        })
                                                        .size(px(13.0))
                                                        .text_color(chrome_muted),
                                                    ),
                                            ),
                                    )
                                    .when(*category_dropdown_open, |el| {
                                        el.child(render_category_dropdown_panel(
                                            categories,
                                            selected_category_id,
                                            default_dir.as_path(),
                                            cx,
                                            on_pick_category.clone(),
                                        ))
                                    }),
                            )
                            .child(
                                div()
                                    .w_full()
                                    .flex()
                                    .flex_col()
                                    .child(compact_input_row_with_action(
                                        "SAVE TO",
                                        dir_input,
                                        Some(
                                            div()
                                                .h_full()
                                                .flex()
                                                .items_center()
                                                .child(
                                                    GridCellButton::new("pf-dir-dropdown")
                                                        .icon(if *dir_dropdown_open {
                                                            IconName::ChevronUp
                                                        } else {
                                                            IconName::ChevronDown
                                                        })
                                                        .tooltip("Choose default or recent save folder")
                                                        .tone(GridTone::Chrome)
                                                        .border(CellBorder::Left)
                                                        .compact(true)
                                                        .on_click(move |_, window, cx| {
                                                            on_toggle_dir_dropdown(window, cx)
                                                        }),
                                                )
                                                .child(
                                                    GridCellButton::new("pf-browse-dir")
                                                        .icon(IconName::FolderOpen)
                                                        .tooltip("Choose another save folder")
                                                        .tone(GridTone::Chrome)
                                                        .border(CellBorder::Left)
                                                        .compact(true)
                                                        .on_click(move |_, window, cx| {
                                                            on_browse_dir(window, cx)
                                                        }),
                                                )
                                                .into_any_element(),
                                        ),
                                    ))
                                    .when(*dir_dropdown_open, |el| {
                                        el.child(render_folder_dropdown_panel(
                                            "pf-dir-menu",
                                            default_dir.as_path(),
                                            recent_folders,
                                            &current_dir_text,
                                            cx,
                                            on_pick_recent_dir.clone(),
                                        ))
                                    }),
                            ),
                    )
                    .when_some(duplicate_match.clone(), |el, dup| {
                        let dup_id = dup.id.clone();
                        let is_failed = dup.state == DownloadState::Failed;
                        let is_paused = dup.state == DownloadState::Paused;
                        let is_completed_present =
                            dup.state == DownloadState::Completed && dup.file_exists;
                        let progress_note = match dup.content_length {
                            Some(total) if total > 0 && dup.state != DownloadState::Completed => {
                                format!(" ({}/{})", format_bytes(dup.bytes_downloaded), format_bytes(total))
                            }
                            Some(total) if total > 0 => format!(" ({})", format_bytes(total)),
                            _ if dup.bytes_downloaded > 0 => format!(" ({})", format_bytes(dup.bytes_downloaded)),
                            _ => String::new(),
                        };
                        let banner_text = match dup.state {
                            DownloadState::Completed if dup.file_exists => {
                                format!("Already downloaded{progress_note}: {}", dup.filename)
                            }
                            DownloadState::Completed => {
                                format!("Downloaded earlier (file moved/deleted): {}", dup.filename)
                            }
                            DownloadState::Active | DownloadState::Probing | DownloadState::Queued => {
                                format!("Transfer {} {progress_note}: {}", dup.state.as_str(), dup.filename)
                            }
                            _ => format!("Already in queue ({}){progress_note}: {}", dup.state.as_str(), dup.filename),
                        };
                        let tip_text = match &dup.output_path {
                            Some(path) if dup.state == DownloadState::Completed && !dup.file_exists => {
                                format!("Recorded path missing on disk: {}", path.to_string_lossy())
                            }
                            Some(path) => format!("Existing path: {}", path.to_string_lossy()),
                            None => format!("Matched queue entry {}", dup.id.as_str()),
                        };
                        let dot_color = if is_failed {
                            danger
                        } else if is_completed_present {
                            success
                        } else {
                            accent
                        };
                        let on_dup_primary = on_open_duplicate.clone();
                        let on_dup_secondary = on_open_duplicate.clone();
                        let primary_action = if is_completed_present {
                            dup.output_path
                                .clone()
                                .map(DuplicateBannerAction::OpenFile)
                                .unwrap_or_else(|| DuplicateBannerAction::ShowExisting(dup_id.clone()))
                        } else if is_paused || is_failed {
                            DuplicateBannerAction::ResumeExisting(dup_id.clone())
                        } else {
                            DuplicateBannerAction::ShowExisting(dup_id.clone())
                        };
                        let primary_label = if is_completed_present {
                            "OPEN FILE"
                        } else if is_paused {
                            "RESUME"
                        } else if is_failed {
                            "RETRY"
                        } else {
                            "SHOW"
                        };
                        el.child(
                            div()
                                .id("pf-dup-banner")
                                .mt_2()
                                .px_2p5()
                                .py_1p5()
                                .rounded(RADIUS_BADGE)
                                .bg(chrome_elevated)
                                .border_1()
                                .border_color(chrome_border)
                                .flex()
                                .items_center()
                                .justify_between()
                                .gap_2()
                                .tooltip(move |window, cx| {
                                    gpui_kit::component::tooltip::Tooltip::new(tip_text.clone())
                                        .build(window, cx)
                                })
                                .child(
                                    div()
                                        .flex_1()
                                        .min_w_0()
                                        .flex()
                                        .items_center()
                                        .gap_2()
                                        .child(GridStatusDot::new(dot_color).size(6.0).round(true))
                                        .child(
                                            div()
                                                .flex_1()
                                                .min_w_0()
                                                .font_family(mono.clone())
                                                .text_size(TYPE_MICRO)
                                                .text_color(chrome_text)
                                                .truncate()
                                                .child(banner_text),
                                        ),
                                )
                                .when(is_completed_present, |row| {
                                    let show_id = dup_id.clone();
                                    row.child(
                                        div()
                                            .id("pf-show-duplicate-secondary")
                                            .px_1p5()
                                            .py_0p5()
                                            .rounded(px(2.0))
                                            .font_family(mono.clone())
                                            .text_size(TYPE_MICRO)
                                            .text_color(chrome_muted)
                                            .cursor_pointer()
                                            .hover(move |s| s.text_color(chrome_text))
                                            .on_click(move |_, window, cx| {
                                                on_dup_secondary(
                                                    DuplicateBannerAction::ShowExisting(
                                                        show_id.clone(),
                                                    ),
                                                    window,
                                                    cx,
                                                );
                                            })
                                            .child("SHOW"),
                                    )
                                })
                                .child(
                                    div()
                                        .id("pf-open-duplicate")
                                        .px_2()
                                        .py_0p5()
                                        .rounded(px(2.0))
                                        .bg(chrome_bg)
                                        .border_1()
                                        .border_color(chrome_border)
                                        .font_family(mono.clone())
                                        .text_size(TYPE_MICRO)
                                        .font_weight(FontWeight::MEDIUM)
                                        .text_color(chrome_text)
                                        .cursor_pointer()
                                        .hover(move |s| {
                                            s.bg(surface_hover).border_color(accent)
                                        })
                                        .on_click(move |_, window, cx| {
                                            on_dup_primary(primary_action.clone(), window, cx);
                                        })
                                        .child(primary_label),
                                ),
                        )
                    })
                })
                .when(*active_tab == PreflightTab::Options, |el| {
                    el.child(
                        div()
                            .w_full()
                            .flex()
                            .flex_col()
                            .gap_1()
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
                                            .gap_2()
                                            .font_family(mono.clone())
                                            .text_size(TYPE_MICRO)
                                            .child(
                                                div().text_color(chrome_text).child(
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
                                                        accent
                                                    } else {
                                                        chrome_muted
                                                    })
                                                    .child(segment_meta),
                                            ),
                                    )
                                    .child(
                                        div()
                                            .h(px(24.0))
                                            .border_1()
                                            .border_color(chrome_border)
                                            .flex()
                                            .items_center()
                                            .when(info.supports_ranges, |el| {
                                                let dec = on_worker_delta.clone();
                                                el.child(
                                                    GridCellButton::new("w-dec")
                                                        .icon(IconName::Minus)
                                                        .tooltip("Decrease workers")
                                                        .tone(GridTone::Chrome)
                                                        .border(CellBorder::Right)
                                                        .compact(true)
                                                        .on_click(move |_, window, cx| {
                                                            dec(-1, window, cx)
                                                        }),
                                                )
                                            })
                                            .child(
                                                div()
                                                    .w(px(28.0))
                                                    .text_center()
                                                    .font_family(mono.clone())
                                                    .text_size(TYPE_BODY)
                                                    .text_color(chrome_text)
                                                    .child(workers.to_string()),
                                            )
                                            .when(info.supports_ranges, |el| {
                                                el.child(
                                                    GridCellButton::new("w-inc")
                                                        .icon(IconName::Plus)
                                                        .tooltip("Increase workers")
                                                        .tone(GridTone::Chrome)
                                                        .border(CellBorder::Left)
                                                        .compact(true)
                                                        .on_click(move |_, window, cx| {
                                                            on_worker_delta(1, window, cx)
                                                        }),
                                                )
                                            }),
                                    ),
                            )
                            .child(
                                div()
                                    .child(
                                        GridProgressTrack::new()
                                            .height(px(8.0))
                                            .border_color(chrome_border)
                                            .children((0..segment_count).map(|idx| {
                                                let fill = if !info.supports_ranges {
                                                    chrome_muted.opacity(0.45)
                                                } else if idx == 0 {
                                                    accent
                                                } else {
                                                    info_color.opacity(0.85)
                                                };
                                                div()
                                                    .flex_1()
                                                    .h_full()
                                                    .bg(fill)
                                                    .when(idx > 0, |el| {
                                                        el.border_l_1().border_color(track_bg)
                                                    })
                                            })),
                                    )
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
                            .pt_1p5()
                            .flex()
                            .items_center()
                            .justify_between()
                            .child(
                                div()
                                    .font_family(mono)
                                    .text_size(TYPE_MICRO)
                                    .text_color(chrome_muted)
                                    .child("REQUEST HEADERS"),
                            )
                            .when_some(header_summary, |el, summary| {
                                el.child(
                                    div()
                                        .id("preflight-headers-badge")
                                        .tooltip(move |window, cx| {
                                            gpui_kit::component::tooltip::Tooltip::new(
                                                summary.clone(),
                                            )
                                            .build(window, cx)
                                        })
                                        .child(GridBadge::new("cURL").tone(BadgeTone::Accent)),
                                )
                            }),
                    )
                    .child(compact_input_row("COOKIE", cookie_input))
                    .child(compact_input_row("REFERER", referer_input))
                    .child(compact_input_row("AGENT", user_agent_input))
                }),
                )
                .child(div().w_full().h(px(1.0)).bg(chrome_border))
                .child(
                    div()
                        .h(px(34.0))
                        .w_full()
                        .bg(chrome_bg)
                        .flex()
                        .justify_end()
                        .child(
                            GridCellButton::new("pf-cancel")
                                .icon(IconName::X)
                                .label("Cancel")
                                .tone(GridTone::Chrome)
                                .border(CellBorder::Left)
                                .on_click(move |_, window, cx| on_close(window, cx)),
                        )
                        .when(show_add_paused, |el| {
                            el.child(
                                GridCellButton::new("pf-queue")
                                    .icon(IconName::Clock)
                                    .label("ADD PAUSED")
                                    .tooltip(
                                        "Save to list in Paused state without starting transfer",
                                    )
                                    .tone(GridTone::Chrome)
                                    .border(CellBorder::Left)
                                    .on_click(move |_, window, cx| confirm_queue(false, window, cx)),
                            )
                        })
                        .child(
                            GridCellButton::new("pf-now")
                                .icon(IconName::Download)
                                .label("DOWNLOAD NOW")
                                .tone(GridTone::Primary)
                                .border(CellBorder::Left)
                                .on_click(move |_, window, cx| confirm_now(true, window, cx)),
                        ),
                )
        }
    }
    .into_any_element()
}

#[allow(clippy::too_many_arguments)]
pub(crate) fn render_file_op_modal<F1, F2, F3, F4, F5, F6, F7>(
    state: &FileOpModalState,
    cx: &App,
    on_cancel: F1,
    on_confirm: F2,
    on_copy_hash: F3,
    on_paste_expected: F4,
    on_toggle_move_dropdown: F5,
    on_pick_move_dir: F6,
    on_browse_move_dir: F7,
) -> AnyElement
where
    F1: Fn(&mut Window, &mut App) + Clone + 'static,
    F2: Fn(&mut Window, &mut App) + 'static,
    F3: Fn(&'static str, String, &mut Window, &mut App) + Clone + 'static,
    F4: Fn(&mut Window, &mut App) + 'static,
    F5: Fn(&mut Window, &mut App) + 'static,
    F6: Fn(PathBuf, &mut Window, &mut App) + Clone + 'static,
    F7: Fn(&mut Window, &mut App) + Clone + 'static,
{
    let palette = cx.chrono().palette;
    let chrome_elevated = palette.chrome_elevated.hsla();
    let chrome_border = palette.chrome_border.hsla();
    let chrome_text = palette.chrome_text.hsla();
    let chrome_muted = palette.chrome_muted.hsla();
    let surface_header_bg = palette.surface_header_bg.hsla();
    let accent = palette.accent.hsla();
    let success = palette.success.hsla();
    let danger = palette.danger.hsla();
    let mono = Theme::global(cx).mono_font_family.clone();

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
            cx,
            on_cancel,
            on_confirm,
        ),
        FileOpModalState::Move {
            input,
            default_dir,
            recent_folders,
            dropdown_open,
            error,
            ..
        } => {
            let current_dir_text = input.read(cx).value();
            let footer = GridModalFooter::new()
                .tone(GridTone::Chrome)
                .child(
                    GridCellButton::new("fileop-cancel")
                        .icon(IconName::X)
                        .label("CANCEL")
                        .tone(GridTone::Chrome)
                        .border(CellBorder::Left)
                        .on_click(move |_, window, cx| on_cancel(window, cx)),
                )
                .child(
                    GridCellButton::new("fileop-save")
                        .icon(IconName::FolderOpen)
                        .label("Move File")
                        .tone(GridTone::Primary)
                        .border(CellBorder::Left)
                        .on_click(move |_, window, cx| on_confirm(window, cx)),
                );
            ModalPlate::new(px(520.0))
                .border_color(chrome_border)
                .child(
                    div()
                        .text_size(TYPE_HEADING)
                        .font_weight(FontWeight::SEMIBOLD)
                        .text_color(chrome_text)
                        .child("Move Completed File"),
                )
                .when_some(error.as_deref().map(str::to_string), |el, err| {
                    el.child(
                        div()
                            .font_family(mono.clone())
                            .text_size(TYPE_MICRO)
                            .text_color(danger)
                            .child(err),
                    )
                })
                .child(
                    div()
                        .w_full()
                        .flex()
                        .flex_col()
                        .child(compact_input_row_with_action(
                            "SAVE TO",
                            input,
                            Some(
                                div()
                                    .h_full()
                                    .flex()
                                    .items_center()
                                    .child(
                                        GridCellButton::new("move-dir-dropdown")
                                            .icon(if *dropdown_open {
                                                IconName::ChevronUp
                                            } else {
                                                IconName::ChevronDown
                                            })
                                            .tooltip("Choose default or recent save folder")
                                            .tone(GridTone::Chrome)
                                            .border(CellBorder::Left)
                                            .compact(true)
                                            .on_click(move |_, window, cx| {
                                                on_toggle_move_dropdown(window, cx)
                                            }),
                                    )
                                    .child(
                                        GridCellButton::new("move-browse-dir")
                                            .icon(IconName::FolderOpen)
                                            .tooltip("Choose another save folder")
                                            .tone(GridTone::Chrome)
                                            .border(CellBorder::Left)
                                            .compact(true)
                                            .on_click(move |_, window, cx| {
                                                on_browse_move_dir(window, cx)
                                            }),
                                    )
                                    .into_any_element(),
                            ),
                        ))
                        .when(*dropdown_open, |el| {
                            el.child(render_folder_dropdown_panel(
                                "move-dir-menu",
                                default_dir,
                                recent_folders,
                                &current_dir_text,
                                cx,
                                on_pick_move_dir,
                            ))
                        }),
                )
                .footer(footer)
        }
        FileOpModalState::Integrity {
            filename,
            expected_input,
            checksums,
            server_digest,
            error,
        } => {
            let expected_raw = expected_input.read(cx).value();
            let parsed_expected = parse_expected_checksum(&expected_raw)
                .or_else(|| server_digest.as_ref().map(|(a, h)| (Some(*a), h.clone())));
            let verdict = match (checksums.as_ref(), parsed_expected.as_ref()) {
                (Some(sums), Some((algo, hex))) => match sums.match_expected(*algo, hex) {
                    Some(matched) => Some((true, format!("MATCH ({})", matched.label()))),
                    None => Some((
                        false,
                        format!(
                            "MISMATCH ({})",
                            algo.map_or("SHA-256 / BLAKE3", |a| a.label())
                        ),
                    )),
                },
                (None, Some(_)) => None,
                _ if !expected_raw.trim().is_empty() => {
                    Some((false, "Unrecognized checksum format (expected 8/32/40/64/128 hex)".to_string()))
                }
                _ => None,
            };

            let copy_row = |label: &'static str, value: String, id_key: &'static str| {
                let on_copy = on_copy_hash.clone();
                let value_for_copy = value.clone();
                div()
                    .bg(surface_header_bg)
                    .border_1()
                    .border_color(chrome_border)
                    .rounded(RADIUS_CONTROL)
                    .overflow_hidden()
                    .flex()
                    .items_stretch()
                    .child(
                        div()
                            .flex_1()
                            .min_w_0()
                            .px_2p5()
                            .py_1p5()
                            .flex()
                            .flex_col()
                            .gap_0p5()
                            .child(
                                div()
                                    .font_family(mono.clone())
                                    .text_size(TYPE_MICRO)
                                    .text_color(chrome_muted)
                                    .child(label),
                            )
                            .child(
                                div()
                                    .font_family(mono.clone())
                                    .text_size(TYPE_MICRO)
                                    .text_color(success)
                                    .truncate()
                                    .child(value),
                            ),
                    )
                    .child(
                        GridCellButton::new(id_key)
                            .icon(IconName::Copy)
                            .label("COPY")
                            .tone(GridTone::Chrome)
                            .border(CellBorder::Left)
                            .on_click(move |_, window, cx| {
                                on_copy(label, value_for_copy.clone(), window, cx);
                            }),
                    )
            };

            let footer = GridModalFooter::new().tone(GridTone::Chrome).child(
                GridCellButton::new("hash-close")
                    .icon(IconName::Check)
                    .label("CLOSE")
                    .tone(GridTone::Primary)
                    .border(CellBorder::Left)
                    .on_click(move |_, window, cx| on_cancel(window, cx)),
            );
            ModalPlate::new(px(580.0))
                .border_color(chrome_border)
                .child(
                    div()
                        .flex()
                        .items_center()
                        .justify_between()
                        .child(
                            div()
                                .text_size(TYPE_HEADING)
                                .font_weight(FontWeight::SEMIBOLD)
                                .text_color(chrome_text)
                                .child("Verify Checksum / File Integrity"),
                        )
                        .when_some(verdict, |el, (matched, text)| {
                            el.child(GridBadge::new(text).tone(if matched {
                                BadgeTone::Success
                            } else {
                                BadgeTone::Danger
                            }))
                        }),
                )
                .child(
                    div()
                        .font_family(mono.clone())
                        .text_size(TYPE_MICRO)
                        .text_color(chrome_muted)
                        .truncate()
                        .child(filename.clone()),
                )
                .child(compact_input_row_with_action(
                    "EXPECT",
                    expected_input,
                    Some(
                        GridCellButton::new("hash-paste-expected")
                            .icon(IconName::Clipboard)
                            .label("PASTE")
                            .tooltip("Paste expected SHA-256 / SHA-512 / MD5 / CRC32 / BLAKE3 hash")
                            .tone(GridTone::Chrome)
                            .border(CellBorder::Left)
                            .on_click(move |_, window, cx| on_paste_expected(window, cx))
                            .into_any_element(),
                    ),
                ))
                .when_some(error.clone(), |el, err| {
                    el.child(
                        div()
                            .font_family(mono.clone())
                            .text_size(TYPE_MICRO)
                            .text_color(danger)
                            .child(err),
                    )
                })
                .when_some(checksums.clone(), |el, sums| {
                    el.child(
                        div()
                            .flex()
                            .flex_col()
                            .gap_1p5()
                            .child(copy_row("SHA-256", sums.sha256, "copy-sha256"))
                            .child(copy_row("BLAKE3", sums.blake3, "copy-blake3"))
                            .child(copy_row("MD5", sums.md5, "copy-md5"))
                            .child(copy_row("CRC32", sums.crc32, "copy-crc32")),
                    )
                })
                .when(checksums.is_none() && error.is_none(), |el| {
                    el.child(
                        div()
                            .p_2p5()
                            .bg(surface_header_bg)
                            .border_1()
                            .border_color(chrome_border)
                            .rounded(RADIUS_CONTROL)
                            .font_family(mono)
                            .text_size(TYPE_MICRO)
                            .text_color(accent)
                            .child("Computing SHA-256, BLAKE3, SHA-512, MD5, and CRC32 digests..."),
                    )
                })
                .footer(footer)
        }
    };

    let _ = chrome_elevated;
    ModalBackdrop::new(card)
        .on_close(move |window, cx| on_backdrop_close(window, cx))
        .into_any_element()
}

#[allow(clippy::too_many_arguments)]
pub(crate) fn render_confirm_modal<F1, F2, F3, F4>(
    action: &ConfirmAction,
    cx: &App,
    on_cancel: F1,
    on_confirm: F2,
    on_change_file_mode: F3,
    on_alt_confirm: F4,
) -> AnyElement
where
    F1: Fn(&mut Window, &mut App) + Clone + 'static,
    F2: Fn(&mut Window, &mut App) + 'static,
    F3: Fn(Option<DeleteFileMode>, &mut Window, &mut App) + Clone + 'static,
    F4: Fn(&mut Window, &mut App) + 'static,
{
    let (title, detail, confirm_label) = match action {
        ConfirmAction::DeleteDownload {
            filename,
            file_mode: Some(DeleteFileMode::RecycleBin),
            ..
        } => (
            "Move File to Recycle Bin",
            format!(
                "Move \"{filename}\" to the Windows Recycle Bin and remove it from the list?"
            ),
            "Move to Recycle Bin",
        ),
        ConfirmAction::DeleteDownload {
            filename,
            file_mode: Some(DeleteFileMode::Permanent),
            ..
        } => (
            "Permanently Delete File",
            format!(
                "Permanently delete \"{filename}\" from disk (bypassing Recycle Bin)? This cannot be undone."
            ),
            "Delete Permanently",
        ),
        ConfirmAction::DeleteDownload {
            filename,
            file_mode: None,
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
            file_mode: Some(DeleteFileMode::RecycleBin),
            ..
        } => (
            "Move Selected Files to Recycle Bin",
            format!(
                "Move {} selected download(s) to the Windows Recycle Bin and remove them from the list?",
                ids.len()
            ),
            "Move to Recycle Bin",
        ),
        ConfirmAction::DeleteMultiple {
            ids,
            file_mode: Some(DeleteFileMode::Permanent),
            ..
        } => (
            "Permanently Delete Selected Files",
            format!(
                "Permanently delete {} selected download(s) from disk (bypassing Recycle Bin)? This cannot be undone.",
                ids.len()
            ),
            "Delete Permanently",
        ),
        ConfirmAction::DeleteMultiple {
            ids,
            file_mode: None,
            ..
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
        ConfirmAction::DuplicateOngoing {
            existing_state,
            existing_filename,
            disambiguated_path,
            ..
        } => {
            let new_name = disambiguated_path
                .file_name()
                .and_then(|n| n.to_str())
                .unwrap_or("separate copy");
            let is_paused = *existing_state == DownloadState::Paused;
            (
                if is_paused {
                    "Paused Transfer Already Exists"
                } else {
                    "Download Already in Progress"
                },
                format!(
                    "\"{existing_filename}\" is currently {} in your queue. {} it instead, or save a separate copy as \"{new_name}\"?",
                    existing_state.as_str(),
                    if is_paused { "Resume" } else { "Show" },
                ),
                if is_paused {
                    "RESUME EXISTING"
                } else {
                    "SHOW ONGOING"
                },
            )
        }
    };

    let (active_mode, show_mode_selector) = match action {
        ConfirmAction::DeleteDownload {
            file_mode,
            has_disk_file,
            allow_mode_switch,
            ..
        }
        | ConfirmAction::DeleteMultiple {
            file_mode,
            has_disk_file,
            allow_mode_switch,
            ..
        } => (*file_mode, *has_disk_file && *allow_mode_switch),
        ConfirmAction::ClearCompleted { .. }
        | ConfirmAction::OverwriteCollision { .. }
        | ConfirmAction::DuplicateOngoing { .. } => (None, false),
    };

    let palette = cx.chrono().palette;
    let chrome_elevated = palette.chrome_elevated.hsla();
    let chrome_border = palette.chrome_border.hsla();
    let chrome_text = palette.chrome_text.hsla();
    let chrome_muted = palette.chrome_muted.hsla();
    let accent = palette.accent.hsla();
    let accent_fg = palette.accent_foreground.hsla();
    let danger = palette.danger.hsla();
    let danger_fg = palette.danger_foreground.hsla();
    let mono = Theme::global(cx).mono_font_family.clone();

    let is_duplicate_ongoing = matches!(action, ConfirmAction::DuplicateOngoing { .. });
    let (accent_color, confirm_tone) = match action {
        ConfirmAction::DeleteDownload {
            file_mode: Some(DeleteFileMode::Permanent),
            ..
        }
        | ConfirmAction::DeleteMultiple {
            file_mode: Some(DeleteFileMode::Permanent),
            ..
        }
        | ConfirmAction::OverwriteCollision { .. } => (danger, GridTone::Danger),
        ConfirmAction::DeleteDownload {
            file_mode: Some(DeleteFileMode::RecycleBin),
            ..
        }
        | ConfirmAction::DeleteMultiple {
            file_mode: Some(DeleteFileMode::RecycleBin),
            ..
        }
        | ConfirmAction::DuplicateOngoing { .. } => (accent, GridTone::Primary),
        ConfirmAction::DeleteDownload { .. }
        | ConfirmAction::DeleteMultiple { .. }
        | ConfirmAction::ClearCompleted { .. } => (chrome_border, GridTone::Danger),
    };

    let on_backdrop_close = on_cancel.clone();
    let footer = GridModalFooter::new()
        .tone(GridTone::Chrome)
        .child(
            GridCellButton::new("confirm-cancel")
                .icon(IconName::X)
                .label("CANCEL")
                .tone(GridTone::Chrome)
                .border(CellBorder::Left)
                .on_click(move |_, window, cx| on_cancel(window, cx)),
        )
        .when(is_duplicate_ongoing, |footer| {
            footer.child(
                GridCellButton::new("confirm-alt")
                    .icon(IconName::Download)
                    .label("SEPARATE COPY")
                    .tooltip("Download a separate copy under an auto-renamed filename")
                    .tone(GridTone::Chrome)
                    .border(CellBorder::Left)
                    .on_click(move |_, window, cx| on_alt_confirm(window, cx)),
            )
        })
        .child(
            GridCellButton::new("confirm-proceed")
                .icon(if is_duplicate_ongoing {
                    IconName::ArrowUpRight
                } else {
                    IconName::Trash
                })
                .label(confirm_label)
                .tone(confirm_tone)
                .border(CellBorder::Left)
                .on_click(move |_, window, cx| on_confirm(window, cx)),
        );

    let card = ModalPlate::new(px(460.0))
        .border_color(accent_color)
        .child(
            div()
                .flex()
                .items_center()
                .gap_2()
                .child(
                    Icon::new(IconName::TriangleAlert)
                        .size(px(16.0))
                        .text_color(if confirm_tone == GridTone::Danger {
                            danger
                        } else {
                            accent
                        }),
                )
                .child(
                    div()
                        .text_size(TYPE_HEADING)
                        .line_height(px(22.0))
                        .font_weight(FontWeight::BOLD)
                        .text_color(chrome_text)
                        .child(title),
                ),
        )
        .child(
            div()
                .text_size(TYPE_BODY)
                .line_height(px(18.0))
                .text_color(chrome_muted)
                .child(detail),
        )
        .when(show_mode_selector, |el| {
            let mode_chip = |id: &'static str,
                             label: &'static str,
                             target: Option<DeleteFileMode>,
                             is_danger: bool| {
                let selected = active_mode == target;
                let cb = on_change_file_mode.clone();
                div()
                    .id(id)
                    .flex_1()
                    .h(px(28.0))
                    .px_2()
                    .border_1()
                    .border_color(if selected {
                        if is_danger { danger } else { accent }
                    } else {
                        chrome_border
                    })
                    .bg(if selected {
                        if is_danger { danger } else { accent }
                    } else {
                        chrome_elevated
                    })
                    .rounded(RADIUS_BADGE)
                    .flex()
                    .items_center()
                    .justify_center()
                    .font_family(mono.clone())
                    .text_size(TYPE_MICRO)
                    .font_weight(if selected {
                        FontWeight::BOLD
                    } else {
                        FontWeight::MEDIUM
                    })
                    .text_color(if selected {
                        if is_danger { danger_fg } else { accent_fg }
                    } else if is_danger {
                        danger
                    } else {
                        chrome_text
                    })
                    .cursor_pointer()
                    .on_click(move |_, window, cx| cb(target, window, cx))
                    .child(label)
            };

            el.child(
                div()
                    .flex()
                    .flex_col()
                    .gap_1p5()
                    .child(
                        div()
                            .font_family(mono.clone())
                            .text_size(TYPE_MICRO)
                            .text_color(chrome_muted)
                            .child("COMPLETED FILE ON DISK"),
                    )
                    .child(
                        div()
                            .flex()
                            .items_center()
                            .gap_1p5()
                            .child(mode_chip("confirm-mode-keep", "KEEP ON DISK", None, false))
                            .child(mode_chip(
                                "confirm-mode-recycle",
                                "RECYCLE BIN",
                                Some(DeleteFileMode::RecycleBin),
                                false,
                            ))
                            .child(mode_chip(
                                "confirm-mode-permanent",
                                "PERMANENT",
                                Some(DeleteFileMode::Permanent),
                                true,
                            )),
                    ),
            )
        })
        .footer(footer);

    ModalBackdrop::new(card)
        .on_close(move |window, cx| on_backdrop_close(window, cx))
        .into_any_element()
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
    cx: &App,
    on_cancel: F1,
    on_confirm: F2,
) -> ModalPlate
where
    F1: Fn(&mut Window, &mut App) + 'static,
    F2: Fn(&mut Window, &mut App) + 'static,
{
    let palette = cx.chrono().palette;
    let chrome_border = palette.chrome_border.hsla();
    let chrome_text = palette.chrome_text.hsla();
    let danger = palette.danger.hsla();
    let mono = Theme::global(cx).mono_font_family.clone();

    let footer = GridModalFooter::new()
        .tone(GridTone::Chrome)
        .child(
            GridCellButton::new("fileop-cancel")
                .icon(IconName::X)
                .label("CANCEL")
                .tone(GridTone::Chrome)
                .border(CellBorder::Left)
                .on_click(move |_, window, cx| on_cancel(window, cx)),
        )
        .child(
            GridCellButton::new("fileop-save")
                .icon(confirm_icon)
                .label(confirm_label)
                .tone(GridTone::Primary)
                .border(CellBorder::Left)
                .on_click(move |_, window, cx| on_confirm(window, cx)),
        );
    ModalPlate::new(width)
        .border_color(chrome_border)
        .child(
            div()
                .text_size(TYPE_HEADING)
                .font_weight(FontWeight::SEMIBOLD)
                .text_color(chrome_text)
                .child(title),
        )
        .when_some(error.map(str::to_string), |el, err| {
            el.child(
                div()
                    .font_family(mono)
                    .text_size(TYPE_MICRO)
                    .text_color(danger)
                    .child(err),
            )
        })
        .child(labeled_input(input_label, input, cx))
        .footer(footer)
}

fn compact_input_row(label: &'static str, state: &Entity<InputState>) -> impl IntoElement {
    GridCompactInputRow::new(label, state)
}

fn compact_input_row_with_action(
    label: &'static str,
    state: &Entity<InputState>,
    trailing_action: Option<AnyElement>,
) -> impl IntoElement {
    let row = GridCompactInputRow::new(label, state);
    match trailing_action {
        Some(action) => row.trailing_action(action),
        None => row,
    }
}

pub(crate) fn folder_paths_equal(a: &Path, b: &Path) -> bool {
    let norm = |p: &Path| {
        let s = p.to_string_lossy();
        let trimmed = s.trim().trim_end_matches(['/', '\\']);
        let base = if trimmed.is_empty() { s.trim() } else { trimmed };
        if cfg!(windows) {
            base.to_ascii_lowercase()
        } else {
            base.to_string()
        }
    };
    norm(a) == norm(b)
}

pub(crate) fn format_folder_entry_label(folder: &Path, all: &[PathBuf]) -> String {
    let leaf = folder
        .file_name()
        .and_then(|n| n.to_str())
        .filter(|s| !s.is_empty())
        .map(ToString::to_string)
        .unwrap_or_else(|| folder.to_string_lossy().to_string());
    let dup_count = all
        .iter()
        .filter(|other| {
            let other_leaf = other
                .file_name()
                .and_then(|n| n.to_str())
                .filter(|s| !s.is_empty())
                .unwrap_or_else(|| other.to_str().unwrap_or(""));
            if cfg!(windows) {
                other_leaf.eq_ignore_ascii_case(&leaf)
            } else {
                other_leaf == leaf
            }
        })
        .count();
    if dup_count > 1
        && let Some(parent) = folder.parent()
        && let Some(hint) = parent
            .file_name()
            .and_then(|n| n.to_str())
            .filter(|s| !s.is_empty())
            .or_else(|| {
                let s = parent.to_string_lossy();
                let trimmed = s.trim_end_matches(['/', '\\']);
                (!trimmed.is_empty()).then_some(trimmed).map(|_| "")
            })
    {
        if !hint.is_empty() {
            return format!("{leaf} ({hint})");
        }
        let drive = parent.to_string_lossy();
        let clean = drive.trim_end_matches(['/', '\\']);
        if !clean.is_empty() {
            return format!("{leaf} ({clean})");
        }
    }
    leaf
}

pub(crate) fn category_dropdown_extra_height(categories: &[DownloadCategory]) -> f32 {
    let visible = categories.len().min(6);
    (visible as f32) * 26.0 + 2.0
}

fn render_category_dropdown_panel<F1>(
    categories: &[DownloadCategory],
    selected_id: &str,
    default_dir: &Path,
    cx: &App,
    on_pick: F1,
) -> AnyElement
where
    F1: Fn(String, &mut Window, &mut App) + Clone + 'static,
{
    let palette = cx.chrono().palette;
    let popover_bg = palette.popover_bg.hsla();
    let chrome_border = palette.chrome_border.hsla();
    let chrome_muted = palette.chrome_muted.hsla();
    let mono = Theme::global(cx).mono_font_family.clone();

    div()
        .id("pf-cat-menu")
        .w_full()
        .max_h(px(158.0))
        .overflow_y_scroll()
        .bg(popover_bg)
        .border_b_1()
        .border_l_1()
        .border_r_1()
        .border_color(chrome_border)
        .flex()
        .flex_col()
        .children(categories.iter().enumerate().map(|(idx, cat)| {
            let pick_cb = on_pick.clone();
            let cat_id = cat.id.clone();
            let selected = cat.id == selected_id;
            let resolved_dir = effective_category_dir(cat, default_dir)
                .to_string_lossy()
                .to_string();
            let dir_el = div()
                .flex_1()
                .min_w_0()
                .font_family(mono.clone())
                .text_size(TYPE_MICRO)
                .text_color(chrome_muted)
                .text_right()
                .truncate()
                .child(resolved_dir)
                .into_any_element();
            GridCheckOptionRow::new(("pf-cat-opt", idx), cat.name.clone())
                .height(px(26.0))
                .selected(selected)
                .trailing(dir_el)
                .on_click(move |_, window, cx| pick_cb(cat_id.clone(), window, cx))
        }))
        .into_any_element()
}

pub(crate) fn folder_dropdown_extra_height(default_dir: &Path, recent_folders: &[PathBuf]) -> f32 {
    let extra_recent = recent_folders
        .iter()
        .filter(|f| !folder_paths_equal(f, default_dir))
        .take(5)
        .count();
    let default_row_and_borders = 30.0;
    if extra_recent == 0 {
        default_row_and_borders + 25.0
    } else {
        default_row_and_borders + 21.0 + (extra_recent as f32) * 26.0
    }
}

fn render_folder_dropdown_panel<F1>(
    id_prefix: &'static str,
    default_dir: &Path,
    recent_folders: &[PathBuf],
    current_dir_text: &str,
    cx: &App,
    on_pick: F1,
) -> AnyElement
where
    F1: Fn(PathBuf, &mut Window, &mut App) + Clone + 'static,
{
    let palette = cx.chrono().palette;
    let popover_bg = palette.popover_bg.hsla();
    let chrome_elevated = palette.chrome_elevated.hsla();
    let chrome_border = palette.chrome_border.hsla();
    let chrome_text = palette.chrome_text.hsla();
    let chrome_muted = palette.chrome_muted.hsla();
    let surface_selected = palette.surface_selected.hsla();
    let surface_hover = palette.surface_hover.hsla();
    let accent = palette.accent.hsla();
    let mono = Theme::global(cx).mono_font_family.clone();

    let current_path = PathBuf::from(current_dir_text.trim());
    let filtered_recent: Vec<PathBuf> = recent_folders
        .iter()
        .filter(|f| !folder_paths_equal(f, default_dir))
        .take(5)
        .cloned()
        .collect();
    let mut all_for_labels = Vec::with_capacity(1 + filtered_recent.len());
    all_for_labels.push(default_dir.to_path_buf());
    all_for_labels.extend(filtered_recent.iter().cloned());

    let folder_row = |row_idx: usize,
                      folder: PathBuf,
                      role_hint: Option<&'static str>,
                      label: String,
                      selected: bool| {
        let pick_cb = on_pick.clone();
        let full_path = folder.to_string_lossy().to_string();
        let tip_path = full_path.clone();
        div()
            .id((id_prefix, row_idx))
            .h(px(26.0))
            .px_2p5()
            .flex()
            .items_center()
            .justify_between()
            .gap_2()
            .cursor_pointer()
            .bg(if selected {
                surface_selected
            } else {
                Hsla::transparent_black()
            })
            .hover(move |s| s.bg(surface_hover))
            .tooltip(move |window, cx| {
                gpui_kit::component::tooltip::Tooltip::new(tip_path.clone()).build(window, cx)
            })
            .on_click(move |_, window, cx| pick_cb(folder.clone(), window, cx))
            .child(
                div()
                    .flex()
                    .items_center()
                    .gap_1p5()
                    .min_w_0()
                    .child(
                        div()
                            .w(px(12.0))
                            .flex()
                            .items_center()
                            .justify_center()
                            .when(selected, |el| {
                                el.child(
                                    Icon::new(IconName::Check)
                                        .size(px(11.0))
                                        .text_color(accent),
                                )
                            }),
                    )
                    .child(
                        div()
                            .font_family(mono.clone())
                            .text_size(TYPE_MICRO)
                            .font_weight(if selected {
                                FontWeight::MEDIUM
                            } else {
                                FontWeight::NORMAL
                            })
                            .text_color(chrome_text)
                            .child(label),
                    )
                    .when_some(role_hint, |el, tag| {
                        el.child(
                            div()
                                .font_family(mono.clone())
                                .text_size(px(9.5))
                                .text_color(chrome_muted.opacity(0.65))
                                .child(tag),
                        )
                    }),
            )
            .child(
                div()
                    .flex_1()
                    .min_w_0()
                    .font_family(mono.clone())
                    .text_size(TYPE_MICRO)
                    .text_color(chrome_muted)
                    .text_right()
                    .truncate()
                    .child(full_path),
            )
    };

    let def_label = format_folder_entry_label(default_dir, &all_for_labels);
    let def_selected = folder_paths_equal(default_dir, &current_path);

    div()
        .id((id_prefix, 999usize))
        .w_full()
        .bg(popover_bg)
        .border_b_1()
        .border_l_1()
        .border_r_1()
        .border_color(chrome_border)
        .flex()
        .flex_col()
        .overflow_hidden()
        .child(folder_row(
            0,
            default_dir.to_path_buf(),
            Some("DEFAULT"),
            def_label,
            def_selected,
        ))
        .when(filtered_recent.is_empty(), |el| {
            el.child(div().w_full().h(px(1.0)).bg(chrome_border))
                .child(
                    div()
                        .h(px(24.0))
                        .px_2p5()
                        .flex()
                        .items_center()
                        .font_family(mono.clone())
                        .text_size(TYPE_MICRO)
                        .text_color(chrome_muted.opacity(0.7))
                        .child("Use the folder button to save to another location"),
                )
        })
        .when(!filtered_recent.is_empty(), |el| {
            el.child(div().w_full().h(px(1.0)).bg(chrome_border))
                .child(
                    div()
                        .h(px(20.0))
                        .px_2p5()
                        .bg(chrome_elevated)
                        .flex()
                        .items_center()
                        .font_family(mono.clone())
                        .text_size(px(9.0))
                        .text_color(chrome_muted)
                        .child("RECENT LOCATIONS"),
                )
                .children(filtered_recent.into_iter().enumerate().map(|(i, folder)| {
                    let lbl = format_folder_entry_label(&folder, &all_for_labels);
                    let sel = folder_paths_equal(&folder, &current_path);
                    folder_row(i + 1, folder, None, lbl, sel)
                }))
        })
        .into_any_element()
}

pub(crate) fn labeled_input(
    label: &'static str,
    state: &Entity<InputState>,
    cx: &App,
) -> impl IntoElement {
    let palette = cx.chrono().palette;
    div().border_t_1().border_color(palette.chrome_border.hsla()).child(
        GridKvRow::new(label, GridInputBox::new(state))
            .label_width(160.0)
            .tone(GridTone::Chrome)
            .value_bg(palette.input_bg.hsla()),
    )
}

#[cfg(test)]
mod tests {
    use std::path::PathBuf;

    use super::{folder_paths_equal, format_folder_entry_label, split_filename_extension};

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

    #[test]
    fn folder_paths_equal_and_disambiguation_handle_slashes_and_duplicate_leafs() {
        let a = PathBuf::from(r"E:\Downloads\");
        let b = PathBuf::from(r"e:\downloads");
        assert!(folder_paths_equal(&a, &b));

        let c_dl = PathBuf::from(r"C:\Users\alice\Downloads");
        let e_dl = PathBuf::from(r"E:\Media\Downloads");
        let iso = PathBuf::from(r"E:\ISOs");
        let list = vec![c_dl.clone(), e_dl.clone(), iso.clone()];
        assert_eq!(
            format_folder_entry_label(&c_dl, &list),
            "Downloads (alice)"
        );
        assert_eq!(
            format_folder_entry_label(&e_dl, &list),
            "Downloads (Media)"
        );
        assert_eq!(format_folder_entry_label(&iso, &list), "ISOs");
    }
}
