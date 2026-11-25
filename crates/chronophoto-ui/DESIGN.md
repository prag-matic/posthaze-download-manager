---
name: "Chronophoto UI"
description: "Calibrated Chronophotographic Interval Plate design system built on GPUI and GPUI Kit."
world: "Chronophotographic Interval Plate"
colors:
  accent: "#e58a35"
  info: "#3b82f6"
  success: "#22c55e"
  danger: "#ef4444"
  chrome-bg: "#141311"
  chrome-elevated: "#1b1a17"
  chrome-border: "#2a2825"
  chrome-text: "#eae6df"
  chrome-muted: "#9b968d"
  surface-bg: "#1e1d1a"
  surface-header-bg: "#181714"
  surface-selected: "#2d2b26"
  surface-hover: "#262420"
  surface-border: "#2c2a27"
  track-bg: "#0d0c0b"
typography:
  display:
    fontSize: "18px"
    fontWeight: 700
    lineHeight: 1.33
    letterSpacing: "0em"
  heading:
    fontSize: "15px"
    fontWeight: 700
    lineHeight: 1.33
    letterSpacing: "0em"
  row-primary:
    fontSize: "13.5px"
    fontWeight: 600
    lineHeight: 1.33
    letterSpacing: "0em"
  body:
    fontSize: "12px"
    fontWeight: 400
    lineHeight: 1.33
    letterSpacing: "0em"
  micro:
    fontSize: "11px"
    fontWeight: 400
    lineHeight: 1.27
    letterSpacing: "0em"
  header:
    fontSize: "10.5px"
    fontWeight: 600
    lineHeight: 1.33
    letterSpacing: "0.02em"
rounded:
  badge: "3px"
  control: "4px"
  modal: "6px"
spacing:
  xs: "4px"
  sm: "8px"
  md: "12px"
  lg: "16px"
  xl: "20px"
chrome:
  section-header-height: "28px"
  modal-footer-height: "34px"
  indicator-bar-width: "3px"
---

# Chronophoto UI — Chronophotographic Interval Plate

`chronophoto-ui` is a pure-Rust GPUI design system implementing the **Chronophotographic Interval Plate** visual language on top of `gpui-kit`.

## Overview

The system pairs a deep structural chrome ground (`chrome-bg`, `#141311` in the default `Chronophotographic` preset) with a slightly lifted work surface (`surface-bg`, `#1E1D1A`) so dense tabular and telemetry interfaces remain legible without visual fatigue.

- **Tonal separation:** Structural rails, titlebars, inspectors, and modals sit on `chrome-bg`, while primary work surfaces and toolbars sit on `surface-bg`.
- **Semantic telemetry hues:** Four calibrated hues—Selenium Amber (`accent`), Cyanotype Blue (`info`), Halide Emerald (`success`), and Errata Vermilion (`danger`)—encode live state without decorative clutter.
- **Cross-platform typography:** System UI sans (`".SystemUIFont"`) pairs with platform-probed tabular monospace (`Consolas` on Windows, `Menlo` on macOS, `DejaVu Sans Mono` on Linux).
- **Sharp instrument corners:** `3px` badges, `4px` controls, `6px` modals, and `0px` square ends on progress tracks and table-grid cells.

## Semantic Palette Roles

`ThemePalette` uses domain- and polarity-neutral roles:

- **Chrome / rail:** `titlebar_bg`, `chrome_bg`, `chrome_elevated`, `chrome_border`, `chrome_text`, `chrome_muted`
- **Work surface:** `surface_bg`, `surface_header_bg`, `surface_selected`, `surface_hover`, `surface_border`, `surface_text`, `surface_muted`
- **Controls & overlays:** `input_bg`, `track_bg`, `modal_scrim`, `modal_bg`, `popover_bg`
- **Semantic & accent:** `accent`, `accent_hover`, `accent_active`, `accent_foreground`, `info`, `success`, `danger`, `danger_foreground`, `focus_ring`

## Windows 11 Mica (Variant C: Unified Translucent Plate)

When `mica` is enabled and available on Windows 11 (build `>= 22621`), structural chrome carries native DWM Mica (`DWMSBT_MAINWINDOW` / `DWMSBT_TABBEDWINDOW`) while dense work surfaces, inputs, modals, and popovers sit on calibrated frosted plates. Outside Windows 11 Mica, `resolve_palette` degrades cleanly to paired opaque fallback palettes (`MICA_DARK_FALLBACK`, `MICA_LIGHT_FALLBACK`).
