//! GPUI global theme state (`ChronoTheme`), typography configuration, and `gpui_kit::component::Theme` synchronization.

use gpui_kit::component::{Theme, ThemeMode};
use gpui_kit::{App, Global, Hsla, Pixels, SharedString, WindowAppearance, px};

use crate::palette::{PaletteTone, ThemeBackdrop, ThemeCatalog, ThemePalette};
use crate::tokens::{RADIUS_CONTROL, RADIUS_MODAL};

/// Cross-platform typography configuration for [`ChronoTheme`].
///
/// `sans_font` defaults to `".SystemUIFont"` and `mono_font` defaults to `None`
/// so `gpui-kit` automatically resolves the platform UI sans and probes `"Consolas"`
/// (Windows), `"Menlo"` (macOS), or `"DejaVu Sans Mono"` (Linux).
#[derive(Debug, Clone, PartialEq)]
pub struct ThemeTypography {
    /// Optional UI sans-serif font family override (defaults to `Some(".SystemUIFont")`).
    pub sans_font: Option<SharedString>,
    /// Optional monospace font family override (`None` lets `gpui-kit` probe per OS).
    pub mono_font: Option<SharedString>,
    /// UI scale factor applied to scaled typography helpers (defaults to `1.0`).
    pub ui_scale: f32,
}

impl Default for ThemeTypography {
    fn default() -> Self {
        Self {
            sans_font: Some(".SystemUIFont".into()),
            mono_font: None,
            ui_scale: 1.0,
        }
    }
}

impl ThemeTypography {
    /// Scales a logical pixel size by [`Self::ui_scale`].
    pub fn scale(&self, size: Pixels) -> Pixels {
        px(f32::from(size) * self.ui_scale)
    }
}

/// GPUI [`Global`] holding the active resolved [`ThemePalette`] and [`ThemeTypography`].
#[derive(Debug, Clone)]
pub struct ChronoTheme {
    /// Active semantic color palette.
    pub palette: ThemePalette,
    /// Active typography and UI scale settings.
    pub typography: ThemeTypography,
}

impl Global for ChronoTheme {}

impl ChronoTheme {
    /// Constructs a [`ChronoTheme`] from a resolved palette and typography settings.
    pub const fn new(palette: ThemePalette, typography: ThemeTypography) -> Self {
        Self {
            palette,
            typography,
        }
    }

    /// Returns a reference to the active [`ChronoTheme`] global.
    #[inline(always)]
    pub fn global(cx: &App) -> &Self {
        cx.global::<Self>()
    }

    /// Applies `palette` and `typography` to [`ChronoTheme`] and synchronizes
    /// `gpui_kit::component::Theme` via `Theme::change` followed by `Theme::update`.
    pub fn apply(palette: ThemePalette, typography: ThemeTypography, cx: &mut App) {
        cx.set_global(Self::new(palette, typography.clone()));

        let mode = match palette.tone {
            PaletteTone::Dark => ThemeMode::Dark,
            PaletteTone::Light => ThemeMode::Light,
        };
        Theme::change(mode, None, cx);

        Theme::update(cx, |theme| {
            theme.radius = RADIUS_CONTROL;
            theme.radius_lg = RADIUS_MODAL;
            theme.window_radius = px(0.0);

            if palette.backdrop != ThemeBackdrop::Opaque {
                theme.background = Hsla::transparent_black();
                theme.tokens.background = Hsla::transparent_black().into();
            } else {
                let bg = palette.chrome_bg.hsla();
                theme.background = bg;
                theme.tokens.background = bg.into();
            }

            theme.foreground = palette.chrome_text.hsla();
            theme.border = palette.surface_border.hsla();
            theme.window_border = palette.chrome_border.hsla();
            theme.drag_border = palette.accent.hsla();
            theme.scrollbar = palette.chrome_bg.hsla();
            theme.scrollbar_thumb = palette.chrome_border.hsla();
            theme.scrollbar_thumb_hover = palette.chrome_muted.hsla();

            theme.input = palette.input_bg.hsla();
            theme.ring = palette.focus_ring.hsla();
            theme.selection = palette.accent.hsla().opacity(0.28);
            theme.caret = palette.accent.hsla();

            theme.popover = palette.popover_bg.hsla();
            theme.popover_foreground = palette.chrome_text.hsla();
            theme.title_bar = palette.titlebar_bg.hsla();
            theme.title_bar_border = palette.chrome_border.hsla();

            theme.muted = palette.chrome_elevated.hsla();
            theme.muted_foreground = palette.chrome_muted.hsla();

            theme.secondary = palette.chrome_elevated.hsla();
            theme.secondary_hover = palette.surface_hover.hsla();
            theme.secondary_active = palette.surface_selected.hsla();
            theme.secondary_foreground = palette.chrome_text.hsla();

            theme.primary = palette.accent.hsla();
            theme.primary_hover = palette.accent_hover.hsla();
            theme.primary_active = palette.accent_active.hsla();
            theme.primary_foreground = palette.accent_foreground.hsla();

            theme.accent = palette.surface_hover.hsla();
            theme.accent_foreground = palette.surface_text.hsla();

            theme.info = palette.info.hsla();
            theme.success = palette.success.hsla();

            theme.danger = palette.danger.hsla();
            theme.danger_hover = palette.danger.hsla();
            theme.danger_active = palette.danger.hsla();
            theme.danger_foreground = palette.danger_foreground.hsla();

            theme.button = palette.chrome_elevated.hsla();
            theme.button_hover = palette.surface_hover.hsla();
            theme.button_active = palette.surface_selected.hsla();
            theme.button_foreground = palette.chrome_text.hsla();

            theme.button_primary = palette.accent.hsla();
            theme.button_primary_hover = palette.accent_hover.hsla();
            theme.button_primary_active = palette.accent_active.hsla();
            theme.button_primary_foreground = palette.accent_foreground.hsla();

            theme.button_secondary = palette.chrome_elevated.hsla();
            theme.button_secondary_hover = palette.surface_hover.hsla();
            theme.button_secondary_active = palette.surface_selected.hsla();
            theme.button_secondary_foreground = palette.chrome_text.hsla();

            theme.button_danger = palette.danger.hsla();
            theme.button_danger_hover = palette.danger.hsla();
            theme.button_danger_active = palette.danger.hsla();
            theme.button_danger_foreground = palette.danger_foreground.hsla();

            if let Some(ref sans) = typography.sans_font {
                theme.font_family = sans.clone();
            }
            if let Some(ref mono) = typography.mono_font {
                theme.mono_font_family = mono.clone();
            }
        });
    }
}

/// Extension trait on [`App`] for ergonomic access to [`ChronoTheme`].
pub trait ChronoAppExt {
    /// Returns the active [`ChronoTheme`] global.
    fn chrono(&self) -> &ChronoTheme;
}

impl ChronoAppExt for App {
    #[inline(always)]
    fn chrono(&self) -> &ChronoTheme {
        ChronoTheme::global(self)
    }
}

/// Resolves `id` with an optional accent override and applies it via [`ChronoTheme::apply`].
pub fn apply_theme_with_accent(
    id: &str,
    accent_override: Option<&str>,
    os_appearance: WindowAppearance,
    mica_available: bool,
    cx: &mut App,
) -> ThemePalette {
    let palette = ThemeCatalog::new().resolve_palette(id, accent_override, os_appearance, mica_available);
    ChronoTheme::apply(palette, ThemeTypography::default(), cx);
    palette
}

/// Resolves `id` without an accent override and applies it via [`ChronoTheme::apply`].
pub fn apply_theme(
    id: &str,
    os_appearance: WindowAppearance,
    mica_available: bool,
    cx: &mut App,
) -> ThemePalette {
    apply_theme_with_accent(id, None, os_appearance, mica_available, cx)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn typography_defaults_are_cross_platform() {
        let typo = ThemeTypography::default();
        assert_eq!(typo.sans_font.as_deref(), Some(".SystemUIFont"));
        assert_eq!(typo.mono_font, None);
        assert!((typo.ui_scale - 1.0).abs() < 1e-6);
        assert!((f32::from(typo.scale(px(12.0))) - 12.0).abs() < 1e-3);
    }
}

#[cfg(all(test, feature = "test-support"))]
mod gpui_tests {
    use super::*;
    use crate::palette::{CHRONOPHOTOGRAPHIC, MICA_DARK};
    use gpui_kit::TestAppContext;

    #[gpui_kit::test]
    fn apply_theme_syncs_chrono_global_and_base_theme(cx: &mut TestAppContext) {
        cx.update(|cx| {
            gpui_kit::init(cx);
            let applied = apply_theme("chronophotographic", WindowAppearance::Dark, false, cx);
            assert_eq!(applied, CHRONOPHOTOGRAPHIC);
            assert_eq!(cx.chrono().palette, CHRONOPHOTOGRAPHIC);

            let kit_theme = Theme::global(cx);
            assert_eq!(kit_theme.radius, RADIUS_CONTROL);
            assert_eq!(kit_theme.radius_lg, RADIUS_MODAL);
            assert_eq!(kit_theme.background, CHRONOPHOTOGRAPHIC.chrome_bg.hsla());
            assert_eq!(kit_theme.primary, CHRONOPHOTOGRAPHIC.accent.hsla());

            let base_theme = cx.global::<gpui_kit::base::Theme>();
            assert_eq!(
                base_theme.appearance,
                gpui_kit::base::ThemeAppearance::Dark
            );
            assert_eq!(
                base_theme.tokens.colors.background,
                CHRONOPHOTOGRAPHIC.chrome_bg.hsla()
            );
            assert_eq!(
                base_theme.tokens.colors.primary,
                CHRONOPHOTOGRAPHIC.accent.hsla()
            );
        });
    }

    #[gpui_kit::test]
    fn apply_mica_theme_sets_transparent_root_and_syncs_base(cx: &mut TestAppContext) {
        cx.update(|cx| {
            gpui_kit::init(cx);
            let applied = apply_theme("mica-dark", WindowAppearance::Dark, true, cx);
            assert_eq!(applied, MICA_DARK);
            assert_eq!(cx.chrono().palette, MICA_DARK);

            let kit_theme = Theme::global(cx);
            assert_eq!(kit_theme.background, Hsla::transparent_black());
            assert_eq!(kit_theme.title_bar, Hsla::transparent_black());

            let base_theme = cx.global::<gpui_kit::base::Theme>();
            assert_eq!(base_theme.tokens.colors.background, Hsla::transparent_black());
        });
    }
}
