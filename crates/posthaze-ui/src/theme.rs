//! Theme palettes, GPUI Kit token synchronization, and typographic role scale.

use std::sync::RwLock;

use gpui_kit::component::{Theme, ThemeMode};
use gpui_kit::{App, Hsla, Pixels, Window, px, rgb};

/// Authoritative tabular monospaced font for byte offsets, speeds, ETAs, and HTTP validators.
pub const MONO_FONT: &str = "Consolas";

/// Role scale sizes (in logical pixels) for Operate-mode hierarchy.
pub const TYPE_DISPLAY: Pixels = px(18.0);
pub const TYPE_HEADING: Pixels = px(15.0);
pub const TYPE_ROW_PRIMARY: Pixels = px(13.5);
pub const TYPE_BODY: Pixels = px(12.0);
pub const TYPE_MICRO: Pixels = px(11.0);
pub const TYPE_HEADER: Pixels = px(10.5);

/// Light/dark polarity for a [`ThemePalette`].
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum PaletteTone {
    /// Dark surface palette.
    Dark,
    /// Light surface palette.
    Light,
}

/// Complete semantic color palette for Posthaze UI.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct ThemePalette {
    /// Stable persisted identifier (stored in `Settings.ui_theme`).
    pub id: &'static str,
    /// Human-readable display name shown in Settings > Interface.
    pub name: &'static str,
    /// Short description shown under the theme name.
    pub description: &'static str,
    /// Light/dark mode passed to `gpui_kit::component::Theme`.
    pub tone: PaletteTone,
    /// Left rail, titlebar, and bottom inspector background.
    pub graphite_bg: u32,
    /// Raised surface for cards, badges, and secondary buttons.
    pub graphite_elevated: u32,
    /// Hairline rule inside rail/inspector/modal surfaces.
    pub graphite_border: u32,
    /// Primary text on rail, titlebar, inspector, and modals.
    pub silver_text: u32,
    /// Secondary/muted metadata text on rail and inspector.
    pub silver_muted: u32,
    /// Main download table & toolbar work surface.
    pub paper_bg: u32,
    /// Recessed modular table header background.
    pub table_header_bg: u32,
    /// Selected table row background.
    pub paper_selected: u32,
    /// Hovered table row / button background.
    pub paper_hover: u32,
    /// Construction-grid row and column rule on the table surface.
    pub paper_border: u32,
    /// Primary ink on the table work surface.
    pub carbon_text: u32,
    /// Secondary ink on the table work surface.
    pub carbon_muted: u32,
    /// Deep recessed byte-map ruler track.
    pub byte_track_bg: u32,
    /// Primary accent (in-flight segments, active selection, primary CTAs).
    pub accent: u32,
    /// Primary CTA hover tint.
    pub accent_hover: u32,
    /// Primary CTA active/pressed tint.
    pub accent_active: u32,
    /// Foreground text/icon color on primary accent buttons.
    pub accent_foreground: u32,
    /// Writer-acknowledged disk checkpoint progress color.
    pub checkpoint_blue: u32,
    /// Completed state indicator color.
    pub completed_emerald: u32,
    /// Failed / resource-changed state indicator color.
    pub failed_vermilion: u32,
    /// Focus ring outline color.
    pub focus_ring: u32,
}

/// Default warm graphite and selenium-amber palette.
pub const CHRONOPHOTOGRAPHIC: ThemePalette = ThemePalette {
    id: "chronophotographic",
    name: "Chronophotographic",
    description: "Warm graphite emulsion & selenium amber",
    tone: PaletteTone::Dark,
    graphite_bg: 0x141311,
    graphite_elevated: 0x1B1A17,
    graphite_border: 0x2A2825,
    silver_text: 0xEAE6DF,
    silver_muted: 0x9B968D,
    paper_bg: 0x1E1D1A,
    table_header_bg: 0x181714,
    paper_selected: 0x2D2B26,
    paper_hover: 0x262420,
    paper_border: 0x2C2A27,
    carbon_text: 0xEAE6DF,
    carbon_muted: 0x9B968D,
    byte_track_bg: 0x0D0C0B,
    accent: 0xE58A35,
    accent_hover: 0xD97724,
    accent_active: 0xC2661B,
    accent_foreground: 0x141311,
    checkpoint_blue: 0x3B82F6,
    completed_emerald: 0x22C55E,
    failed_vermilion: 0xEF4444,
    focus_ring: 0x524E48,
};

/// Deep Prussian midnight slate with electric cyanotype sky accent.
pub const BLUEPRINT_CYANOTYPE: ThemePalette = ThemePalette {
    id: "cyanotype",
    name: "Blueprint Cyanotype",
    description: "Prussian midnight slate & electric sky",
    tone: PaletteTone::Dark,
    graphite_bg: 0x0D131D,
    graphite_elevated: 0x151E2E,
    graphite_border: 0x233148,
    silver_text: 0xE6EEF8,
    silver_muted: 0x8BA0BD,
    paper_bg: 0x121A28,
    table_header_bg: 0x0F1622,
    paper_selected: 0x1E2B42,
    paper_hover: 0x192437,
    paper_border: 0x212E45,
    carbon_text: 0xE6EEF8,
    carbon_muted: 0x8BA0BD,
    byte_track_bg: 0x090D14,
    accent: 0x38BDF8,
    accent_hover: 0x22AEEB,
    accent_active: 0x0E96D4,
    accent_foreground: 0x090D14,
    checkpoint_blue: 0x60A5FA,
    completed_emerald: 0x34D399,
    failed_vermilion: 0xF87171,
    focus_ring: 0x3A5175,
};

/// High-contrast OLED CRT noir with phosphor amber accent.
pub const PHOSPHOR_AMBER: ThemePalette = ThemePalette {
    id: "phosphor",
    name: "Phosphor Amber",
    description: "High-contrast OLED noir & CRT amber",
    tone: PaletteTone::Dark,
    graphite_bg: 0x0A0B0A,
    graphite_elevated: 0x131613,
    graphite_border: 0x222822,
    silver_text: 0xECEFE9,
    silver_muted: 0x8E998D,
    paper_bg: 0x101210,
    table_header_bg: 0x0C0E0C,
    paper_selected: 0x1E231D,
    paper_hover: 0x181C17,
    paper_border: 0x202620,
    carbon_text: 0xECEFE9,
    carbon_muted: 0x8E998D,
    byte_track_bg: 0x050605,
    accent: 0xF59E0B,
    accent_hover: 0xD98A06,
    accent_active: 0xB87305,
    accent_foreground: 0x0A0B0A,
    checkpoint_blue: 0x38BDF8,
    completed_emerald: 0x10B981,
    failed_vermilion: 0xEF4444,
    focus_ring: 0x455245,
};

/// Cool arctic charcoal with glacial teal accent.
pub const NORDIC_FROST: ThemePalette = ThemePalette {
    id: "nordic",
    name: "Nordic Frost",
    description: "Arctic charcoal & glacial teal",
    tone: PaletteTone::Dark,
    graphite_bg: 0x151920,
    graphite_elevated: 0x1E242E,
    graphite_border: 0x2D3542,
    silver_text: 0xE7ECF2,
    silver_muted: 0x94A0B2,
    paper_bg: 0x1B2028,
    table_header_bg: 0x171B22,
    paper_selected: 0x27303D,
    paper_hover: 0x222934,
    paper_border: 0x2B3340,
    carbon_text: 0xE7ECF2,
    carbon_muted: 0x94A0B2,
    byte_track_bg: 0x0E1116,
    accent: 0x2DD4BF,
    accent_hover: 0x1EC2AE,
    accent_active: 0x14A694,
    accent_foreground: 0x0E1116,
    checkpoint_blue: 0x60A5FA,
    completed_emerald: 0x4ADE80,
    failed_vermilion: 0xFB7185,
    focus_ring: 0x455266,
};

/// Warm daylight archival paper with burnt sienna accent.
pub const ARCHIVAL_PAPER: ThemePalette = ThemePalette {
    id: "archival",
    name: "Archival Paper",
    description: "Warm daylight paper & burnt sienna",
    tone: PaletteTone::Light,
    graphite_bg: 0xECE8DF,
    graphite_elevated: 0xE2DDD2,
    graphite_border: 0xCEC7B8,
    silver_text: 0x1E1C19,
    silver_muted: 0x666056,
    paper_bg: 0xF7F5F0,
    table_header_bg: 0xEFECE4,
    paper_selected: 0xE5DFD2,
    paper_hover: 0xEDE9DF,
    paper_border: 0xD8D1C3,
    carbon_text: 0x1E1C19,
    carbon_muted: 0x666056,
    byte_track_bg: 0xDED8CA,
    accent: 0xC2590A,
    accent_hover: 0xAA4D08,
    accent_active: 0x8F3F06,
    accent_foreground: 0xFFFFFF,
    checkpoint_blue: 0x2563EB,
    completed_emerald: 0x16A34A,
    failed_vermilion: 0xDC2626,
    focus_ring: 0x9E9585,
};

/// Ordered list of all built-in application themes.
pub const BUILTIN_THEMES: [ThemePalette; 5] = [
    CHRONOPHOTOGRAPHIC,
    BLUEPRINT_CYANOTYPE,
    PHOSPHOR_AMBER,
    NORDIC_FROST,
    ARCHIVAL_PAPER,
];

/// Optional accent override preset layered over a [`ThemePalette`].
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct AccentPreset {
    /// Persisted override identifier (stored in `Settings.ui_accent_override`).
    pub id: &'static str,
    /// Human-readable label shown in Settings > Interface.
    pub name: &'static str,
    pub accent: u32,
    pub accent_hover: u32,
    pub accent_active: u32,
    pub accent_foreground: u32,
    pub focus_ring: u32,
}

/// Curated accent override presets for Settings > Interface.
pub const ACCENT_PRESETS: &[AccentPreset] = &[
    AccentPreset {
        id: "amber",
        name: "Selenium Amber",
        accent: 0xE58A35,
        accent_hover: 0xF59E42,
        accent_active: 0xCC7524,
        accent_foreground: 0x141311,
        focus_ring: 0xE58A35,
    },
    AccentPreset {
        id: "blue",
        name: "Cyanotype Blue",
        accent: 0x3B82F6,
        accent_hover: 0x60A5FA,
        accent_active: 0x2563EB,
        accent_foreground: 0xFFFFFF,
        focus_ring: 0x3B82F6,
    },
    AccentPreset {
        id: "emerald",
        name: "Halide Emerald",
        accent: 0x22C55E,
        accent_hover: 0x4ADE80,
        accent_active: 0x16A34A,
        accent_foreground: 0x0A0B0A,
        focus_ring: 0x22C55E,
    },
    AccentPreset {
        id: "cyan",
        name: "Phosphor Cyan",
        accent: 0x22D3EE,
        accent_hover: 0x67E8F9,
        accent_active: 0x06B6D4,
        accent_foreground: 0x090D14,
        focus_ring: 0x22D3EE,
    },
    AccentPreset {
        id: "crimson",
        name: "Errata Crimson",
        accent: 0xEF4444,
        accent_hover: 0xF87171,
        accent_active: 0xDC2626,
        accent_foreground: 0xFFFFFF,
        focus_ring: 0xEF4444,
    },
];

/// Looks up an [`AccentPreset`] by `id`.
pub fn find_accent_preset(id: &str) -> Option<&'static AccentPreset> {
    ACCENT_PRESETS.iter().find(|p| p.id == id)
}

static ACTIVE_THEME: RwLock<ThemePalette> = RwLock::new(CHRONOPHOTOGRAPHIC);

/// Looks up a built-in [`ThemePalette`] by `id`, falling back to [`CHRONOPHOTOGRAPHIC`].
pub fn find_theme(id: &str) -> &'static ThemePalette {
    BUILTIN_THEMES
        .iter()
        .find(|t| t.id == id)
        .unwrap_or(&CHRONOPHOTOGRAPHIC)
}

/// Returns a copy of the currently active [`ThemePalette`].
pub fn active_theme() -> ThemePalette {
    *ACTIVE_THEME
        .read()
        .unwrap_or_else(|poisoned| poisoned.into_inner())
}

/// Sets the active palette with an optional accent preset override and returns the resolved copy.
pub fn set_active_theme_with_accent(id: &str, accent_override: Option<&str>) -> ThemePalette {
    let mut palette = *find_theme(id);
    if let Some(preset) = accent_override.and_then(find_accent_preset) {
        palette.accent = preset.accent;
        palette.accent_hover = preset.accent_hover;
        palette.accent_active = preset.accent_active;
        palette.accent_foreground = preset.accent_foreground;
        palette.focus_ring = preset.focus_ring;
    }
    *ACTIVE_THEME
        .write()
        .unwrap_or_else(|poisoned| poisoned.into_inner()) = palette;
    palette
}

/// Activates `id` with an optional accent override and synchronizes `gpui_kit::component::Theme` globals.
pub fn apply_theme_with_accent(
    id: &str,
    accent_override: Option<&str>,
    window: Option<&mut Window>,
    cx: &mut App,
) -> ThemePalette {
    let palette = set_active_theme_with_accent(id, accent_override);
    let mode = match palette.tone {
        PaletteTone::Dark => ThemeMode::Dark,
        PaletteTone::Light => ThemeMode::Light,
    };
    Theme::change(mode, window, cx);

    let theme = Theme::global_mut(cx);
    theme.radius = px(4.0);
    theme.radius_lg = px(6.0);
    theme.background = rgb(palette.graphite_bg).into();
    theme.foreground = rgb(palette.silver_text).into();
    theme.border = rgb(palette.paper_border).into();
    theme.input = rgb(palette.graphite_bg).into();
    theme.muted = rgb(palette.graphite_elevated).into();
    theme.muted_foreground = rgb(palette.silver_muted).into();
    theme.secondary = rgb(palette.graphite_elevated).into();
    theme.secondary_hover = rgb(palette.paper_hover).into();
    theme.secondary_active = rgb(palette.paper_selected).into();
    theme.secondary_foreground = rgb(palette.silver_text).into();
    theme.primary = rgb(palette.accent).into();
    theme.primary_hover = rgb(palette.accent_hover).into();
    theme.primary_active = rgb(palette.accent_active).into();
    theme.primary_foreground = rgb(palette.accent_foreground).into();
    theme.button_primary = rgb(palette.accent).into();
    theme.button_primary_hover = rgb(palette.accent_hover).into();
    theme.button_primary_active = rgb(palette.accent_active).into();
    theme.button_primary_foreground = rgb(palette.accent_foreground).into();
    theme.button_secondary = rgb(palette.graphite_elevated).into();
    theme.button_secondary_hover = rgb(palette.paper_hover).into();
    theme.button_secondary_active = rgb(palette.paper_selected).into();
    theme.button_secondary_foreground = rgb(palette.silver_text).into();
    theme.button_hover = rgb(palette.paper_hover).into();
    theme.button_active = rgb(palette.paper_selected).into();
    theme.button_foreground = rgb(palette.silver_text).into();
    theme.ring = rgb(palette.focus_ring).into();
    theme.title_bar = rgb(palette.graphite_bg).into();
    theme.title_bar_border = rgb(palette.graphite_border).into();
    theme.tokens.primary.background = rgb(palette.accent).into();
    theme.tokens.primary_hover.background = rgb(palette.accent_hover).into();
    theme.tokens.primary_active.background = rgb(palette.accent_active).into();
    theme.tokens.button_primary.background = rgb(palette.accent).into();
    theme.tokens.button_primary_hover.background = rgb(palette.accent_hover).into();
    theme.tokens.button_primary_active.background = rgb(palette.accent_active).into();
    theme.tokens.button_secondary.background = rgb(palette.graphite_elevated).into();
    theme.tokens.button_secondary_hover.background = rgb(palette.paper_hover).into();
    theme.tokens.button_secondary_active.background = rgb(palette.paper_selected).into();
    theme.tokens.button.background = rgb(palette.graphite_elevated).into();
    theme.tokens.button_hover.background = rgb(palette.paper_hover).into();
    theme.tokens.button_active.background = rgb(palette.paper_selected).into();

    palette
}

/// Activates `id` without an accent override.
#[allow(dead_code)]
pub fn apply_theme(id: &str, window: Option<&mut Window>, cx: &mut App) -> ThemePalette {
    apply_theme_with_accent(id, None, window, cx)
}

/// Rail & bottom inspector background.
pub fn graphite_bg() -> Hsla {
    rgb(active_theme().graphite_bg).into()
}

/// Slightly raised surface for cards, badges, and secondary controls.
pub fn graphite_elevated() -> Hsla {
    rgb(active_theme().graphite_elevated).into()
}

/// Hairline rule inside rail, inspector, and modal surfaces.
pub fn graphite_border() -> Hsla {
    rgb(active_theme().graphite_border).into()
}

/// Primary text on rail, inspector, and modal surfaces.
pub fn silver_text() -> Hsla {
    rgb(active_theme().silver_text).into()
}

/// Muted metadata text on rail and inspector surfaces.
pub fn silver_muted() -> Hsla {
    rgb(active_theme().silver_muted).into()
}

/// Main download table & toolbar work surface.
pub fn paper_bg() -> Hsla {
    rgb(active_theme().paper_bg).into()
}

/// Recessed modular table header background.
pub fn table_header_bg() -> Hsla {
    rgb(active_theme().table_header_bg).into()
}

/// Selected row tint on the table work surface.
pub fn paper_selected() -> Hsla {
    rgb(active_theme().paper_selected).into()
}

/// Hover row tint on the table work surface.
pub fn paper_hover() -> Hsla {
    rgb(active_theme().paper_hover).into()
}

/// Construction-grid row & column rule on the table work surface.
pub fn paper_border() -> Hsla {
    rgb(active_theme().paper_border).into()
}

/// Primary ink on the table work surface.
pub fn carbon_text() -> Hsla {
    rgb(active_theme().carbon_text).into()
}

/// Secondary ink on the table work surface.
pub fn carbon_muted() -> Hsla {
    rgb(active_theme().carbon_muted).into()
}

/// Deep recessed byte-map ruler track.
pub fn byte_track_bg() -> Hsla {
    rgb(active_theme().byte_track_bg).into()
}

/// Primary accent for active in-flight segments & primary CTAs.
pub fn selenium_amber() -> Hsla {
    rgb(active_theme().accent).into()
}

/// Writer-acknowledged disk `Checkpoint` bytes color.
pub fn cyanotype_blue() -> Hsla {
    rgb(active_theme().checkpoint_blue).into()
}

/// `Completed` state indicator color.
pub fn halide_emerald() -> Hsla {
    rgb(active_theme().completed_emerald).into()
}

/// `Failed` / `ResourceChanged` state indicator color.
pub fn errata_vermilion() -> Hsla {
    rgb(active_theme().failed_vermilion).into()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn builtin_themes_have_unique_ids_and_fallback() {
        let ids: Vec<&str> = BUILTIN_THEMES.iter().map(|t| t.id).collect();
        assert_eq!(ids.len(), 5);
        for (i, id) in ids.iter().enumerate() {
            assert!(!ids[i + 1..].contains(id), "duplicate theme id: {id}");
        }

        assert_eq!(find_theme("cyanotype").name, "Blueprint Cyanotype");
        assert_eq!(find_theme("archival").tone, PaletteTone::Light);
        assert_eq!(find_theme("nonexistent").id, CHRONOPHOTOGRAPHIC.id);
    }
}
