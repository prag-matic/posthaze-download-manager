//! Left filter rail and live telemetry sidebar view for [PosthazeApp].

use gpui_kit::assets::IconName;
use gpui_kit::component::button::{Button, ButtonVariants};
use gpui_kit::component::tooltip::Tooltip;
use gpui_kit::component::{Icon, Sizable};
use gpui_kit::*;

use crate::PosthazeApp;
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
                .py_2p5()
                .child(
                    div().w_full().flex().flex_col().gap_1().children(
                        tabs.into_iter().map(|tab| {
                                    let count =
                                        all_rows.iter().filter(|r| tab.matches(r.state)).count();
                                    let selected = self.filter == tab;
                                    let tab_icon = filter_tab_icon(tab);
                                    let tab_color = if selected {
                                        selenium_amber()
                                    } else {
                                        silver_muted()
                                    };
                                    div()
                                        .id(SharedString::from(format!("rail-{}", tab.label())))
                                        .w_full()
                                        .py_2()
                                        .flex()
                                        .items_center()
                                        .justify_center()
                                        .cursor_pointer()
                                        .bg(if selected {
                                            paper_hover()
                                        } else {
                                            graphite_bg()
                                        })
                                        .border_l_2()
                                        .border_color(if selected {
                                            selenium_amber()
                                        } else {
                                            graphite_bg()
                                        })
                                        .hover(|s| s.bg(graphite_elevated()))
                                        .tooltip({
                                            let label = format!("{} ({count})", tab.label());
                                            move |window, cx| {
                                                Tooltip::new(label.clone()).build(window, cx)
                                            }
                                        })
                                        .on_click(cx.listener(move |app, _, _, cx| {
                                            app.filter = tab;
                                            cx.notify();
                                        }))
                                        .child(
                                            Icon::new(tab_icon)
                                                .size(px(15.0))
                                                .text_color(tab_color),
                                        )
                                }),
                            ),
                )
                .child(
                    div()
                        .w_full()
                        .pt_2p5()
                        .border_t_1()
                        .border_color(graphite_border())
                        .flex()
                        .flex_col()
                        .items_center()
                        .gap_1p5()
                        .child(div().w(px(6.0)).h(px(6.0)).mb_1().bg(if active_slots > 0 {
                            selenium_amber()
                        } else {
                            graphite_border()
                        }))
                        .child(
                            Button::new("open-settings")
                                .small()
                                .ghost()
                                .icon(IconName::Settings)
                                .tooltip("Engine Settings")
                                .on_click(cx.listener(|app, _, window, cx| {
                                    app.open_settings_modal(window, cx);
                                })),
                        )
                        .child(
                            Button::new("sidebar-expand")
                                .small()
                                .ghost()
                                .icon(IconName::PanelLeftOpen)
                                .tooltip("Expand Sidebar")
                                .on_click(cx.listener(|app, _, _, cx| {
                                    app.sidebar_collapsed = false;
                                    app.motion
                                        .animate_sidebar(RAIL_SIDEBAR_WIDTH, app.sidebar_width);
                                    app.ensure_motion_loop(cx);
                                    cx.notify();
                                })),
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
            .p_3()
            .child(
                // Filter Rail (starts cleanly at top; branding lives in TitleBar)
                div().flex().flex_col().gap_1().children(tabs.into_iter().map(
                            |tab| {
                                let count =
                                    all_rows.iter().filter(|r| tab.matches(r.state)).count();
                                let selected = self.filter == tab;
                                let tab_icon = filter_tab_icon(tab);
                                let tab_color = if selected {
                                    selenium_amber()
                                } else {
                                    silver_muted()
                                };
                                div()
                                    .id(SharedString::from(format!("filter-{}", tab.label())))
                                    .flex()
                                    .items_center()
                                    .justify_between()
                                    .px_2p5()
                                    .py_1p5()
                                    .cursor_pointer()
                                    .bg(if selected {
                                        paper_hover()
                                    } else {
                                        graphite_bg()
                                    })
                                    .border_l_2()
                                    .border_color(if selected {
                                        selenium_amber()
                                    } else {
                                        graphite_bg()
                                    })
                                    .hover(|s| s.bg(graphite_elevated()))
                                    .text_size(px(12.5))
                                    .line_height(px(18.0))
                                    .font_weight(if selected {
                                        FontWeight::SEMIBOLD
                                    } else {
                                        FontWeight::NORMAL
                                    })
                                    .text_color(if selected {
                                        silver_text()
                                    } else {
                                        silver_muted()
                                    })
                                    .on_click(cx.listener(move |app, _, _, cx| {
                                        app.filter = tab;
                                        cx.notify();
                                    }))
                                    .child(
                                        div()
                                            .flex()
                                            .items_center()
                                            .gap_2()
                                            .child(
                                                Icon::new(tab_icon)
                                                    .size(px(13.0))
                                                    .text_color(tab_color),
                                            )
                                            .child(tab.label()),
                                    )
                                    .child(
                                        div()
                                            .px_1p5()
                                            .bg(graphite_bg())
                                            .border_1()
                                            .border_color(graphite_border())
                                            .font_family(MONO_FONT)
                                            .text_size(TYPE_MICRO)
                                            .text_color(silver_text())
                                            .child(count.to_string()),
                                    )
                            },
                        )),
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
                            .bg(graphite_elevated())
                            .border_1()
                            .border_color(graphite_border())
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
                                            .font_family(MONO_FONT)
                                            .text_size(TYPE_HEADER)
                                            .line_height(px(14.0))
                                            .font_weight(FontWeight::SEMIBOLD)
                                            .text_color(silver_muted())
                                            .child("ACTIVE THROUGHPUT"),
                                    )
                                    .child(div().w(px(6.0)).h(px(6.0)).bg(if active_slots > 0 {
                                        selenium_amber()
                                    } else {
                                        graphite_border()
                                    })),
                            )
                            .child(
                                div()
                                    .font_family(MONO_FONT)
                                    .text_size(TYPE_DISPLAY)
                                    .line_height(px(22.0))
                                    .font_weight(FontWeight::BOLD)
                                    .text_color(silver_text())
                                    .child(format_speed(total_speed)),
                            )
                            .child(
                                div()
                                    .font_family(MONO_FONT)
                                    .text_size(TYPE_MICRO)
                                    .line_height(px(14.0))
                                    .text_color(silver_muted())
                                    .child(format!(
                                        "{:.1} Mbps",
                                        (total_speed as f64 * 8.0) / 1_000_000.0
                                    )),
                            )
                            .child(
                                div()
                                    .pt_1p5()
                                    .mt_1()
                                    .border_t_1()
                                    .border_color(graphite_border())
                                    .flex()
                                    .flex_col()
                                    .gap_1()
                                    .child(
                                        div()
                                            .flex()
                                            .items_center()
                                            .justify_between()
                                            .font_family(MONO_FONT)
                                            .text_size(TYPE_MICRO)
                                            .line_height(px(14.0))
                                            .child(
                                                div()
                                                    .text_color(silver_muted())
                                                    .child("QUEUE SLOTS"),
                                            )
                                            .child(
                                                div()
                                                    .font_weight(FontWeight::SEMIBOLD)
                                                    .text_color(silver_text())
                                                    .child(format!(
                                                        "{active_slots} / {}",
                                                        self.queue.settings.max_concurrent_downloads
                                                    )),
                                            ),
                                    )
                                    .child(
                                        div().flex().gap_1().children(
                                            (0..self.queue.settings.max_concurrent_downloads).map(|i| {
                                                div().h(px(5.0)).flex_1().bg(if i < active_slots {
                                                    selenium_amber()
                                                } else {
                                                    graphite_border()
                                                })
                                            }),
                                        ),
                                    ),
                            ),
                    )
                    .child(
                        div()
                            .flex()
                            .items_center()
                            .gap_1p5()
                            .child(
                                div().flex_1().child(
                                    Button::new("open-settings")
                                        .small()
                                        .w_full()
                                        .icon(IconName::Settings)
                                        .label("Engine Settings")
                                        .on_click(cx.listener(|app, _, window, cx| {
                                            app.open_settings_modal(window, cx);
                                        })),
                                ),
                            )
                            .child(
                                Button::new("sidebar-collapse")
                                    .small()
                                    .ghost()
                                    .icon(IconName::PanelLeftClose)
                                    .tooltip("Collapse to Rail")
                                    .on_click(cx.listener(|app, _, _, cx| {
                                        app.sidebar_collapsed = true;
                                        app.motion
                                            .animate_sidebar(app.sidebar_width, RAIL_SIDEBAR_WIDTH);
                                        app.ensure_motion_loop(cx);
                                        cx.notify();
                                    })),
                            ),
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
