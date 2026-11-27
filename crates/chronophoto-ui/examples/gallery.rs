//! Isolated visual workbench for `chronophoto-ui`.
//!
//! Run with:
//! `cargo run -p chronophoto-ui --example gallery --features inspector`

use std::time::{Duration, Instant};

use chronophoto_ui::assets::CompositeAssetSource;
use chronophoto_ui::components::{
    AlertBanner, AlertTone, BadgeTone, BrandPlate, CellBorder, GridBadge, GridBoxedButton,
    GridCellButton, GridCheckOptionRow, GridCompactInputRow, GridDropdownSurface, GridInputBox,
    GridKvRow, GridModalFooter, GridNavRow, GridProgressTrack, GridSectionHeader, GridSelectionBar,
    GridStatusDot, GridTone, ModalBackdrop, ModalPlate, NavRowTone, SashAxis, SplitSash,
};
use chronophoto_ui::motion::{DecayPulse, InterpolatedScalar, PanelMotion};
use chronophoto_ui::palette::{
    ACCENT_PRESETS, BUILTIN_THEME_PRESETS, ThemeCatalog, ThemePalette,
};
use chronophoto_ui::platform::{apply_window_backdrop, initial_os_appearance, mica_availability};
use chronophoto_ui::theme::{ChronoAppExt, ChronoTheme, ThemeTypography};
use chronophoto_ui::tokens::{TYPE_BODY, TYPE_DISPLAY, TYPE_HEADER, TYPE_MICRO};
use gpui_kit::assets::{IconName, IconNamed};
use gpui_kit::component::TitleBar;
use gpui_kit::component::input::InputState;
use gpui_kit::prelude::FluentBuilder;
use gpui_kit::*;

const BRAND_MARK_SVG: &[u8] = br#"<svg xmlns="http://www.w3.org/2000/svg" viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="2"><rect x="3" y="3" width="18" height="18"/><line x1="9" y1="3" x2="9" y2="21"/><line x1="15" y1="3" x2="15" y2="21"/><line x1="3" y1="12" x2="21" y2="12"/></svg>"#;

const GALLERY_ASSETS: &[(&str, &[u8])] = &[("brand/chronophoto.svg", BRAND_MARK_SVG)];

const UI_SCALE_MIN: f32 = 0.90;
const UI_SCALE_MAX: f32 = 1.25;
const DEFAULT_SIDEBAR_WIDTH: f32 = 256.0;
const MIN_SIDEBAR_WIDTH: f32 = 196.0;
const MAX_SIDEBAR_WIDTH: f32 = 380.0;
const DEFAULT_DRAWER_HEIGHT: f32 = 132.0;
const MIN_DRAWER_HEIGHT: f32 = 72.0;
const MAX_DRAWER_HEIGHT: f32 = 240.0;
const SASH_DETENT_SNAP_PX: f32 = 10.0;

#[derive(Clone, Copy)]
enum GalleryIcon {
    BrandMark,
}

impl IconNamed for GalleryIcon {
    fn path(self) -> SharedString {
        match self {
            Self::BrandMark => "brand/chronophoto.svg".into(),
        }
    }
}

/// Pure workbench configuration state for live theme, accent, scale, and Mica controls.
#[derive(Debug, Clone, PartialEq)]
struct GalleryConfig {
    theme_id: &'static str,
    accent_id: Option<&'static str>,
    ui_scale: f32,
    simulated_mica_available: bool,
    os_appearance: WindowAppearance,
}

impl GalleryConfig {
    fn new(os_appearance: WindowAppearance, mica_available: bool) -> Self {
        Self {
            theme_id: BUILTIN_THEME_PRESETS[0].id,
            accent_id: None,
            ui_scale: 1.0,
            simulated_mica_available: mica_available,
            os_appearance,
        }
    }

    fn set_ui_scale(&mut self, scale: f32) {
        self.ui_scale = clamp_ui_scale(scale);
    }

    fn resolve_palette(&self) -> ThemePalette {
        ThemeCatalog::new().resolve_palette(
            self.theme_id,
            self.accent_id,
            self.os_appearance,
            self.simulated_mica_available,
        )
    }
}

fn clamp_ui_scale(scale: f32) -> f32 {
    (scale.clamp(UI_SCALE_MIN, UI_SCALE_MAX) * 100.0).round() / 100.0
}

fn snap_with_detent(value: f32, default: f32, min: f32, max: f32) -> (f32, bool) {
    let clamped = value.clamp(min, max);
    if (clamped - default).abs() <= SASH_DETENT_SNAP_PX {
        (default, true)
    } else {
        (clamped, false)
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum GallerySection {
    All,
    Buttons,
    BadgesAndStatus,
    Navigation,
    OverlaysAndMotion,
}

#[derive(Debug, Clone, Copy)]
enum ActiveDrag {
    Sidebar { start_x: f32, start_width: f32 },
    Drawer { start_y: f32, start_height: f32 },
}

struct GalleryApp {
    config: GalleryConfig,
    section: GallerySection,
    sidebar_width: f32,
    sidebar_snapped: bool,
    drawer_height: f32,
    drawer_snapped: bool,
    active_drag: Option<ActiveDrag>,
    motion: PanelMotion,
    pulse: DecayPulse,
    scalar: InterpolatedScalar,
    animating: bool,
    modal_open: bool,
    dropdown_selected: [bool; 3],
    input_state: Entity<InputState>,
    compact_input_state: Entity<InputState>,
}

impl GalleryApp {
    fn new(window: &mut Window, cx: &mut Context<Self>) -> Self {
        let initial_mica = mica_availability().is_ok();
        let config = GalleryConfig::new(initial_os_appearance(), initial_mica);

        let input_state = cx.new(|cx| {
            let mut state = InputState::new(window, cx);
            state.set_placeholder("Filter tokens or type sample text...", window, cx);
            state
        });
        let compact_input_state = cx.new(|cx| {
            let mut state = InputState::new(window, cx);
            state.set_placeholder("https://example.invalid/archive.tar.zst", window, cx);
            state
        });

        let app = Self {
            config,
            section: GallerySection::All,
            sidebar_width: DEFAULT_SIDEBAR_WIDTH,
            sidebar_snapped: true,
            drawer_height: DEFAULT_DRAWER_HEIGHT,
            drawer_snapped: true,
            active_drag: None,
            motion: PanelMotion::new(),
            pulse: DecayPulse::new(600.0),
            scalar: InterpolatedScalar::new(0.0, 10.0, 0.01),
            animating: false,
            modal_open: false,
            dropdown_selected: [true, false, true],
            input_state,
            compact_input_state,
        };
        app.apply_theme_to_window(window, cx);
        app
    }

    fn apply_theme_to_window(&self, window: &mut Window, cx: &mut App) {
        let palette = self.config.resolve_palette();
        let typography = ThemeTypography {
            ui_scale: self.config.ui_scale,
            ..ThemeTypography::default()
        };
        ChronoTheme::apply(palette, typography, cx);
        let _ = apply_window_backdrop(window, palette.backdrop, palette.tone);
        window.refresh();
    }

    fn ensure_animation_loop(&mut self, cx: &mut Context<Self>) {
        if self.animating {
            return;
        }
        self.animating = true;
        cx.spawn(async move |this, cx| {
            loop {
                cx.background_executor()
                    .timer(Duration::from_millis(16))
                    .await;
                let keep_going = this
                    .update(cx, |app, cx| {
                        let now = Instant::now();
                        let panel_active = app.motion.step(now);
                        let pulse_active = app.pulse.step(now);
                        let scalar_active = app.scalar.step(1.0 / 60.0);
                        cx.notify();
                        if !panel_active && !pulse_active && !scalar_active {
                            app.animating = false;
                            false
                        } else {
                            true
                        }
                    })
                    .unwrap_or(false);
                if !keep_going {
                    break;
                }
            }
        })
        .detach();
    }

    fn trigger_pulse(&mut self, cx: &mut Context<Self>) {
        let now = Instant::now();
        self.pulse.trigger(now);
        self.motion.trigger_sash_detent(now);
        let next_target = if self.scalar.target() > 0.5 { 0.0 } else { 1.0 };
        self.scalar.set_target(next_target);
        self.ensure_animation_loop(cx);
        cx.notify();
    }

    fn toggle_sidebar_preset(&mut self, cx: &mut Context<Self>) {
        let from = self.sidebar_width;
        let to = if (self.sidebar_width - DEFAULT_SIDEBAR_WIDTH).abs() < 4.0 {
            320.0
        } else {
            DEFAULT_SIDEBAR_WIDTH
        };
        self.sidebar_width = to;
        self.sidebar_snapped = (to - DEFAULT_SIDEBAR_WIDTH).abs() < 1.0;
        self.motion.animate_sidebar(from, to);
        self.ensure_animation_loop(cx);
        cx.notify();
    }

    fn render_controls_bar(&self, cx: &mut Context<Self>) -> impl IntoElement {
        let palette = cx.chrono().palette;
        let scale_pct = (self.config.ui_scale * 100.0).round() as i32;
        let is_dark = matches!(
            self.config.os_appearance,
            WindowAppearance::Dark | WindowAppearance::VibrantDark
        );

        div()
            .w_full()
            .bg(palette.chrome_bg.hsla())
            .border_b_1()
            .border_color(palette.chrome_border.hsla())
            .flex()
            .flex_col()
            .child(
                div()
                    .h(px(36.0))
                    .px_3()
                    .border_b_1()
                    .border_color(palette.chrome_border.hsla())
                    .flex()
                    .items_center()
                    .justify_between()
                    .gap_2()
                    .child(
                        BrandPlate::new("CHRONOPHOTO GALLERY")
                            .logo_icon(GalleryIcon::BrandMark)
                            .subtitle("v0.1.0 workbench"),
                    )
                    .child(
                        div()
                            .flex()
                            .items_center()
                            .gap_1p5()
                            .when(cfg!(feature = "inspector"), |el| {
                                el.child(GridBadge::new("INSPECTOR").tone(BadgeTone::Accent))
                            })
                            .child(
                                GridBoxedButton::new("toggle-appearance")
                                    .icon(if is_dark {
                                        IconName::Moon
                                    } else {
                                        IconName::Sun
                                    })
                                    .label(if is_dark { "OS: DARK" } else { "OS: LIGHT" })
                                    .tone(GridTone::Chrome)
                                    .compact(true)
                                    .on_click(cx.listener(|app, _, window, cx| {
                                        app.config.os_appearance = match app.config.os_appearance {
                                            WindowAppearance::Dark
                                            | WindowAppearance::VibrantDark => {
                                                WindowAppearance::Light
                                            }
                                            _ => WindowAppearance::Dark,
                                        };
                                        app.apply_theme_to_window(window, cx);
                                        cx.notify();
                                    })),
                            )
                            .child(
                                GridBoxedButton::new("toggle-mica")
                                    .icon(IconName::Layers)
                                    .label(if self.config.simulated_mica_available {
                                        "MICA: ON"
                                    } else {
                                        "MICA: FALLBACK"
                                    })
                                    .tone(if self.config.simulated_mica_available {
                                        GridTone::Primary
                                    } else {
                                        GridTone::Chrome
                                    })
                                    .compact(true)
                                    .on_click(cx.listener(|app, _, window, cx| {
                                        app.config.simulated_mica_available =
                                            !app.config.simulated_mica_available;
                                        app.apply_theme_to_window(window, cx);
                                        cx.notify();
                                    })),
                            )
                            .child(
                                GridBoxedButton::new("scale-down")
                                    .label("A-")
                                    .tone(GridTone::Chrome)
                                    .compact(true)
                                    .on_click(cx.listener(|app, _, window, cx| {
                                        let next = app.config.ui_scale - 0.05;
                                        app.config.set_ui_scale(next);
                                        app.apply_theme_to_window(window, cx);
                                        cx.notify();
                                    })),
                            )
                            .child(
                                GridBadge::new(format!("{scale_pct}%")).tone(BadgeTone::Neutral),
                            )
                            .child(
                                GridBoxedButton::new("scale-up")
                                    .label("A+")
                                    .tone(GridTone::Chrome)
                                    .compact(true)
                                    .on_click(cx.listener(|app, _, window, cx| {
                                        let next = app.config.ui_scale + 0.05;
                                        app.config.set_ui_scale(next);
                                        app.apply_theme_to_window(window, cx);
                                        cx.notify();
                                    })),
                            )
                            .child(
                                GridBoxedButton::new("open-modal")
                                    .icon(IconName::Square)
                                    .label("MODAL")
                                    .tone(GridTone::Primary)
                                    .compact(true)
                                    .on_click(cx.listener(|app, _, _, cx| {
                                        app.modal_open = true;
                                        cx.notify();
                                    })),
                            ),
                    ),
            )
            .child(
                div()
                    .px_3()
                    .py_1p5()
                    .border_b_1()
                    .border_color(palette.chrome_border.hsla())
                    .flex()
                    .flex_wrap()
                    .items_center()
                    .gap_1p5()
                    .child(
                        div()
                            .text_size(cx.chrono().typography.scale(TYPE_MICRO))
                            .text_color(palette.chrome_muted.hsla())
                            .mr_1()
                            .child("THEME:"),
                    )
                    .children(BUILTIN_THEME_PRESETS.iter().enumerate().map(|(idx, preset)| {
                        let active = self.config.theme_id == preset.id;
                        let preset_id = preset.id;
                        GridBoxedButton::new(("theme", idx))
                            .label(preset.name)
                            .height(px(24.0))
                            .compact(true)
                            .tone(if active {
                                GridTone::Primary
                            } else {
                                GridTone::Chrome
                            })
                            .on_click(cx.listener(move |app, _, window, cx| {
                                app.config.theme_id = preset_id;
                                app.apply_theme_to_window(window, cx);
                                cx.notify();
                            }))
                    })),
            )
            .child(
                div()
                    .px_3()
                    .py_1p5()
                    .flex()
                    .flex_wrap()
                    .items_center()
                    .gap_1p5()
                    .child(
                        div()
                            .text_size(cx.chrono().typography.scale(TYPE_MICRO))
                            .text_color(palette.chrome_muted.hsla())
                            .mr_1()
                            .child("ACCENT:"),
                    )
                    .child(
                        GridBoxedButton::new("accent-default")
                            .label("Preset Default")
                            .height(px(24.0))
                            .compact(true)
                            .tone(if self.config.accent_id.is_none() {
                                GridTone::Primary
                            } else {
                                GridTone::Chrome
                            })
                            .on_click(cx.listener(|app, _, window, cx| {
                                app.config.accent_id = None;
                                app.apply_theme_to_window(window, cx);
                                cx.notify();
                            })),
                    )
                    .children(ACCENT_PRESETS.iter().enumerate().map(|(idx, accent)| {
                        let active = self.config.accent_id == Some(accent.id);
                        let accent_id = accent.id;
                        GridBoxedButton::new(("accent", idx))
                            .label(accent.name)
                            .height(px(24.0))
                            .compact(true)
                            .tone(if active {
                                GridTone::Primary
                            } else {
                                GridTone::Chrome
                            })
                            .on_click(cx.listener(move |app, _, window, cx| {
                                app.config.accent_id = Some(accent_id);
                                app.apply_theme_to_window(window, cx);
                                cx.notify();
                            }))
                    })),
            )
    }

    fn render_sidebar(&self, width: f32, cx: &mut Context<Self>) -> impl IntoElement {
        let palette = cx.chrono().palette;
        let nav_items = [
            (GallerySection::All, IconName::LayoutGrid, "ALL SPECIMENS", Some(18)),
            (GallerySection::Buttons, IconName::MousePointer, "CELL BUTTONS", Some(5)),
            (
                GallerySection::BadgesAndStatus,
                IconName::Tag,
                "BADGES & STATUS",
                Some(5),
            ),
            (GallerySection::Navigation, IconName::List, "NAV & DROPDOWN", Some(4)),
            (
                GallerySection::OverlaysAndMotion,
                IconName::Sparkles,
                "MOTION & SASHES",
                None,
            ),
        ];

        div()
            .w(px(width))
            .h_full()
            .flex_shrink_0()
            .bg(palette.chrome_bg.hsla())
            .flex()
            .flex_col()
            .child(
                GridSectionHeader::new("SPECIMEN INDEX")
                    .tone(GridTone::Chrome)
                    .trailing(GridBadge::new(format!("{}px", width.round() as i32)).tone(BadgeTone::Muted)),
            )
            .children(nav_items.into_iter().enumerate().map(|(idx, (sec, icon, label, count))| {
                let selected = self.section == sec;
                GridNavRow::new(("nav", idx), icon, label)
                    .selected(selected)
                    .trailing_count(count)
                    .tone(NavRowTone::Chrome)
                    .on_click(cx.listener(move |app, _, _, cx| {
                        app.section = sec;
                        cx.notify();
                    }))
            }))
            .child(
                GridSectionHeader::new("SCALE PRESETS").tone(GridTone::Chrome),
            )
            .child(
                div()
                    .p_2()
                    .flex()
                    .flex_wrap()
                    .gap_1p5()
                    .children([0.90_f32, 1.00, 1.10, 1.25].into_iter().map(|scale| {
                        let active = (self.config.ui_scale - scale).abs() < 0.005;
                        let pct = (scale * 100.0).round() as i32;
                        GridBoxedButton::new(("scale-preset", pct as usize))
                            .label(format!("{pct}%"))
                            .height(px(24.0))
                            .compact(true)
                            .tone(if active {
                                GridTone::Primary
                            } else {
                                GridTone::Chrome
                            })
                            .on_click(cx.listener(move |app, _, window, cx| {
                                app.config.set_ui_scale(scale);
                                app.apply_theme_to_window(window, cx);
                                cx.notify();
                            }))
                    })),
            )
            .child(div().flex_1())
            .child(
                GridKvRow::new("BACKDROP", format!("{:?}", palette.backdrop)).label_width(96.0),
            )
            .child(
                GridKvRow::new("TONE", format!("{:?}", palette.tone)).label_width(96.0),
            )
    }

    fn render_specimen_canvas(&self, cx: &mut Context<Self>) -> impl IntoElement {
        let palette = cx.chrono().palette;
        let show_all = self.section == GallerySection::All;
        let show_buttons = show_all || self.section == GallerySection::Buttons;
        let show_badges = show_all || self.section == GallerySection::BadgesAndStatus;
        let show_nav = show_all || self.section == GallerySection::Navigation;
        let show_motion = show_all || self.section == GallerySection::OverlaysAndMotion;

        let now = Instant::now();
        let pulse_val = self.pulse.intensity(now);
        let detent_val = self.motion.sash_detent_flash(now);
        let scalar_val = self.scalar.current() as f32;

        div()
            .id("specimen-scroll")
            .flex_1()
            .min_w_0()
            .h_full()
            .overflow_y_scroll()
            .bg(palette.surface_bg.hsla())
            .p_4()
            .flex()
            .flex_col()
            .gap_4()
            .when(show_all, |el| {
                el.child(
                    div()
                        .border_1()
                        .border_color(palette.surface_border.hsla())
                        .flex()
                        .flex_col()
                        .child(
                            GridSectionHeader::new("ALERT BANNERS (ALL TONES)")
                                .tone(GridTone::Surface)
                                .trailing(GridBadge::new("3 TONES").tone(BadgeTone::Accent)),
                        )
                        .child(
                            div()
                                .p_3()
                                .flex()
                                .flex_col()
                                .gap_2()
                                .child(
                                    AlertBanner::new("Danger: DWM backdrop rejected or checksum mismatch.")
                                        .id("alert-danger")
                                        .tone(AlertTone::Danger)
                                        .tooltip("AlertTone::Danger specimen")
                                        .action(
                                            GridCellButton::new("alert-danger-action")
                                                .icon(IconName::RotateCcw)
                                                .label("RETRY")
                                                .tone(GridTone::Danger)
                                                .border(CellBorder::Left),
                                        ),
                                )
                                .child(
                                    AlertBanner::new("Success: Palette contrast gates verified (>= 4.5:1).")
                                        .id("alert-success")
                                        .tone(AlertTone::Success)
                                        .action(
                                            GridCellButton::new("alert-success-action")
                                                .icon(IconName::Check)
                                                .label("OK")
                                                .tone(GridTone::Chrome)
                                                .border(CellBorder::Left),
                                        ),
                                )
                                .child(
                                    AlertBanner::new("Accent: Live theme and scale changes synchronize across windows.")
                                        .id("alert-accent")
                                        .tone(AlertTone::Accent),
                                ),
                        ),
                )
            })
            .when(show_buttons, |el| {
                let tones = [
                    ("Surface", GridTone::Surface),
                    ("SurfaceSubtle", GridTone::SurfaceSubtle),
                    ("Chrome", GridTone::Chrome),
                    ("Primary", GridTone::Primary),
                    ("Danger", GridTone::Danger),
                ];
                let borders = [
                    ("None", CellBorder::None),
                    ("Left", CellBorder::Left),
                    ("Right", CellBorder::Right),
                    ("Both", CellBorder::Both),
                ];

                el.child(
                    div()
                        .border_1()
                        .border_color(palette.surface_border.hsla())
                        .flex()
                        .flex_col()
                        .child(
                            GridSectionHeader::new("GRID CELL BUTTONS (TONE x BORDER)")
                                .tone(GridTone::Surface)
                                .trailing(GridBadge::new("5 x 4").tone(BadgeTone::Neutral)),
                        )
                        .children(tones.into_iter().enumerate().map(|(row_idx, (tone_name, tone))| {
                            div()
                                .h(px(32.0))
                                .border_b_1()
                                .border_color(palette.surface_border.hsla())
                                .flex()
                                .items_center()
                                .child(
                                    div()
                                        .w(px(120.0))
                                        .px_2()
                                        .text_size(cx.chrono().typography.scale(TYPE_MICRO))
                                        .text_color(palette.surface_muted.hsla())
                                        .child(tone_name),
                                )
                                .children(borders.into_iter().enumerate().map(move |(col_idx, (border_name, border))| {
                                    GridCellButton::new(("cell-btn", row_idx * 4 + col_idx))
                                        .icon(IconName::Plus)
                                        .label(border_name)
                                        .tooltip(format!("{tone_name} / {border_name}"))
                                        .tone(tone)
                                        .border(border)
                                }))
                        }))
                        .child(
                            div()
                                .p_3()
                                .flex()
                                .flex_wrap()
                                .items_center()
                                .gap_2()
                                .children(tones.into_iter().enumerate().map(|(idx, (tone_name, tone))| {
                                    GridBoxedButton::new(("boxed-btn", idx))
                                        .icon(IconName::Sparkles)
                                        .label(format!("Boxed {tone_name}"))
                                        .tone(tone)
                                        .height(px(28.0))
                                })),
                        ),
                )
            })
            .when(show_badges, |el| {
                let badge_tones = [
                    ("NEUTRAL", BadgeTone::Neutral),
                    ("MUTED", BadgeTone::Muted),
                    ("ACCENT", BadgeTone::Accent),
                    ("SUCCESS", BadgeTone::Success),
                    ("DANGER", BadgeTone::Danger),
                ];

                el.child(
                    div()
                        .border_1()
                        .border_color(palette.surface_border.hsla())
                        .flex()
                        .flex_col()
                        .child(
                            GridSectionHeader::new("BADGES, STATUS DOTS & PROGRESS TRACK")
                                .tone(GridTone::Surface),
                        )
                        .child(
                            div()
                                .p_3()
                                .flex()
                                .flex_wrap()
                                .items_center()
                                .gap_2()
                                .children(badge_tones.into_iter().map(|(label, tone)| {
                                    GridBadge::new(label).tone(tone)
                                }))
                                .child(GridStatusDot::new(palette.accent.hsla()).size(10.0))
                                .child(GridStatusDot::new(palette.success.hsla()).size(10.0))
                                .child(GridStatusDot::new(palette.info.hsla()).size(10.0).round(false))
                                .child(GridStatusDot::new(palette.danger.hsla()).size(10.0).round(false)),
                        )
                        .child(
                            div()
                                .px_3()
                                .pb_3()
                                .flex()
                                .flex_col()
                                .gap_2()
                                .child(
                                    GridProgressTrack::new()
                                        .height(px(8.0))
                                        .border_color(palette.surface_border.hsla())
                                        .child(div().w(px(140.0)).h_full().bg(palette.success.hsla()))
                                        .child(div().w(px(90.0)).h_full().bg(palette.accent.hsla()))
                                        .child(div().w(px(45.0)).h_full().bg(palette.info.hsla())),
                                )
                                .child(
                                    GridKvRow::new("DISPLAY TYPE", {
                                        div()
                                            .text_size(cx.chrono().typography.scale(TYPE_DISPLAY))
                                            .text_color(palette.surface_text.hsla())
                                            .child("Chronophotographic Interval Plate")
                                    })
                                    .tone(GridTone::Surface),
                                )
                                .child(
                                    GridKvRow::new("BODY TYPE", {
                                        div()
                                            .text_size(cx.chrono().typography.scale(TYPE_BODY))
                                            .text_color(palette.surface_text.hsla())
                                            .child("1px construction-grid rules & tabular monospace")
                                    })
                                    .tone(GridTone::Surface),
                                ),
                        ),
                )
            })
            .when(show_nav, |el| {
                el.child(
                    div()
                        .border_1()
                        .border_color(palette.surface_border.hsla())
                        .flex()
                        .flex_col()
                        .child(
                            GridSectionHeader::new("NAV ROWS, SELECTION BARS, DROPDOWN & INPUTS")
                                .tone(GridTone::Surface),
                        )
                        .child(
                            div()
                                .p_3()
                                .flex()
                                .flex_wrap()
                                .gap_4()
                                .child(
                                    div()
                                        .w(px(240.0))
                                        .border_1()
                                        .border_color(palette.surface_border.hsla())
                                        .flex()
                                        .flex_col()
                                        .child(
                                            GridNavRow::new(
                                                "demo-nav-chrome",
                                                IconName::Folder,
                                                "CHROME SELECTED",
                                            )
                                            .selected(true)
                                            .trailing_count(Some(24))
                                            .tone(NavRowTone::Chrome),
                                        )
                                        .child(
                                            GridNavRow::new(
                                                "demo-nav-surface",
                                                IconName::Check,
                                                "SURFACE SELECTED",
                                            )
                                            .selected(true)
                                            .trailing_count(Some(8))
                                            .tone(NavRowTone::SurfaceSelection),
                                        )
                                        .child(
                                            GridNavRow::new(
                                                "demo-nav-idle",
                                                IconName::Clock,
                                                "UNSELECTED ROW",
                                            )
                                            .selected(false)
                                            .tone(NavRowTone::SurfaceSelection),
                                        ),
                                )
                                .child(
                                    div()
                                        .w(px(240.0))
                                        .child(
                                            GridDropdownSurface::new("demo-dropdown")
                                                .children(["Verify SHA-256", "Auto-resume", "Mica backdrop"].into_iter().enumerate().map(|(idx, label)| {
                                                    let checked = self.dropdown_selected[idx];
                                                    GridCheckOptionRow::new(("dd-opt", idx), label)
                                                        .selected(checked)
                                                        .trailing(GridBadge::new(if checked { "ON" } else { "OFF" }).tone(if checked { BadgeTone::Accent } else { BadgeTone::Muted }))
                                                        .on_click(cx.listener(move |app, _, _, cx| {
                                                            app.dropdown_selected[idx] = !app.dropdown_selected[idx];
                                                            cx.notify();
                                                        }))
                                                })),
                                        ),
                                )
                                .child(
                                    div()
                                        .h(px(34.0))
                                        .border_1()
                                        .border_color(palette.surface_border.hsla())
                                        .flex()
                                        .items_center()
                                        .gap_2()
                                        .px_2()
                                        .child(GridSelectionBar::new(true))
                                        .child(
                                            div()
                                                .text_size(cx.chrono().typography.scale(TYPE_MICRO))
                                                .text_color(palette.surface_text.hsla())
                                                .child("ACTIVE"),
                                        )
                                        .child(GridSelectionBar::new(false))
                                        .child(
                                            div()
                                                .text_size(cx.chrono().typography.scale(TYPE_MICRO))
                                                .text_color(palette.surface_muted.hsla())
                                                .child("IDLE"),
                                        ),
                                ),
                        )
                        .child(
                            div()
                                .px_3()
                                .pb_3()
                                .flex()
                                .flex_col()
                                .gap_2()
                                .child(
                                    div()
                                        .h(px(30.0))
                                        .px_2()
                                        .bg(palette.input_bg.hsla())
                                        .border_1()
                                        .border_color(palette.chrome_border.hsla())
                                        .flex()
                                        .items_center()
                                        .child(GridInputBox::new(&self.input_state)),
                                )
                                .child(
                                    GridCompactInputRow::new("SOURCE", &self.compact_input_state)
                                        .trailing_action(
                                            GridCellButton::new("compact-copy")
                                                .icon(IconName::Copy)
                                                .label("COPY")
                                                .tone(GridTone::Chrome)
                                                .border(CellBorder::Left),
                                        ),
                                ),
                        ),
                )
            })
            .when(show_motion, |el| {
                el.child(
                    div()
                        .border_1()
                        .border_color(palette.surface_border.hsla())
                        .flex()
                        .flex_col()
                        .child(
                            GridSectionHeader::new("MOTION PRIMITIVES (DECAY PULSE & INTERPOLATED SCALAR)")
                                .tone(GridTone::Surface)
                                .trailing(
                                    GridBadge::new(format!("PULSE {:.2}", pulse_val))
                                        .tone(BadgeTone::Accent),
                                ),
                        )
                        .child(
                            div()
                                .p_3()
                                .flex()
                                .flex_col()
                                .gap_2p5()
                                .child(
                                    div()
                                        .flex()
                                        .items_center()
                                        .gap_2()
                                        .child(
                                            GridBoxedButton::new("trigger-pulse")
                                                .icon(IconName::Zap)
                                                .label("TRIGGER PULSE & DETENT")
                                                .tone(GridTone::Primary)
                                                .on_click(cx.listener(|app, _, _, cx| {
                                                    app.trigger_pulse(cx);
                                                })),
                                        )
                                        .child(
                                            GridBoxedButton::new("animate-sidebar")
                                                .icon(IconName::PanelLeft)
                                                .label("ANIMATE SIDEBAR")
                                                .tone(GridTone::Chrome)
                                                .on_click(cx.listener(|app, _, _, cx| {
                                                    app.toggle_sidebar_preset(cx);
                                                })),
                                        ),
                                )
                                .child(
                                    GridProgressTrack::new()
                                        .height(px(10.0))
                                        .border_color(palette.surface_border.hsla())
                                        .child(
                                            div()
                                                .w(px(320.0 * pulse_val))
                                                .h_full()
                                                .bg(palette.accent.hsla()),
                                        ),
                                )
                                .child(
                                    GridProgressTrack::new()
                                        .height(px(10.0))
                                        .border_color(palette.surface_border.hsla())
                                        .child(
                                            div()
                                                .w(px(320.0 * scalar_val.clamp(0.0, 1.0)))
                                                .h_full()
                                                .bg(palette.info.hsla()),
                                        ),
                                )
                                .child(
                                    div()
                                        .text_size(cx.chrono().typography.scale(TYPE_MICRO))
                                        .text_color(palette.surface_muted.hsla())
                                        .child(format!(
                                            "DecayPulse: {:.2} | InterpolatedScalar: {:.2} | SashDetentFlash: {:.2}",
                                            pulse_val, scalar_val, detent_val
                                        )),
                                ),
                        ),
                )
            })
    }

    fn render_bottom_drawer(&self, height: f32, cx: &mut Context<Self>) -> impl IntoElement {
        let palette = cx.chrono().palette;
        let reason_note = match mica_availability() {
            Ok(()) => "Native Windows 11 Mica is available on this host.".to_string(),
            Err(reason) => reason.status_note().to_string(),
        };

        div()
            .h(px(height))
            .w_full()
            .flex_shrink_0()
            .bg(palette.chrome_bg.hsla())
            .flex()
            .flex_col()
            .child(
                GridSectionHeader::new("INSPECTOR & PLATFORM DIAGNOSTICS")
                    .tone(GridTone::Chrome)
                    .trailing(
                        GridBadge::new(format!("{}px", height.round() as i32))
                            .tone(BadgeTone::Neutral),
                    ),
            )
            .child(
                div()
                    .flex_1()
                    .p_3()
                    .flex()
                    .flex_col()
                    .gap_1p5()
                    .child(
                        div()
                            .text_size(cx.chrono().typography.scale(TYPE_HEADER))
                            .text_color(palette.chrome_text.hsla())
                            .child(format!(
                                "Active Theme: {} ({}) | Simulated Mica: {}",
                                palette.name, palette.id, self.config.simulated_mica_available
                            )),
                    )
                    .child(
                        div()
                            .text_size(cx.chrono().typography.scale(TYPE_MICRO))
                            .text_color(palette.chrome_muted.hsla())
                            .child(reason_note),
                    ),
            )
    }

    fn render_modal(&self, cx: &mut Context<Self>) -> impl IntoElement {
        let palette = cx.chrono().palette;
        let entity = cx.entity().clone();
        ModalBackdrop::new(
            ModalPlate::new(px(440.0))
                .child(
                    div()
                        .text_size(cx.chrono().typography.scale(TYPE_DISPLAY))
                        .font_weight(FontWeight::SEMIBOLD)
                        .text_color(palette.chrome_text.hsla())
                        .child("ModalPlate Specimen"),
                )
                .child(
                    div()
                        .text_size(cx.chrono().typography.scale(TYPE_BODY))
                        .text_color(palette.chrome_muted.hsla())
                        .child("Demonstrates ModalBackdrop scrim occlusion, entrance animation, and flush GridModalFooter."),
                )
                .child(GridKvRow::new("THEME", palette.name))
                .child(GridKvRow::new(
                    "UI SCALE",
                    format!("{}%", (self.config.ui_scale * 100.0).round() as i32),
                ))
                .footer(
                    GridModalFooter::new()
                        .child(
                            GridCellButton::new("modal-cancel")
                                .label("CLOSE")
                                .tone(GridTone::Chrome)
                                .border(CellBorder::Left)
                                .on_click(cx.listener(|app, _, _, cx| {
                                    app.modal_open = false;
                                    cx.notify();
                                })),
                        )
                        .child(
                            GridCellButton::new("modal-confirm")
                                .icon(IconName::Check)
                                .label("CONFIRM")
                                .tone(GridTone::Primary)
                                .border(CellBorder::Left)
                                .on_click(cx.listener(|app, _, _, cx| {
                                    app.modal_open = false;
                                    cx.notify();
                                })),
                        ),
                ),
        )
        .on_close(move |_, cx| {
            entity.update(cx, |app, cx| {
                app.modal_open = false;
                cx.notify();
            });
        })
    }
}

impl Render for GalleryApp {
    fn render(&mut self, _window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let now = Instant::now();
        let detent_flash = self.motion.sash_detent_flash(now);
        let sidebar_width = self.motion.current_sidebar_width(self.sidebar_width);
        let drawer_height = self.motion.current_inspector_height(self.drawer_height);
        let dragging_sidebar = matches!(self.active_drag, Some(ActiveDrag::Sidebar { .. }));
        let dragging_drawer = matches!(self.active_drag, Some(ActiveDrag::Drawer { .. }));

        div()
            .id("gallery-root")
            .size_full()
            .relative()
            .flex()
            .flex_col()
            .on_mouse_move(cx.listener(|app, ev: &MouseMoveEvent, _, cx| {
                let Some(drag) = app.active_drag else {
                    return;
                };
                match drag {
                    ActiveDrag::Sidebar {
                        start_x,
                        start_width,
                    } => {
                        let raw = start_width + (f32::from(ev.position.x) - start_x);
                        let (next, snapped) = snap_with_detent(
                            raw,
                            DEFAULT_SIDEBAR_WIDTH,
                            MIN_SIDEBAR_WIDTH,
                            MAX_SIDEBAR_WIDTH,
                        );
                        if snapped && !app.sidebar_snapped {
                            app.motion.trigger_sash_detent(Instant::now());
                            app.ensure_animation_loop(cx);
                        }
                        app.sidebar_width = next;
                        app.sidebar_snapped = snapped;
                        app.motion.set_sidebar_immediate();
                        cx.notify();
                    }
                    ActiveDrag::Drawer {
                        start_y,
                        start_height,
                    } => {
                        let raw = start_height + (start_y - f32::from(ev.position.y));
                        let (next, snapped) = snap_with_detent(
                            raw,
                            DEFAULT_DRAWER_HEIGHT,
                            MIN_DRAWER_HEIGHT,
                            MAX_DRAWER_HEIGHT,
                        );
                        if snapped && !app.drawer_snapped {
                            app.motion.trigger_sash_detent(Instant::now());
                            app.ensure_animation_loop(cx);
                        }
                        app.drawer_height = next;
                        app.drawer_snapped = snapped;
                        app.motion.set_inspector_immediate();
                        cx.notify();
                    }
                }
            }))
            .on_mouse_up(
                MouseButton::Left,
                cx.listener(|app, _, _, cx| {
                    if app.active_drag.is_some() {
                        app.active_drag = None;
                        cx.notify();
                    }
                }),
            )
            .child(self.render_controls_bar(cx))
            .child(
                div()
                    .flex_1()
                    .min_h_0()
                    .w_full()
                    .flex()
                    .child(self.render_sidebar(sidebar_width, cx))
                    .child(
                        SplitSash::new("sidebar-sash", SashAxis::Vertical)
                            .active(dragging_sidebar)
                            .snapped(self.sidebar_snapped)
                            .detent_flash(detent_flash)
                            .readout(format!(
                                "{} px{}",
                                sidebar_width.round() as i32,
                                if self.sidebar_snapped { " · DEFAULT" } else { "" }
                            ))
                            .on_mouse_down(cx.listener(|app, ev: &MouseDownEvent, _, cx| {
                                app.active_drag = Some(ActiveDrag::Sidebar {
                                    start_x: f32::from(ev.position.x),
                                    start_width: app.sidebar_width,
                                });
                                cx.notify();
                            }))
                            .on_double_click(cx.listener(|app, _, _, cx| {
                                let from = app.sidebar_width;
                                app.sidebar_width = DEFAULT_SIDEBAR_WIDTH;
                                app.sidebar_snapped = true;
                                app.motion.animate_sidebar(from, DEFAULT_SIDEBAR_WIDTH);
                                app.motion.trigger_sash_detent(Instant::now());
                                app.ensure_animation_loop(cx);
                                cx.notify();
                            })),
                    )
                    .child(self.render_specimen_canvas(cx)),
            )
            .child(
                SplitSash::new("drawer-sash", SashAxis::Horizontal)
                    .active(dragging_drawer)
                    .snapped(self.drawer_snapped)
                    .detent_flash(detent_flash)
                    .readout(format!(
                        "{} px{}",
                        drawer_height.round() as i32,
                        if self.drawer_snapped { " · DEFAULT" } else { "" }
                    ))
                    .on_mouse_down(cx.listener(|app, ev: &MouseDownEvent, _, cx| {
                        app.active_drag = Some(ActiveDrag::Drawer {
                            start_y: f32::from(ev.position.y),
                            start_height: app.drawer_height,
                        });
                        cx.notify();
                    }))
                    .on_double_click(cx.listener(|app, _, _, cx| {
                        let from = app.drawer_height;
                        app.drawer_height = DEFAULT_DRAWER_HEIGHT;
                        app.drawer_snapped = true;
                        app.motion.animate_inspector(from, DEFAULT_DRAWER_HEIGHT);
                        app.motion.trigger_sash_detent(Instant::now());
                        app.ensure_animation_loop(cx);
                        cx.notify();
                    })),
            )
            .child(self.render_bottom_drawer(drawer_height, cx))
            .when(self.modal_open, |el| el.child(self.render_modal(cx)))
    }
}

fn main() {
    gpui_kit::application()
        .with_assets(CompositeAssetSource::new(GALLERY_ASSETS))
        .run(|cx| {
            gpui_kit::init(cx);
            let initial_os = initial_os_appearance();
            let initial_mica = mica_availability().is_ok();
            let initial_config = GalleryConfig::new(initial_os, initial_mica);
            let initial_palette = initial_config.resolve_palette();
            ChronoTheme::apply(initial_palette, ThemeTypography::default(), cx);

            let window_options = WindowOptions {
                window_bounds: Some(WindowBounds::Windowed(Bounds::centered(
                    None,
                    size(px(1180.0), px(780.0)),
                    cx,
                ))),
                window_background: initial_palette.backdrop.to_window_background(),
                window_min_size: Some(size(px(900.0), px(600.0))),
                ..TitleBar::window_options()
            };

            gpui_kit::open_window(window_options, cx, |window, cx| {
                cx.new(|cx| GalleryApp::new(window, cx))
            })
            .expect("failed to open chronophoto-ui gallery window");
        });
}

#[cfg(test)]
mod tests {
    use super::*;
    use chronophoto_ui::palette::{MICA_DARK, MICA_DARK_FALLBACK, MICA_LIGHT, MICA_LIGHT_FALLBACK};

    #[::core::prelude::v1::test]
    fn ui_scale_clamps_to_supported_workbench_range() {
        assert_eq!(clamp_ui_scale(0.50), 0.90);
        assert_eq!(clamp_ui_scale(0.90), 0.90);
        assert_eq!(clamp_ui_scale(1.05), 1.05);
        assert_eq!(clamp_ui_scale(1.25), 1.25);
        assert_eq!(clamp_ui_scale(2.00), 1.25);
    }

    #[::core::prelude::v1::test]
    fn gallery_config_exercises_all_presets_accents_and_mica_fallback() {
        let mut config = GalleryConfig::new(WindowAppearance::Dark, true);
        for preset in BUILTIN_THEME_PRESETS {
            config.theme_id = preset.id;
            let resolved = config.resolve_palette();
            assert_eq!(
                resolved.id,
                if preset.id == "mica-system" {
                    "mica-dark"
                } else {
                    preset.id
                }
            );
        }

        config.theme_id = "mica-system";
        config.os_appearance = WindowAppearance::Light;
        config.simulated_mica_available = true;
        assert_eq!(config.resolve_palette(), MICA_LIGHT);

        config.simulated_mica_available = false;
        assert_eq!(config.resolve_palette(), MICA_LIGHT_FALLBACK);

        config.os_appearance = WindowAppearance::Dark;
        assert_eq!(config.resolve_palette(), MICA_DARK_FALLBACK);

        config.simulated_mica_available = true;
        assert_eq!(config.resolve_palette(), MICA_DARK);

        for accent in ACCENT_PRESETS {
            config.accent_id = Some(accent.id);
            let resolved = config.resolve_palette();
            assert_eq!(
                resolved.accent,
                chronophoto_ui::palette::RgbaHex::rgb(accent.accent)
            );
        }
    }

    #[::core::prelude::v1::test]
    fn sash_detent_snaps_within_radius_and_clamps_bounds() {
        assert_eq!(snap_with_detent(250.0, 256.0, 196.0, 380.0), (256.0, true));
        assert_eq!(snap_with_detent(300.0, 256.0, 196.0, 380.0), (300.0, false));
        assert_eq!(snap_with_detent(120.0, 256.0, 196.0, 380.0), (196.0, false));
        assert_eq!(snap_with_detent(500.0, 256.0, 196.0, 380.0), (380.0, false));
    }
}
