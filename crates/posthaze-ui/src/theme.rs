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

/// Warm graphite emulsion shell rail & bottom inspector background (`#181715`).
pub fn graphite_bg() -> Hsla {
    rgb(0x181715).into()
}

/// Slightly raised graphite surface for cards/inputs on dark rails (`#22201D`).
pub fn graphite_elevated() -> Hsla {
    rgb(0x22201D).into()
}

/// Hairline slate rule inside dark graphite surfaces (`#2C2A27`).
pub fn graphite_border() -> Hsla {
    rgb(0x2C2A27).into()
}

/// Silver-halide primary text on dark graphite (`#EAE6DF`).
pub fn silver_text() -> Hsla {
    rgb(0xEAE6DF).into()
}

/// Muted metadata text on dark graphite (`#9C968B`).
pub fn silver_muted() -> Hsla {
    rgb(0x9C968B).into()
}

/// Silver-halide warm paper work surface for the Downloads table (`#F2EFE9`).
pub fn paper_bg() -> Hsla {
    rgb(0xF2EFE9).into()
}

/// Selected row tint on silver-halide paper (`#E6E0D4`).
pub fn paper_selected() -> Hsla {
    rgb(0xE6E0D4).into()
}

/// Hover row tint on silver-halide paper (`#ECE8DF`).
pub fn paper_hover() -> Hsla {
    rgb(0xECE8DF).into()
}

/// Subtle row rule on silver-halide paper (`#DFDBD2`).
pub fn paper_border() -> Hsla {
    rgb(0xDFDBD2).into()
}

/// Carbon ink primary text on silver-halide paper (`#141413`).
pub fn carbon_text() -> Hsla {
    rgb(0x141413).into()
}

/// Secondary ink on silver-halide paper (`#5C5852`).
pub fn carbon_muted() -> Hsla {
    rgb(0x5C5852).into()
}

/// Selenium Amber for active in-flight `Segment` bytes (`#D97724`).
pub fn selenium_amber() -> Hsla {
    rgb(0xD97724).into()
}

/// Cyanotype Blue for writer-acknowledged disk `Checkpoint` bytes (`#2563EB`).
pub fn cyanotype_blue() -> Hsla {
    rgb(0x2563EB).into()
}

/// Halide Emerald for `Completed` state (`#15803D`).
pub fn halide_emerald() -> Hsla {
    rgb(0x15803D).into()
}

/// Errata Vermilion for `Failed` / `ResourceChanged` (`#DC2626`).
pub fn errata_vermilion() -> Hsla {
    rgb(0xDC2626).into()
}
