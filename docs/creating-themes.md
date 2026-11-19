# Creating Themes for Posthaze

Posthaze's desktop UI (`posthaze-ui`) uses a semantic palette system defined in [`crates/posthaze-ui/src/theme.rs`](../crates/posthaze-ui/src/theme.rs), paired with SQLite persistence (`Settings.ui_theme` in `posthaze-engine`).

## Built-In Themes

| ID | Name | Tone | Accent |
| --- | --- | --- | --- |
| `chronophotographic` | Chronophotographic (default) | Dark | `#E58A35` Selenium Amber |
| `cyanotype` | Blueprint Cyanotype | Dark | `#38BDF8` Electric Sky |
| `phosphor` | Phosphor Amber | Dark | `#F59E0B` Phosphor Amber |
| `nordic` | Nordic Frost | Dark | `#2DD4BF` Glacial Teal |
| `archival` | Archival Paper | Light | `#C2590A` Burnt Sienna |

---

## Architecture Overview

1. **Palette Definition (`ThemePalette`)**: Every built-in theme is a `const ThemePalette` in [`crates/posthaze-ui/src/theme.rs`](../crates/posthaze-ui/src/theme.rs) registered in `BUILTIN_THEMES`.
2. **Runtime Lookup**: All UI modules call semantic helper functions (`graphite_bg()`, `paper_bg()`, `selenium_amber()`, etc.) which read from `ACTIVE_THEME` (`RwLock<ThemePalette>`).
3. **GPUI Kit Synchronization**: `theme::apply_theme(id, window, cx)` updates `ACTIVE_THEME`, switches `gpui_kit::component::ThemeMode` (`Dark` or `Light`), and writes matching tokens into `Theme::global_mut(cx)` so stock `gpui-kit` controls (`Button`, `Input`, `Tooltip`, `TitleBar`) stay consistent with custom surfaces.
4. **Persistence**: `Settings.ui_theme` (`String`) is stored in SQLite (`0005_ui_theme.sql`). Selecting a theme in **Settings > Interface** previews immediately via `apply_theme`, commits on **Save Settings**, and reverts on **Cancel** or `Escape`.

---

## `ThemePalette` Token Reference

### Rail, Titlebar, Inspector & Modal Surfaces
- `graphite_bg`: Left navigation rail, custom titlebar, and bottom byte-map inspector background.
- `graphite_elevated`: Raised cards, badges, segmented-control tracks, and secondary buttons.
- `graphite_border`: `1px` structural dividers inside rail, inspector, and modal surfaces.
- `silver_text`: Primary text on rail, titlebar, inspector, and modals.
- `silver_muted`: Secondary labels, descriptions, and inactive metadata.

### Download Table & Toolbar Work Surface
- `paper_bg`: Main download table and top toolbar background.
- `table_header_bg`: Recessed column header strip.
- `paper_selected`: Active/selected download row tint.
- `paper_hover`: Hovered download row and secondary hover tint.
- `paper_border`: Construction-grid row/column borders.
- `carbon_text`: Primary filename and speed text in table rows.
- `carbon_muted`: Secondary table metadata (byte counts, ETA, segment count).
- `byte_track_bg`: Recessed background track for progress bars and the byte-map visualizer.

### Accents & Telemetry States
- `accent` / `accent_hover` / `accent_active`: Primary CTA buttons, active sidebar tab highlight, selection bar, and in-flight segment progress.
- `accent_foreground`: Text/icon color rendered on top of `accent` (use dark ink `#141311` on bright accents, `#FFFFFF` on darker accents).
- `checkpoint_blue`: Writer-acknowledged disk `Checkpoint` progress bar color.
- `completed_emerald`: `Completed` state icon, label, and progress fill.
- `failed_vermilion`: `Failed` / `ResourceChanged` error indicator and danger actions.
- `focus_ring`: Keyboard focus ring color for inputs and buttons.

---

## Step-by-Step: Adding a New Built-In Theme

1. **Define a `ThemePalette` constant** in [`crates/posthaze-ui/src/theme.rs`](../crates/posthaze-ui/src/theme.rs):

   ```rust
   pub const MY_CUSTOM_THEME: ThemePalette = ThemePalette {
       id: "my-theme",
       name: "My Custom Theme",
       description: "Short 4-6 word description",
       tone: PaletteTone::Dark, // or PaletteTone::Light
       graphite_bg: 0x121417,
       graphite_elevated: 0x1A1D22,
       graphite_border: 0x2A2F37,
       silver_text: 0xEAECEF,
       silver_muted: 0x9098A4,
       paper_bg: 0x171A1F,
       table_header_bg: 0x13161A,
       paper_selected: 0x242A33,
       paper_hover: 0x1E232B,
       paper_border: 0x282D36,
       carbon_text: 0xEAECEF,
       carbon_muted: 0x9098A4,
       byte_track_bg: 0x0B0D10,
       accent: 0xF97316,
       accent_hover: 0xEA580C,
       accent_active: 0xC2410C,
       accent_foreground: 0x121417,
       checkpoint_blue: 0x3B82F6,
       completed_emerald: 0x22C55E,
       failed_vermilion: 0xEF4444,
       focus_ring: 0x475569,
   };
   ```

2. **Register it in `BUILTIN_THEMES`** in [`crates/posthaze-ui/src/theme.rs`](../crates/posthaze-ui/src/theme.rs) (update the array length and add `MY_CUSTOM_THEME`). **Settings > Interface** iterates `BUILTIN_THEMES` automatically, so the new swatch card will appear without editing `settings_modal.rs`.
3. **Update the unit test** in `theme::tests::builtin_themes_have_unique_ids_and_fallback` to match the updated `BUILTIN_THEMES.len()`.
4. **Verify contrast and build checks**:
   - Ensure `silver_text` on `graphite_bg` and `carbon_text` on `paper_bg` maintain strong contrast ($\ge 4.5:1$ for body text).
   - Ensure `accent_foreground` is legible on `accent`.
   - Run:
     ```powershell
     cargo test --workspace
     cargo clippy --workspace --all-targets -- -D warnings
     $env:RUSTDOCFLAGS="-D warnings"; cargo doc --workspace --no-deps
     ```
