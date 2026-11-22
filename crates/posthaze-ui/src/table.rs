//! Top toolbar and Download queue table/row views for [PosthazeApp].

use std::sync::Arc;
use std::time::{Duration, Instant};

use gpui_kit::assets::IconName;
use gpui_kit::base::{Scrollbar, ScrollbarMode};
use gpui_kit::component::input::Input;
use gpui_kit::component::tooltip::Tooltip;
use gpui_kit::component::{Icon, InteractiveElementExt as _};
use gpui_kit::prelude::FluentBuilder;
use gpui_kit::*;
use posthaze_engine::{DownloadId, DownloadState};

use crate::PosthazeApp;
use crate::assets::BRAND_LOGO_SVG;
use crate::context_menu::ContextMenuState;
use crate::grid_cells::{
    CellBorder, GridTone, grid_boxed_button, grid_cell_button, grid_cell_button_dyn,
};
use crate::motion::RowMotionSample;
use crate::theme::*;
use crate::view_model::{
    COLLAPSED_INSPECTOR_HEIGHT, DownloadRowView, FilterTab, LayoutProfile,
    SelectionToolbar, SortColumn, SortDirection, derive_display_filename, format_bytes, format_eta,
    format_speed, state_icon,
};

const COL_STATE: f32 = 104.0;
const COL_SPEED: f32 = 90.0;
const COL_ETA: f32 = 64.0;
const COL_SEGS: f32 = 48.0;
const COL_ACTIONS: f32 = 148.0;

impl PosthazeApp {
    pub(crate) fn render_toolbar(&self, profile: LayoutProfile, cx: &mut Context<Self>) -> impl IntoElement {
        let selection = self.queue.selection_toolbar();
        let url_hidden = selection.is_some();
        div()
            .w_full()
            .flex()
            .flex_col()
            .child(
                div()
                    .h(px(40.0))
                    .w_full()
                    .flex()
                    .items_center()
                    .border_b_1()
                    .border_color(paper_border())
                    // Intake cell: URL field, or the selection summary while downloads are selected
                    .child(match &selection {
                        Some(sel) => self.render_selection_summary(sel),
                        None => div()
                            .flex_1()
                            .min_w(px(180.0))
                            .h_full()
                            .pl_4()
                            .pr_2()
                            .border_r_1()
                            .border_color(paper_border())
                            .bg(input_bg())
                            .flex()
                            .items_center()
                            .child(
                                div()
                                    .flex_1()
                                    .min_w_0()
                                    .font_family(MONO_FONT)
                                    .text_size(TYPE_BODY)
                                    .child(Input::new(&self.url_input).appearance(false)),
                            )
                            .into_any_element(),
                    })
                    // ADD URL button cell (right after the URL field, full cell width & height)
                    .child(
                        div()
                            .id("add-url-btn")
                            .w(px(COL_STATE))
                            .flex_shrink_0()
                            .h_full()
                            .border_r_1()
                            .border_color(paper_border())
                            .bg(table_header_bg())
                            .hover(|s| {
                                s.bg(selenium_amber())
                                    .text_color(active_theme().accent_foreground.hsla())
                            })
                            .cursor_pointer()
                            .flex()
                            .items_center()
                            .justify_center()
                            .gap_1p5()
                            .font_family(MONO_FONT)
                            .text_size(TYPE_MICRO)
                            .font_weight(FontWeight::SEMIBOLD)
                            .text_color(carbon_text())
                            .tooltip(|window, cx| {
                                Tooltip::new("Probe entered URL, or Paste & Probe from clipboard")
                                    .build(window, cx)
                            })
                            .on_click(cx.listener(move |app, _, window, cx| {
                                let has_text = !url_hidden
                                    && !app
                                        .url_input
                                        .read(cx)
                                        .value()
                                        .trim()
                                        .is_empty();
                                if has_text {
                                    app.submit_url_from_input(window, cx);
                                } else {
                                    app.paste_and_probe_from_clipboard(window, cx);
                                }
                            }))
                            .child(Icon::new(IconName::Plus).size(px(12.0)))
                            .child("ADD URL"),
                    )
                    // Extensible top-header tools slot
                    .child(self.render_toolbar_tools(profile, selection.as_ref(), cx)),
            )
            // Filter row (fixed 34px height prevents table shift)
            .child(self.render_filter_row(cx))
            // Merged full-width separator row between toolbar and download list
            .child(
                div()
                    .h(px(6.0))
                    .w_full()
                    .bg(table_header_bg())
                    .border_b_1()
                    .border_color(paper_border()),
            )
    }

    /// Always-on filter row; selection actions live in the top toolbar row.
    fn render_filter_row(&self, cx: &mut Context<Self>) -> AnyElement {
        let has_query = !self.search_input.read(cx).value().is_empty();
        div()
                .h(px(34.0))
                .w_full()
                .flex()
                .items_center()
                .border_b_1()
                .border_color(paper_border())
                .child(
                    div()
                        .w(px(16.0 + COL_STATE))
                        .flex_shrink_0()
                        .h_full()
                        .px_3()
                        .border_r_1()
                        .border_color(paper_border())
                        .bg(table_header_bg())
                        .flex()
                        .items_center()
                        .gap_1p5()
                        .font_family(MONO_FONT)
                        .text_size(TYPE_MICRO)
                        .font_weight(FontWeight::SEMIBOLD)
                        .text_color(carbon_muted())
                        .child(Icon::new(IconName::Search).size(px(12.0)))
                        .child("FILTER"),
                )
                .child(
                    div()
                        .flex_1()
                        .min_w(px(120.0))
                        .h_full()
                        .px_2()
                        .bg(input_bg())
                        .flex()
                        .items_center()
                        .child(
                            div()
                                .flex_1()
                                .min_w_0()
                                .font_family(MONO_FONT)
                                .text_size(TYPE_BODY)
                                .child(Input::new(&self.search_input).appearance(false)),
                        ),
                )
                .when(has_query, |el| {
                    el.child(self.toolbar_cell_button(
                        "search-clear",
                        IconName::X,
                        None,
                        "Clear Filter",
                        false,
                        cx.listener(|app, _, window, cx| {
                            app.search_input.update(cx, |state, cx| {
                                state.set_value("", window, cx);
                            });
                            app.focus_handle.focus(window, cx);
                            cx.notify();
                        }),
                    ))
                })
                .into_any_element()
    }

    /// `N SELECTED` badge plus filename (single) or count summary, shown in place of the URL field.
    fn render_selection_summary(&self, sel: &SelectionToolbar) -> AnyElement {
        let badge = format!("{} SELECTED", sel.count);
        let summary = match &sel.single {
            Some(rec) => derive_display_filename(rec),
            None => format!("{} downloads selected", sel.count),
        };
        div()
            .flex_1()
            .min_w(px(180.0))
            .h_full()
            .bg(paper_bg())
            .border_r_1()
            .border_color(paper_border())
            .flex()
            .items_center()
            .overflow_hidden()
            .child(
                div()
                    .h_full()
                    .flex_shrink_0()
                    .px_3()
                    .border_r_1()
                    .border_color(paper_border())
                    .bg(paper_selected())
                    .flex()
                    .items_center()
                    .font_family(MONO_FONT)
                    .text_size(TYPE_MICRO)
                    .font_weight(FontWeight::SEMIBOLD)
                    .text_color(selenium_amber())
                    .child(badge),
            )
            .child(
                div()
                    .flex_1()
                    .min_w_0()
                    .px_3()
                    .font_family(MONO_FONT)
                    .text_size(TYPE_BODY)
                    .text_color(carbon_text())
                    .child(div().truncate().child(summary)),
            )
            .into_any_element()
    }

    /// Labelled selection actions for the top toolbar (icon-only in the compact profile).
    fn render_selection_tools(
        &self,
        profile: LayoutProfile,
        sel: &SelectionToolbar,
        cx: &mut Context<Self>,
    ) -> AnyElement {
        let compact = profile.compact_toolbar;
        let label = |text: &str, n: Option<usize>| -> Option<SharedString> {
            (!compact).then(|| match n {
                Some(n) if sel.count > 1 => SharedString::from(format!("{text} ({n})")),
                _ => SharedString::from(text.to_string()),
            })
        };
        type ClickHandler = Box<dyn Fn(&ClickEvent, &mut Window, &mut App)>;
        let button = |id: &'static str,
                      icon: IconName,
                      text: Option<SharedString>,
                      tip: &'static str,
                      tone: GridTone,
                      border: CellBorder,
                      on_click: ClickHandler| {
            grid_cell_button_dyn(id, Some(icon), text, Some(tip), tone, border, false, on_click)
        };

        let mut tools = div().h_full().flex_shrink_0().flex().items_center();

        if !sel.pause_ids.is_empty() {
            let ids = sel.pause_ids.clone();
            tools = tools.child(button(
                "sel-pause",
                IconName::Pause,
                label("PAUSE", Some(ids.len())),
                "Pause Selected",
                GridTone::Primary,
                CellBorder::Right,
                Box::new(cx.listener(move |app, _, _, cx| {
                    for id in &ids {
                        app.pause_download(id.clone(), cx);
                    }
                })),
            ));
        }
        if !sel.resume_ids.is_empty() {
            let ids = sel.resume_ids.clone();
            tools = tools.child(button(
                "sel-resume",
                IconName::Play,
                label("RESUME", Some(ids.len())),
                "Resume Selected",
                GridTone::Primary,
                CellBorder::Right,
                Box::new(cx.listener(move |app, _, _, cx| {
                    for id in &ids {
                        app.resume_download(id.clone(), cx);
                    }
                })),
            ));
        }
        if !sel.restart_ids.is_empty() {
            let ids = sel.restart_ids.clone();
            tools = tools.child(button(
                "sel-restart",
                IconName::RotateCcw,
                label("RESTART", Some(ids.len())),
                "Restart Fresh",
                GridTone::Paper,
                CellBorder::Right,
                Box::new(cx.listener(move |app, _, _, cx| {
                    for id in &ids {
                        app.restart_fresh_download(id.clone(), cx);
                    }
                })),
            ));
        }
        if !sel.cancel_ids.is_empty() {
            let ids = sel.cancel_ids.clone();
            tools = tools.child(button(
                "sel-cancel",
                IconName::X,
                label("CANCEL", Some(ids.len())),
                "Cancel Selected",
                GridTone::Paper,
                CellBorder::Right,
                Box::new(cx.listener(move |app, _, _, cx| {
                    for id in &ids {
                        app.cancel_download(id.clone(), cx);
                    }
                })),
            ));
        }
        if let Some(rec) = sel
            .single
            .as_ref()
            .filter(|r| r.state == DownloadState::Completed)
        {
            let open_path = rec.output_path.clone();
            let folder_path = rec.output_path.clone();
            tools = tools
                .child(button(
                    "sel-open-file",
                    IconName::File,
                    label("OPEN", None),
                    "Open File",
                    GridTone::Paper,
                    CellBorder::Right,
                    Box::new(cx.listener(move |app, _, _, cx| {
                        app.open_downloaded_file(open_path.clone(), cx);
                    })),
                ))
                .child(button(
                    "sel-open-dir",
                    IconName::FolderOpen,
                    label("FOLDER", None),
                    "Open Folder",
                    GridTone::Paper,
                    CellBorder::Right,
                    Box::new(cx.listener(move |app, _, _, cx| {
                        app.open_download_folder(folder_path.clone(), cx);
                    })),
                ));
        }
        if let Some(rec) = &sel.single {
            let url = rec.url.clone();
            tools = tools.child(button(
                "sel-copy-url",
                IconName::Copy,
                None,
                "Copy URL",
                GridTone::Paper,
                CellBorder::Right,
                Box::new(move |_, _, cx| {
                    cx.write_to_clipboard(ClipboardItem::new_string(url.clone()));
                }),
            ));
        }
        tools
            .child(button(
                "sel-delete",
                IconName::Trash,
                label("DELETE", None),
                "Delete Selected",
                GridTone::Danger,
                CellBorder::Right,
                Box::new(cx.listener(|app, _, _, cx| {
                    app.request_delete_selected(cx);
                })),
            ))
            .child(button(
                "sel-clear",
                IconName::X,
                None,
                "Clear Selection",
                GridTone::Paper,
                CellBorder::None,
                Box::new(cx.listener(|app, _, _, cx| {
                    app.queue.clear_selection();
                    app.collapse_inspector_if_open(cx);
                    cx.notify();
                })),
            ))
            .into_any_element()
    }

    /// Modular tool strip on the right of the top intake row; add future header tools here.
    fn render_toolbar_tools(
        &self,
        profile: LayoutProfile,
        selection: Option<&SelectionToolbar>,
        cx: &mut Context<Self>,
    ) -> AnyElement {
        if let Some(sel) = selection {
            return self.render_selection_tools(profile, sel, cx);
        }
        div()
            .h_full()
            .flex_shrink_0()
            .flex()
            .items_center()
            .child(self.toolbar_cell_button(
                "resume-all",
                IconName::Play,
                (!profile.compact_toolbar).then_some("RESUME ALL"),
                "Resume All",
                true,
                cx.listener(|app, _, _, cx| {
                    app.resume_all(cx);
                }),
            ))
            .child(self.toolbar_cell_button(
                "pause-all",
                IconName::Pause,
                (!profile.compact_toolbar).then_some("PAUSE ALL"),
                "Pause All",
                true,
                cx.listener(|app, _, _, cx| {
                    app.pause_all(cx);
                }),
            ))
            .child(self.toolbar_cell_button(
                "clear-completed",
                IconName::Trash,
                (!profile.compact_toolbar).then_some("CLEAR COMPLETED"),
                "Clear Completed",
                false,
                cx.listener(|app, _, _, cx| {
                    app.request_clear_completed(cx);
                }),
            ))
            .into_any_element()
    }

    #[allow(clippy::too_many_arguments)]
    fn toolbar_cell_button(
        &self,
        id: impl Into<ElementId>,
        icon: IconName,
        label: Option<&'static str>,
        tooltip_text: &'static str,
        right_border: bool,
        on_click: impl Fn(&ClickEvent, &mut Window, &mut App) + 'static,
    ) -> AnyElement {
        grid_cell_button(
            id,
            Some(icon),
            label,
            Some(tooltip_text),
            GridTone::Paper,
            if right_border {
                CellBorder::Right
            } else {
                CellBorder::None
            },
            false,
            on_click,
        )
    }

    fn row_action_cell(
        &self,
        id: SharedString,
        icon: IconName,
        label: Option<&'static str>,
        tooltip_text: &'static str,
        on_click: impl Fn(&ClickEvent, &mut Window, &mut App) + 'static,
    ) -> AnyElement {
        grid_cell_button(
            id,
            Some(icon),
            label,
            Some(tooltip_text),
            GridTone::PaperSubtle,
            CellBorder::Left,
            true,
            on_click,
        )
    }

    pub(crate) fn render_table(
        &self,
        visible_ids: Arc<Vec<DownloadId>>,
        is_empty_queue: bool,
        profile: LayoutProfile,
        cx: &mut Context<Self>,
    ) -> impl IntoElement {
        let row_count = self.queue.visible_indices().len();
        if row_count == 0 {
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
                    el.child(grid_boxed_button(
                        "empty-paste",
                        px(34.0),
                        Some(IconName::ClipboardPaste),
                        Some("PASTE URL FROM CLIPBOARD & PROBE"),
                        None,
                        GridTone::Primary,
                        false,
                        cx.listener(|app, _, window, cx| {
                            app.paste_and_probe_from_clipboard(window, cx);
                        }),
                    ))
                })
                .when(!is_empty_queue, |el| {
                    el.child(grid_boxed_button(
                        "empty-show-all",
                        px(34.0),
                        Some(IconName::Folder),
                        Some("SHOW ALL DOWNLOADS"),
                        None,
                        GridTone::Paper,
                        false,
                        cx.listener(|app, _, window, cx| {
                            app.filter = FilterTab::All;
                            app.search_input.update(cx, |state, cx| {
                                state.set_value("", window, cx);
                            });
                            cx.notify();
                        }),
                    ))
                })
                .into_any_element();
        }

        let entity = cx.entity().clone();
        let scroll_handle = self.table_scroll_handle.clone();
        let scrubber = render_table_scrubber(
            self.queue.all_rows(),
            self.queue.visible_indices(),
            self.queue.selected_id.as_ref(),
            self.queue.settings.ui_show_scrollbar_minimap,
            &scroll_handle,
        );

        div()
            .flex_1()
            .flex()
            .flex_col()
            .overflow_hidden()
            // Table Header (shared column constants with render_row)
            .child(
                div()
                    .h(px(28.0))
                    .flex()
                    .items_center()
                    .pl_4()
                    .pr(px(14.0))
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
            // Virtualized Scrollable Rows + Calibrated Telemetry Scrubber
            .child(
                div()
                    .relative()
                    .flex_1()
                    .size_full()
                    .overflow_hidden()
                    .bg(paper_bg())
                    .child(
                        uniform_list("downloads-table-scroll", row_count, move |range, _window, cx| {
                            entity.update(cx, |app, cx| {
                                range
                                    .filter_map(|idx| {
                                        let row = app.queue.visible_row(idx)?;
                                        Some(
                                            app.render_row(row, visible_ids.clone(), profile, cx)
                                                .into_any_element(),
                                        )
                                    })
                                    .collect::<Vec<_>>()
                            })
                        })
                        .size_full()
                        .track_scroll(&scroll_handle),
                    )
                    .child(scrubber),
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
        visible_ids: Arc<Vec<DownloadId>>,
        profile: LayoutProfile,
        cx: &mut Context<Self>,
    ) -> impl IntoElement + use<> {
        let is_selected = self.queue.is_selected(&row.id);
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

        let row_height = self.queue.settings.ui_table_density.row_height_px();

        div()
            .id(SharedString::from(format!("row-{}", row.id.as_str())))
            .w_full()
            .h(px(row_height))
            .flex()
            .items_center()
            .pr(px(14.0))
            .border_b_1()
            .border_color(paper_border())
            .bg(if is_selected {
                paper_selected()
            } else {
                Hsla::transparent_black()
            })
            .hover(|s| s.bg(paper_hover()))
            .cursor_pointer()
            .on_click(cx.listener(move |app, ev: &ClickEvent, window, cx| {
                app.focus_handle.focus(window, cx);
                let mods = ev.modifiers();
                let ctrl = mods.control || mods.platform;
                let shift = mods.shift;
                let is_double_click = ev.click_count() >= 2;
                let mut deselected = false;
                if shift {
                    app.queue
                        .select_range(&visible_ids, id_select.clone(), ctrl);
                } else if ctrl {
                    app.queue.toggle_select(id_select.clone());
                } else if is_double_click {
                    app.queue.select_single(id_select.clone());
                } else {
                    let selected = app.queue.selected_list();
                    if selected.len() == 1 && selected[0] == id_select {
                        app.queue.clear_selection();
                        deselected = true;
                    } else {
                        app.queue.select_single(id_select.clone());
                    }
                }
                if let Some(lead) = app.queue.selected_id.clone() {
                    app.load_segments_for(lead, cx);
                } else {
                    app.collapse_inspector_if_open(cx);
                }
                if !deselected
                    && !ctrl
                    && !shift
                    && app.queue.settings.ui_click_row_opens_inspector
                    && app.inspector_collapsed
                {
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
                    move |app, ev: &MouseDownEvent, window, cx| {
                        cx.stop_propagation();
                        app.focus_handle.focus(window, cx);
                        if !app.queue.is_selected(&id_ctx) {
                            app.queue.select_single(id_ctx.clone());
                            app.load_segments_for(id_ctx.clone(), cx);
                        }
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
                        Hsla::transparent_black()
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
                            .bg(byte_track_bg())
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
                    .flex()
                    .items_center()
                    .justify_end()
                    .overflow_hidden()
                    .on_click(|_, _, cx| cx.stop_propagation())
                    .when(
                        matches!(
                            row.state,
                            DownloadState::Active | DownloadState::Queued | DownloadState::Probing
                        ),
                        |el| {
                            el.child(self.row_action_cell(
                                SharedString::from(format!("pause-{}", row.id.as_str())),
                                IconName::Pause,
                                None,
                                "Pause",
                                cx.listener(move |app, _, _, cx| {
                                    app.pause_download(id_pause.clone(), cx);
                                }),
                            ))
                        },
                    )
                    .when(
                        matches!(row.state, DownloadState::Paused | DownloadState::Failed),
                        |el| {
                            let id_action = id_resume.clone();
                            if row.is_resource_changed {
                                el.child(self.row_action_cell(
                                    SharedString::from(format!("restart-{}", row.id.as_str())),
                                    IconName::RotateCcw,
                                    Some("RESTART"),
                                    "Restart Fresh",
                                    cx.listener(move |app, _, _, cx| {
                                        app.restart_fresh_download(id_action.clone(), cx);
                                    }),
                                ))
                            } else {
                                el.child(self.row_action_cell(
                                    SharedString::from(format!("resume-{}", row.id.as_str())),
                                    IconName::Play,
                                    None,
                                    "Resume",
                                    cx.listener(move |app, _, _, cx| {
                                        app.resume_download(id_action.clone(), cx);
                                    }),
                                ))
                            }
                        },
                    )
                    .when(
                        matches!(
                            row.state,
                            DownloadState::Active | DownloadState::Queued | DownloadState::Probing
                        ),
                        |el| {
                            el.child(self.row_action_cell(
                                SharedString::from(format!("cancel-{}", row.id.as_str())),
                                IconName::X,
                                None,
                                "Cancel",
                                cx.listener(move |app, _, _, cx| {
                                    app.cancel_download(id_cancel.clone(), cx);
                                }),
                            ))
                        },
                    )
                    .when(row.state == DownloadState::Completed, |el| {
                        el.child(self.row_action_cell(
                            SharedString::from(format!("open-dir-{}", row.id.as_str())),
                            IconName::FolderOpen,
                            None,
                            "Open Folder",
                            cx.listener(move |app, _, _, cx| {
                                app.open_download_folder(output_path.clone(), cx);
                            }),
                        ))
                    })
                    .child(self.row_action_cell(
                        SharedString::from(format!("del-{}", row.id.as_str())),
                        IconName::Trash,
                        None,
                        "Delete",
                        cx.listener(move |app, _, _, cx| {
                            app.request_delete_download(
                                id_delete.clone(),
                                filename_del.clone(),
                                cx,
                            );
                        }),
                    )),
            )
    }
}

const MINIMAP_BUCKETS: usize = 128;

#[derive(Clone, Copy, PartialEq, Eq)]
enum MinimapMark {
    None,
    Active,
    Failed,
}

fn render_table_scrubber(
    all_rows: &[DownloadRowView],
    visible_indices: &[usize],
    selected_id: Option<&DownloadId>,
    show_minimap: bool,
    scroll_handle: &UniformListScrollHandle,
) -> impl IntoElement + use<> {
    let row_count = visible_indices.len();
    let mut buckets = [MinimapMark::None; MINIMAP_BUCKETS];
    let mut selected_ratio = None;

    if show_minimap && row_count > 0 {
        for (idx, &row_idx) in visible_indices.iter().enumerate() {
            let row = &all_rows[row_idx];
            let bucket = (idx * MINIMAP_BUCKETS) / row_count;
            match row.state {
                DownloadState::Failed => buckets[bucket] = MinimapMark::Failed,
                DownloadState::Active | DownloadState::Probing
                    if buckets[bucket] != MinimapMark::Failed =>
                {
                    buckets[bucket] = MinimapMark::Active;
                }
                _ => {}
            }
            if selected_id == Some(&row.id) {
                selected_ratio = Some((idx as f32 + 0.5) / row_count as f32);
            }
        }
    }

    let has_overflow = {
        let inner = scroll_handle.0.borrow();
        inner.base_handle.max_offset().y > px(0.5)
    };

    div()
        .id("downloads-table-scrubber")
        .absolute()
        .top_0()
        .right_0()
        .bottom_0()
        .w(px(14.0))
        .when(has_overflow, |this| {
            this.border_l_1()
                .border_color(paper_border())
                .bg(table_header_bg().opacity(0.65))
                .when(show_minimap, |el| {
                    el.child(
                        div()
                            .absolute()
                            .inset_0()
                            .child(
                                canvas(
                                    |_, _, _| {},
                                    move |bounds, _, window, _| {
                                        let track_h = f32::from(bounds.size.height);
                                        if track_h <= 4.0 {
                                            return;
                                        }
                                        let active_color = selenium_amber().opacity(0.85);
                                        let failed_color = errata_vermilion().opacity(0.9);
                                        for (i, mark) in buckets.iter().enumerate() {
                                            let color = match mark {
                                                MinimapMark::None => continue,
                                                MinimapMark::Active => active_color,
                                                MinimapMark::Failed => failed_color,
                                            };
                                            let y = bounds.origin.y
                                                + px(
                                                    (i as f32 / MINIMAP_BUCKETS as f32)
                                                        * (track_h - 3.0),
                                                );
                                            window.paint_quad(fill(
                                                Bounds::new(
                                                    point(bounds.origin.x + px(1.0), y),
                                                    size(px(3.0), px(2.5)),
                                                ),
                                                color,
                                            ));
                                        }
                                        if let Some(ratio) = selected_ratio {
                                            let y = bounds.origin.y + px(ratio * (track_h - 3.0));
                                            window.paint_quad(fill(
                                                Bounds::new(
                                                    point(bounds.origin.x + px(1.0), y),
                                                    size(px(12.0), px(2.0)),
                                                ),
                                                cyanotype_blue().opacity(0.75),
                                            ));
                                        }
                                    },
                                )
                                .size_full(),
                            ),
                    )
                })
        })
        .child(
            div()
                .absolute()
                .inset_0()
                .child(
                    Scrollbar::vertical(scroll_handle)
                        .viewport_from_layout()
                        .mode(ScrollbarMode::Always)
                        .max_fps(60)
                        .styles(|s| {
                            s.track(|t| t.width(px(14.0)).bg(Hsla::transparent_black()))
                                .track_hover(|t| {
                                    t.width(px(14.0)).bg(paper_border().opacity(0.28))
                                })
                                .track_active(|t| {
                                    t.width(px(14.0)).bg(paper_border().opacity(0.4))
                                })
                                .thumb(|th| {
                                    th.width(px(6.0))
                                        .inset(px(2.0))
                                        .radius(px(1.5))
                                        .min_length(px(28.0))
                                        .bg(carbon_muted().opacity(0.55))
                                })
                                .thumb_hover(|th| {
                                    th.width(px(8.0))
                                        .inset(px(2.0))
                                        .radius(px(1.5))
                                        .bg(carbon_text().opacity(0.85))
                                })
                                .thumb_active(|th| {
                                    th.width(px(8.0))
                                        .inset(px(2.0))
                                        .radius(px(1.5))
                                        .bg(selenium_amber())
                                })
                        }),
                ),
        )
}
