//! Pre-Flight Probe Confirmation Modal and Persisted Settings Modal.

use std::time::Duration;

use gpui_kit::assets::IconName;
use gpui_kit::component::button::{Button, ButtonVariants};
use gpui_kit::component::input::{Input, InputState};
use gpui_kit::component::{Icon, Sizable};
use gpui_kit::prelude::FluentBuilder;
use gpui_kit::*;
use posthaze_engine::{DownloadId, PreflightInfo, Settings};

use crate::theme::*;
use crate::view_model::format_bytes;

/// Active confirmation prompt for destructive queue/record actions.
pub(crate) enum ConfirmAction {
    DeleteDownload { id: DownloadId, filename: String },
    ClearCompleted { count: usize },
}

/// Active state of the Pre-Flight Add Download Confirmation Modal.
pub(crate) enum PreflightModalState {
    Probing {
        url: String,
    },
    Error {
        url: String,
        message: String,
    },
    Resolved {
        info: PreflightInfo,
        filename_input: Entity<InputState>,
        dir_input: Entity<InputState>,
        workers: usize,
    },
}

/// Active state of the Persisted Settings Modal.
pub(crate) struct SettingsModalState {
    pub dir_input: Entity<InputState>,
    pub workers_input: Entity<InputState>,
    pub concurrent_input: Entity<InputState>,
    pub sync_on_complete: bool,
    pub error: Option<String>,
}

impl SettingsModalState {
    pub fn from_settings(settings: &Settings, window: &mut Window, cx: &mut App) -> Self {
        let dir_input = cx.new(|cx| {
            let mut st = InputState::new(window, cx).placeholder("Default output directory");
            st.set_value(settings.default_output_dir.to_string_lossy().to_string(), window, cx);
            st
        });
        let workers_input = cx.new(|cx| {
            let mut st = InputState::new(window, cx).placeholder("1..32");
            st.set_value(settings.max_workers.to_string(), window, cx);
            st
        });
        let concurrent_input = cx.new(|cx| {
            let mut st = InputState::new(window, cx).placeholder("1+");
            st.set_value(settings.max_concurrent_downloads.to_string(), window, cx);
            st
        });
        Self {
            dir_input,
            workers_input,
            concurrent_input,
            sync_on_complete: settings.sync_on_complete,
            error: None,
        }
    }
}

pub(crate) fn render_preflight_modal<F1, F2, F3, F4, F5>(
    state: &PreflightModalState,
    on_close: F1,
    on_retry: F2,
    on_worker_delta: F3,
    on_confirm: F4,
    on_edit_url: F5,
) -> AnyElement
where
    F1: Fn(&mut Window, &mut App) + Clone + 'static,
    F2: Fn(String, &mut Window, &mut App) + 'static,
    F3: Fn(isize, &mut Window, &mut App) + Clone + 'static,
    F4: Fn(bool, &mut Window, &mut App) + Clone + 'static,
    F5: Fn(String, &mut Window, &mut App) + 'static,
{
    let on_backdrop_close = on_close.clone();
    let card = match state {
        PreflightModalState::Probing { url } => div()
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
                    .line_height(px(22.0))
                    .font_weight(FontWeight::BOLD)
                    .text_color(silver_text())
                    .child("Checking Download"),
            )
            .child(
                div()
                    .flex()
                    .items_center()
                    .gap_2()
                    .child(
                        div()
                            .flex()
                            .items_center()
                            .child(
                                Icon::new(IconName::LoaderCircle)
                                    .size(px(14.0))
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
                            .text_size(TYPE_BODY)
                            .line_height(px(18.0))
                            .text_color(selenium_amber())
                            .child("Checking file size, filename, and parallel support..."),
                    ),
            )
            .child(
                div()
                    .font_family(MONO_FONT)
                    .text_size(TYPE_MICRO)
                    .line_height(px(16.0))
                    .text_color(silver_muted())
                    .truncate()
                    .child(url.clone()),
            )
            .child(
                div()
                    .flex()
                    .justify_end()
                    .child(
                        Button::new("probe-cancel")
                            .small()
                            .icon(IconName::X)
                            .label("Cancel")
                            .on_click(move |_, window, cx| on_close(window, cx)),
                    ),
            ),
        PreflightModalState::Error { url, message } => {
            let url_retry = url.clone();
            let url_edit = url.clone();
            div()
                .w(px(520.0))
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
                                .font_weight(FontWeight::SEMIBOLD)
                                .text_color(errata_vermilion())
                                .child("Could Not Reach File"),
                        ),
                )
                .child(
                    div()
                        .font_family(MONO_FONT)
                        .text_size(TYPE_BODY)
                        .line_height(px(18.0))
                        .text_color(silver_text())
                        .child(message.clone()),
                )
                .child(
                    div()
                        .font_family(MONO_FONT)
                        .text_size(TYPE_MICRO)
                        .line_height(px(16.0))
                        .text_color(silver_muted())
                        .truncate()
                        .child(url.clone()),
                )
                .child(
                    div()
                        .flex()
                        .justify_end()
                        .gap_2()
                        .child(
                            Button::new("probe-edit")
                                .small()
                                .icon(IconName::Clipboard)
                                .label("Edit URL")
                                .on_click(move |_, window, cx| {
                                    on_edit_url(url_edit.clone(), window, cx);
                                }),
                        )
                        .child(
                            Button::new("probe-close")
                                .small()
                                .icon(IconName::X)
                                .label("Close")
                                .on_click(move |_, window, cx| on_close(window, cx)),
                        )
                        .child(
                            Button::new("probe-retry")
                                .small()
                                .primary()
                                .icon(IconName::RefreshCw)
                                .label("Retry Probe")
                                .on_click(move |_, window, cx| {
                                    on_retry(url_retry.clone(), window, cx);
                                }),
                        ),
                )
        }
        PreflightModalState::Resolved {
            info,
            filename_input,
            dir_input,
            workers,
        } => {
            let size_label = info
                .content_length
                .map(format_bytes)
                .unwrap_or_else(|| "Unknown Size (Stream)".to_string());
            let confirm_now = on_confirm.clone();
            let confirm_queue = on_confirm;

            div()
                .w(px(560.0))
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
                        .flex()
                        .items_center()
                        .justify_between()
                        .child(
                            div()
                                .text_size(TYPE_HEADING)
                                .line_height(px(22.0))
                                .font_weight(FontWeight::SEMIBOLD)
                                .text_color(silver_text())
                                .child("Add Download"),
                        )
                        .child(
                            div()
                                .px_2()
                                .py_0p5()
                                .rounded(px(3.0))
                                .bg(graphite_elevated())
                                .border_1()
                                .border_color(graphite_border())
                                .font_family(MONO_FONT)
                                .text_size(TYPE_MICRO)
                                .line_height(px(15.0))
                                .font_weight(FontWeight::MEDIUM)
                                .text_color(if info.supports_ranges {
                                    silver_text()
                                } else {
                                    silver_muted()
                                })
                                .child(if info.supports_ranges {
                                    "Multi-Segment (HTTP 206)"
                                } else {
                                    "Single-Stream (HTTP 200)"
                                }),
                        ),
                )
                .child(
                    div()
                        .font_family(MONO_FONT)
                        .text_size(TYPE_MICRO)
                        .line_height(px(16.0))
                        .text_color(silver_muted())
                        .truncate()
                        .child(format!("Size: {size_label} · {}", info.final_url)),
                )
                .child(labeled_input("Filename", filename_input))
                .child(labeled_input("Destination Directory", dir_input))
                .child(
                    div()
                        .flex()
                        .items_center()
                        .justify_between()
                        .child(
                            div()
                                .text_xs()
                                .text_color(silver_muted())
                                .child(if info.supports_ranges {
                                    "Concurrent Workers (1..32)"
                                } else {
                                    "Concurrent Workers (Locked to 1 for single-stream)"
                                }),
                        )
                        .child(
                            div()
                                .flex()
                                .items_center()
                                .gap_2()
                                .when(info.supports_ranges, |el| {
                                    let dec = on_worker_delta.clone();
                                    el.child(
                                        Button::new("w-dec")
                                            .xsmall()
                                            .icon(IconName::Minus)
                                            .tooltip("Decrease workers")
                                            .on_click(move |_, window, cx| dec(-1, window, cx)),
                                    )
                                })
                                .child(
                                    div()
                                        .w(px(32.0))
                                        .text_center()
                                        .font_family(MONO_FONT)
                                        .text_size(TYPE_BODY)
                                        .text_color(silver_text())
                                        .child(workers.to_string()),
                                )
                                .when(info.supports_ranges, |el| {
                                    el.child(
                                        Button::new("w-inc")
                                            .xsmall()
                                            .icon(IconName::Plus)
                                            .tooltip("Increase workers")
                                            .on_click(move |_, window, cx| {
                                                on_worker_delta(1, window, cx)
                                            }),
                                    )
                                }),
                        ),
                )
                .child(
                    div()
                        .flex()
                        .justify_end()
                        .gap_2()
                        .pt_2()
                        .child(
                            Button::new("pf-cancel")
                                .small()
                                .icon(IconName::X)
                                .label("Cancel")
                                .on_click(move |_, window, cx| on_close(window, cx)),
                        )
                        .child(
                            Button::new("pf-queue")
                                .small()
                                .icon(IconName::Clock)
                                .label("Add Paused")
                                .tooltip("Save to list in Paused state without starting transfer")
                                .on_click(move |_, window, cx| confirm_queue(false, window, cx)),
                        )
                        .child(
                            Button::new("pf-now")
                                .small()
                                .primary()
                                .icon(IconName::Download)
                                .label("Download Now")
                                .on_click(move |_, window, cx| confirm_now(true, window, cx)),
                        ),
                )
        }
    };

    modal_backdrop(card, on_backdrop_close)
}

pub(crate) fn render_settings_modal<F1, F2, F3>(
    state: &SettingsModalState,
    on_toggle_sync: F1,
    on_cancel: F2,
    on_save: F3,
) -> AnyElement
where
    F1: Fn(&mut Window, &mut App) + 'static,
    F2: Fn(&mut Window, &mut App) + Clone + 'static,
    F3: Fn(&mut Window, &mut App) + 'static,
{
    let on_backdrop_close = on_cancel.clone();
    let card = div()
        .w(px(500.0))
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
                .line_height(px(22.0))
                .font_weight(FontWeight::SEMIBOLD)
                .text_color(silver_text())
                .child("Settings"),
        )
        .when_some(state.error.clone(), |el, err| {
            el.child(
                div()
                    .px_3()
                    .py_1p5()
                    .rounded(px(4.0))
                    .bg(rgba(0xEF444424))
                    .border_1()
                    .border_color(errata_vermilion())
                    .flex()
                    .items_center()
                    .gap_2()
                    .child(
                        Icon::new(IconName::TriangleAlert)
                            .size(px(14.0))
                            .text_color(errata_vermilion()),
                    )
                    .child(
                        div()
                            .flex_1()
                            .font_family(MONO_FONT)
                            .text_size(TYPE_BODY)
                            .line_height(px(18.0))
                            .text_color(silver_text())
                            .child(err),
                    ),
            )
        })
        .child(labeled_input("Default Output Directory", &state.dir_input))
        .child(
            div()
                .flex()
                .gap_3()
                .child(div().flex_1().child(labeled_input("Default Workers (1..32)", &state.workers_input)))
                .child(div().flex_1().child(labeled_input("Max Concurrent Downloads (>= 1)", &state.concurrent_input))),
        )
        .child(
            div()
                .flex()
                .items_center()
                .justify_between()
                .child(
                    div()
                        .text_xs()
                        .text_color(silver_muted())
                        .child("Flush file buffers to disk before marking complete"),
                )
                .child(
                    Button::new("settings-sync")
                        .xsmall()
                        .icon(if state.sync_on_complete {
                            IconName::Check
                        } else {
                            IconName::X
                        })
                        .label(if state.sync_on_complete { "ON" } else { "OFF" })
                        .on_click(move |_, window, cx| on_toggle_sync(window, cx)),
                ),
        )
        .child(
            div()
                .flex()
                .justify_end()
                .gap_2()
                .pt_2()
                .child(
                    Button::new("settings-cancel")
                        .small()
                        .icon(IconName::X)
                        .label("Cancel")
                        .on_click(move |_, window, cx| on_cancel(window, cx)),
                )
                .child(
                    Button::new("settings-save")
                        .small()
                        .primary()
                        .icon(IconName::Check)
                        .label("Save Settings")
                        .on_click(move |_, window, cx| on_save(window, cx)),
                ),
        );

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
        ConfirmAction::DeleteDownload { filename, .. } => (
            "Remove Download",
            format!(
                "Remove \"{filename}\" from the list? Partial progress will be discarded; completed files on disk are kept."
            ),
            "Remove",
        ),
        ConfirmAction::ClearCompleted { count } => (
            "Clear Finished Downloads",
            format!(
                "Remove {count} completed or cancelled entry(s) from the list? Downloaded files on disk are kept."
            ),
            "Clear List",
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
            div()
                .flex()
                .justify_end()
                .gap_2()
                .pt_2()
                .child(
                    Button::new("confirm-cancel")
                        .small()
                        .icon(IconName::X)
                        .label("Cancel")
                        .on_click(move |_, window, cx| on_cancel(window, cx)),
                )
                .child(
                    Button::new("confirm-proceed")
                        .small()
                        .danger()
                        .icon(IconName::Trash)
                        .label(confirm_label)
                        .on_click(move |_, window, cx| on_confirm(window, cx)),
                ),
        );

    modal_backdrop(card, on_backdrop_close)
}

fn labeled_input(label: &'static str, state: &Entity<InputState>) -> impl IntoElement {
    div()
        .flex()
        .flex_col()
        .gap_1()
        .child(
            div()
                .text_size(TYPE_BODY)
                .line_height(px(18.0))
                .font_weight(FontWeight::MEDIUM)
                .text_color(silver_muted())
                .child(label),
        )
        .child(
            div()
                .h(px(32.0))
                .px_2p5()
                .bg(table_header_bg())
                .border_1()
                .border_color(paper_border())
                .rounded(px(4.0))
                .flex()
                .items_center()
                .font_family(MONO_FONT)
                .text_size(TYPE_BODY)
                .child(Input::new(state).appearance(false)),
        )
}

fn modal_backdrop<F>(card: impl IntoElement, on_close: F) -> AnyElement
where
    F: Fn(&mut Window, &mut App) + 'static,
{
    div()
        .id("modal-backdrop")
        .absolute()
        .inset_0()
        .p_4()
        .bg(rgba(0x141413AA))
        .flex()
        .items_center()
        .justify_center()
        .on_click(move |_, window, cx| on_close(window, cx))
        .child(
            div()
                .id("modal-card-stop")
                .max_w_full()
                .max_h_full()
                .on_mouse_down(MouseButton::Left, |_, _, cx| cx.stop_propagation())
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
