//! Chronophotographic Interval Plate palette & typographic role scale (`seed:a6d67de4e1140bf8`).

use gpui_kit::{Hsla, Pixels, px, rgb};

/// Authoritative tabular monospaced font for byte offsets, speeds, ETAs, and HTTP validators.
pub const MONO_FONT: &str = "Consolas";

/// Role scale sizes (in logical pixels) for Operate-mode hierarchy.
pub const TYPE_DISPLAY: Pixels = px(18.0);
pub const TYPE_HEADING: Pixels = px(15.0);
pub const TYPE_ROW_PRIMARY: Pixels = px(13.5);
pub const TYPE_BODY: Pixels = px(12.0);
pub const TYPE_MICRO: Pixels = px(11.0);
pub const TYPE_HEADER: Pixels = px(10.5);

/// Deep warm graphite emulsion rail & bottom inspector background (`#141311`).
pub fn graphite_bg() -> Hsla {
    rgb(0x141311).into()
}

/// Slightly raised graphite surface for cards/badges on dark rails (`#1B1A17`).
pub fn graphite_elevated() -> Hsla {
    rgb(0x1B1A17).into()
}

/// Hairline slate rule inside dark graphite surfaces (`#2A2825`).
pub fn graphite_border() -> Hsla {
    rgb(0x2A2825).into()
}

/// Silver-halide primary text (`#EAE6DF`).
pub fn silver_text() -> Hsla {
    rgb(0xEAE6DF).into()
}

/// Muted metadata text (`#9B968D`).
pub fn silver_muted() -> Hsla {
    rgb(0x9B968D).into()
}

/// Warm charcoal table & toolbar work surface (`#1E1D1A`).
pub fn paper_bg() -> Hsla {
    rgb(0x1E1D1A).into()
}

/// Recessed modular table header background (`#181714`).
pub fn table_header_bg() -> Hsla {
    rgb(0x181714).into()
}

/// Selected row tint on warm charcoal (`#2D2B26`).
pub fn paper_selected() -> Hsla {
    rgb(0x2D2B26).into()
}

/// Hover row tint on warm charcoal (`#262420`).
pub fn paper_hover() -> Hsla {
    rgb(0x262420).into()
}

/// Construction-grid row & column rule on warm charcoal (`#2C2A27`).
pub fn paper_border() -> Hsla {
    rgb(0x2C2A27).into()
}

/// Primary ink on warm charcoal work surface (`#EAE6DF`).
pub fn carbon_text() -> Hsla {
    rgb(0xEAE6DF).into()
}

/// Secondary ink on warm charcoal work surface (`#9B968D`).
pub fn carbon_muted() -> Hsla {
    rgb(0x9B968D).into()
}

/// Deep recessed byte-map ruler track (`#0D0C0B`).
pub fn byte_track_bg() -> Hsla {
    rgb(0x0D0C0B).into()
}

/// Selenium Amber for active in-flight `Segment` bytes & primary CTAs (`#E58A35`).
pub fn selenium_amber() -> Hsla {
    rgb(0xE58A35).into()
}

/// Cyanotype Blue for writer-acknowledged disk `Checkpoint` bytes (`#3B82F6`).
pub fn cyanotype_blue() -> Hsla {
    rgb(0x3B82F6).into()
}

/// Halide Emerald for `Completed` state (`#22C55E`).
pub fn halide_emerald() -> Hsla {
    rgb(0x22C55E).into()
}

/// Errata Vermilion for `Failed` / `ResourceChanged` (`#EF4444`).
pub fn errata_vermilion() -> Hsla {
    rgb(0xEF4444).into()
}
