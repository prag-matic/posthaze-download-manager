---
name: "Posthaze Download Manager"
description: "Calibrated Chronophotographic Interval Plate desktop download manager built on GPUI and GPUI Kit."
world: "Chronophotographic Interval Plate"
colors:
  selenium-amber: "#e58a35"
  cyanotype-blue: "#3b82f6"
  halide-emerald: "#22c55e"
  errata-vermilion: "#ef4444"
  graphite-bg: "#141311"
  graphite-elevated: "#1b1a17"
  graphite-border: "#2a2825"
  silver-text: "#eae6df"
  silver-muted: "#9b968d"
  table-bg: "#1e1d1a"
  table-header: "#181714"
  table-selected: "#2d2b26"
  table-hover: "#262420"
  table-border: "#2c2a27"
  byte-track-bg: "#0d0c0b"
typography:
  display:
    fontFamily: "Consolas, monospace"
    fontSize: "18px"
    fontWeight: 700
    lineHeight: 1.33
    letterSpacing: "0em"
  headline:
    fontFamily: "Segoe UI, system-ui, sans-serif"
    fontSize: "15px"
    fontWeight: 700
    lineHeight: 1.33
    letterSpacing: "0em"
  title:
    fontFamily: "Segoe UI, system-ui, sans-serif"
    fontSize: "13.5px"
    fontWeight: 600
    lineHeight: 1.33
    letterSpacing: "0em"
  body:
    fontFamily: "Segoe UI, system-ui, sans-serif"
    fontSize: "12px"
    fontWeight: 400
    lineHeight: 1.33
    letterSpacing: "0em"
  label:
    fontFamily: "Segoe UI, system-ui, sans-serif"
    fontSize: "10.5px"
    fontWeight: 600
    lineHeight: 1.33
    letterSpacing: "0.02em"
  mono-micro:
    fontFamily: "Consolas, monospace"
    fontSize: "11px"
    fontWeight: 400
    lineHeight: 1.27
    letterSpacing: "0em"
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
components:
  filter-tab-active:
    backgroundColor: "{colors.graphite-elevated}"
    textColor: "{colors.silver-text}"
    rounded: "{rounded.control}"
    padding: "6px 10px"
  url-input:
    backgroundColor: "{colors.paper-bg}"
    textColor: "{colors.carbon-text}"
    rounded: "{rounded.control}"
    padding: "8px 12px"
  modal-plate:
    backgroundColor: "{colors.graphite-bg}"
    textColor: "{colors.silver-text}"
    rounded: "{rounded.modal}"
    padding: "20px"
  inspector-deck:
    backgroundColor: "{colors.graphite-bg}"
    textColor: "{colors.silver-text}"
    height: "240px"
    padding: "12px 16px"
---

# Design System: Posthaze Download Manager

## Overview

**Creative North Star: "Chronophotographic Interval Plate"**

Posthaze Download Manager pairs a deep warm graphite shell (`#141311`) with a warm charcoal table work surface (`#1E1D1A`) so power users can scan dozens of concurrent HTTP/HTTPS `Download`s without visual fatigue while reading exact byte-range `Checkpoint` telemetry at a glance. Rather than mimicking dark-neon cyber downloaders or cramped 2000s shareware grids, every surface separates structural chrome from live transfer evidence through calibrated contrast and tabular precision.

Implemented in pure Rust (`crates/posthaze-ui`) with GPUI and GPUI Kit `0.10.0`, the system enforces an instrument-grade hierarchy: `#141311` graphite owns the custom titlebar, collapsible icon rail, confirmation modals, and the selected `Download` byte-map deck, while `#1E1D1A` warm charcoal owns the toolbar and sortable `Downloads` table.

**Key Characteristics:**
- Tonal dark-ground separation between `#141311` instrument rails and `#1E1D1A` charcoal table workspace.
- Two-tier transfer honesty distinguishing writer-acknowledged disk `Checkpoint`s (Cyanotype Blue) from in-flight `Chunk`s (Selenium Amber).
- Tabular monospaced telemetry (`Consolas`) paired with Windows system UI sans (`Segoe UI`).
- Hairline `1px` borders with flat elevation and crisp 90ms two-frame state snaps.

## Colors

A restrained dark-instrument palette where warm neutrals carry 90% of the viewport and four semantic telemetry hues encode live engine state.

### Primary
- **Selenium Amber** (`#E58A35`): Active `Download` state mark, primary CTAs (`Paste & Probe`, `Download Now`, `Save Settings`), selected row/filter accent bar, and in-flight `Segment` bar fill ahead of the last disk `Checkpoint`.

### Secondary
- **Cyanotype Blue** (`#3B82F6`): Writer-acknowledged disk `Checkpoint` bar fill, `206 MULTI-SEGMENT` capability badge, and `Probing`/`Queued` state marks.

### Tertiary
- **Halide Emerald** (`#22C55E`): `Completed` state mark and finished `Segment` fill inside the byte-map ruler.
- **Errata Vermilion** (`#EF4444`): `Failed` state mark, probe/confirm error borders, dismissible status banner, and the inline diagnostic / `ResourceChanged` errata recovery strip.

### Neutral
- **Graphite Emulsion** (`#141311`): Custom titlebar, left filter rail, bottom inspector deck, recessed URL input, and modal plates.
- **Graphite Elevated** (`#1B1A17`): Telemetry readout card, validator badges, and segment cards.
- **Graphite Rule** (`#2A2825`): `1px` structural dividers on `#141311` graphite surfaces.
- **Silver Halide Ink** (`#EAE6DF`): Primary text across all dark surfaces.
- **Silver Muted** (`#9B968D`): Secondary metadata, labels, and ruler tick text.
- **Charcoal Table** (`#1E1D1A` / `#181714` header): Center `Downloads` table work surface and top dispatch toolbar.
- **Table Selected / Hover** (`#2D2B26` / `#262420`): Selected and hovered `Download` row fills.
- **Table Rule** (`#2C2A27`): `1px` construction-grid row and column dividers.
- **Byte Track** (`#0D0C0B`): Deep recessed track for the Graduated Interval Byte-Map Ruler.

### Named Rules
**The Dual-Ground Truth Rule.** Never paint active telemetry colors onto decorative backgrounds; Selenium Amber (`#E58A35`) and Cyanotype Blue (`#3B82F6`) exclusively signify live bytes in flight versus flushed bytes on disk.

## Typography

**Display Font:** Consolas (with monospace fallback) for aggregate speed readouts
**Body Font:** Segoe UI (Windows system UI sans) for filenames, navigation, and actions
**Label/Mono Font:** Consolas for byte intervals, ETAs, FIFO rank badges, and HTTP `Validator`s

**Character:** System sans carries navigation and filenames cleanly at desktop DPI, while `Consolas` guarantees tabular alignment across rapidly updating speeds, byte offsets, and `[start..end)` `Segment` ranges on both Windows 10 and Windows 11.

### Hierarchy
- **Display** (`700`, `18px`, `1.33`): Empty-state title and sidebar aggregate `THROUGHPUT` readout.
- **Headline** (`700`, `15px`, `1.33`): Brand plate `POSTHAZE`, modal headings, and selected inspector filename.
- **Title** (`600`, `13.5px`, `1.33`): Table `filename` cells inside `40px` rows.
- **Body** (`400`, `12px`, `1.33`): Speed, ETA, segment count, and modal field labels (`18px` leading on `#181715` dark surfaces).
- **Mono Micro** (`400`, `11px`, `1.27`): Progress byte counters, ruler quarter ticks, `Validator` badges, and `Segment` grid cells.
- **Label** (`600`, `10.5px`, `1.33`, uppercase): Table column headers and sidebar section kickers.

### Named Rules
**The Light-on-Dark Compensation Rule.** Text rendered on `#181715` graphite surfaces steps up line height (`18px` for `12px` body, `16px` for `11px` micro) so silver ink stays legible without glare.

## Layout

Single-window desktop shell (`1020×640` default & minimum size, expandable):
- **Custom Titlebar**: Fixed `34px` GPUI Kit `TitleBar` on `#141311` graphite with a `1px` `#2A2825` bottom rule, framed Selenium Amber plate mark, `POSTHAZE // v0.3.0-m3` monospace identifier, window drag / double-click maximize, and native Windows caption controls.
- **Left Filter Rail**: Defaults to the collapsed `52px` icon rail; expandable & resizable (`220px` default expanded width, `180px–380px` drag range, snapping back to `52px` below `130px`, double-click to reset) on `#141311` graphite.
- **Top URL Dispatch Toolbar**: Fixed `54px` height on `#1E1D1A` warm charcoal with `16px` horizontal padding, unified `#141311` URL input with tucked icon-only `Paste & Probe` button, and ghost batch controls.
- **Center `Downloads` Table**: `flex-1` scrollable region with `28px` sortable modular construction-grid header (`#181714`) and `38px` data rows using shared column width stops (`104px` state, `flex-1 min 120px` filename, `210px` progress, `90px` speed, `64px` ETA, `48px` segments, `148px` actions).
- **Bottom Selected Download Inspector**: Defaults to the collapsed `28px` summary strip; expandable & resizable (`240px` default expanded height, `150px–460px` drag range, double-click to reset) on `#141311` graphite.

## Elevation & Depth

Flat tonal layering separated by `1px` hairline borders (`#2C2A27` on dark graphite, `#DFDBD2` on warm paper). Structural panes at rest use zero drop shadows or blur-based elevation. Floating overlays isolate focus above the shell: standard modals (`Settings`, `File-Op`) and context menus pair a `1px` `graphite_border()` with a subtle elevation shadow (`shadow_lg()` / `shadow_md()`) so frosted plates stay crisp over translucent Mica glass, while destructive `Confirm` modals retain their `1px` `errata_vermilion()` border behind a translucent scrim (`modal_scrim()`).

### Named Rules
**The Hairline Partition Rule.** Structural depth across resting panes comes from ground contrast (`#181715` vs `#F2EFE9`) and `1px` structural rules—never soft shadows or glowing halos on non-floating chrome.

## Shapes

Sharp, instrument-calibrated corners across the entire shell: `3px` on `Validator` badges, `4px` on filter pills and action buttons, `6px` on modal confirmation plates, and `0px` square ends on table progress bars and the **Graduated Interval Byte-Map Ruler** so byte boundaries read cleanly without rounded-cap distortion.

## Components

### Buttons
- **Shape:** Subtle `4px` radius (`rounded.control`), compact `small` / `xsmall` sizing in toolbars and table rows.
- **Primary:** `gpui-kit` primary variant for `Paste & Probe`, `Download Now`, `Save Settings`, and `Restart Fresh`.
- **Secondary:** `gpui-kit` default button for `Resume All`, `Pause All`, `Clear Completed`, `Add to Queue`, and inline row controls.

### Cards / Containers
- **Corner Style:** `4px` for sidebar telemetry card (`#22201D`), `6px` for modal plates (`#181715`).
- **Border:** `1px` `#2C2A27` (`#DC2626` on probe error plates).
- **Internal Padding:** `10px` on telemetry cards, `20px` on modals.

### Inputs / Fields
- **Style:** `gpui-kit` `Input` wrapped in `Consolas` (`12px`) with `1px` border.
- **Focus:** Auto-focused on URL dispatch bar when the queue is empty; `Enter` triggers `DownloadEngine::probe`.

### Navigation
- **Left Filter Rail:** Vertical stack of six filter pills (`All Transfers`, `Active`, `Queued`, `Paused`, `Completed`, `Failed`) with live tabular counts; selected pill switches to `#22201D` fill with a `1px` `#D97724` Selenium Amber border.

### Graduated Interval Byte-Map Ruler
- Full-width `18px` horizontal bar framed by engraved `0 B (0%)`, `25%`, `50%`, `75%`, and `100%` quarter tick marks, rendering proportional `[start, end)` `Segment` cells with `1px` `#181715` work-steal split dividers, Cyanotype Blue (`#2563EB`) `Checkpoint` fill, and Selenium Amber (`#D97724`) in-flight fill.

### Table-Grid Cell System (`crates/posthaze-ui/src/grid_cells.rs`)
All interactive surfaces (`table.rs`, `sidebar.rs`, `inspector.rs`, `modals.rs`, `settings_modal.rs`, and future views) compose from `grid_cells.rs` rather than standalone rounded pill buttons:
- **Cell Buttons (`grid_cell_button` & `grid_boxed_button`):** Full-height (`h_full()`) or standalone `1px`-bordered (`grid_boxed_button`) cells with `0px` corner radius and `1px` vertical `paper_border()` / `graphite_border()` rules (`CellBorder::Left | Right | Both`). Uppercase `Consolas` `11px` (`TYPE_MICRO`) semibold labels; hover fills the entire cell with `selenium_amber()` and `accent_foreground` (or `errata_vermilion()` + `#FFFFFF` for `GridTone::Danger`).
- **Section Headers (`grid_section_header`) & Modal Footers (`grid_modal_footer`):** `28px` modular header bar (`table_header_bg()` on paper, `graphite_elevated()` on graphite) and flush `34px` modal footer action strip with `1px` rules.
- **Key-Value Rows (`grid_kv_row`) & Errata Tint (`errata_surface_bg()`):** Fixed-width `LABEL` cell | `1px` vertical divider | `flex_1` value/input cell (`labeled_input`, sidebar telemetry, inspector metadata), with `errata_surface_bg()` for diagnostic/validation strips.

### Do's and Don'ts

### Do:
- **Do** compute all `Segment` ratios and formatting in pure Rust (`crates/posthaze-ui/src/view_model.rs`) with unit tests.
- **Do** keep `crates/posthaze-ui` strictly decoupled from HTTP, SQLite, and `.part` files via `EngineBridge`.
- **Do** surface `EngineError::ResourceChanged` inline with `Restart Fresh` and `Delete` actions.

### Don't:
- **Don't** round the ends of the **Graduated Interval Byte-Map Ruler** or progress bars (`0px` radius keeps byte ratios exact).
- **Don't** block GPUI's render thread with `DownloadEngine` calls; dispatch via `EngineBridge` on the background Tokio runtime.
- **Don't** invent placeholder data or skip `probe()` before starting a URL from the UI.

## Windows 11 Mica Themes (Variant C)

Posthaze adds three native Windows 11 DWM Mica presets (`Mica (System)`, `Mica Dark`, and `Mica Light`) using **Variant C ("Unified Translucent Plate")** surface layering. Structural chrome carries the primary Mica translucency while dense tabular data, inputs, modals, and context menus sit on calibrated frosted plates that preserve legibility over arbitrary desktop wallpapers.

### DWM Materials & Polarity Calibration
- **`MICA_DARK`** (`ThemeBackdrop::Mica`, `DWMSBT_MAINWINDOW`): Standard Windows 11 dark Mica paired with Selenium Amber (`#E58A35`) accents and warm silver ink (`#F7F5F2`).
- **`MICA_LIGHT`** (`ThemeBackdrop::MicaAlt`, `DWMSBT_TABBEDWINDOW`): Windows 11 Mica Alt light glass paired with burnt sienna (`#C2590A`) accents and deep carbon ink (`#111113`).
- **`Mica (System)`** (`id: "mica-system"`, badge `AUTO`): Resolves dynamically to `MICA_DARK` or `MICA_LIGHT` from Windows Dark/Light appearance (`window.appearance()` / `observe_window_appearance`). Forced polarity presets (`mica-dark`, `mica-light`) ignore OS polarity changes and re-assert `DWMWA_USE_IMMERSIVE_DARK_MODE` after `WM_SETTINGCHANGE`.

### Paired Neutral Opaque Fallback Palettes
When native Mica translucency is unavailable—Windows 10 or builds `< 22621`, `GPUI_DISABLE_DIRECT_COMPOSITION=1`, Windows High Contrast mode (`SPI_GETHIGHCONTRAST`), Windows Transparency Effects disabled (`EnableTransparency == 0`), or a DWM `HRESULT < 0` rejection—`resolve_palette` degrades seamlessly to calibrated neutral opaque palettes (`ThemeBackdrop::Opaque`) and displays an inline status note in **Settings > Interface**:
- **`MICA_DARK_FALLBACK`**: Neutral `#202020` chrome (`titlebar_bg`, `graphite_bg`), `#272727` `paper_bg`, `#1C1C1C` `table_header_bg`, `#181818` `input_bg`, `#242424` `modal_bg`, `#282828` `context_menu_bg`.
- **`MICA_LIGHT_FALLBACK`**: Neutral `#F3F3F3` chrome (`titlebar_bg`, `graphite_bg`), `#FAFAFA` `paper_bg`, `#EFEFEF` `table_header_bg`, `#FFFFFF` `input_bg`, `#FAFAFA` `modal_bg`, `#FFFFFF` `context_menu_bg`.

### Variant C Token Table (`RgbaHex`)

| Token | `MICA_DARK` (`Mica`) | `MICA_LIGHT` (`MicaAlt`) | `MICA_DARK_FALLBACK` (`Opaque`) | `MICA_LIGHT_FALLBACK` (`Opaque`) |
|---|---|---|---|---|
| `titlebar_bg` | `#00000000` (0%) | `#00000000` (0%) | `#202020FF` | `#F3F3F3FF` |
| `graphite_bg` | `#08080828` (16%) | `#FFFFFF2E` (18%) | `#202020FF` | `#F3F3F3FF` |
| `graphite_elevated` | `#FFFFFF09` (3.5%) | `#FFFFFF66` (40%) | `#2B2B2BFF` | `#FFFFFFFF` |
| `graphite_border` | `#FFFFFF14` (8%) | `#0000001A` (10%) | `#363636FF` | `#DCDCDCFF` |
| `paper_bg` | `#0A0A0A52` (32%) | `#FFFFFF8A` (54%) | `#272727FF` | `#FAFAFAFF` |
| `table_header_bg` | `#0606065C` (36%) | `#F4F4F473` (45%) | `#1C1C1CFF` | `#EFEFEFFF` |
| `paper_hover` | `#FFFFFF0A` (4%) | `#0000000F` (6%) | `#323232FF` | `#EAEAEAFF` |
| `paper_selected` | `#FFFFFF0D` (5%) | `#0000001A` (10%) | `#3A3A3AFF` | `#E0E0E0FF` |
| `paper_border` | `#FFFFFF11` (6.7%) | `#00000018` (9.4%) | `#343434FF` | `#E2E2E2FF` |
| `input_bg` | `#05050573` (45%) | `#FFFFFFA6` (65%) | `#181818FF` | `#FFFFFFFF` |
| `byte_track_bg` | `#04040499` (60%) | `#0000001C` (11%) | `#121212FF` | `#E4E4E4FF` |
| `modal_scrim` | `#00000073` (45%) | `#00000038` (22%) | `#00000080` | `#00000040` |
| `modal_bg` | `#121212DB` (86%) | `#FAFAFAD9` (85%) | `#242424FF` | `#FAFAFAFF` |
| `context_menu_bg` | `#141414E6` (90%) | `#FFFFFFE6` (90%) | `#282828FF` | `#FFFFFFFF` |

### `Root` & `TitleBar` Compositing Rule
When `palette.backdrop != ThemeBackdrop::Opaque`, `apply_theme_with_accent` MUST set **both** `theme.background = Hsla::transparent_black()` and `theme.tokens.background = Hsla::transparent_black().into()` on `gpui_kit::component::Theme`. Omitting either causes `gpui-component`'s `RootPlugin::style` to paint an opaque sheet that occludes DWM Mica, and causes `TitleBar` to blend `theme.background` into `theme.title_bar`. In `Opaque` mode, both fields revert to `palette.graphite_bg.hsla()`.

### Single-Layer Variant C Surface Hierarchy
Every translucent surface is painted as a **single layer**—never stack duplicate `.bg(...)` fills on nested containers, which compounds alpha and muddies Mica glass:
- **Main `PosthazeApp` shell**: Transparent outer root over `Root` / DWM Mica; `TitleBar` at `0%` (`titlebar_bg()`); left filter rail and bottom inspector at `graphite_bg()` (`16%` obsidian dark / `18%` light); top toolbar and `Downloads` table at `paper_bg()` (`32%` obsidian dark / `54%` light) with `table_header_bg()` (`36%`/`45%`) and `1px` `paper_border()` / `graphite_border()` flush hairline rules.
- **Standalone `PreflightWindow`**: Transparent outer root so `TitleBar` shows pure `0%` DWM Mica, single-layer `paper_bg()` probe/form card body, and single-layer `graphite_bg()` footer bar.
- **In-window modals (`Settings`, `File-Op`, `Confirm`)**: `modal_scrim()` backdrop (`45%`/`22%`) behind a single-layer `modal_bg()` card (`86%`/`85%`) with `shadow_lg()`, without redundant `.bg(graphite_bg())` on nested columns such as the Settings navigation rail; `Settings` and `File-Op` use a `1px` `graphite_border()`, while destructive `Confirm` modals retain their `1px` `errata_vermilion()` border.
- **Context menu**: `context_menu_bg()` (`90%`/`90%`) with `1px` `graphite_border()` and `shadow_md()`, using `paper_hover()` for non-danger menu-item hover states so hover contrast stays crisp in both `Mica Light` and `Mica Dark`.

### Dual-Ground Truth & Contrast Preservation
Mica themes preserve the **Dual-Ground Truth Rule**: calibrated Selenium Amber (`#E58A35` dark / `#C2590A` light) denotes in-flight `Chunk`s and primary CTAs, strictly separated from Cyanotype Blue (`#3B82F6` dark / `#2563EB` light) writer-acknowledged disk `Checkpoint`s, Halide Emerald (`#22C55E` / `#15803D`) `Completed` state, and Errata Vermilion (`#EF4444` / `#DC2626`) `Failed` / `ResourceChanged` state. `Settings.ui_accent_override` layers over Mica presets without altering `ThemeBackdrop` or surface translucency.
