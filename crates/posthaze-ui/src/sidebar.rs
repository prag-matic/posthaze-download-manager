//! Left filter rail and live telemetry sidebar view for [PosthazeApp].

use chronophoto_ui::components::{
    CellBorder, GridCellButton, GridKvRow, GridNavRow, GridSectionHeader, GridSelectionBar,
    GridStatusDot, GridTone, NavRowTone,
};
use chronophoto_ui::theme::ChronoAppExt;
use chronophoto_ui::tokens::{TYPE_BODY, TYPE_MICRO};
use gpui_kit::assets::IconName;
use gpui_kit::component::tooltip::Tooltip;
use gpui_kit::component::{Icon, Theme};
use gpui_kit::*;

use crate::PosthazeApp;
use crate::view_model::{DownloadRowView, FilterTab, RAIL_SIDEBAR_WIDTH, format_speed};

impl PosthazeApp {
    pub(crate) fn render_sidebar(
        &self,
        all_rows: &[DownloadRowView],
        total_speed: u64,
        active_slots: usize,
        cx: &mut Context<Self>,
    ) -> impl IntoElement {
        let palette = cx.chrono().palette;
        let chrome_bg = palette.chrome_bg.hsla();
        let chrome_elevated = palette.chrome_elevated.hsla();
        let chrome_border = palette.chrome_border.hsla();
        let chrome_text = palette.chrome_text.hsla();
        let chrome_muted = palette.chrome_muted.hsla();
        let accent = palette.accent.hsla();
        let mono = Theme::global(cx).mono_font_family.clone();

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
                .bg(chrome_bg)
                .child(
                    div()
                        .w_full()
                        .flex()
                        .flex_col()
                        .children(tabs.into_iter().enumerate().map(|(i, tab)| {
                            let count = counts[i];
                            let selected = self.filter == tab;
                            let tab_icon = filter_tab_icon(tab);
                            let tab_color = if selected { accent } else { chrome_muted };
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
                                    chrome_elevated
                                } else {
                                    Hsla::transparent_black()
                                })
                                .border_b_1()
                                .border_color(chrome_border)
                                .hover(move |s| s.bg(chrome_elevated))
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
                                        .child(GridSelectionBar::new(selected)),
                                )
                                .child(Icon::new(tab_icon).size(px(16.0)).text_color(tab_color))
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
                                .border_color(chrome_border)
                                .flex()
                                .items_center()
                                .justify_center()
                                .child(
                                    GridStatusDot::new(if active_slots > 0 {
                                        accent
                                    } else {
                                        chrome_border
                                    })
                                    .size(6.0)
                                    .round(false),
                                ),
                        )
                        .child(
                            div()
                                .w_full()
                                .h(px(34.0))
                                .border_t_1()
                                .border_color(chrome_border)
                                .child(
                                    GridCellButton::new("open-settings")
                                        .icon(IconName::Settings)
                                        .tooltip("Engine Settings")
                                        .tone(GridTone::Chrome)
                                        .border(CellBorder::None)
                                        .compact(true)
                                        .on_click(cx.listener(|app, _, _, cx| {
                                            app.open_settings_window(
                                                crate::settings_modal::SettingsTab::General,
                                                cx,
                                            );
                                        })),
                                ),
                        )
                        .child(
                            div()
                                .w_full()
                                .h(px(34.0))
                                .border_t_1()
                                .border_color(chrome_border)
                                .child(
                                    GridCellButton::new("sidebar-expand")
                                        .icon(IconName::PanelLeftOpen)
                                        .tooltip("Expand Sidebar")
                                        .tone(GridTone::Chrome)
                                        .border(CellBorder::None)
                                        .compact(true)
                                        .on_click(cx.listener(|app, _, _, cx| {
                                            app.sidebar_collapsed = false;
                                            app.motion.animate_sidebar(
                                                RAIL_SIDEBAR_WIDTH,
                                                app.sidebar_width,
                                            );
                                            app.ensure_motion_loop(cx);
                                            cx.notify();
                                        })),
                                ),
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
            .bg(chrome_bg)
            // Filter Grid
            .child(
                div()
                    .w_full()
                    .flex()
                    .flex_col()
                    .child(GridSectionHeader::new("Queue filter").tone(GridTone::Chrome))
                    .children(tabs.into_iter().enumerate().map(|(i, tab)| {
                        let count = counts[i];
                        let selected = self.filter == tab;
                        GridNavRow::new(
                            SharedString::from(format!("filter-{}", tab.label())),
                            filter_tab_icon(tab),
                            tab.label(),
                        )
                        .selected(selected)
                        .trailing_count(Some(count))
                        .tone(NavRowTone::Chrome)
                        .on_click(cx.listener(move |app, _, _, cx| {
                            app.filter = tab;
                            cx.notify();
                        }))
                    })),
            )
            // Global Telemetry Grid & Footer Cells
            .child(
                div()
                    .w_full()
                    .flex()
                    .flex_col()
                    .border_t_1()
                    .border_color(chrome_border)
                    .child(GridSectionHeader::new("Telemetry").tone(GridTone::Chrome))
                    .child(
                        GridKvRow::new(
                            "Speed",
                            div()
                                .w_full()
                                .flex()
                                .items_center()
                                .justify_between()
                                .font_family(mono.clone())
                                .child(
                                    div()
                                        .text_size(TYPE_BODY)
                                        .font_weight(FontWeight::SEMIBOLD)
                                        .text_color(chrome_text)
                                        .child(format_speed(total_speed)),
                                )
                                .child(
                                    div()
                                        .text_size(TYPE_MICRO)
                                        .text_color(chrome_muted)
                                        .child(format!(
                                            "{:.1} Mbps",
                                            (total_speed as f64 * 8.0) / 1_000_000.0
                                        )),
                                ),
                        )
                        .label_width(76.0)
                        .tone(GridTone::Chrome),
                    )
                    .child(
                        GridKvRow::new(
                            "Slots",
                            div()
                                .w_full()
                                .flex()
                                .items_center()
                                .gap_2()
                                .child(
                                    div().flex_1().flex().gap_1().children(
                                        (0..self.queue.settings.max_concurrent_downloads).map(
                                            |i| {
                                                div().h(px(6.0)).flex_1().bg(if i < active_slots {
                                                    accent
                                                } else {
                                                    chrome_border
                                                })
                                            },
                                        ),
                                    ),
                                )
                                .child(
                                    div()
                                        .font_family(mono)
                                        .text_size(TYPE_MICRO)
                                        .font_weight(FontWeight::SEMIBOLD)
                                        .text_color(chrome_text)
                                        .child(format!(
                                            "{active_slots}/{}",
                                            self.queue.settings.max_concurrent_downloads
                                        )),
                                ),
                        )
                        .label_width(76.0)
                        .tone(GridTone::Chrome),
                    )
                    .child(
                        div()
                            .h(px(34.0))
                            .w_full()
                            .flex()
                            .items_center()
                            .child(
                                div().flex_1().h_full().child(
                                    GridCellButton::new("open-settings")
                                        .icon(IconName::Settings)
                                        .label("Settings")
                                        .tooltip("Engine Settings")
                                        .tone(GridTone::Chrome)
                                        .border(CellBorder::Right)
                                        .on_click(cx.listener(|app, _, _, cx| {
                                            app.open_settings_window(
                                                crate::settings_modal::SettingsTab::General,
                                                cx,
                                            );
                                        })),
                                ),
                            )
                            .child(
                                GridCellButton::new("sidebar-collapse")
                                    .icon(IconName::PanelLeftClose)
                                    .tooltip("Collapse to Rail")
                                    .tone(GridTone::Chrome)
                                    .border(CellBorder::None)
                                    .compact(true)
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
