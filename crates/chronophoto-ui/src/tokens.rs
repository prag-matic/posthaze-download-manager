//! Pure-Rust design tokens for spacing, typography scale, radii, and structural chrome.

use gpui_kit::{Pixels, px};

/// Extra-small spacing token (`4px`).
pub const SPACING_XS: Pixels = px(4.0);
/// Small spacing token (`8px`).
pub const SPACING_SM: Pixels = px(8.0);
/// Medium spacing token (`12px`).
pub const SPACING_MD: Pixels = px(12.0);
/// Large spacing token (`16px`).
pub const SPACING_LG: Pixels = px(16.0);
/// Extra-large spacing token (`20px`).
pub const SPACING_XL: Pixels = px(20.0);

/// Display role font size (`18px`).
pub const TYPE_DISPLAY: Pixels = px(18.0);
/// Heading role font size (`15px`).
pub const TYPE_HEADING: Pixels = px(15.0);
/// Primary table-row font size (`13.5px`).
pub const TYPE_ROW_PRIMARY: Pixels = px(13.5);
/// Standard body font size (`12px`).
pub const TYPE_BODY: Pixels = px(12.0);
/// Micro / tabular telemetry font size (`11px`).
pub const TYPE_MICRO: Pixels = px(11.0);
/// Uppercase section/column header font size (`10.5px`).
pub const TYPE_HEADER: Pixels = px(10.5);

/// Corner radius for compact badges and pills (`3px`).
pub const RADIUS_BADGE: Pixels = px(3.0);
/// Corner radius for buttons, inputs, and controls (`4px`).
pub const RADIUS_CONTROL: Pixels = px(4.0);
/// Corner radius for modal plates (`6px`).
pub const RADIUS_MODAL: Pixels = px(6.0);

/// Standard height for modular section and table headers (`28px`).
pub const SECTION_HEADER_HEIGHT: Pixels = px(28.0);
/// Standard height for flush modal footer action strips (`34px`).
pub const MODAL_FOOTER_HEIGHT: Pixels = px(34.0);
/// Width of the vertical selection indicator bar (`3px`).
pub const INDICATOR_BAR_WIDTH: Pixels = px(3.0);

/// Duration in milliseconds for magnetic split-sash detent flashes.
pub const DETENT_FLASH_MS: f32 = 240.0;

#[cfg(test)]
mod tests {
    use super::*;
    use crate::palette::{CHRONOPHOTOGRAPHIC, RgbaHex};

    fn extract_frontmatter(md: &str) -> &str {
        let rest = md
            .strip_prefix("---\n")
            .or_else(|| md.strip_prefix("---\r\n"))
            .expect("DESIGN.md must start with YAML frontmatter ---");
        let end = rest
            .find("\n---")
            .expect("DESIGN.md must close YAML frontmatter with ---");
        &rest[..end]
    }

    fn parse_px(raw: &str) -> f32 {
        let trimmed = raw.trim().trim_matches('"').trim_matches('\'');
        trimmed
            .strip_suffix("px")
            .expect("expected px suffix")
            .parse::<f32>()
            .expect("expected numeric px value")
    }

    fn parse_hex_rgb(raw: &str) -> RgbaHex {
        let trimmed = raw.trim().trim_matches('"').trim_matches('\'');
        let hex = trimmed.strip_prefix('#').expect("expected #RRGGBB hex");
        let val = u32::from_str_radix(hex, 16).expect("valid hex");
        RgbaHex::rgb(val)
    }

    #[test]
    fn design_md_frontmatter_matches_rust_tokens_and_palette() {
        let design_md = include_str!("../DESIGN.md");
        let yaml = extract_frontmatter(design_md);

        let mut section = "";
        let mut sub_section = "";
        let mut seen_colors = 0usize;
        let mut seen_types = 0usize;
        let mut seen_radii = 0usize;
        let mut seen_spacing = 0usize;
        let mut seen_chrome = 0usize;

        for raw_line in yaml.lines() {
            let line = raw_line.trim_end_matches('\r');
            if line.trim().is_empty() {
                continue;
            }
            let indent = line.len() - line.trim_start().len();
            let trimmed = line.trim();
            if indent == 0 {
                if let Some((k, _)) = trimmed.split_once(':') {
                    section = k.trim();
                    sub_section = "";
                }
                continue;
            }

            if let Some((k, v)) = trimmed.split_once(':') {
                let key = k.trim();
                let val = v.trim();
                match section {
                    "colors" => {
                        let actual = parse_hex_rgb(val);
                        let expected = match key {
                            "accent" => CHRONOPHOTOGRAPHIC.accent,
                            "info" => CHRONOPHOTOGRAPHIC.info,
                            "success" => CHRONOPHOTOGRAPHIC.success,
                            "danger" => CHRONOPHOTOGRAPHIC.danger,
                            "chrome-bg" => CHRONOPHOTOGRAPHIC.chrome_bg,
                            "chrome-elevated" => CHRONOPHOTOGRAPHIC.chrome_elevated,
                            "chrome-border" => CHRONOPHOTOGRAPHIC.chrome_border,
                            "chrome-text" => CHRONOPHOTOGRAPHIC.chrome_text,
                            "chrome-muted" => CHRONOPHOTOGRAPHIC.chrome_muted,
                            "surface-bg" => CHRONOPHOTOGRAPHIC.surface_bg,
                            "surface-header-bg" => CHRONOPHOTOGRAPHIC.surface_header_bg,
                            "surface-selected" => CHRONOPHOTOGRAPHIC.surface_selected,
                            "surface-hover" => CHRONOPHOTOGRAPHIC.surface_hover,
                            "surface-border" => CHRONOPHOTOGRAPHIC.surface_border,
                            "track-bg" => CHRONOPHOTOGRAPHIC.track_bg,
                            other => panic!("unexpected color key in DESIGN.md: {other}"),
                        };
                        assert_eq!(actual, expected, "DESIGN.md color '{key}' drifted");
                        seen_colors += 1;
                    }
                    "typography" => {
                        if indent == 2 && val.is_empty() {
                            sub_section = key;
                        } else if key == "fontSize" {
                            let actual = parse_px(val);
                            let expected = match sub_section {
                                "display" => f32::from(TYPE_DISPLAY),
                                "heading" => f32::from(TYPE_HEADING),
                                "row-primary" => f32::from(TYPE_ROW_PRIMARY),
                                "body" => f32::from(TYPE_BODY),
                                "micro" => f32::from(TYPE_MICRO),
                                "header" => f32::from(TYPE_HEADER),
                                other => panic!("unexpected typography role in DESIGN.md: {other}"),
                            };
                            assert!(
                                (actual - expected).abs() < 1e-3,
                                "DESIGN.md typography.{sub_section}.fontSize {actual} != {expected}"
                            );
                            seen_types += 1;
                        }
                    }
                    "rounded" => {
                        let actual = parse_px(val);
                        let expected = match key {
                            "badge" => f32::from(RADIUS_BADGE),
                            "control" => f32::from(RADIUS_CONTROL),
                            "modal" => f32::from(RADIUS_MODAL),
                            other => panic!("unexpected rounded key in DESIGN.md: {other}"),
                        };
                        assert!(
                            (actual - expected).abs() < 1e-3,
                            "DESIGN.md rounded.{key} {actual} != {expected}"
                        );
                        seen_radii += 1;
                    }
                    "spacing" => {
                        let actual = parse_px(val);
                        let expected = match key {
                            "xs" => f32::from(SPACING_XS),
                            "sm" => f32::from(SPACING_SM),
                            "md" => f32::from(SPACING_MD),
                            "lg" => f32::from(SPACING_LG),
                            "xl" => f32::from(SPACING_XL),
                            other => panic!("unexpected spacing key in DESIGN.md: {other}"),
                        };
                        assert!(
                            (actual - expected).abs() < 1e-3,
                            "DESIGN.md spacing.{key} {actual} != {expected}"
                        );
                        seen_spacing += 1;
                    }
                    "chrome" => {
                        let actual = parse_px(val);
                        let expected = match key {
                            "section-header-height" => f32::from(SECTION_HEADER_HEIGHT),
                            "modal-footer-height" => f32::from(MODAL_FOOTER_HEIGHT),
                            "indicator-bar-width" => f32::from(INDICATOR_BAR_WIDTH),
                            other => panic!("unexpected chrome key in DESIGN.md: {other}"),
                        };
                        assert!(
                            (actual - expected).abs() < 1e-3,
                            "DESIGN.md chrome.{key} {actual} != {expected}"
                        );
                        seen_chrome += 1;
                    }
                    _ => {}
                }
            }
        }

        assert_eq!(seen_colors, 15, "expected 15 color tokens in DESIGN.md");
        assert_eq!(seen_types, 6, "expected 6 typography tokens in DESIGN.md");
        assert_eq!(seen_radii, 3, "expected 3 radius tokens in DESIGN.md");
        assert_eq!(seen_spacing, 5, "expected 5 spacing tokens in DESIGN.md");
        assert_eq!(seen_chrome, 3, "expected 3 chrome tokens in DESIGN.md");
    }
}
