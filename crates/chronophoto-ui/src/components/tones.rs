//! Semantic tone and border enums for Chronophoto components.

use gpui_kit::assets::IconName;
use gpui_kit::{FontWeight, Hsla};

use crate::palette::ThemePalette;

/// Surface polarity and semantic emphasis for Table-Grid Cell components.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum GridTone {
    /// Work-surface header/toolbar tone (`surface_header_bg`, `surface_text`, `surface_border`).
    Surface,
    /// Subtle row-inline work-surface tone (`surface_header_bg` at 45% opacity, `surface_border`).
    SurfaceSubtle,
    /// Structural chrome, navigation rail, and modal tone (`chrome_elevated`, `chrome_text`, `chrome_border`).
    #[default]
    Chrome,
    /// High-emphasis primary action cell (`accent` fill at rest, `accent_foreground` ink).
    Primary,
    /// Destructive action cell (`danger` ink at rest, `danger` fill on hover).
    Danger,
}

impl GridTone {
    /// Returns the `1px` construction-grid border color for this tone.
    pub fn border_color(self, palette: &ThemePalette) -> Hsla {
        match self {
            Self::Surface | Self::SurfaceSubtle => palette.surface_border.hsla(),
            Self::Chrome | Self::Primary | Self::Danger => palette.chrome_border.hsla(),
        }
    }

    /// Returns the resting background fill for this tone.
    pub fn rest_bg(self, palette: &ThemePalette) -> Hsla {
        match self {
            Self::Surface => palette.surface_header_bg.hsla(),
            Self::SurfaceSubtle => palette.surface_header_bg.hsla().opacity(0.45),
            Self::Chrome | Self::Danger => palette.chrome_elevated.hsla(),
            Self::Primary => palette.accent.hsla(),
        }
    }

    /// Returns the resting text/icon foreground color for this tone.
    pub fn rest_fg(self, palette: &ThemePalette) -> Hsla {
        match self {
            Self::Surface | Self::SurfaceSubtle => palette.surface_text.hsla(),
            Self::Chrome => palette.chrome_text.hsla(),
            Self::Primary => palette.accent_foreground.hsla(),
            Self::Danger => palette.danger.hsla(),
        }
    }

    /// Returns the hovered background fill for this tone.
    pub fn hover_bg(self, palette: &ThemePalette) -> Hsla {
        match self {
            Self::Surface | Self::SurfaceSubtle | Self::Chrome => palette.accent.hsla(),
            Self::Primary => palette.accent_hover.hsla(),
            Self::Danger => palette.danger.hsla(),
        }
    }

    /// Returns the hovered text/icon foreground color for this tone.
    pub fn hover_fg(self, palette: &ThemePalette) -> Hsla {
        match self {
            Self::Surface | Self::SurfaceSubtle | Self::Chrome | Self::Primary => {
                palette.accent_foreground.hsla()
            }
            Self::Danger => palette.danger_foreground.hsla(),
        }
    }

    /// Returns the recessed header/label background fill for this tone.
    pub fn header_bg(self, palette: &ThemePalette) -> Hsla {
        match self {
            Self::Surface | Self::SurfaceSubtle => palette.surface_header_bg.hsla(),
            Self::Chrome | Self::Primary | Self::Danger => palette.chrome_elevated.hsla(),
        }
    }

    /// Returns the muted header/label foreground color for this tone.
    pub fn muted_fg(self, palette: &ThemePalette) -> Hsla {
        match self {
            Self::Surface | Self::SurfaceSubtle => palette.surface_muted.hsla(),
            Self::Chrome | Self::Primary | Self::Danger => palette.chrome_muted.hsla(),
        }
    }

    /// Returns the default value-cell background fill for key-value rows of this tone.
    pub fn kv_value_bg(self, palette: &ThemePalette) -> Hsla {
        match self {
            Self::Surface | Self::SurfaceSubtle => palette.surface_bg.hsla(),
            Self::Chrome | Self::Primary | Self::Danger => Hsla::transparent_black(),
        }
    }
}

/// Which vertical `1px` cell dividers to draw on a [`crate::components::GridCellButton`].
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum CellBorder {
    /// No vertical dividers.
    #[default]
    None,
    /// `1px` left divider only.
    Left,
    /// `1px` right divider only.
    Right,
    /// Both `1px` left and right dividers.
    Both,
}

impl CellBorder {
    /// Returns `true` when the left `1px` border should be rendered.
    pub const fn has_left(self) -> bool {
        matches!(self, Self::Left | Self::Both)
    }

    /// Returns `true` when the right `1px` border should be rendered.
    pub const fn has_right(self) -> bool {
        matches!(self, Self::Right | Self::Both)
    }
}

/// Semantic emphasis for a stamped monospace [`crate::components::GridBadge`].
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum BadgeTone {
    /// Standard neutral badge (`chrome_text` on `chrome_elevated`, `chrome_border`).
    #[default]
    Neutral,
    /// Muted metadata badge (`chrome_muted` on `chrome_elevated`, `chrome_border`).
    Muted,
    /// Active / highlighted badge (`accent` ink on `chrome_elevated`, `chrome_border`).
    Accent,
    /// Verified / completed badge (`success` ink and border).
    Success,
    /// Error / mismatch badge (`danger` ink and border).
    Danger,
}

impl BadgeTone {
    /// Returns the border color for this badge tone.
    pub fn border_color(self, palette: &ThemePalette) -> Hsla {
        match self {
            Self::Neutral | Self::Muted | Self::Accent => palette.chrome_border.hsla(),
            Self::Success => palette.success.hsla(),
            Self::Danger => palette.danger.hsla(),
        }
    }

    /// Returns the text color for this badge tone.
    pub fn text_color(self, palette: &ThemePalette) -> Hsla {
        match self {
            Self::Neutral => palette.chrome_text.hsla(),
            Self::Muted => palette.chrome_muted.hsla(),
            Self::Accent => palette.accent.hsla(),
            Self::Success => palette.success.hsla(),
            Self::Danger => palette.danger.hsla(),
        }
    }

    /// Returns the font weight for this badge tone.
    pub const fn font_weight(self) -> FontWeight {
        match self {
            Self::Neutral | Self::Muted => FontWeight::MEDIUM,
            Self::Accent | Self::Success | Self::Danger => FontWeight::SEMIBOLD,
        }
    }
}

/// Surface polarity for a vertical navigation row in [`crate::components::GridNavRow`].
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum NavRowTone {
    /// Chrome rail (`chrome_elevated` selected and hovered).
    #[default]
    Chrome,
    /// Work-surface selection rail (`surface_selected` selected, `surface_hover` hovered).
    SurfaceSelection,
}

impl NavRowTone {
    /// Returns the background fill when the navigation row is selected.
    pub fn selected_bg(self, palette: &ThemePalette) -> Hsla {
        match self {
            Self::Chrome => palette.chrome_elevated.hsla(),
            Self::SurfaceSelection => palette.surface_selected.hsla(),
        }
    }

    /// Returns the background fill when the navigation row is hovered.
    pub fn hover_bg(self, palette: &ThemePalette) -> Hsla {
        match self {
            Self::Chrome => palette.chrome_elevated.hsla(),
            Self::SurfaceSelection => palette.surface_hover.hsla(),
        }
    }
}

/// Semantic emphasis for an [`crate::components::AlertBanner`] strip.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum AlertTone {
    /// Error / failure diagnostic strip (`danger`).
    #[default]
    Danger,
    /// Positive confirmation strip (`success`).
    Success,
    /// Highlighted notice / warning strip (`accent`).
    Accent,
}

impl AlertTone {
    /// Returns the translucent strip background fill.
    pub fn surface_bg(self, palette: &ThemePalette) -> Hsla {
        match self {
            Self::Danger => palette.danger_surface(),
            Self::Success => palette.success.hsla().opacity(0.14),
            Self::Accent => palette.accent.hsla().opacity(0.14),
        }
    }

    /// Returns the border and icon accent color for this alert tone.
    pub fn accent_color(self, palette: &ThemePalette) -> Hsla {
        match self {
            Self::Danger => palette.danger.hsla(),
            Self::Success => palette.success.hsla(),
            Self::Accent => palette.accent.hsla(),
        }
    }

    /// Returns the default Lucide icon for this alert tone.
    pub const fn default_icon(self) -> IconName {
        match self {
            Self::Danger => IconName::TriangleAlert,
            Self::Success => IconName::CircleCheck,
            Self::Accent => IconName::Info,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::palette::CHRONOPHOTOGRAPHIC;

    #[test]
    fn grid_tone_maps_neutral_palette_roles() {
        let p = &CHRONOPHOTOGRAPHIC;
        assert_eq!(GridTone::Surface.border_color(p), p.surface_border.hsla());
        assert_eq!(GridTone::Surface.rest_bg(p), p.surface_header_bg.hsla());
        assert_eq!(GridTone::Surface.rest_fg(p), p.surface_text.hsla());
        assert_eq!(GridTone::Chrome.border_color(p), p.chrome_border.hsla());
        assert_eq!(GridTone::Chrome.rest_bg(p), p.chrome_elevated.hsla());
        assert_eq!(GridTone::Chrome.rest_fg(p), p.chrome_text.hsla());
        assert_eq!(GridTone::Primary.rest_bg(p), p.accent.hsla());
        assert_eq!(GridTone::Primary.hover_bg(p), p.accent_hover.hsla());
        assert_eq!(GridTone::Danger.rest_fg(p), p.danger.hsla());
        assert_eq!(GridTone::Danger.hover_fg(p), p.danger_foreground.hsla());
    }

    #[test]
    fn cell_border_flags_match_variants() {
        assert!(!CellBorder::None.has_left() && !CellBorder::None.has_right());
        assert!(CellBorder::Left.has_left() && !CellBorder::Left.has_right());
        assert!(!CellBorder::Right.has_left() && CellBorder::Right.has_right());
        assert!(CellBorder::Both.has_left() && CellBorder::Both.has_right());
    }

    #[test]
    fn badge_and_alert_tones_map_semantic_colors() {
        let p = &CHRONOPHOTOGRAPHIC;
        assert_eq!(BadgeTone::Success.text_color(p), p.success.hsla());
        assert_eq!(BadgeTone::Danger.border_color(p), p.danger.hsla());
        assert_eq!(BadgeTone::Accent.text_color(p), p.accent.hsla());

        assert_eq!(AlertTone::Danger.surface_bg(p), p.danger_surface());
        assert_eq!(AlertTone::Success.accent_color(p), p.success.hsla());
        assert_eq!(AlertTone::Accent.accent_color(p), p.accent.hsla());
    }
}
