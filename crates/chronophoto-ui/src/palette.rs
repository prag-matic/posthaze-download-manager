//! Semantic color palettes, presets, accent overrides, and WCAG/alpha calibration helpers.

use gpui_kit::{Hsla, WindowAppearance, WindowBackgroundAppearance};

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

    /// Returns the red channel (`0..=255`).
    pub const fn r(self) -> u8 {
        ((self.0 >> 24) & 0xFF) as u8
    }

    /// Returns the green channel (`0..=255`).
    pub const fn g(self) -> u8 {
        ((self.0 >> 16) & 0xFF) as u8
    }

    /// Returns the blue channel (`0..=255`).
    pub const fn b(self) -> u8 {
        ((self.0 >> 8) & 0xFF) as u8
    }

    /// Returns the alpha channel (`0..=255`).
    pub const fn a(self) -> u8 {
        (self.0 & 0xFF) as u8
    }

    /// Converts this packed RGBA hex value into a GPUI [`Hsla`] color.
    pub fn hsla(self) -> Hsla {
        gpui_kit::rgba(self.0).into()
    }
}

/// Converts an 8-bit sRGB channel (`0..=255`) into linear-light luminance component.
fn srgb_to_linear(channel: u8) -> f64 {
    let c = f64::from(channel) / 255.0;
    if c <= 0.04045 {
        c / 12.92
    } else {
        ((c + 0.055) / 1.055).powf(2.4)
    }
}

/// Composites `fg` over an opaque `bg` using standard source-over alpha blending.
pub fn composite_over(fg: RgbaHex, bg: RgbaHex) -> RgbaHex {
    let alpha = f64::from(fg.a()) / 255.0;
    let inv = 1.0 - alpha;
    let r = (f64::from(fg.r()) * alpha + f64::from(bg.r()) * inv).round() as u8;
    let g = (f64::from(fg.g()) * alpha + f64::from(bg.g()) * inv).round() as u8;
    let b = (f64::from(fg.b()) * alpha + f64::from(bg.b()) * inv).round() as u8;
    RgbaHex::rgba(
        (u32::from(r) << 24) | (u32::from(g) << 16) | (u32::from(b) << 8) | 0xFF,
    )
}

/// Computes the WCAG 2.1 relative luminance (`0.0..=1.0`) of an opaque color.
pub fn relative_luminance(color: RgbaHex) -> f64 {
    0.2126 * srgb_to_linear(color.r())
        + 0.7152 * srgb_to_linear(color.g())
        + 0.0722 * srgb_to_linear(color.b())
}

/// Computes the WCAG 2.1 contrast ratio (`1.0..=21.0`) between two colors.
///
/// Translucent foregrounds are composited over `bg` first; if `bg` itself is
/// translucent, callers should composite `bg` over an opaque backdrop first.
pub fn contrast_ratio(fg: RgbaHex, bg: RgbaHex) -> f64 {
    let fg_resolved = if fg.a() < 255 {
        composite_over(fg, bg)
    } else {
        fg
    };
    let l1 = relative_luminance(fg_resolved);
    let l2 = relative_luminance(bg);
    let (lighter, darker) = if l1 >= l2 { (l1, l2) } else { (l2, l1) };
    (lighter + 0.05) / (darker + 0.05)
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

/// Polarity badge displayed on theme preset cards.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ThemeBadge {
    /// Forced dark theme.
    Dark,
    /// Forced light theme.
    Light,
    /// Follows OS Dark/Light appearance.
    Auto,
}

/// User-facing theme preset metadata shown in theme pickers.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct ThemePreset {
    /// Stable persisted identifier.
    pub id: &'static str,
    /// Human-readable display name.
    pub name: &'static str,
    /// Short description shown under the theme name.
    pub description: &'static str,
    /// Polarity badge (`Dark`, `Light`, or `Auto`).
    pub badge: ThemeBadge,
    /// Three opaque `0xRRGGBB` preview swatches.
    pub swatches: [u32; 3],
    /// Whether this preset requests a Windows 11 Mica backdrop.
    pub is_mica: bool,
}

/// Complete domain- and polarity-neutral semantic color palette.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct ThemePalette {
    /// Stable persisted identifier.
    pub id: &'static str,
    /// Human-readable display name.
    pub name: &'static str,
    /// Short description shown under the theme name.
    pub description: &'static str,
    /// Window backdrop material (`Opaque`, `Mica`, or `MicaAlt`).
    pub backdrop: ThemeBackdrop,
    /// Light/dark mode passed to `gpui_kit::component::Theme`.
    pub tone: PaletteTone,
    /// Custom titlebar fill (`0%` transparent in Mica mode; `chrome_bg` in Opaque mode).
    pub titlebar_bg: RgbaHex,
    /// Structural chrome, navigation rail, and bottom inspector background.
    pub chrome_bg: RgbaHex,
    /// Raised chrome surface for cards, badges, and secondary buttons.
    pub chrome_elevated: RgbaHex,
    /// Hairline rule inside chrome, inspector, and modal surfaces.
    pub chrome_border: RgbaHex,
    /// Primary text on chrome, titlebar, inspector, and modals.
    pub chrome_text: RgbaHex,
    /// Secondary/muted metadata text on chrome and inspector surfaces.
    pub chrome_muted: RgbaHex,
    /// Main work surface and toolbar background.
    pub surface_bg: RgbaHex,
    /// Recessed modular section/table header background.
    pub surface_header_bg: RgbaHex,
    /// Selected row or item background on the work surface.
    pub surface_selected: RgbaHex,
    /// Hovered row or button background on the work surface.
    pub surface_hover: RgbaHex,
    /// Construction-grid row and column rule on the work surface.
    pub surface_border: RgbaHex,
    /// Primary ink on the work surface.
    pub surface_text: RgbaHex,
    /// Secondary ink on the work surface.
    pub surface_muted: RgbaHex,
    /// Text input field surface background.
    pub input_bg: RgbaHex,
    /// Deep recessed progress/meter track background.
    pub track_bg: RgbaHex,
    /// Backdrop scrim painted behind in-window modal dialogs.
    pub modal_scrim: RgbaHex,
    /// Surface background for in-window modal cards.
    pub modal_bg: RgbaHex,
    /// Surface background for context menus and popovers.
    pub popover_bg: RgbaHex,
    /// Primary accent color (active selection, primary CTAs, live indicators).
    pub accent: RgbaHex,
    /// Primary CTA hover tint.
    pub accent_hover: RgbaHex,
    /// Primary CTA active/pressed tint.
    pub accent_active: RgbaHex,
    /// Foreground text/icon color on primary accent surfaces.
    pub accent_foreground: RgbaHex,
    /// Informational / secondary telemetry status color.
    pub info: RgbaHex,
    /// Success / completed state indicator color.
    pub success: RgbaHex,
    /// Danger / error state indicator color.
    pub danger: RgbaHex,
    /// High-contrast foreground ink on filled danger surfaces.
    pub danger_foreground: RgbaHex,
    /// Focus ring outline color.
    pub focus_ring: RgbaHex,
}

impl ThemePalette {
    /// Translucent danger surface tint for alert banners and inline diagnostics.
    pub fn danger_surface(&self) -> Hsla {
        self.danger.hsla().opacity(0.14)
    }
}

/// Default warm graphite and amber palette.
pub const CHRONOPHOTOGRAPHIC: ThemePalette = ThemePalette {
    id: "chronophotographic",
    name: "Chronophotographic",
    description: "Warm graphite emulsion & selenium amber",
    backdrop: ThemeBackdrop::Opaque,
    tone: PaletteTone::Dark,
    titlebar_bg: RgbaHex::rgb(0x141311),
    chrome_bg: RgbaHex::rgb(0x141311),
    chrome_elevated: RgbaHex::rgb(0x1B1A17),
    chrome_border: RgbaHex::rgb(0x2A2825),
    chrome_text: RgbaHex::rgb(0xEAE6DF),
    chrome_muted: RgbaHex::rgb(0x9B968D),
    surface_bg: RgbaHex::rgb(0x1E1D1A),
    surface_header_bg: RgbaHex::rgb(0x181714),
    surface_selected: RgbaHex::rgb(0x2D2B26),
    surface_hover: RgbaHex::rgb(0x262420),
    surface_border: RgbaHex::rgb(0x2C2A27),
    surface_text: RgbaHex::rgb(0xEAE6DF),
    surface_muted: RgbaHex::rgb(0x9B968D),
    input_bg: RgbaHex::rgb(0x141311),
    track_bg: RgbaHex::rgb(0x0D0C0B),
    modal_scrim: RgbaHex::rgba(0x141413AA),
    modal_bg: RgbaHex::rgb(0x141311),
    popover_bg: RgbaHex::rgb(0x141311),
    accent: RgbaHex::rgb(0xE58A35),
    accent_hover: RgbaHex::rgb(0xD97724),
    accent_active: RgbaHex::rgb(0xC2661B),
    accent_foreground: RgbaHex::rgb(0x141311),
    info: RgbaHex::rgb(0x3B82F6),
    success: RgbaHex::rgb(0x22C55E),
    danger: RgbaHex::rgb(0xEF4444),
    danger_foreground: RgbaHex::rgb(0x141311),
    focus_ring: RgbaHex::rgb(0x524E48),
};

/// Deep Prussian midnight slate with electric sky accent.
pub const BLUEPRINT_CYANOTYPE: ThemePalette = ThemePalette {
    id: "cyanotype",
    name: "Blueprint Cyanotype",
    description: "Prussian midnight slate & electric sky",
    backdrop: ThemeBackdrop::Opaque,
    tone: PaletteTone::Dark,
    titlebar_bg: RgbaHex::rgb(0x0D131D),
    chrome_bg: RgbaHex::rgb(0x0D131D),
    chrome_elevated: RgbaHex::rgb(0x151E2E),
    chrome_border: RgbaHex::rgb(0x233148),
    chrome_text: RgbaHex::rgb(0xE6EEF8),
    chrome_muted: RgbaHex::rgb(0x8BA0BD),
    surface_bg: RgbaHex::rgb(0x121A28),
    surface_header_bg: RgbaHex::rgb(0x0F1622),
    surface_selected: RgbaHex::rgb(0x1E2B42),
    surface_hover: RgbaHex::rgb(0x192437),
    surface_border: RgbaHex::rgb(0x212E45),
    surface_text: RgbaHex::rgb(0xE6EEF8),
    surface_muted: RgbaHex::rgb(0x8BA0BD),
    input_bg: RgbaHex::rgb(0x0D131D),
    track_bg: RgbaHex::rgb(0x090D14),
    modal_scrim: RgbaHex::rgba(0x141413AA),
    modal_bg: RgbaHex::rgb(0x0D131D),
    popover_bg: RgbaHex::rgb(0x0D131D),
    accent: RgbaHex::rgb(0x38BDF8),
    accent_hover: RgbaHex::rgb(0x22AEEB),
    accent_active: RgbaHex::rgb(0x0E96D4),
    accent_foreground: RgbaHex::rgb(0x090D14),
    info: RgbaHex::rgb(0x60A5FA),
    success: RgbaHex::rgb(0x34D399),
    danger: RgbaHex::rgb(0xF87171),
    danger_foreground: RgbaHex::rgb(0x090D14),
    focus_ring: RgbaHex::rgb(0x3A5175),
};

/// High-contrast OLED noir with phosphor amber accent.
pub const PHOSPHOR_AMBER: ThemePalette = ThemePalette {
    id: "phosphor",
    name: "Phosphor Amber",
    description: "High-contrast OLED noir & CRT amber",
    backdrop: ThemeBackdrop::Opaque,
    tone: PaletteTone::Dark,
    titlebar_bg: RgbaHex::rgb(0x0A0B0A),
    chrome_bg: RgbaHex::rgb(0x0A0B0A),
    chrome_elevated: RgbaHex::rgb(0x131613),
    chrome_border: RgbaHex::rgb(0x222822),
    chrome_text: RgbaHex::rgb(0xECEFE9),
    chrome_muted: RgbaHex::rgb(0x8E998D),
    surface_bg: RgbaHex::rgb(0x101210),
    surface_header_bg: RgbaHex::rgb(0x0C0E0C),
    surface_selected: RgbaHex::rgb(0x1E231D),
    surface_hover: RgbaHex::rgb(0x181C17),
    surface_border: RgbaHex::rgb(0x202620),
    surface_text: RgbaHex::rgb(0xECEFE9),
    surface_muted: RgbaHex::rgb(0x8E998D),
    input_bg: RgbaHex::rgb(0x0A0B0A),
    track_bg: RgbaHex::rgb(0x050605),
    modal_scrim: RgbaHex::rgba(0x141413AA),
    modal_bg: RgbaHex::rgb(0x0A0B0A),
    popover_bg: RgbaHex::rgb(0x0A0B0A),
    accent: RgbaHex::rgb(0xF59E0B),
    accent_hover: RgbaHex::rgb(0xD98A06),
    accent_active: RgbaHex::rgb(0xB87305),
    accent_foreground: RgbaHex::rgb(0x0A0B0A),
    info: RgbaHex::rgb(0x38BDF8),
    success: RgbaHex::rgb(0x10B981),
    danger: RgbaHex::rgb(0xEF4444),
    danger_foreground: RgbaHex::rgb(0x0A0B0A),
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
    chrome_bg: RgbaHex::rgb(0x151920),
    chrome_elevated: RgbaHex::rgb(0x1E242E),
    chrome_border: RgbaHex::rgb(0x2D3542),
    chrome_text: RgbaHex::rgb(0xE7ECF2),
    chrome_muted: RgbaHex::rgb(0x94A0B2),
    surface_bg: RgbaHex::rgb(0x1B2028),
    surface_header_bg: RgbaHex::rgb(0x171B22),
    surface_selected: RgbaHex::rgb(0x27303D),
    surface_hover: RgbaHex::rgb(0x222934),
    surface_border: RgbaHex::rgb(0x2B3340),
    surface_text: RgbaHex::rgb(0xE7ECF2),
    surface_muted: RgbaHex::rgb(0x94A0B2),
    input_bg: RgbaHex::rgb(0x151920),
    track_bg: RgbaHex::rgb(0x0E1116),
    modal_scrim: RgbaHex::rgba(0x141413AA),
    modal_bg: RgbaHex::rgb(0x151920),
    popover_bg: RgbaHex::rgb(0x151920),
    accent: RgbaHex::rgb(0x2DD4BF),
    accent_hover: RgbaHex::rgb(0x1EC2AE),
    accent_active: RgbaHex::rgb(0x14A694),
    accent_foreground: RgbaHex::rgb(0x0E1116),
    info: RgbaHex::rgb(0x60A5FA),
    success: RgbaHex::rgb(0x4ADE80),
    danger: RgbaHex::rgb(0xFB7185),
    danger_foreground: RgbaHex::rgb(0x0E1116),
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
    chrome_bg: RgbaHex::rgb(0xECE8DF),
    chrome_elevated: RgbaHex::rgb(0xE2DDD2),
    chrome_border: RgbaHex::rgb(0xCEC7B8),
    chrome_text: RgbaHex::rgb(0x1E1C19),
    chrome_muted: RgbaHex::rgb(0x666056),
    surface_bg: RgbaHex::rgb(0xF7F5F0),
    surface_header_bg: RgbaHex::rgb(0xEFECE4),
    surface_selected: RgbaHex::rgb(0xE5DFD2),
    surface_hover: RgbaHex::rgb(0xEDE9DF),
    surface_border: RgbaHex::rgb(0xD8D1C3),
    surface_text: RgbaHex::rgb(0x1E1C19),
    surface_muted: RgbaHex::rgb(0x666056),
    input_bg: RgbaHex::rgb(0xECE8DF),
    track_bg: RgbaHex::rgb(0xDED8CA),
    modal_scrim: RgbaHex::rgba(0x141413AA),
    modal_bg: RgbaHex::rgb(0xECE8DF),
    popover_bg: RgbaHex::rgb(0xECE8DF),
    accent: RgbaHex::rgb(0xB85208),
    accent_hover: RgbaHex::rgb(0xAA4D08),
    accent_active: RgbaHex::rgb(0x8F3F06),
    accent_foreground: RgbaHex::rgb(0xFFFFFF),
    info: RgbaHex::rgb(0x2563EB),
    success: RgbaHex::rgb(0x16A34A),
    danger: RgbaHex::rgb(0xDC2626),
    danger_foreground: RgbaHex::rgb(0xFFFFFF),
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
    chrome_bg: RgbaHex::rgba(0x08080828),
    chrome_elevated: RgbaHex::rgba(0xFFFFFF09),
    chrome_border: RgbaHex::rgba(0xFFFFFF14),
    chrome_text: RgbaHex::rgba(0xF7F5F2FF),
    chrome_muted: RgbaHex::rgba(0xB3AFA8FF),
    surface_bg: RgbaHex::rgba(0x0A0A0A52),
    surface_header_bg: RgbaHex::rgba(0x0606065C),
    surface_selected: RgbaHex::rgba(0xFFFFFF0D),
    surface_hover: RgbaHex::rgba(0xFFFFFF0A),
    surface_border: RgbaHex::rgba(0xFFFFFF11),
    surface_text: RgbaHex::rgba(0xF7F5F2FF),
    surface_muted: RgbaHex::rgba(0xB3AFA8FF),
    input_bg: RgbaHex::rgba(0x05050573),
    track_bg: RgbaHex::rgba(0x04040499),
    modal_scrim: RgbaHex::rgba(0x00000073),
    modal_bg: RgbaHex::rgb(0x141414),
    popover_bg: RgbaHex::rgba(0x141414E6),
    accent: RgbaHex::rgba(0xE58A35FF),
    accent_hover: RgbaHex::rgba(0xD97724FF),
    accent_active: RgbaHex::rgba(0xC2661BFF),
    accent_foreground: RgbaHex::rgba(0x141311FF),
    info: RgbaHex::rgba(0x3B82F6FF),
    success: RgbaHex::rgba(0x22C55EFF),
    danger: RgbaHex::rgba(0xEF4444FF),
    danger_foreground: RgbaHex::rgba(0x141311FF),
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
    chrome_bg: RgbaHex::rgba(0xFFFFFF2E),
    chrome_elevated: RgbaHex::rgba(0xFFFFFF66),
    chrome_border: RgbaHex::rgba(0x0000001A),
    chrome_text: RgbaHex::rgba(0x111113FF),
    chrome_muted: RgbaHex::rgba(0x44444BFF),
    surface_bg: RgbaHex::rgba(0xFFFFFF8A),
    surface_header_bg: RgbaHex::rgba(0xF4F4F473),
    surface_selected: RgbaHex::rgba(0x0000001A),
    surface_hover: RgbaHex::rgba(0x0000000F),
    surface_border: RgbaHex::rgba(0x00000018),
    surface_text: RgbaHex::rgba(0x111113FF),
    surface_muted: RgbaHex::rgba(0x44444BFF),
    input_bg: RgbaHex::rgba(0xFFFFFFA6),
    track_bg: RgbaHex::rgba(0x0000001C),
    modal_scrim: RgbaHex::rgba(0x00000038),
    modal_bg: RgbaHex::rgb(0xFAFAFA),
    popover_bg: RgbaHex::rgba(0xFFFFFFE6),
    accent: RgbaHex::rgba(0xB85208FF),
    accent_hover: RgbaHex::rgba(0xAA4D08FF),
    accent_active: RgbaHex::rgba(0x8F3F06FF),
    accent_foreground: RgbaHex::rgba(0xFFFFFFFF),
    info: RgbaHex::rgba(0x2563EBFF),
    success: RgbaHex::rgba(0x15803DFF),
    danger: RgbaHex::rgba(0xDC2626FF),
    danger_foreground: RgbaHex::rgba(0xFFFFFFFF),
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
    chrome_bg: RgbaHex::rgba(0x202020FF),
    chrome_elevated: RgbaHex::rgba(0x2B2B2BFF),
    chrome_border: RgbaHex::rgba(0x363636FF),
    chrome_text: RgbaHex::rgba(0xF7F5F2FF),
    chrome_muted: RgbaHex::rgba(0xB3AFA8FF),
    surface_bg: RgbaHex::rgba(0x272727FF),
    surface_header_bg: RgbaHex::rgba(0x1C1C1CFF),
    surface_selected: RgbaHex::rgba(0x3A3A3AFF),
    surface_hover: RgbaHex::rgba(0x323232FF),
    surface_border: RgbaHex::rgba(0x343434FF),
    surface_text: RgbaHex::rgba(0xF7F5F2FF),
    surface_muted: RgbaHex::rgba(0xB3AFA8FF),
    input_bg: RgbaHex::rgba(0x181818FF),
    track_bg: RgbaHex::rgba(0x121212FF),
    modal_scrim: RgbaHex::rgba(0x00000080),
    modal_bg: RgbaHex::rgba(0x242424FF),
    popover_bg: RgbaHex::rgba(0x282828FF),
    accent: RgbaHex::rgba(0xE58A35FF),
    accent_hover: RgbaHex::rgba(0xD97724FF),
    accent_active: RgbaHex::rgba(0xC2661BFF),
    accent_foreground: RgbaHex::rgba(0x141311FF),
    info: RgbaHex::rgba(0x3B82F6FF),
    success: RgbaHex::rgba(0x22C55EFF),
    danger: RgbaHex::rgba(0xEF4444FF),
    danger_foreground: RgbaHex::rgba(0x141311FF),
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
    chrome_bg: RgbaHex::rgba(0xF3F3F3FF),
    chrome_elevated: RgbaHex::rgba(0xFFFFFFFF),
    chrome_border: RgbaHex::rgba(0xDCDCDCFF),
    chrome_text: RgbaHex::rgba(0x111113FF),
    chrome_muted: RgbaHex::rgba(0x44444BFF),
    surface_bg: RgbaHex::rgba(0xFAFAFAFF),
    surface_header_bg: RgbaHex::rgba(0xEFEFEFFF),
    surface_selected: RgbaHex::rgba(0xE0E0E0FF),
    surface_hover: RgbaHex::rgba(0xEAEAEAFF),
    surface_border: RgbaHex::rgba(0xE2E2E2FF),
    surface_text: RgbaHex::rgba(0x111113FF),
    surface_muted: RgbaHex::rgba(0x44444BFF),
    input_bg: RgbaHex::rgba(0xFFFFFFFF),
    track_bg: RgbaHex::rgba(0xE4E4E4FF),
    modal_scrim: RgbaHex::rgba(0x00000040),
    modal_bg: RgbaHex::rgba(0xFAFAFAFF),
    popover_bg: RgbaHex::rgba(0xFFFFFFFF),
    accent: RgbaHex::rgba(0xB85208FF),
    accent_hover: RgbaHex::rgba(0xAA4D08FF),
    accent_active: RgbaHex::rgba(0x8F3F06FF),
    accent_foreground: RgbaHex::rgba(0xFFFFFFFF),
    info: RgbaHex::rgba(0x2563EBFF),
    success: RgbaHex::rgba(0x15803DFF),
    danger: RgbaHex::rgba(0xDC2626FF),
    danger_foreground: RgbaHex::rgba(0xFFFFFFFF),
    focus_ring: RgbaHex::rgba(0xC2590AFF),
};

/// Ordered list of the 5 built-in opaque theme palettes.
pub const BUILTIN_OPAQUE_THEMES: [ThemePalette; 5] = [
    CHRONOPHOTOGRAPHIC,
    BLUEPRINT_CYANOTYPE,
    PHOSPHOR_AMBER,
    NORDIC_FROST,
    ARCHIVAL_PAPER,
];

/// Ordered catalog of all 8 built-in theme presets.
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
        swatches: [0xECE8DF, 0xF7F5F0, 0xB85208],
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
        swatches: [0xEAEAEA, 0xF7F7F7, 0xB85208],
        is_mica: true,
    },
];

/// Optional accent override preset layered over a [`ThemePalette`].
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct AccentPreset {
    /// Persisted override identifier.
    pub id: &'static str,
    /// Human-readable display name.
    pub name: &'static str,
    /// Primary accent color (`0xRRGGBB`).
    pub accent: u32,
    /// Hover tint (`0xRRGGBB`).
    pub accent_hover: u32,
    /// Active/pressed tint (`0xRRGGBB`).
    pub accent_active: u32,
    /// Foreground ink on primary accent fills (`0xRRGGBB`).
    pub accent_foreground: u32,
    /// Focus ring outline color (`0xRRGGBB`).
    pub focus_ring: u32,
}

/// Curated accent override presets.
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
        accent_foreground: 0x090D14,
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
        accent_foreground: 0x141311,
        focus_ring: 0xEF4444,
    },
];

/// Extensible catalog combining built-in themes and accents with caller-supplied overrides.
#[derive(Debug, Clone, Copy, Default)]
pub struct ThemeCatalog<'a> {
    custom_palettes: &'a [ThemePalette],
    custom_presets: &'a [ThemePreset],
    custom_accents: &'a [AccentPreset],
}

impl<'a> ThemeCatalog<'a> {
    /// Creates a catalog containing only the built-in presets and accents.
    pub const fn new() -> Self {
        Self {
            custom_palettes: &[],
            custom_presets: &[],
            custom_accents: &[],
        }
    }

    /// Registers caller-supplied [`ThemePalette`] entries checked before built-ins.
    pub const fn with_palettes(mut self, palettes: &'a [ThemePalette]) -> Self {
        self.custom_palettes = palettes;
        self
    }

    /// Registers caller-supplied [`ThemePreset`] metadata entries checked before built-ins.
    pub const fn with_presets(mut self, presets: &'a [ThemePreset]) -> Self {
        self.custom_presets = presets;
        self
    }

    /// Registers caller-supplied [`AccentPreset`] entries checked before built-ins.
    pub const fn with_accents(mut self, accents: &'a [AccentPreset]) -> Self {
        self.custom_accents = accents;
        self
    }

    /// Looks up an [`AccentPreset`] by `id`.
    pub fn find_accent_preset(&self, id: &str) -> Option<&AccentPreset> {
        self.custom_accents
            .iter()
            .chain(ACCENT_PRESETS.iter())
            .find(|p| p.id == id)
    }

    /// Looks up a [`ThemePreset`] by `id`, falling back to `chronophotographic`.
    pub fn find_theme_preset(&self, id: &str) -> &ThemePreset {
        self.custom_presets
            .iter()
            .chain(BUILTIN_THEME_PRESETS.iter())
            .find(|p| p.id == id)
            .unwrap_or(&BUILTIN_THEME_PRESETS[0])
    }

    /// Resolves a theme preset `id`, optional accent override, OS appearance, and Mica capability
    /// into a concrete [`ThemePalette`] without any platform side effects.
    pub fn resolve_palette(
        &self,
        id: &str,
        accent_override: Option<&str>,
        os_appearance: WindowAppearance,
        mica_available: bool,
    ) -> ThemePalette {
        let os_is_light = matches!(
            os_appearance,
            WindowAppearance::Light | WindowAppearance::VibrantLight
        );
        let mut palette = if let Some(custom) = self.custom_palettes.iter().find(|p| p.id == id) {
            *custom
        } else {
            match id {
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
            }
        };

        if let Some(preset) = accent_override.and_then(|aid| self.find_accent_preset(aid)) {
            palette.accent = RgbaHex::rgb(preset.accent);
            palette.accent_hover = RgbaHex::rgb(preset.accent_hover);
            palette.accent_active = RgbaHex::rgb(preset.accent_active);
            palette.accent_foreground = RgbaHex::rgb(preset.accent_foreground);
            palette.focus_ring = RgbaHex::rgb(preset.focus_ring);
        }

        palette
    }
}

/// Looks up a built-in [`AccentPreset`] by `id`.
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

/// Resolves a built-in theme preset `id`, optional accent override, OS appearance, and Mica capability
/// into a concrete [`ThemePalette`] without any platform side effects.
pub fn resolve_palette(
    id: &str,
    accent_override: Option<&str>,
    os_appearance: WindowAppearance,
    mica_available: bool,
) -> ThemePalette {
    ThemeCatalog::new().resolve_palette(id, accent_override, os_appearance, mica_available)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn wcag_contrast_passes_for_all_opaque_themes_and_fallbacks() {
        let palettes = [
            CHRONOPHOTOGRAPHIC,
            BLUEPRINT_CYANOTYPE,
            PHOSPHOR_AMBER,
            NORDIC_FROST,
            ARCHIVAL_PAPER,
            MICA_DARK_FALLBACK,
            MICA_LIGHT_FALLBACK,
        ];
        for p in palettes {
            let chrome_ratio = contrast_ratio(p.chrome_text, p.chrome_bg);
            assert!(
                chrome_ratio >= 4.5,
                "{}: chrome_text on chrome_bg contrast {chrome_ratio:.2} < 4.5",
                p.id
            );
            let surface_ratio = contrast_ratio(p.surface_text, p.surface_bg);
            assert!(
                surface_ratio >= 4.5,
                "{}: surface_text on surface_bg contrast {surface_ratio:.2} < 4.5",
                p.id
            );
            let accent_ratio = contrast_ratio(p.accent_foreground, p.accent);
            assert!(
                accent_ratio >= 4.5,
                "{}: accent_foreground on accent contrast {accent_ratio:.2} < 4.5",
                p.id
            );
            let danger_ratio = contrast_ratio(p.danger_foreground, p.danger);
            assert!(
                danger_ratio >= 4.5,
                "{}: danger_foreground on danger contrast {danger_ratio:.2} < 4.5",
                p.id
            );
            let chrome_muted_ratio = contrast_ratio(p.chrome_muted, p.chrome_bg);
            assert!(
                chrome_muted_ratio >= 3.0,
                "{}: chrome_muted on chrome_bg contrast {chrome_muted_ratio:.2} < 3.0",
                p.id
            );
            let surface_muted_ratio = contrast_ratio(p.surface_muted, p.surface_bg);
            assert!(
                surface_muted_ratio >= 3.0,
                "{}: surface_muted on surface_bg contrast {surface_muted_ratio:.2} < 3.0",
                p.id
            );
        }
    }

    #[test]
    fn wcag_contrast_passes_for_all_accent_presets() {
        for preset in ACCENT_PRESETS {
            let ratio = contrast_ratio(
                RgbaHex::rgb(preset.accent_foreground),
                RgbaHex::rgb(preset.accent),
            );
            assert!(
                ratio >= 4.5,
                "accent preset {}: accent_foreground on accent contrast {ratio:.2} < 4.5",
                preset.id
            );
        }
    }

    #[test]
    fn variant_c_alpha_calibration_bounds_hold() {
        for mica in [MICA_DARK, MICA_LIGHT] {
            assert_eq!(mica.titlebar_bg.a(), 0, "{}: titlebar_bg must be 0%", mica.id);
            assert!(
                mica.chrome_bg.a() > 0 && mica.chrome_bg.a() < mica.surface_bg.a(),
                "{}: expected 0 < chrome_bg.a ({}) < surface_bg.a ({})",
                mica.id,
                mica.chrome_bg.a(),
                mica.surface_bg.a()
            );
            assert!(
                mica.surface_bg.a() < 255,
                "{}: surface_bg must be translucent",
                mica.id
            );
            assert_eq!(mica.modal_bg.a(), 255, "{}: modal_bg must be opaque", mica.id);
        }

        for opaque in [
            CHRONOPHOTOGRAPHIC,
            BLUEPRINT_CYANOTYPE,
            PHOSPHOR_AMBER,
            NORDIC_FROST,
            ARCHIVAL_PAPER,
            MICA_DARK_FALLBACK,
            MICA_LIGHT_FALLBACK,
        ] {
            for (field, color) in [
                ("titlebar_bg", opaque.titlebar_bg),
                ("chrome_bg", opaque.chrome_bg),
                ("chrome_elevated", opaque.chrome_elevated),
                ("surface_bg", opaque.surface_bg),
                ("surface_header_bg", opaque.surface_header_bg),
                ("input_bg", opaque.input_bg),
                ("track_bg", opaque.track_bg),
                ("modal_bg", opaque.modal_bg),
                ("popover_bg", opaque.popover_bg),
            ] {
                assert_eq!(
                    color.a(),
                    255,
                    "{}: non-scrim surface {field} must be opaque (a=255)",
                    opaque.id
                );
            }
        }
    }

    #[test]
    fn builtin_presets_and_catalog_resolve_correctly() {
        assert_eq!(BUILTIN_THEME_PRESETS.len(), 8);
        assert_eq!(ACCENT_PRESETS.len(), 5);
        assert_eq!(find_theme_preset("cyanotype").name, "Blueprint Cyanotype");
        assert_eq!(find_theme_preset("nonexistent").id, CHRONOPHOTOGRAPHIC.id);
        assert_eq!(
            resolve_palette("mica-system", None, WindowAppearance::Dark, true),
            MICA_DARK
        );
        assert_eq!(
            resolve_palette("mica-system", None, WindowAppearance::Light, false),
            MICA_LIGHT_FALLBACK
        );

        let custom_palette = ThemePalette {
            id: "custom-lab",
            name: "Custom Lab",
            ..CHRONOPHOTOGRAPHIC
        };
        let custom_accent = AccentPreset {
            id: "violet",
            name: "Violet",
            accent: 0x7C3AED,
            accent_hover: 0x8B5CF6,
            accent_active: 0x6D28D9,
            accent_foreground: 0xFFFFFF,
            focus_ring: 0x7C3AED,
        };
        let catalog = ThemeCatalog::new()
            .with_palettes(std::slice::from_ref(&custom_palette))
            .with_accents(std::slice::from_ref(&custom_accent));
        let resolved =
            catalog.resolve_palette("custom-lab", Some("violet"), WindowAppearance::Dark, false);
        assert_eq!(resolved.id, "custom-lab");
        assert_eq!(resolved.accent, RgbaHex::rgb(0x7C3AED));
    }
}
