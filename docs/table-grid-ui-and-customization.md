# Table-Grid Cell UI System & Interface Customization

Posthaze's desktop UI (`crates/posthaze-ui`) is built on the reusable **Table-Grid Cell UI System** in [`crates/chronophoto-ui`](../crates/chronophoto-ui/DESIGN.md) (`chronophoto_ui::components`) so every surface—the `Downloads` table, integrated toolbar, left filter rail, bottom byte-map inspector, titlebar, and modals—shares the same `1px` construction-grid rules, tabular monospace typography, `0px` hardware-plate corners, and `accent` hover fills.

---

## 1. Shared `RenderOnce` Primitives (`chronophoto_ui::components`) & Tokens (`chronophoto_ui::tokens`)

All primitives read the active `ChronoTheme` global (`cx.chrono()`) automatically inside `render(window, cx)`:

| Primitive / Token | Purpose |
| :--- | :--- |
| `GridTone` | Surface polarity & emphasis: `Surface`, `SurfaceSubtle`, `Chrome`, `Primary`, `Danger`. |
| `CellBorder` | `1px` vertical divider placement (`None`, `Left`, `Right`, `Both`). |
| `GridCellButton` | Full-height (`h_full()`) flush cell button with optional icon (`impl IconNamed`), uppercase label, tooltip, and accent hover fill. |
| `GridBoxedButton` | Standalone `1px`-bordered cell button with explicit height (for empty states and inline CTAs). |
| `GridSectionHeader` | `28px` (`SECTION_HEADER_HEIGHT`) modular header bar with `1px` bottom rule and optional trailing element. |
| `GridKvRow` | Two-column `LABEL \| VALUE` row with `1px` vertical and horizontal rules. |
| `GridModalFooter` | Flush `34px` (`MODAL_FOOTER_HEIGHT`) modal footer strip with `1px` top border and right-aligned action cells. |
| `GridBadge` / `BadgeTone` | Stamped `3px`-radius (`RADIUS_BADGE`) monospace badge for validators, FIFO ranks, protocol tags, role tags, and checksum verdicts (`Neutral`, `Muted`, `Accent`, `Success`, `Danger`). |
| `GridSelectionBar` | Full-height `3px` (`INDICATOR_BAR_WIDTH`) vertical `accent` active/selection accent strip. |
| `GridNavRow` / `NavRowTone` | `34px` vertical navigation row with `GridSelectionBar`, `14px` icon, monospace label, and optional right-bordered count cell (`Chrome`, `SurfaceSelection`). |
| `BrandPlate` | Canonical `TitleBar` brand plate accepting caller-supplied wordmark and optional SVG mark (`BRAND_LOGO_SVG`). |
| `GridInputBox` / `GridCompactInputRow` | Monospace borderless `Input` container and `30px` `LABEL \| INPUT \| TRAILING` recessed form row. |
| `GridDropdownSurface` / `GridCheckOptionRow` | Floating popover surface (`popover_bg`, `RADIUS_CONTROL`, `shadow_md()`) and checkable dropdown row with `12px` checkmark slot. |
| `GridProgressTrack` / `GridStatusDot` | `0px`-radius recessed `track_bg` progress bar track and square/round status dot indicator. |
| `ModalBackdrop` / `ModalPlate` | Full-window modal scrim (`modal_scrim`) and `RADIUS_MODAL` plate card with flush `chrome_bg` footer. |
| `SplitSash` / `SashAxis` | Unified vertical/horizontal resize sash with magnetic detent flash (`PanelMotion`) and floating px readout pill. |
| `AlertBanner` / `AlertTone` | Full-width semantic diagnostic strip (`Danger`, `Success`, `Accent`) with tooltip and trailing action cells. |
| `RADIUS_BADGE` / `RADIUS_CONTROL` / `RADIUS_MODAL` | Shape radius constants (`3px`, `4px`, `6px`) in `chronophoto_ui::tokens`. |

See [DESIGN.md](DESIGN.md) and [`crates/chronophoto-ui/DESIGN.md`](../crates/chronophoto-ui/DESIGN.md) for color and typography token definitions.

---

## 2. Virtualized Table, Scrubber & Multi-Select (`table.rs`, `main.rs`, `view_model.rs`)

- **Integrated Dual-Row Toolbar:**
  - **Row 1 (`40px`, dynamic):** With nothing selected: full-width URL input, clickable `ADD URL` cell (`submit_url_from_input` when non-empty, `paste_and_probe_from_clipboard` when empty), and `RESUME ALL` / `PAUSE ALL` / `CLEAR COMPLETED` cells. With one or more rows selected, the URL input is replaced by an `N SELECTED` badge and the filename (or count), `ADD URL` always pastes from the clipboard, and the tool strip swaps to labelled selection actions computed by `QueueController::selection_toolbar`: `PAUSE (n)` and `RESUME (n)` (accent-styled, each acting only on matching selected rows), `RESTART`, `CANCEL`, and for a single selection `OPEN`/`FOLDER` (completed), `COPY URL`, plus `DELETE` and clear-selection. Labels collapse to icons in the compact profile.
  - **Row 2 (`34px` fixed height):** Always the live `FILTER` search input (case-insensitive substring match on filename and URL via zero-allocation `contains_ascii_case_insensitive`). Keeping Row 2 at a fixed `34px` height prevents vertical table shifts when clicking rows.
- **Calibrated Telemetry Scrubber:** `Scrollbar::vertical` overlaid on a `14px` right rail with a 128-bucket minimap painting `Active` (`selenium_amber()`), `Failed` (`errata_vermilion()`), and selected-row (`cyanotype_blue()`) markers.
- **Multi-Select & Smooth Keyboard Navigation:**
  - `Click` selects a single row (or unselects if it is the sole selected row, collapsing the bottom inspector; double-clicking keeps the row selected and opens `Completed` files).
  - `Ctrl+Click` / `Cmd+Click` toggles individual rows; `Shift+Click` and `Shift+Up`/`Down` select contiguous ranges; `Ctrl+A` selects all visible rows.
  - `Up` / `Down` arrows animate `table_scroll_handle` smoothly at 60fps via `DownloadMotion::animate_table_scroll` (`chronophoto_ui::motion::PanelMotion`) and yield immediately if the user drags the scrollbar or wheels.

---

## 3. Expanded Interface Customization (`0006_ui_customization.sql`)

Persisted in SQLite (`settings` table) and editable under **Settings > Interface** (placed directly below **General**) with live preview and Cancel-revert:

| Setting | Options / Default | Effect |
| :--- | :--- | :--- |
| **Appearance Theme** | 8 built-in presets: 5 opaque themes (`chronophotographic` default) + 3 Windows 11 Mica presets (`Mica (System)`, `Mica Dark`, `Mica Light`) | Switches dark/light `ThemePalette` and Windows 11 22H2+ DWM Mica backdrop live across all windows (with automatic opaque fallback on Windows 10, High Contrast, or Transparency Effects off). |
| **Accent Override** | `Theme Default` or `amber`, `blue`, `emerald`, `cyan`, `crimson` | Layers `ACCENT_PRESETS` over the active theme via `chronophoto_ui::theme::apply_theme_with_accent`. |
| **Table Row Density** | `Compact` (`30px`), `Standard` (`38px`, default), `Comfortable` (`46px`) | Adjusts `render_row` height and keyboard smooth-scroll step height. |
| **Visible Columns** | `SPEED`, `ETA`, `SEGS` (all `true` by default) | Toggles optional columns alongside responsive width breakpoints (`LayoutProfile::with_settings`). |
| **Interface Scale** | `90%`, `100%`, `110%`, `125%` | Scales UI controls and typography. |
| **Workspace & Chrome** | `ui_click_row_opens_inspector`, `ui_show_add_paused`, `ui_start_sidebar_collapsed`, `ui_show_titlebar_telemetry`, `ui_show_toolbar_labels`, `ui_show_scrollbar_minimap` | Controls inspector auto-expand, Pre-Flight `Add Paused`, startup rail collapse, titlebar telemetry cell, toolbar button labels, and scrollbar minimap ticks. |

Under **Settings > Reset**, **Copy Config JSON** exports the active `Settings` snapshot to the clipboard as formatted JSON.
