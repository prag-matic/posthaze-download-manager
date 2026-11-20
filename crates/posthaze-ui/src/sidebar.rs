//! Left filter rail and live telemetry sidebar view for [PosthazeApp].

use gpui_kit::assets::IconName;
use gpui_kit::component::tooltip::Tooltip;
use gpui_kit::component::Icon;
use gpui_kit::*;

use crate::PosthazeApp;
use crate::grid_cells::{
    CellBorder, GridTone, grid_cell_button, grid_kv_row, grid_section_header,
};
use crate::theme::*;
use crate::view_model::{
    DownloadRowView, FilterTab, RAIL_SIDEBAR_WIDTH, format_speed,
};

impl PosthazeApp {
    pub(crate) fn render_sidebar(
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
        let mut counts = [0usize; 6];
        counts[0] = all_rows.len();
        for r in all_rows {
            match r.state {
                posthaze_engine::DownloadState::Active
                | posthaze_engine::DownloadState::Probing => counts[1] += 1,
                posthaze_engine::DownloadState::Queued => counts[2] += 1,
                posthaze_engine::DownloadState::Paused => counts[3] += 1,
                posthaze_engine::DownloadState::Completed => counts[4] += 1,
                posthaze_engine::DownloadState::Failed => counts[5] += 1,
                posthaze_engine::DownloadState::Cancelled => {}
            }
        }

        let target_width = if self.sidebar_collapsed {
            RAIL_SIDEBAR_WIDTH
        } else {
            self.sidebar_width
        };
        let sidebar_width = self.motion.current_sidebar_width(target_width);

        if self.sidebar_collapsed && (sidebar_width - RAIL_SIDEBAR_WIDTH).abs() <= 1.0 {
            return div()
                .relative()
                .w(px(RAIL_SIDEBAR_WIDTH))
                .h_full()
                .flex()
                .flex_col()
                .justify_between()
                .items_center()
                .bg(graphite_bg())
                .child(
                    div().w_full().flex().flex_col().children(tabs.into_iter().enumerate().map(|(i, tab)| {
                        let count = counts[i];
                        let selected = self.filter == tab;
                        let tab_icon = filter_tab_icon(tab);
                        let tab_color = if selected {
                            selenium_amber()
                        } else {
                            silver_muted()
                        };
                        div()
                            .id(SharedString::from(format!("rail-{}", tab.label())))
                            .relative()
                            .w_full()
                            .h(px(36.0))
                            .flex()
                            .items_center()
                            .justify_center()
                            .cursor_pointer()
                            .bg(if selected {
                                graphite_elevated()
                            } else {
                                graphite_bg()
                            })
                            .border_b_1()
                            .border_color(graphite_border())
                            .hover(|s| s.bg(graphite_elevated()))
                            .tooltip({
                                let label = format!("{} ({count})", tab.label());
                                move |window, cx| Tooltip::new(label.clone()).build(window, cx)
                            })
                            .on_click(cx.listener(move |app, _, _, cx| {
                                app.filter = tab;
                                cx.notify();
                            }))
                            .child(
                                div()
                                    .absolute()
                                    .left_0()
                                    .top_0()
                                    .bottom_0()
                                    .w(px(2.0))
                                    .bg(if selected {
                                        selenium_amber()
                                    } else {
                                        Hsla::transparent_black()
                                    }),
                            )
                            .child(Icon::new(tab_icon).size(px(15.0)).text_color(tab_color))
                    })),
                )
                .child(
                    div()
                        .w_full()
                        .flex()
                        .flex_col()
                        .items_center()
                        .child(
                            div()
                                .w_full()
                                .h(px(24.0))
                                .border_t_1()
                                .border_color(graphite_border())
                                .flex()
                                .items_center()
                                .justify_center()
                                .child(div().w(px(6.0)).h(px(6.0)).bg(if active_slots > 0 {
                                    selenium_amber()
                                } else {
                                    graphite_border()
                                })),
                        )
                        .child(
                            div()
                                .w_full()
                                .h(px(34.0))
                                .border_t_1()
                                .border_color(graphite_border())
                                .child(grid_cell_button(
                                    "open-settings",
                                    Some(IconName::Settings),
                                    None,
                                    Some("Engine Settings"),
                                    GridTone::Graphite,
                                    CellBorder::None,
                                    true,
                                    cx.listener(|app, _, window, cx| {
                                        app.open_settings_modal(window, cx);
                                    }),
                                )),
                        )
                        .child(
                            div()
                                .w_full()
                                .h(px(34.0))
                                .border_t_1()
                                .border_color(graphite_border())
                                .child(grid_cell_button(
                                    "sidebar-expand",
                                    Some(IconName::PanelLeftOpen),
                                    None,
                                    Some("Expand Sidebar"),
                                    GridTone::Graphite,
                                    CellBorder::None,
                                    true,
                                    cx.listener(|app, _, _, cx| {
                                        app.sidebar_collapsed = false;
                                        app.motion
                                            .animate_sidebar(RAIL_SIDEBAR_WIDTH, app.sidebar_width);
                                        app.ensure_motion_loop(cx);
                                        cx.notify();
                                    }),
                                )),
                        ),
                )
                .into_any_element();
        }

        div()
            .relative()
            .w(px(sidebar_width))
            .h_full()
            .overflow_hidden()
            .flex()
            .flex_col()
            .justify_between()
            .bg(graphite_bg())
            // Filter Grid
            .child(
                div()
                    .w_full()
                    .flex()
                    .flex_col()
                    .child(grid_section_header("QUEUE FILTER", GridTone::Graphite))
                    .children(tabs.into_iter().enumerate().map(|(i, tab)| {
                        let count = counts[i];
                        let selected = self.filter == tab;
                        let tab_icon = filter_tab_icon(tab);
                        let tab_color = if selected {
                            selenium_amber()
                        } else {
                            silver_muted()
                        };
                        div()
                            .id(SharedString::from(format!("filter-{}", tab.label())))
                            .h(px(34.0))
                            .w_full()
                            .flex()
                            .items_center()
                            .justify_between()
                            .cursor_pointer()
                            .bg(if selected {
                                graphite_elevated()
                            } else {
                                graphite_bg()
                            })
                            .border_b_1()
                            .border_color(graphite_border())
                            .hover(|s| s.bg(graphite_elevated()))
                            .on_click(cx.listener(move |app, _, _, cx| {
                                app.filter = tab;
                                cx.notify();
                            }))
                            .child(
                                div()
                                    .h_full()
                                    .flex()
                                    .items_center()
                                    .child(div().w(px(3.0)).h_full().mr_2p5().bg(if selected {
                                        selenium_amber()
                                    } else {
                                        Hsla::transparent_black()
                                    }))
                                    .child(
                                        Icon::new(tab_icon)
                                            .size(px(13.0))
                                            .text_color(tab_color),
                                    )
                                    .child(
                                        div()
                                            .ml_2()
                                            .font_family(MONO_FONT)
                                            .text_size(TYPE_MICRO)
                                            .font_weight(FontWeight::SEMIBOLD)
                                            .text_color(if selected {
                                                silver_text()
                                            } else {
                                                silver_muted()
                                            })
                                            .child(tab.label().to_uppercase()),
                                    ),
                            )
                            .child(
                                div()
                                    .w(px(44.0))
                                    .h_full()
                                    .border_l_1()
                                    .border_color(graphite_border())
                                    .flex()
                                    .items_center()
                                    .justify_center()
                                    .font_family(MONO_FONT)
                                    .text_size(TYPE_MICRO)
                                    .text_color(if selected {
                                        silver_text()
                                    } else {
                                        silver_muted()
                                    })
                                    .child(count.to_string()),
                            )
                    })),
            )
            // Global Telemetry Grid & Footer Cells
            .child(
                div()
                    .w_full()
                    .flex()
                    .flex_col()
                    .border_t_1()
                    .border_color(graphite_border())
                    .child(grid_section_header("TELEMETRY", GridTone::Graphite))
                    .child(grid_kv_row(
                        "SPEED",
                        76.0,
                        div()
                            .w_full()
                            .flex()
                            .items_center()
                            .justify_between()
                            .font_family(MONO_FONT)
                            .child(
                                div()
                                    .text_size(TYPE_BODY)
                                    .font_weight(FontWeight::BOLD)
                                    .text_color(silver_text())
                                    .child(format_speed(total_speed)),
                            )
                            .child(
                                div()
                                    .text_size(TYPE_MICRO)
                                    .text_color(silver_muted())
                                    .child(format!(
                                        "{:.1} Mbps",
                                        (total_speed as f64 * 8.0) / 1_000_000.0
                                    )),
                            ),
                        GridTone::Graphite,
                    ))
                    .child(grid_kv_row(
                        "SLOTS",
                        76.0,
                        div()
                            .w_full()
                            .flex()
                            .items_center()
                            .gap_2()
                            .child(
                                div().flex_1().flex().gap_1().children(
                                    (0..self.queue.settings.max_concurrent_downloads).map(|i| {
                                        div().h(px(6.0)).flex_1().bg(if i < active_slots {
                                            selenium_amber()
                                        } else {
                                            graphite_border()
                                        })
                                    }),
                                ),
                            )
                            .child(
                                div()
                                    .font_family(MONO_FONT)
                                    .text_size(TYPE_MICRO)
                                    .font_weight(FontWeight::SEMIBOLD)
                                    .text_color(silver_text())
                                    .child(format!(
                                        "{active_slots}/{}",
                                        self.queue.settings.max_concurrent_downloads
                                    )),
                            ),
                        GridTone::Graphite,
                    ))
                    .child(
                        div()
                            .h(px(34.0))
                            .w_full()
                            .flex()
                            .items_center()
                            .child(
                                div().flex_1().h_full().child(grid_cell_button(
                                    "open-settings",
                                    Some(IconName::Settings),
                                    Some("SETTINGS"),
                                    Some("Engine Settings"),
                                    GridTone::Graphite,
                                    CellBorder::Right,
                                    false,
                                    cx.listener(|app, _, window, cx| {
                                        app.open_settings_modal(window, cx);
                                    }),
                                )),
                            )
                            .child(grid_cell_button(
                                "sidebar-collapse",
                                Some(IconName::PanelLeftClose),
                                None,
                                Some("Collapse to Rail"),
                                GridTone::Graphite,
                                CellBorder::None,
                                true,
                                cx.listener(|app, _, _, cx| {
                                    app.sidebar_collapsed = true;
                                    app.motion
                                        .animate_sidebar(app.sidebar_width, RAIL_SIDEBAR_WIDTH);
                                    app.ensure_motion_loop(cx);
                                    cx.notify();
                                }),
                            )),
                    ),
            )
            .into_any_element()
    }
}

fn filter_tab_icon(tab: FilterTab) -> IconName {
    match tab {
        FilterTab::All => IconName::Folder,
        FilterTab::Active => IconName::ArrowDown,
        FilterTab::Queued => IconName::Clock,
        FilterTab::Paused => IconName::Pause,
        FilterTab::Completed => IconName::CircleCheck,
        FilterTab::Failed => IconName::TriangleAlert,
    }
}
