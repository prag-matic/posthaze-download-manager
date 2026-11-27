# Creating Themes for Posthaze

Posthaze's desktop UI (`posthaze-ui`) uses the domain- and polarity-neutral semantic palette system from [`chronophoto-ui`](../crates/chronophoto-ui/src/palette.rs) (`palette.rs` and `theme.rs`), paired with SQLite persistence (`Settings.ui_theme` in `posthaze-engine`).

## Built-In Themes

| ID | Name | Polarity / Badge | Backdrop | Accent |
| --- | --- | --- | --- | --- |
| `chronophotographic` | Chronophotographic (default) | Dark (`DARK`) | Opaque | `#E58A35` Selenium Amber |
| `cyanotype` | Blueprint Cyanotype | Dark (`DARK`) | Opaque | `#38BDF8` Electric Sky |
| `phosphor` | Phosphor Amber | Dark (`DARK`) | Opaque | `#F59E0B` Phosphor Amber |
| `nordic` | Nordic Frost | Dark (`DARK`) | Opaque | `#2DD4BF` Glacial Teal |
| `archival` | Archival Paper | Light (`LIGHT`) | Opaque | `#C2590A` Burnt Sienna |
| `mica-system` | Mica (System) | Follows OS (`AUTO`) | Mica / Mica Alt* | `#E58A35` / `#C2590A` |
| `mica-dark` | Mica Dark | Dark (`DARK`) | Mica (`DWMSBT_MAINWINDOW`)* | `#E58A35` Selenium Amber |
| `mica-light` | Mica Light | Light (`LIGHT`) | Mica Alt (`DWMSBT_TABBEDWINDOW`)* | `#C2590A` Burnt Sienna |

\* Native Mica translucency requires **Windows 11 22H2 (build 22621) or newer** with DirectComposition and Windows Transparency Effects enabled and High Contrast mode off. On Windows 10, older builds, `GPUI_DISABLE_DIRECT_COMPOSITION=1`, High Contrast mode, or when Windows Transparency Effects are turned off, all three Mica presets automatically degrade to calibrated neutral opaque fallback palettes (`#202020` dark / `#F3F3F3` light) and display a status note in **Settings > Interface**.

---

## Architecture Overview

1. **Palette & Preset Definitions (`ThemePalette`, `ThemePreset`, `RgbaHex`)**: Every opaque built-in theme and Mica/fallback palette is a `const ThemePalette` in [`crates/chronophoto-ui/src/palette.rs`](../crates/chronophoto-ui/src/palette.rs) using 32-bit `RgbaHex` (`0xRRGGBBAA`) color tokens and `ThemeBackdrop` (`Opaque`, `Mica`, `MicaAlt`). `BUILTIN_THEME_PRESETS` (8 entries) drives the **Settings > Interface** wrap-grid.
2. **Pure Palette Resolution (`ThemeCatalog`)**: `ThemeCatalog::resolve_palette(id, accent_override, os_appearance, mica_available)` resolves the active `ThemePalette` headlessly—mapping `mica-system` to dark or light from `WindowAppearance`, selecting opaque fallbacks when `mica_available` is `false`, and applying any `Settings.ui_accent_override` preset. Host apps can also register custom `ThemePalette` / `AccentPreset` slices on `ThemeCatalog`.
3. **Runtime Lookup (`ChronoTheme` Global)**: All UI modules and `chronophoto-ui` components read the active palette and typography from the GPUI `ChronoTheme` global via `cx.chrono()` (`ChronoAppExt`).
4. **GPUI Kit & Win32 DWM Synchronization**: `chronophoto_ui::theme::apply_theme_with_accent(...)` updates `ChronoTheme`, calls `Theme::change(mode, None, cx)`, and mutates `gpui_kit::component::Theme` inside `Theme::update(cx, |theme| ...)` (setting both `theme.background` and `theme.tokens.background` to `Hsla::transparent_black()` whenever `palette.backdrop != ThemeBackdrop::Opaque`). The multi-window coordinator (`chronophoto_ui::platform::sync_theme_and_backdrops` + `apply_window_backdrop`) then applies Win32 DWM attributes live across `PosthazeApp`, `SettingsWindow`, and all open `PreflightWindow` instances.
5. **Persistence**: `Settings.ui_theme` (`String`) is stored in SQLite (`0005_ui_theme.sql`; no additional migration was required for Mica presets). Selecting a theme in **Settings > Interface** previews both palette and DWM backdrop immediately, commits on **Save Settings**, and reverts cleanly on **Cancel** or `Escape`.

---

## `ThemePalette` Token Reference

### Chrome, Rail & Structural Surfaces
- `backdrop`: Requested window material (`ThemeBackdrop::Opaque`, `ThemeBackdrop::Mica`, or `ThemeBackdrop::MicaAlt`).
- `titlebar_bg`: Custom `TitleBar` fill (`#00000000` pure DWM Mica in Mica mode; `chrome_bg` in Opaque mode).
- `chrome_bg`: Left navigation rail, bottom byte-map inspector, and modal footer background.
- `chrome_elevated`: Raised cards, badges, segmented-control tracks, and secondary buttons.
- `chrome_border`: `1px` structural dividers inside rail, inspector, modal, and popover surfaces.
- `chrome_text`: Primary text on rail, titlebar, inspector, and modals.
- `chrome_muted`: Secondary labels, descriptions, and inactive metadata.

### Work Surface, Inputs & Overlay Surfaces
- `surface_bg`: Main download table and top toolbar background.
- `surface_header_bg`: Recessed column header strip.
- `surface_selected`: Active/selected download row tint.
- `surface_hover`: Hovered download row, secondary hover tint, and non-danger context-menu item hover.
- `surface_border`: Construction-grid row/column borders.
- `surface_text`: Primary filename and speed text in table rows.
- `surface_muted`: Secondary table metadata (byte counts, ETA, segment count).
- `input_bg`: Recessed URL bar and form input surface.
- `track_bg`: Recessed background track for progress bars and the byte-map visualizer.
- `modal_scrim`: Translucent overlay scrim behind in-window modals.
- `modal_bg`: Card surface for `Settings`, `Confirm`, and `File-Op` modals.
- `popover_bg`: Floating download-row context menu and dropdown surface.

### Accents & Telemetry States
- `accent` / `accent_hover` / `accent_active`: Primary CTA buttons, active sidebar tab highlight, selection bar, and in-flight segment progress.
- `accent_foreground`: Text/icon color rendered on top of `accent` (use dark ink `#141311` on bright accents, `#FFFFFF` on darker accents).
- `info`: Writer-acknowledged disk `Checkpoint` progress bar color and probing/queued marks.
- `success`: `Completed` state icon, label, and progress fill.
- `danger` / `danger_foreground`: `Failed` / `ResourceChanged` error indicator, danger actions, and high-contrast ink on filled danger controls (`danger_surface()` provides the translucent banner tint).
- `focus_ring`: Keyboard focus ring color for inputs and buttons.

---

## Step-by-Step: Adding a New Built-In Theme

1. **Define a `ThemePalette` constant** in [`crates/chronophoto-ui/src/palette.rs`](../crates/chronophoto-ui/src/palette.rs) using `RgbaHex::rgb(...)` (for opaque colors) or `RgbaHex::rgba(...)` (for translucent surfaces):

   ```rust
   pub const MY_CUSTOM_THEME: ThemePalette = ThemePalette {
       id: "my-theme",
       name: "My Custom Theme",
       description: "Short 4-6 word description",
       backdrop: ThemeBackdrop::Opaque,
       tone: PaletteTone::Dark, // or PaletteTone::Light
       titlebar_bg: RgbaHex::rgb(0x121417),
       chrome_bg: RgbaHex::rgb(0x121417),
       chrome_elevated: RgbaHex::rgb(0x1A1D22),
       chrome_border: RgbaHex::rgb(0x2A2F37),
       chrome_text: RgbaHex::rgb(0xEAECEF),
       chrome_muted: RgbaHex::rgb(0x9098A4),
       surface_bg: RgbaHex::rgb(0x171A1F),
       surface_header_bg: RgbaHex::rgb(0x13161A),
       surface_selected: RgbaHex::rgb(0x242A33),
       surface_hover: RgbaHex::rgb(0x1E232B),
       surface_border: RgbaHex::rgb(0x282D36),
       surface_text: RgbaHex::rgb(0xEAECEF),
       surface_muted: RgbaHex::rgb(0x9098A4),
       input_bg: RgbaHex::rgb(0x121417),
       track_bg: RgbaHex::rgb(0x0B0D10),
       modal_scrim: RgbaHex::rgba(0x141413AA),
       modal_bg: RgbaHex::rgb(0x121417),
       popover_bg: RgbaHex::rgb(0x121417),
       accent: RgbaHex::rgb(0xF97316),
       accent_hover: RgbaHex::rgb(0xEA580C),
       accent_active: RgbaHex::rgb(0xC2410C),
       accent_foreground: RgbaHex::rgb(0x121417),
       info: RgbaHex::rgb(0x3B82F6),
       success: RgbaHex::rgb(0x22C55E),
       danger: RgbaHex::rgb(0xEF4444),
       danger_foreground: RgbaHex::rgb(0xFFFFFF),
       focus_ring: RgbaHex::rgb(0x475569),
   };
   ```

2. **Register it in `BUILTIN_OPAQUE_THEMES` and `BUILTIN_THEME_PRESETS`** in [`crates/chronophoto-ui/src/palette.rs`](../crates/chronophoto-ui/src/palette.rs) (update the array lengths and add a matching `ThemePreset` entry). **Settings > Interface** iterates `BUILTIN_THEME_PRESETS` automatically, so the new swatch card will appear without editing `settings_modal.rs`.
3. **Run the automated WCAG 2.1, alpha-calibration, and catalog tests** in `chronophoto-ui`:
   - `chrome_text` on `chrome_bg`, `surface_text` on `surface_bg`, `accent_foreground` on `accent`, and `danger_foreground` on `danger` must each achieve $\ge 4.5:1$ contrast.
   - `chrome_muted` on `chrome_bg` and `surface_muted` on `surface_bg` must achieve $\ge 3.0:1$ contrast.
   - Run:
     ```powershell
     cargo test --workspace
     cargo clippy --workspace --all-targets -- -D warnings
     $env:RUSTDOCFLAGS="-D warnings"; cargo doc --workspace --no-deps
     ```
