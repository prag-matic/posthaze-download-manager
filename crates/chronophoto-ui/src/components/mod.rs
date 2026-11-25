//! Fluent `RenderOnce` UI primitives, overlays, and sashes for Chronophoto.
//!
//! Every component reads [`crate::theme::ChronoTheme`] via `cx.chrono()` inside
//! [`gpui_kit::RenderOnce::render`], keeping call sites free of manual palette or font arguments.

mod grid;
mod overlays;
mod tones;

pub use grid::*;
pub use overlays::*;
pub use tones::*;

use gpui_kit::{App, SharedString};

use crate::theme::ChronoAppExt;

/// Resolves the active monospace font family from `ChronoTheme` or `gpui_kit::component::Theme`.
pub(crate) fn resolve_mono_font(cx: &App) -> SharedString {
    if let Some(ref font) = cx.chrono().typography.mono_font {
        return font.clone();
    }
    if let Some(theme) = cx.try_global::<gpui_kit::component::Theme>() {
        return theme.mono_font_family.clone();
    }
    "monospace".into()
}

#[cfg(all(test, feature = "test-support"))]
mod gpui_tests {
    use super::*;
    use crate::theme::apply_theme;
    use gpui_kit::assets::{IconName, IconNamed};
    use gpui_kit::component::input::InputState;
    use gpui_kit::{
        AppContext, IntoElement, ParentElement, SharedString, TestAppContext, WindowAppearance,
        div, px,
    };

    #[derive(Clone, Copy)]
    enum CustomIcon {
        Logo,
    }

    impl IconNamed for CustomIcon {
        fn path(self) -> SharedString {
            match self {
                Self::Logo => "brand/logo.svg".into(),
            }
        }
    }

    #[gpui_kit::test]
    fn components_render_with_chrono_theme_and_custom_icons(cx: &mut TestAppContext) {
        let (window, cx) = cx.add_window_view(|window, cx| {
            gpui_kit::init(cx);
            apply_theme("chronophotographic", WindowAppearance::Dark, false, cx);

            let input_state = cx.new(|cx| InputState::new(window, cx));

            struct GalleryView {
                input_state: gpui_kit::Entity<InputState>,
            }

            impl gpui_kit::Render for GalleryView {
                fn render(
                    &mut self,
                    _window: &mut gpui_kit::Window,
                    _cx: &mut gpui_kit::Context<Self>,
                ) -> impl IntoElement {
                    div()
                        .child(
                            BrandPlate::new("CHRONO")
                                .logo_icon(CustomIcon::Logo)
                                .subtitle("v0.1.0"),
                        )
                        .child(
                            GridSectionHeader::new("SECTION")
                                .tone(GridTone::Surface)
                                .trailing(GridBadge::new("3").tone(BadgeTone::Accent)),
                        )
                        .child(
                            GridCellButton::new("btn-custom")
                                .icon(CustomIcon::Logo)
                                .label("Custom")
                                .tone(GridTone::Primary)
                                .border(CellBorder::Both),
                        )
                        .child(
                            GridBoxedButton::new("btn-boxed")
                                .icon(IconName::Plus)
                                .label("Add")
                                .height(px(30.0))
                                .compact(true),
                        )
                        .child(
                            GridNavRow::new("nav-all", IconName::Folder, "ALL")
                                .selected(true)
                                .trailing_count(Some(12))
                                .tone(NavRowTone::SurfaceSelection),
                        )
                        .child(GridKvRow::new("KEY", GridStatusDot::new(gpui_kit::red())))
                        .child(GridInputBox::new(&self.input_state))
                        .child(GridCompactInputRow::new("URL", &self.input_state))
                        .child(
                            GridDropdownSurface::new("dd").child(
                                GridCheckOptionRow::new("opt-1", "Option 1").selected(true),
                            ),
                        )
                        .child(GridProgressTrack::new().height(px(8.0)))
                        .child(
                            AlertBanner::new("Something failed")
                                .tone(AlertTone::Danger)
                                .action(
                                    GridCellButton::new("dismiss")
                                        .icon(IconName::X)
                                        .tone(GridTone::Danger),
                                ),
                        )
                        .child(
                            SplitSash::new("sash", SashAxis::Vertical)
                                .active(true)
                                .snapped(true)
                                .readout("240 px · DEFAULT"),
                        )
                        .child(ModalBackdrop::new(
                            ModalPlate::new(px(400.0))
                                .child(div().child("Body"))
                                .footer(
                                    GridModalFooter::new().child(
                                        GridCellButton::new("ok").label("OK"),
                                    ),
                                ),
                        ))
                }
            }

            GalleryView { input_state }
        });

        let _ = (window, cx);
    }
}
