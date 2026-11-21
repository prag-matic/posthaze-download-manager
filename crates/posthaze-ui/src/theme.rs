//! Theme palettes, GPUI Kit token synchronization, and typographic role scale.

use std::sync::RwLock;

use gpui_kit::component::{Theme, ThemeMode};
use gpui_kit::{App, Hsla, Pixels, WindowAppearance, WindowBackgroundAppearance, px};

/// Authoritative tabular monospaced font for byte offsets, speeds, ETAs, and HTTP validators.
pub const MONO_FONT: &str = "Consolas";

/// Role scale sizes (in logical pixels) for Operate-mode hierarchy.
pub const TYPE_DISPLAY: Pixels = px(18.0);
pub const TYPE_HEADING: Pixels = px(15.0);
pub const TYPE_ROW_PRIMARY: Pixels = px(13.5);
pub const TYPE_BODY: Pixels = px(12.0);
pub const TYPE_MICRO: Pixels = px(11.0);
pub const TYPE_HEADER: Pixels = px(10.5);

/// 32-bit packed RGBA color (`0xRRGGBBAA`) supporting opaque and translucent surfaces.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct RgbaHex(pub u32);

impl RgbaHex {
    /// Constructs an opaque color from a 24-bit `0xRRGGBB` hex literal.
    pub const fn rgb(hex: u32) -> Self {
        Self((hex << 8) | 0xFF)
    }

    /// Constructs a color from a 32-bit `0xRRGGBBAA` hex literal.
    pub const fn rgba(hex: u32) -> Self {
        Self(hex)
    }

    /// Converts this packed RGBA hex value into a GPUI [`Hsla`] color.
    pub fn hsla(self) -> Hsla {
        gpui_kit::rgba(self.0).into()
    }
}

/// Light/dark polarity for a [`ThemePalette`].
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum PaletteTone {
    /// Dark surface palette.
    Dark,
    /// Light surface palette.
    Light,
}

/// Window backdrop material requested for a [`ThemePalette`].
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ThemeBackdrop {
    /// Standard opaque surface (`DWMSBT_NONE`).
    Opaque,
    /// Windows 11 Mica material (`DWMSBT_MAINWINDOW`).
    Mica,
    /// Windows 11 Mica Alt material (`DWMSBT_TABBEDWINDOW`).
    MicaAlt,
}

impl ThemeBackdrop {
    /// Converts this backdrop material into GPUI's [`WindowBackgroundAppearance`].
    pub const fn to_window_background(self) -> WindowBackgroundAppearance {
        match self {
            Self::Opaque => WindowBackgroundAppearance::Opaque,
            Self::Mica => WindowBackgroundAppearance::MicaBackdrop,
            Self::MicaAlt => WindowBackgroundAppearance::MicaAltBackdrop,
        }
    }
}

/// Polarity badge displayed on theme preset cards in Settings > Interface.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ThemeBadge {
    /// Forced dark theme.
    Dark,
    /// Forced light theme.
    Light,
    /// Follows Windows Dark/Light appearance.
    Auto,
}

/// User-facing theme preset metadata shown in Settings > Interface.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct ThemePreset {
    /// Stable persisted identifier (stored in `Settings.ui_theme`).
    pub id: &'static str,
    /// Human-readable display name shown in Settings > Interface.
    pub name: &'static str,
    /// Short description shown under the theme name.
    pub description: &'static str,
    /// Polarity badge (`DARK`, `LIGHT`, or `AUTO`).
    pub badge: ThemeBadge,
    /// Three opaque `0xRRGGBB` preview swatches.
    pub swatches: [u32; 3],
    /// Whether this preset requests a Windows 11 Mica backdrop.
    pub is_mica: bool,
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
    /// Window backdrop material (`Opaque`, `Mica`, or `MicaAlt`).
    pub backdrop: ThemeBackdrop,
    /// Light/dark mode passed to `gpui_kit::component::Theme`.
    pub tone: PaletteTone,
    /// Custom titlebar fill (`0%` transparent in Mica mode; `graphite_bg` in Opaque mode).
    pub titlebar_bg: RgbaHex,
    /// Left rail and bottom inspector background.
    pub graphite_bg: RgbaHex,
    /// Raised surface for cards, badges, and secondary buttons.
    pub graphite_elevated: RgbaHex,
    /// Hairline rule inside rail/inspector/modal surfaces.
    pub graphite_border: RgbaHex,
    /// Primary text on rail, titlebar, inspector, and modals.
    pub silver_text: RgbaHex,
    /// Secondary/muted metadata text on rail and inspector.
    pub silver_muted: RgbaHex,
    /// Main download table & toolbar work surface.
    pub paper_bg: RgbaHex,
    /// Recessed modular table header background.
    pub table_header_bg: RgbaHex,
    /// Selected table row background.
    pub paper_selected: RgbaHex,
    /// Hovered table row / button background.
    pub paper_hover: RgbaHex,
    /// Construction-grid row and column rule on the table surface.
    pub paper_border: RgbaHex,
    /// Primary ink on the table work surface.
    pub carbon_text: RgbaHex,
    /// Secondary ink on the table work surface.
    pub carbon_muted: RgbaHex,
    /// Text input field surface background.
    pub input_bg: RgbaHex,
    /// Deep recessed byte-map ruler track.
    pub byte_track_bg: RgbaHex,
    /// Backdrop scrim painted behind in-window modal dialogs.
    pub modal_scrim: RgbaHex,
    /// Surface background for in-window modal cards.
    pub modal_bg: RgbaHex,
    /// Surface background for context menus and popovers.
    pub context_menu_bg: RgbaHex,
    /// Primary accent (in-flight segments, active selection, primary CTAs).
    pub accent: RgbaHex,
    /// Primary CTA hover tint.
    pub accent_hover: RgbaHex,
    /// Primary CTA active/pressed tint.
    pub accent_active: RgbaHex,
    /// Foreground text/icon color on primary accent buttons.
    pub accent_foreground: RgbaHex,
    /// Writer-acknowledged disk checkpoint progress color.
    pub checkpoint_blue: RgbaHex,
    /// Completed state indicator color.
    pub completed_emerald: RgbaHex,
    /// Failed / resource-changed state indicator color.
    pub failed_vermilion: RgbaHex,
    /// Focus ring outline color.
    pub focus_ring: RgbaHex,
}

/// Default warm graphite and selenium-amber palette.
pub const CHRONOPHOTOGRAPHIC: ThemePalette = ThemePalette {
    id: "chronophotographic",
    name: "Chronophotographic",
    description: "Warm graphite emulsion & selenium amber",
    backdrop: ThemeBackdrop::Opaque,
    tone: PaletteTone::Dark,
    titlebar_bg: RgbaHex::rgb(0x141311),
    graphite_bg: RgbaHex::rgb(0x141311),
    graphite_elevated: RgbaHex::rgb(0x1B1A17),
    graphite_border: RgbaHex::rgb(0x2A2825),
    silver_text: RgbaHex::rgb(0xEAE6DF),
    silver_muted: RgbaHex::rgb(0x9B968D),
    paper_bg: RgbaHex::rgb(0x1E1D1A),
    table_header_bg: RgbaHex::rgb(0x181714),
    paper_selected: RgbaHex::rgb(0x2D2B26),
    paper_hover: RgbaHex::rgb(0x262420),
    paper_border: RgbaHex::rgb(0x2C2A27),
    carbon_text: RgbaHex::rgb(0xEAE6DF),
    carbon_muted: RgbaHex::rgb(0x9B968D),
    input_bg: RgbaHex::rgb(0x141311),
    byte_track_bg: RgbaHex::rgb(0x0D0C0B),
    modal_scrim: RgbaHex::rgba(0x141413AA),
    modal_bg: RgbaHex::rgb(0x141311),
    context_menu_bg: RgbaHex::rgb(0x141311),
    accent: RgbaHex::rgb(0xE58A35),
    accent_hover: RgbaHex::rgb(0xD97724),
    accent_active: RgbaHex::rgb(0xC2661B),
    accent_foreground: RgbaHex::rgb(0x141311),
    checkpoint_blue: RgbaHex::rgb(0x3B82F6),
    completed_emerald: RgbaHex::rgb(0x22C55E),
    failed_vermilion: RgbaHex::rgb(0xEF4444),
    focus_ring: RgbaHex::rgb(0x524E48),
};

/// Deep Prussian midnight slate with electric cyanotype sky accent.
pub const BLUEPRINT_CYANOTYPE: ThemePalette = ThemePalette {
    id: "cyanotype",
    name: "Blueprint Cyanotype",
    description: "Prussian midnight slate & electric sky",
    backdrop: ThemeBackdrop::Opaque,
    tone: PaletteTone::Dark,
    titlebar_bg: RgbaHex::rgb(0x0D131D),
    graphite_bg: RgbaHex::rgb(0x0D131D),
    graphite_elevated: RgbaHex::rgb(0x151E2E),
    graphite_border: RgbaHex::rgb(0x233148),
    silver_text: RgbaHex::rgb(0xE6EEF8),
    silver_muted: RgbaHex::rgb(0x8BA0BD),
    paper_bg: RgbaHex::rgb(0x121A28),
    table_header_bg: RgbaHex::rgb(0x0F1622),
    paper_selected: RgbaHex::rgb(0x1E2B42),
    paper_hover: RgbaHex::rgb(0x192437),
    paper_border: RgbaHex::rgb(0x212E45),
    carbon_text: RgbaHex::rgb(0xE6EEF8),
    carbon_muted: RgbaHex::rgb(0x8BA0BD),
    input_bg: RgbaHex::rgb(0x0D131D),
    byte_track_bg: RgbaHex::rgb(0x090D14),
    modal_scrim: RgbaHex::rgba(0x141413AA),
    modal_bg: RgbaHex::rgb(0x0D131D),
    context_menu_bg: RgbaHex::rgb(0x0D131D),
    accent: RgbaHex::rgb(0x38BDF8),
    accent_hover: RgbaHex::rgb(0x22AEEB),
    accent_active: RgbaHex::rgb(0x0E96D4),
    accent_foreground: RgbaHex::rgb(0x090D14),
    checkpoint_blue: RgbaHex::rgb(0x60A5FA),
    completed_emerald: RgbaHex::rgb(0x34D399),
    failed_vermilion: RgbaHex::rgb(0xF87171),
    focus_ring: RgbaHex::rgb(0x3A5175),
};

/// High-contrast OLED CRT noir with phosphor amber accent.
pub const PHOSPHOR_AMBER: ThemePalette = ThemePalette {
    id: "phosphor",
    name: "Phosphor Amber",
    description: "High-contrast OLED noir & CRT amber",
    backdrop: ThemeBackdrop::Opaque,
    tone: PaletteTone::Dark,
    titlebar_bg: RgbaHex::rgb(0x0A0B0A),
    graphite_bg: RgbaHex::rgb(0x0A0B0A),
    graphite_elevated: RgbaHex::rgb(0x131613),
    graphite_border: RgbaHex::rgb(0x222822),
    silver_text: RgbaHex::rgb(0xECEFE9),
    silver_muted: RgbaHex::rgb(0x8E998D),
    paper_bg: RgbaHex::rgb(0x101210),
    table_header_bg: RgbaHex::rgb(0x0C0E0C),
    paper_selected: RgbaHex::rgb(0x1E231D),
    paper_hover: RgbaHex::rgb(0x181C17),
    paper_border: RgbaHex::rgb(0x202620),
    carbon_text: RgbaHex::rgb(0xECEFE9),
    carbon_muted: RgbaHex::rgb(0x8E998D),
    input_bg: RgbaHex::rgb(0x0A0B0A),
    byte_track_bg: RgbaHex::rgb(0x050605),
    modal_scrim: RgbaHex::rgba(0x141413AA),
    modal_bg: RgbaHex::rgb(0x0A0B0A),
    context_menu_bg: RgbaHex::rgb(0x0A0B0A),
    accent: RgbaHex::rgb(0xF59E0B),
    accent_hover: RgbaHex::rgb(0xD98A06),
    accent_active: RgbaHex::rgb(0xB87305),
    accent_foreground: RgbaHex::rgb(0x0A0B0A),
    checkpoint_blue: RgbaHex::rgb(0x38BDF8),
    completed_emerald: RgbaHex::rgb(0x10B981),
    failed_vermilion: RgbaHex::rgb(0xEF4444),
    focus_ring: RgbaHex::rgb(0x455245),
};

/// Cool arctic charcoal with glacial teal accent.
pub const NORDIC_FROST: ThemePalette = ThemePalette {
    id: "nordic",
    name: "Nordic Frost",
    description: "Arctic charcoal & glacial teal",
    backdrop: ThemeBackdrop::Opaque,
    tone: PaletteTone::Dark,
    titlebar_bg: RgbaHex::rgb(0x151920),
    graphite_bg: RgbaHex::rgb(0x151920),
    graphite_elevated: RgbaHex::rgb(0x1E242E),
    graphite_border: RgbaHex::rgb(0x2D3542),
    silver_text: RgbaHex::rgb(0xE7ECF2),
    silver_muted: RgbaHex::rgb(0x94A0B2),
    paper_bg: RgbaHex::rgb(0x1B2028),
    table_header_bg: RgbaHex::rgb(0x171B22),
    paper_selected: RgbaHex::rgb(0x27303D),
    paper_hover: RgbaHex::rgb(0x222934),
    paper_border: RgbaHex::rgb(0x2B3340),
    carbon_text: RgbaHex::rgb(0xE7ECF2),
    carbon_muted: RgbaHex::rgb(0x94A0B2),
    input_bg: RgbaHex::rgb(0x151920),
    byte_track_bg: RgbaHex::rgb(0x0E1116),
    modal_scrim: RgbaHex::rgba(0x141413AA),
    modal_bg: RgbaHex::rgb(0x151920),
    context_menu_bg: RgbaHex::rgb(0x151920),
    accent: RgbaHex::rgb(0x2DD4BF),
    accent_hover: RgbaHex::rgb(0x1EC2AE),
    accent_active: RgbaHex::rgb(0x14A694),
    accent_foreground: RgbaHex::rgb(0x0E1116),
    checkpoint_blue: RgbaHex::rgb(0x60A5FA),
    completed_emerald: RgbaHex::rgb(0x4ADE80),
    failed_vermilion: RgbaHex::rgb(0xFB7185),
    focus_ring: RgbaHex::rgb(0x455266),
};

/// Warm daylight archival paper with burnt sienna accent.
pub const ARCHIVAL_PAPER: ThemePalette = ThemePalette {
    id: "archival",
    name: "Archival Paper",
    description: "Warm daylight paper & burnt sienna",
    backdrop: ThemeBackdrop::Opaque,
    tone: PaletteTone::Light,
    titlebar_bg: RgbaHex::rgb(0xECE8DF),
    graphite_bg: RgbaHex::rgb(0xECE8DF),
    graphite_elevated: RgbaHex::rgb(0xE2DDD2),
    graphite_border: RgbaHex::rgb(0xCEC7B8),
    silver_text: RgbaHex::rgb(0x1E1C19),
    silver_muted: RgbaHex::rgb(0x666056),
    paper_bg: RgbaHex::rgb(0xF7F5F0),
    table_header_bg: RgbaHex::rgb(0xEFECE4),
    paper_selected: RgbaHex::rgb(0xE5DFD2),
    paper_hover: RgbaHex::rgb(0xEDE9DF),
    paper_border: RgbaHex::rgb(0xD8D1C3),
    carbon_text: RgbaHex::rgb(0x1E1C19),
    carbon_muted: RgbaHex::rgb(0x666056),
    input_bg: RgbaHex::rgb(0xECE8DF),
    byte_track_bg: RgbaHex::rgb(0xDED8CA),
    modal_scrim: RgbaHex::rgba(0x141413AA),
    modal_bg: RgbaHex::rgb(0xECE8DF),
    context_menu_bg: RgbaHex::rgb(0xECE8DF),
    accent: RgbaHex::rgb(0xC2590A),
    accent_hover: RgbaHex::rgb(0xAA4D08),
    accent_active: RgbaHex::rgb(0x8F3F06),
    accent_foreground: RgbaHex::rgb(0xFFFFFF),
    checkpoint_blue: RgbaHex::rgb(0x2563EB),
    completed_emerald: RgbaHex::rgb(0x16A34A),
    failed_vermilion: RgbaHex::rgb(0xDC2626),
    focus_ring: RgbaHex::rgb(0x9E9585),
};

/// Windows 11 Mica dark glass palette (Variant C: Unified Translucent Plate).
pub const MICA_DARK: ThemePalette = ThemePalette {
    id: "mica-dark",
    name: "Mica Dark",
    description: "Windows 11 Mica dark glass & selenium amber",
    backdrop: ThemeBackdrop::Mica,
    tone: PaletteTone::Dark,
    titlebar_bg: RgbaHex::rgba(0x00000000),
    graphite_bg: RgbaHex::rgba(0x08080828),
    graphite_elevated: RgbaHex::rgba(0xFFFFFF09),
    graphite_border: RgbaHex::rgba(0xFFFFFF14),
    silver_text: RgbaHex::rgba(0xF7F5F2FF),
    silver_muted: RgbaHex::rgba(0xB3AFA8FF),
    paper_bg: RgbaHex::rgba(0x0A0A0A52),
    table_header_bg: RgbaHex::rgba(0x0606065C),
    paper_selected: RgbaHex::rgba(0xFFFFFF0D),
    paper_hover: RgbaHex::rgba(0xFFFFFF0A),
    paper_border: RgbaHex::rgba(0xFFFFFF11),
    carbon_text: RgbaHex::rgba(0xF7F5F2FF),
    carbon_muted: RgbaHex::rgba(0xB3AFA8FF),
    input_bg: RgbaHex::rgba(0x05050573),
    byte_track_bg: RgbaHex::rgba(0x04040499),
    modal_scrim: RgbaHex::rgba(0x00000073),
    modal_bg: RgbaHex::rgba(0x121212DB),
    context_menu_bg: RgbaHex::rgba(0x141414E6),
    accent: RgbaHex::rgba(0xE58A35FF),
    accent_hover: RgbaHex::rgba(0xD97724FF),
    accent_active: RgbaHex::rgba(0xC2661BFF),
    accent_foreground: RgbaHex::rgba(0x141311FF),
    checkpoint_blue: RgbaHex::rgba(0x3B82F6FF),
    completed_emerald: RgbaHex::rgba(0x22C55EFF),
    failed_vermilion: RgbaHex::rgba(0xEF4444FF),
    focus_ring: RgbaHex::rgba(0xE58A35FF),
};

/// Windows 11 Mica Alt light glass palette (Variant C: Unified Translucent Plate).
pub const MICA_LIGHT: ThemePalette = ThemePalette {
    id: "mica-light",
    name: "Mica Light",
    description: "Windows 11 Mica Alt light glass & burnt sienna",
    backdrop: ThemeBackdrop::MicaAlt,
    tone: PaletteTone::Light,
    titlebar_bg: RgbaHex::rgba(0x00000000),
    graphite_bg: RgbaHex::rgba(0xFFFFFF2E),
    graphite_elevated: RgbaHex::rgba(0xFFFFFF66),
    graphite_border: RgbaHex::rgba(0x0000001A),
    silver_text: RgbaHex::rgba(0x111113FF),
    silver_muted: RgbaHex::rgba(0x44444BFF),
    paper_bg: RgbaHex::rgba(0xFFFFFF8A),
    table_header_bg: RgbaHex::rgba(0xF4F4F473),
    paper_selected: RgbaHex::rgba(0x0000001A),
    paper_hover: RgbaHex::rgba(0x0000000F),
    paper_border: RgbaHex::rgba(0x00000018),
    carbon_text: RgbaHex::rgba(0x111113FF),
    carbon_muted: RgbaHex::rgba(0x44444BFF),
    input_bg: RgbaHex::rgba(0xFFFFFFA6),
    byte_track_bg: RgbaHex::rgba(0x0000001C),
    modal_scrim: RgbaHex::rgba(0x00000038),
    modal_bg: RgbaHex::rgba(0xFAFAFAD9),
    context_menu_bg: RgbaHex::rgba(0xFFFFFFE6),
    accent: RgbaHex::rgba(0xC2590AFF),
    accent_hover: RgbaHex::rgba(0xAA4D08FF),
    accent_active: RgbaHex::rgba(0x8F3F06FF),
    accent_foreground: RgbaHex::rgba(0xFFFFFFFF),
    checkpoint_blue: RgbaHex::rgba(0x2563EBFF),
    completed_emerald: RgbaHex::rgba(0x15803DFF),
    failed_vermilion: RgbaHex::rgba(0xDC2626FF),
    focus_ring: RgbaHex::rgba(0xC2590AFF),
};

/// Calibrated neutral opaque fallback palette when dark Mica translucency is unavailable.
pub const MICA_DARK_FALLBACK: ThemePalette = ThemePalette {
    id: "mica-dark",
    name: "Mica Dark",
    description: "Windows 11 Mica dark glass & selenium amber",
    backdrop: ThemeBackdrop::Opaque,
    tone: PaletteTone::Dark,
    titlebar_bg: RgbaHex::rgba(0x202020FF),
    graphite_bg: RgbaHex::rgba(0x202020FF),
    graphite_elevated: RgbaHex::rgba(0x2B2B2BFF),
    graphite_border: RgbaHex::rgba(0x363636FF),
    silver_text: RgbaHex::rgba(0xF7F5F2FF),
    silver_muted: RgbaHex::rgba(0xB3AFA8FF),
    paper_bg: RgbaHex::rgba(0x272727FF),
    table_header_bg: RgbaHex::rgba(0x1C1C1CFF),
    paper_selected: RgbaHex::rgba(0x3A3A3AFF),
    paper_hover: RgbaHex::rgba(0x323232FF),
    paper_border: RgbaHex::rgba(0x343434FF),
    carbon_text: RgbaHex::rgba(0xF7F5F2FF),
    carbon_muted: RgbaHex::rgba(0xB3AFA8FF),
    input_bg: RgbaHex::rgba(0x181818FF),
    byte_track_bg: RgbaHex::rgba(0x121212FF),
    modal_scrim: RgbaHex::rgba(0x00000080),
    modal_bg: RgbaHex::rgba(0x242424FF),
    context_menu_bg: RgbaHex::rgba(0x282828FF),
    accent: RgbaHex::rgba(0xE58A35FF),
    accent_hover: RgbaHex::rgba(0xD97724FF),
    accent_active: RgbaHex::rgba(0xC2661BFF),
    accent_foreground: RgbaHex::rgba(0x141311FF),
    checkpoint_blue: RgbaHex::rgba(0x3B82F6FF),
    completed_emerald: RgbaHex::rgba(0x22C55EFF),
    failed_vermilion: RgbaHex::rgba(0xEF4444FF),
    focus_ring: RgbaHex::rgba(0xE58A35FF),
};

/// Calibrated neutral opaque fallback palette when light Mica translucency is unavailable.
pub const MICA_LIGHT_FALLBACK: ThemePalette = ThemePalette {
    id: "mica-light",
    name: "Mica Light",
    description: "Windows 11 Mica Alt light glass & burnt sienna",
    backdrop: ThemeBackdrop::Opaque,
    tone: PaletteTone::Light,
    titlebar_bg: RgbaHex::rgba(0xF3F3F3FF),
    graphite_bg: RgbaHex::rgba(0xF3F3F3FF),
    graphite_elevated: RgbaHex::rgba(0xFFFFFFFF),
    graphite_border: RgbaHex::rgba(0xDCDCDCFF),
    silver_text: RgbaHex::rgba(0x111113FF),
    silver_muted: RgbaHex::rgba(0x44444BFF),
    paper_bg: RgbaHex::rgba(0xFAFAFAFF),
    table_header_bg: RgbaHex::rgba(0xEFEFEFFF),
    paper_selected: RgbaHex::rgba(0xE0E0E0FF),
    paper_hover: RgbaHex::rgba(0xEAEAEAFF),
    paper_border: RgbaHex::rgba(0xE2E2E2FF),
    carbon_text: RgbaHex::rgba(0x111113FF),
    carbon_muted: RgbaHex::rgba(0x44444BFF),
    input_bg: RgbaHex::rgba(0xFFFFFFFF),
    byte_track_bg: RgbaHex::rgba(0xE4E4E4FF),
    modal_scrim: RgbaHex::rgba(0x00000040),
    modal_bg: RgbaHex::rgba(0xFAFAFAFF),
    context_menu_bg: RgbaHex::rgba(0xFFFFFFFF),
    accent: RgbaHex::rgba(0xC2590AFF),
    accent_hover: RgbaHex::rgba(0xAA4D08FF),
    accent_active: RgbaHex::rgba(0x8F3F06FF),
    accent_foreground: RgbaHex::rgba(0xFFFFFFFF),
    checkpoint_blue: RgbaHex::rgba(0x2563EBFF),
    completed_emerald: RgbaHex::rgba(0x15803DFF),
    failed_vermilion: RgbaHex::rgba(0xDC2626FF),
    focus_ring: RgbaHex::rgba(0xC2590AFF),
};

/// Ordered list of the 5 built-in opaque theme palettes.
const BUILTIN_OPAQUE_THEMES: [ThemePalette; 5] = [
    CHRONOPHOTOGRAPHIC,
    BLUEPRINT_CYANOTYPE,
    PHOSPHOR_AMBER,
    NORDIC_FROST,
    ARCHIVAL_PAPER,
];

/// Ordered catalog of all 8 built-in theme presets shown in Settings > Interface.
pub const BUILTIN_THEME_PRESETS: [ThemePreset; 8] = [
    ThemePreset {
        id: CHRONOPHOTOGRAPHIC.id,
        name: CHRONOPHOTOGRAPHIC.name,
        description: CHRONOPHOTOGRAPHIC.description,
        badge: ThemeBadge::Dark,
        swatches: [0x141311, 0x1E1D1A, 0xE58A35],
        is_mica: false,
    },
    ThemePreset {
        id: BLUEPRINT_CYANOTYPE.id,
        name: BLUEPRINT_CYANOTYPE.name,
        description: BLUEPRINT_CYANOTYPE.description,
        badge: ThemeBadge::Dark,
        swatches: [0x0D131D, 0x121A28, 0x38BDF8],
        is_mica: false,
    },
    ThemePreset {
        id: PHOSPHOR_AMBER.id,
        name: PHOSPHOR_AMBER.name,
        description: PHOSPHOR_AMBER.description,
        badge: ThemeBadge::Dark,
        swatches: [0x0A0B0A, 0x101210, 0xF59E0B],
        is_mica: false,
    },
    ThemePreset {
        id: NORDIC_FROST.id,
        name: NORDIC_FROST.name,
        description: NORDIC_FROST.description,
        badge: ThemeBadge::Dark,
        swatches: [0x151920, 0x1B2028, 0x2DD4BF],
        is_mica: false,
    },
    ThemePreset {
        id: ARCHIVAL_PAPER.id,
        name: ARCHIVAL_PAPER.name,
        description: ARCHIVAL_PAPER.description,
        badge: ThemeBadge::Light,
        swatches: [0xECE8DF, 0xF7F5F0, 0xC2590A],
        is_mica: false,
    },
    ThemePreset {
        id: "mica-system",
        name: "Mica (System)",
        description: "Follows Windows 11 Dark/Light Mica",
        badge: ThemeBadge::Auto,
        swatches: [0x202020, 0xF3F3F3, 0xE58A35],
        is_mica: true,
    },
    ThemePreset {
        id: MICA_DARK.id,
        name: MICA_DARK.name,
        description: MICA_DARK.description,
        badge: ThemeBadge::Dark,
        swatches: [0x202020, 0x2C2C2C, 0xE58A35],
        is_mica: true,
    },
    ThemePreset {
        id: MICA_LIGHT.id,
        name: MICA_LIGHT.name,
        description: MICA_LIGHT.description,
        badge: ThemeBadge::Light,
        swatches: [0xEAEAEA, 0xF7F7F7, 0xC2590A],
        is_mica: true,
    },
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

/// Looks up a built-in [`ThemePreset`] by `id`, falling back to `chronophotographic`.
pub fn find_theme_preset(id: &str) -> &'static ThemePreset {
    BUILTIN_THEME_PRESETS
        .iter()
        .find(|p| p.id == id)
        .unwrap_or(&BUILTIN_THEME_PRESETS[0])
}

/// Resolves a theme preset `id`, optional accent override, OS appearance, and Mica capability
/// into a concrete [`ThemePalette`] without any platform side effects.
pub fn resolve_palette(
    id: &str,
    accent_override: Option<&str>,
    os_appearance: WindowAppearance,
    mica_available: bool,
) -> ThemePalette {
    let os_is_light = matches!(
        os_appearance,
        WindowAppearance::Light | WindowAppearance::VibrantLight
    );
    let mut palette = match id {
        "mica-system" => match (os_is_light, mica_available) {
            (false, true) => MICA_DARK,
            (true, true) => MICA_LIGHT,
            (false, false) => MICA_DARK_FALLBACK,
            (true, false) => MICA_LIGHT_FALLBACK,
        },
        "mica-dark" => {
            if mica_available {
                MICA_DARK
            } else {
                MICA_DARK_FALLBACK
            }
        }
        "mica-light" => {
            if mica_available {
                MICA_LIGHT
            } else {
                MICA_LIGHT_FALLBACK
            }
        }
        _ => BUILTIN_OPAQUE_THEMES
            .iter()
            .copied()
            .find(|t| t.id == id)
            .unwrap_or(CHRONOPHOTOGRAPHIC),
    };

    if let Some(preset) = accent_override.and_then(find_accent_preset) {
        palette.accent = RgbaHex::rgb(preset.accent);
        palette.accent_hover = RgbaHex::rgb(preset.accent_hover);
        palette.accent_active = RgbaHex::rgb(preset.accent_active);
        palette.accent_foreground = RgbaHex::rgb(preset.accent_foreground);
        palette.focus_ring = RgbaHex::rgb(preset.focus_ring);
    }

    palette
}

static ACTIVE_THEME: RwLock<ThemePalette> = RwLock::new(CHRONOPHOTOGRAPHIC);

/// Returns a copy of the currently active [`ThemePalette`].
pub fn active_theme() -> ThemePalette {
    *ACTIVE_THEME
        .read()
        .unwrap_or_else(|poisoned| poisoned.into_inner())
}

/// Resolves and stores the active [`ThemePalette`] and returns the resolved copy.
pub fn set_active_theme_with_accent(
    id: &str,
    accent_override: Option<&str>,
    os_appearance: WindowAppearance,
    mica_available: bool,
) -> ThemePalette {
    let palette = resolve_palette(id, accent_override, os_appearance, mica_available);
    *ACTIVE_THEME
        .write()
        .unwrap_or_else(|poisoned| poisoned.into_inner()) = palette;
    palette
}

/// Activates `id` with an optional accent override and synchronizes `gpui_kit::component::Theme` globals.
pub fn apply_theme_with_accent(
    id: &str,
    accent_override: Option<&str>,
    os_appearance: WindowAppearance,
    mica_available: bool,
    cx: &mut App,
) -> ThemePalette {
    let palette = set_active_theme_with_accent(id, accent_override, os_appearance, mica_available);
    let mode = match palette.tone {
        PaletteTone::Dark => ThemeMode::Dark,
        PaletteTone::Light => ThemeMode::Light,
    };
    Theme::change(mode, None, cx);

    let theme = Theme::global_mut(cx);
    theme.radius = px(4.0);
    theme.radius_lg = px(6.0);
    if palette.backdrop != ThemeBackdrop::Opaque {
        theme.background = Hsla::transparent_black();
        theme.tokens.background = Hsla::transparent_black().into();
    } else {
        let bg = palette.graphite_bg.hsla();
        theme.background = bg;
        theme.tokens.background = bg.into();
    }
    theme.foreground = palette.silver_text.hsla();
    theme.border = palette.paper_border.hsla();
    theme.input = palette.input_bg.hsla();
    theme.popover = palette.context_menu_bg.hsla();
    theme.popover_foreground = palette.silver_text.hsla();
    theme.muted = palette.graphite_elevated.hsla();
    theme.muted_foreground = palette.silver_muted.hsla();
    theme.secondary = palette.graphite_elevated.hsla();
    theme.secondary_hover = palette.paper_hover.hsla();
    theme.secondary_active = palette.paper_selected.hsla();
    theme.secondary_foreground = palette.silver_text.hsla();
    theme.primary = palette.accent.hsla();
    theme.primary_hover = palette.accent_hover.hsla();
    theme.primary_active = palette.accent_active.hsla();
    theme.primary_foreground = palette.accent_foreground.hsla();
    theme.button_primary = palette.accent.hsla();
    theme.button_primary_hover = palette.accent_hover.hsla();
    theme.button_primary_active = palette.accent_active.hsla();
    theme.button_primary_foreground = palette.accent_foreground.hsla();
    theme.button_secondary = palette.graphite_elevated.hsla();
    theme.button_secondary_hover = palette.paper_hover.hsla();
    theme.button_secondary_active = palette.paper_selected.hsla();
    theme.button_secondary_foreground = palette.silver_text.hsla();
    theme.button_hover = palette.paper_hover.hsla();
    theme.button_active = palette.paper_selected.hsla();
    theme.button_foreground = palette.silver_text.hsla();
    theme.ring = palette.focus_ring.hsla();
    theme.title_bar = palette.titlebar_bg.hsla();
    theme.title_bar_border = palette.graphite_border.hsla();
    theme.tokens.primary.background = palette.accent.hsla().into();
    theme.tokens.primary_hover.background = palette.accent_hover.hsla().into();
    theme.tokens.primary_active.background = palette.accent_active.hsla().into();
    theme.tokens.button_primary.background = palette.accent.hsla().into();
    theme.tokens.button_primary_hover.background = palette.accent_hover.hsla().into();
    theme.tokens.button_primary_active.background = palette.accent_active.hsla().into();
    theme.tokens.button_secondary.background = palette.graphite_elevated.hsla().into();
    theme.tokens.button_secondary_hover.background = palette.paper_hover.hsla().into();
    theme.tokens.button_secondary_active.background = palette.paper_selected.hsla().into();
    theme.tokens.button.background = palette.graphite_elevated.hsla().into();
    theme.tokens.button_hover.background = palette.paper_hover.hsla().into();
    theme.tokens.button_active.background = palette.paper_selected.hsla().into();

    palette
}

/// Activates `id` without an accent override.
#[allow(dead_code)]
pub fn apply_theme(
    id: &str,
    os_appearance: WindowAppearance,
    mica_available: bool,
    cx: &mut App,
) -> ThemePalette {
    apply_theme_with_accent(id, None, os_appearance, mica_available, cx)
}

/// Custom titlebar surface background.
#[allow(dead_code)]
pub fn titlebar_bg() -> Hsla {
    active_theme().titlebar_bg.hsla()
}

/// Rail & bottom inspector background.
pub fn graphite_bg() -> Hsla {
    active_theme().graphite_bg.hsla()
}

/// Slightly raised surface for cards, badges, and secondary controls.
pub fn graphite_elevated() -> Hsla {
    active_theme().graphite_elevated.hsla()
}

/// Hairline rule inside rail, inspector, and modal surfaces.
pub fn graphite_border() -> Hsla {
    active_theme().graphite_border.hsla()
}

/// Primary text on rail, inspector, and modal surfaces.
pub fn silver_text() -> Hsla {
    active_theme().silver_text.hsla()
}

/// Muted metadata text on rail and inspector surfaces.
pub fn silver_muted() -> Hsla {
    active_theme().silver_muted.hsla()
}

/// Main download table & toolbar work surface.
pub fn paper_bg() -> Hsla {
    active_theme().paper_bg.hsla()
}

/// Recessed modular table header background.
pub fn table_header_bg() -> Hsla {
    active_theme().table_header_bg.hsla()
}

/// Selected row tint on the table work surface.
pub fn paper_selected() -> Hsla {
    active_theme().paper_selected.hsla()
}

/// Hover row tint on the table work surface.
pub fn paper_hover() -> Hsla {
    active_theme().paper_hover.hsla()
}

/// Construction-grid row & column rule on the table work surface.
pub fn paper_border() -> Hsla {
    active_theme().paper_border.hsla()
}

/// Primary ink on the table work surface.
pub fn carbon_text() -> Hsla {
    active_theme().carbon_text.hsla()
}

/// Secondary ink on the table work surface.
pub fn carbon_muted() -> Hsla {
    active_theme().carbon_muted.hsla()
}

/// Text input field surface background.
#[allow(dead_code)]
pub fn input_bg() -> Hsla {
    active_theme().input_bg.hsla()
}

/// Deep recessed byte-map ruler track.
pub fn byte_track_bg() -> Hsla {
    active_theme().byte_track_bg.hsla()
}

/// Backdrop scrim painted behind in-window modal dialogs.
#[allow(dead_code)]
pub fn modal_scrim() -> Hsla {
    active_theme().modal_scrim.hsla()
}

/// Surface background for in-window modal cards.
#[allow(dead_code)]
pub fn modal_bg() -> Hsla {
    active_theme().modal_bg.hsla()
}

/// Surface background for context menus and popovers.
#[allow(dead_code)]
pub fn context_menu_bg() -> Hsla {
    active_theme().context_menu_bg.hsla()
}

/// Primary accent for active in-flight segments & primary CTAs.
pub fn selenium_amber() -> Hsla {
    active_theme().accent.hsla()
}

/// Writer-acknowledged disk `Checkpoint` bytes color.
pub fn cyanotype_blue() -> Hsla {
    active_theme().checkpoint_blue.hsla()
}

/// `Completed` state indicator color.
pub fn halide_emerald() -> Hsla {
    active_theme().completed_emerald.hsla()
}

/// `Failed` / `ResourceChanged` state indicator color.
pub fn errata_vermilion() -> Hsla {
    active_theme().failed_vermilion.hsla()
}

/// Translucent vermilion tint for errata & validation banners.
pub fn errata_surface_bg() -> Hsla {
    errata_vermilion().opacity(0.14)
}

#[cfg(test)]
mod tests {
    use super::*;
    use gpui_kit::WindowAppearance;

    #[test]
    fn rgba_hex_encodes_rgb_and_rgba_channels() {
        let opaque = RgbaHex::rgb(0x141311);
        assert_eq!(opaque.0, 0x141311FF);
        assert!((opaque.hsla().a - 1.0).abs() < 1e-3);

        let translucent = RgbaHex::rgba(0x14141424);
        assert_eq!(translucent.0, 0x14141424);
        assert!((translucent.hsla().a - (0x24 as f32 / 255.0)).abs() < 1e-2);

        let clear = RgbaHex::rgba(0x00000000);
        assert_eq!(clear.hsla().a, 0.0);
    }

    #[test]
    fn builtin_theme_presets_have_eight_unique_ids_and_fallback() {
        let ids: Vec<&str> = BUILTIN_THEME_PRESETS.iter().map(|p| p.id).collect();
        assert_eq!(
            ids,
            vec![
                "chronophotographic",
                "cyanotype",
                "phosphor",
                "nordic",
                "archival",
                "mica-system",
                "mica-dark",
                "mica-light",
            ]
        );
        for (i, id) in ids.iter().enumerate() {
            assert!(!ids[i + 1..].contains(id), "duplicate theme id: {id}");
        }

        assert_eq!(find_theme_preset("cyanotype").name, "Blueprint Cyanotype");
        assert_eq!(find_theme_preset("archival").badge, ThemeBadge::Light);
        assert_eq!(find_theme_preset("mica-system").badge, ThemeBadge::Auto);
        assert!(find_theme_preset("mica-system").is_mica);
        assert!(find_theme_preset("mica-dark").is_mica);
        assert!(find_theme_preset("mica-light").is_mica);
        assert!(!find_theme_preset("chronophotographic").is_mica);
        assert_eq!(find_theme_preset("nonexistent").id, CHRONOPHOTOGRAPHIC.id);
    }

    #[test]
    fn resolve_palette_handles_mica_system_polarity() {
        let dark = resolve_palette("mica-system", None, WindowAppearance::Dark, true);
        assert_eq!(dark, MICA_DARK);
        assert_eq!(dark.backdrop, ThemeBackdrop::Mica);
        assert_eq!(dark.titlebar_bg, RgbaHex::rgba(0x00000000));

        let vibrant_dark = resolve_palette("mica-system", None, WindowAppearance::VibrantDark, true);
        assert_eq!(vibrant_dark, MICA_DARK);

        let light = resolve_palette("mica-system", None, WindowAppearance::Light, true);
        assert_eq!(light, MICA_LIGHT);
        assert_eq!(light.backdrop, ThemeBackdrop::MicaAlt);
        assert_eq!(light.titlebar_bg, RgbaHex::rgba(0x00000000));

        let vibrant_light =
            resolve_palette("mica-system", None, WindowAppearance::VibrantLight, true);
        assert_eq!(vibrant_light, MICA_LIGHT);
    }

    #[test]
    fn resolve_palette_enforces_forced_mica_polarity() {
        let forced_dark = resolve_palette("mica-dark", None, WindowAppearance::Light, true);
        assert_eq!(forced_dark, MICA_DARK);
        assert_eq!(forced_dark.backdrop, ThemeBackdrop::Mica);

        let forced_light = resolve_palette("mica-light", None, WindowAppearance::Dark, true);
        assert_eq!(forced_light, MICA_LIGHT);
        assert_eq!(forced_light.backdrop, ThemeBackdrop::MicaAlt);
    }

    #[test]
    fn resolve_palette_falls_back_to_opaque_when_mica_unavailable() {
        let dark = resolve_palette("mica-dark", None, WindowAppearance::Light, false);
        assert_eq!(dark, MICA_DARK_FALLBACK);
        assert_eq!(dark.backdrop, ThemeBackdrop::Opaque);
        assert_eq!(dark.titlebar_bg, RgbaHex::rgba(0x202020FF));

        let light = resolve_palette("mica-light", None, WindowAppearance::Dark, false);
        assert_eq!(light, MICA_LIGHT_FALLBACK);
        assert_eq!(light.backdrop, ThemeBackdrop::Opaque);
        assert_eq!(light.titlebar_bg, RgbaHex::rgba(0xF3F3F3FF));

        let sys_dark = resolve_palette("mica-system", None, WindowAppearance::Dark, false);
        assert_eq!(sys_dark, MICA_DARK_FALLBACK);
        assert_eq!(sys_dark.backdrop, ThemeBackdrop::Opaque);

        let sys_light = resolve_palette("mica-system", None, WindowAppearance::Light, false);
        assert_eq!(sys_light, MICA_LIGHT_FALLBACK);
        assert_eq!(sys_light.backdrop, ThemeBackdrop::Opaque);
    }

    #[test]
    fn resolve_palette_applies_accent_override_preserving_mica_surfaces() {
        let blue = find_accent_preset("blue").expect("blue preset exists");
        let palette = resolve_palette("mica-dark", Some("blue"), WindowAppearance::Light, true);
        assert_eq!(palette.backdrop, ThemeBackdrop::Mica);
        assert_eq!(palette.graphite_bg, MICA_DARK.graphite_bg);
        assert_eq!(palette.paper_bg, MICA_DARK.paper_bg);
        assert_eq!(palette.accent, RgbaHex::rgb(blue.accent));
        assert_eq!(palette.accent_hover, RgbaHex::rgb(blue.accent_hover));
        assert_eq!(palette.accent_active, RgbaHex::rgb(blue.accent_active));
        assert_eq!(
            palette.accent_foreground,
            RgbaHex::rgb(blue.accent_foreground)
        );
        assert_eq!(palette.focus_ring, RgbaHex::rgb(blue.focus_ring));
    }

    #[test]
    fn resolve_palette_falls_back_to_chronophotographic_for_unknown_id() {
        let palette = resolve_palette("nonexistent", None, WindowAppearance::Light, true);
        assert_eq!(palette, CHRONOPHOTOGRAPHIC);
        assert_eq!(palette.backdrop, ThemeBackdrop::Opaque);
    }

    #[test]
    fn theme_backdrop_maps_to_gpui_window_background() {
        assert_eq!(
            ThemeBackdrop::Opaque.to_window_background(),
            WindowBackgroundAppearance::Opaque
        );
        assert_eq!(
            ThemeBackdrop::Mica.to_window_background(),
            WindowBackgroundAppearance::MicaBackdrop
        );
        assert_eq!(
            ThemeBackdrop::MicaAlt.to_window_background(),
            WindowBackgroundAppearance::MicaAltBackdrop
        );
    }
}
