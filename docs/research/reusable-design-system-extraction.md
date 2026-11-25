# Research: Extracting Posthaze's Design System for Multi-Project Reuse & Continuous Evolution

> **Scope:** Primary-source audit of `crates/posthaze-ui`, `docs/DESIGN.md`, `extensions/chrome/popup.html`, and Cargo-cached `gpui-kit` `0.10.0` (`gpui-component` `0.10.0`, `gpui-base` `0.10.0`, `gpui-kit-assets` `0.10.0`), evaluated against Cargo Reference docs, W3C Design Tokens Format Module, Style Dictionary v4 docs, Microsoft Learn Win32 DWM docs, and MDN CSS specs.
> **Non-goal:** No application Rust code is modified by this report.

---

## 1. Executive Summary

Posthaze's **"Chronophotographic Interval Plate"** visual language—comprising its **Table-Grid Cell UI System** (`grid_cells.rs`), **8 calibrated opaque + Windows 11 Variant C Mica palettes** (`theme.rs`), **exponential smoothing / cubic-decay motion curves** (`motion.rs`), and **Win32 DWM Mica probe & compositor bridge** (`platform_hooks.rs`)—is already **~85% application-agnostic**. Only three narrow seams tie it to Posthaze's download-manager domain:

1. **Brand identity literals:** `grid_brand_plate` (`grid_cells.rs:433-456`) and `table.rs:592` hardcode `BRAND_LOGO_SVG` (`assets.rs:21`) and `"POSTHAZE"`, and `PosthazeAssets` (`assets.rs:34-63`) embeds Posthaze-specific files alongside `gpui_kit::assets::AllAssets`.
2. **Engine telemetry types in `motion.rs`:** `MotionState` mixes generic panel/scroll/detent interpolation (`motion.rs:64-152, 361-401`) with `HashMap<DownloadId, AnimatedTelemetry>` importing `posthaze_engine::{DownloadId, DownloadState, ProgressSnapshot}` (`motion.rs:6`).
3. **App lifecycle & shell helpers in `platform_hooks.rs`:** Reusable DWM Mica probes and backdrop hooks (`platform_hooks.rs:14-590`) live in the same file as Posthaze's startup registry (`Run\Posthaze`), power request (`"Posthaze is downloading"`), completion hooks, and `ITaskbarList3` taskbar progress (`win32_taskbar`, the sole direct consumer of the `windows` `0.62` crate in `platform_hooks.rs`).

Additionally, auditing `theme.rs`, `docs/DESIGN.md`, `extensions/chrome/popup.html`, and `gpui-kit` `0.10.0` reveals **three concrete gaps** that a reusable extraction solves directly:
- **Upstream `gpui_base::Theme` desync:** `theme.rs:733` mutates `Theme::global_mut(cx)` directly without calling `Theme::update(cx, ...)` or `Theme::sync_base(cx)`, leaving `gpui_base::Theme` (which paints scrollbars and resizable handles) out of sync with `ThemePalette` (`gpui-component-0.10.0/src/theme/mod.rs:263-297, 477-496`).
- **Cross-stack token drift:** `extensions/chrome/popup.html:7-24` and `docs/DESIGN.md:5-20, 75-76, 152, 194` already diverge from `crates/posthaze-ui/src/theme.rs:177-209` on hex values, border opacity, and token names.
- **Heavy compile coupling:** Iterating on UI tokens or grid cells inside `posthaze-ui` currently pulls in `posthaze-engine` (`reqwest`, `sqlx`, `tokio`), `ed25519-dalek`, `tray-icon`, and `windows` `0.62` (`crates/posthaze-ui/Cargo.toml:7-53`).

**Recommended Architecture:** Extract a pure-Rust GPUI design crate (e.g., `chronophoto-ui`) with an optional `mica` Windows feature and an isolated `examples/gallery.rs` sandbox binary (**Track A**), backed by a W3C DTCG `.tokens.json` source of truth + lightweight Rust `xtask` codegen (**Track B**) that emits Rust `ThemePalette` constants, CSS Custom Properties (`:root` / `[data-theme]` + `color-scheme` / `light-dark()`), and `DESIGN.md` YAML frontmatter.

---

## 2. Primary Audit: Posthaze Design System Anatomy

### 2.1 Repository Modules & Specs

| File | Current Role | Key Symbols & Line Citations |
|---|---|---|
| `crates/posthaze-ui/src/theme.rs` | Font/scale/radius/chrome constants, `RgbaHex`, `ThemeBackdrop`, `ThemePalette` (26 color + 5 metadata fields), 5 opaque + 2 Mica + 2 Mica fallback `const` palettes, `BUILTIN_THEME_PRESETS` (8), `ACCENT_PRESETS` (5), `resolve_palette`, `ACTIVE_THEME: RwLock<ThemePalette>`, `apply_theme_with_accent`, helper getters | `MONO_FONT`, `TYPE_*`, `RADIUS_*`, `SECTION_HEADER_HEIGHT`, `MODAL_FOOTER_HEIGHT`, `INDICATOR_BAR_WIDTH` (`1-28`); `RgbaHex` (`30-48`); `ThemeBackdrop` (`60-79`); `ThemePalette` (`110-174`); palettes (`177-489`); `resolve_palette` (`646-693`); `ACTIVE_THEME` (`695-716`); `apply_theme_with_accent` (`719-787`); getters (`800-923`) |
| `crates/posthaze-ui/src/grid_cells.rs` | Table-Grid Cell UI System (`1px` structural rules, `0px` cell corners, `MONO_FONT`, accent hover fills) | `GridTone` (`19-84`); `CellBorder` (`34-39`); `grid_cell_button` / `_dyn` (`88-151`); `grid_boxed_button` (`155-180`); `grid_modal_footer` (`183-191`); `grid_section_header` (`194-219`); `grid_kv_row` / `_with_bg` (`222-288`); `BadgeTone` / `grid_badge` (`292-328`); `grid_selection_bar` (`331-341`); `NavRowTone` / `grid_nav_row` (`345-429`); `grid_brand_plate` (`433-456`); `grid_input_box` / `grid_compact_input_row` (`459-501`); `grid_status_dot` (`504-511`); `grid_dropdown_surface` / `grid_check_option_row` (`515-590`); `grid_progress_track` (`593-601`) |
| `crates/posthaze-ui/src/assets.rs` | Composite `AssetSource` serving 5 embedded brand assets (`include_bytes!`) and delegating icon lookups to `gpui_kit::assets::AllAssets` | `BRAND_APP_ICON`, `BRAND_APP_ICON_SVG`, `BRAND_LOGO_MARK`, `BRAND_LOGO_MARK_DARK`, `BRAND_LOGO_SVG` (`9-21`); `PosthazeAssets` (`34-63`) |
| `crates/posthaze-ui/src/platform_hooks.rs` | Win32 DWM Mica availability & backdrop application + app-specific shell/tray/power/taskbar hooks | Mica constants & availability state machine (`14-109`); `windows_build_number`, `is_high_contrast_active`, `read_personalize_dword`, `is_transparency_enabled`, `initial_os_appearance`, `mica_availability` (`121-249`); `apply_window_backdrop`, `assert_window_dark_mode` (`252-395`); `spawn_mica_preference_listener` (`428-590`); tray/EcoQoS/startup/shell/power/taskbar (`592-1099`) |
| `crates/posthaze-ui/src/motion.rs` | Exponential smoothing (`1 - e^(-dt*k)`) for telemetry bars, panel collapse/expand, table keyboard scroll, and cubic ease-out flashes | `SPLIT_FLASH_MS`, `COMPLETION_FLASH_MS`, `DETENT_FLASH_MS` (`9-15`); `PanelAnim` (`19-22`); `AnimatedTelemetry` (`26-35`); `RowMotionSample` (`39-48`); `MotionState` (`52-454`); `decay_ease_out` (`457-466`) |
| `crates/posthaze-ui/Cargo.toml` | UI crate dependencies showing heavy engine/network/signing/Windows coupling | `gpui-kit = "0.10.0"` (`12`); `posthaze-engine`, `reqwest`, `ed25519-dalek`, `sha2`, `tokio` (`8-20`); `tray-icon`, `windows = "0.62"`, `windows-core = "0.62"`, `windows-sys = "0.60"` (`22-53`) |
| `docs/DESIGN.md` | Impeccable-compatible `DESIGN.md` with YAML frontmatter + 8 canonical sections + Variant C Mica token table | YAML frontmatter (`1-89`); Creative North Star & rules (`91-173`); Table-Grid Cell summary (`196-203`); Variant C Mica spec & token table (`216-260`) |
| `docs/creating-themes.md` | Authoring guide for adding built-in opaque & Mica themes | Preset catalog (`5-19`); resolution/persistence architecture (`22-28`); `ThemePalette` reference (`32-64`) |
| `docs/table-grid-ui-and-customization.md` | Primitive catalog, dual-row toolbar, minimap scrollbar, and SQLite interface settings | Primitives table (`11-29`); customization matrix (`47-59`) |
| `docs/adr/0008-gpui-and-gpui-kit-desktop-ui.md` | Architectural decision adopting pure-Rust GPUI + `gpui-kit` over Slint | Decision & consequences (`1-16`) |
| `extensions/chrome/popup.html` | Chrome extension popup styling using CSS custom properties | `:root` CSS variables (`7-24`); `.row`, `.actions`, `.btn`, `.field`, `.status` (`86-257`) |
| `docs/ui-concept/posthaze_desktop_ui_concept.html` | Early HTML/Tailwind prototype (`v0.3.0-m3`) establishing visual rules | `:root` palette vars (`9-27`); `.snap-90` (`52-54`); `.transfer-row` & engraved ticks (`104-140`) |

### 2.2 Upstream `gpui-kit` `0.10.0` Inspection (Local Cargo Registry Cache)

Inspecting the actual source of `gpui-kit` `0.10.0` (`C:\Users\prage\.cargo\registry\src\index.crates.io-1949cf8c6b5b557f\gpui-kit-0.10.0`) and its re-exported crates (`gpui-component-0.10.0`, `gpui-base-0.10.0`, `gpui-kit-assets-0.10.0`) clarifies four critical behaviors:

1. **Umbrella Facade & Feature Flags (`gpui-kit-0.10.0/src/lib.rs`, `Cargo.toml`):**
   - `gpui-kit` re-exports `gpui` (`pub use ::gpui::*`, `lib.rs:101`), `gpui_base` as `base` (`lib.rs:130`), `gpui_component` as `component` behind default feature `component` (`Cargo.toml:41, 47`, `lib.rs:170`), and `gpui_kit_assets` as `assets` behind default feature `assets` (`Cargo.toml:40, 48`, `lib.rs:173`).
   - It defines `#[doc(hidden)] pub use crate as gpui;` (`lib.rs:117`) and `actions!` (`lib.rs:53-81`) so downstream crates do not need a direct `gpui` dependency.
   - It also provides `test-support` (`Cargo.toml:76-83`, `lib.rs:127-128`) enabling headless window tests (`#[gpui_kit::test]`, `VisualTestContext`).

2. **`Theme`, `ThemeMode`, `ThemeTokens`, and `gpui_base::Theme` Synchronization (`gpui-component-0.10.0/src/theme/mod.rs`, `theme_color.rs`, `gpui-base-0.10.0/src/theme.rs`):**
   - `Theme` implements `gpui::Global` (`mod.rs:254`) and holds both `colors: ThemeColor` (flat `Hsla` fields, `mod.rs:163`, `theme_color.rs:61-347`) and `tokens: ThemeTokens` (`ThemeToken { color: Hsla, background: Background }`, `mod.rs:169`, `theme_color.rs:12-15, 360-362`), plus `mode: ThemeMode` (`Light` | `Dark`, `mod.rs:835-839`).
   - **Important upstream finding:** `Theme::global_mut` (`mod.rs:263-273`) explicitly warns:
     > *"An edit made through this reference reaches nothing but the field it touches: `Theme::tokens` keeps the colors it had, and so does the Base projection until `Theme::sync_base` rebuilds it. Prefer `Theme::update`, which does both after the edit and refreshes every window."*
   - `Theme::update(cx, |theme| ...)` (`mod.rs:308-356`) automatically calls `theme.tokens.reconcile(...)`, calls `Theme::sync_base(cx)` (`mod.rs:491-496`) to project `SemanticThemeTokens` (`gpui-base-0.10.0/src/theme_tokens.rs:12-18`), `ScrollbarTheme`, and `ResizableTheme` onto `gpui_base::Theme`, and calls `cx.refresh_windows()`.
   - **Gap in Posthaze today:** `apply_theme_with_accent` (`crates/posthaze-ui/src/theme.rs:731-785`) calls `Theme::change(mode, None, cx)` and then edits `Theme::global_mut(cx)` directly without calling `Theme::sync_base(cx)` or `Theme::update(cx, ...)`. Consequently, `gpui_base::Theme` retains the default `Theme::change` `radius`, `border`, and `colors` rather than Posthaze's `RADIUS_CONTROL` (`4px`) and `ThemePalette` colors!

3. **`Root` and `RootPlugin` (`gpui-base-0.10.0/src/root.rs`, `gpui-component-0.10.0/src/root.rs`):**
   - `gpui_kit::open_window` (`gpui-kit-0.10.0/src/lib.rs:181-196`) wraps the caller's view inside `gpui_base::Root::new(view, window, cx)`.
   - `gpui_component::init(cx)` (`gpui-component-0.10.0/src/lib.rs:131-136`) registers `WindowState` via `Root::register_plugin::<WindowState>` (`root.rs:21-23`).
   - `WindowState::style` (`gpui-component-0.10.0/src/root.rs:729-748`) refines `.bg(cx.theme().tokens.background)`, `.font_family(cx.theme().font_family)`, `.text_color(cx.theme().foreground)`, and rounded window corners onto the root surface, while `WindowState::decorate` (`root.rs:750-758`) wraps it in `window_border().child(surface)`. That is why `theme.tokens.background` *must* be `Hsla::transparent_black().into()` for DWM Mica to show through.

4. **`TitleBar` Compositing (`gpui-component-0.10.0/src/title_bar.rs`):**
   - `TitleBar::window_options()` (`title_bar.rs:84-94`) configures `TitlebarOptions { title: None, appears_transparent: true, traffic_light_position: Some(point(px(9.0), px(9.0))) }` and `app_owns_titlebar_drag: true`.
   - When rendering, `TitleBar` (`title_bar.rs:356-360`) calls `default_title_bar_background(cx.theme().title_bar, cx.theme().background)` (`title_bar.rs:24-39`), which blends `55%` `title_bar` with `45%` `background` into a vertical linear gradient before applying `.refine_style(&self.style)`. Thus **both** `theme.background` and `theme.tokens.background` must be `Hsla::transparent_black()` when `backdrop != Opaque` (`crates/posthaze-ui/src/theme.rs:736-738`), verifying `docs/DESIGN.md:250`.

5. **`AssetSource` and `IconName` (`gpui-kit-assets-0.10.0/src/icon.rs`, `native_assets.rs`, `lib.rs`):**
   - `IconName` (`icon.rs:11-15`) implements `IconNamed { fn path(self) -> SharedString; }` and `RenderOnce` (`icon.rs:19-28`), so custom apps can define their own `IconNamed` enums or SVG paths.
   - `Assets` and `AllAssets` embed Lucide SVGs via `rust_embed` (`native_assets.rs:7-42`), and `gpui_kit::assets::icon_assets!` (`lib.rs:70-91`) generates minimal zero-allocation `AssetSource`s for selected icons. Note that `AllAssets::load(unknown)` returns `Err(...)` (`native_assets.rs:29`), so composite `AssetSource` implementations (like `PosthazeAssets` in `assets.rs:37-46`) must match custom paths *before* delegating to `AllAssets`.

### 2.3 Inventory: App-Agnostic vs. Posthaze-Coupled

| Category | App-Agnostic (Extractable) | Posthaze-Coupled (Keep in `posthaze-ui` or Parameterize) |
|---|---|---|
| **Tokens & Palettes (`theme.rs`)** | `MONO_FONT`, `TYPE_*`, `RADIUS_*`, `SECTION_HEADER_HEIGHT`, `MODAL_FOOTER_HEIGHT`, `INDICATOR_BAR_WIDTH`, `RgbaHex`, `PaletteTone`, `ThemeBackdrop`, `ThemeBadge`, `ThemePreset`, `ThemePalette`, all 8 presets (`CHRONOPHOTOGRAPHIC`..`MICA_LIGHT` + fallbacks), `ACCENT_PRESETS`, `resolve_palette`, `apply_theme_with_accent`, semantic color getters | Doc references to `Settings.ui_theme` / `Settings.ui_accent_override` (`theme.rs:95, 571`); download-domain semantic names (`checkpoint_blue`, `selenium_amber`, `byte_track_bg`—should either be retained as design-system signature names or aliased to generic semantic names like `info`/`accent`/`track_bg`) |
| **Primitives (`grid_cells.rs`)** | `GridTone`, `CellBorder`, `BadgeTone`, `NavRowTone`, `grid_cell_button`, `grid_cell_button_dyn`, `grid_boxed_button`, `grid_modal_footer`, `grid_section_header`, `grid_kv_row`, `grid_kv_row_with_bg`, `grid_badge`, `grid_selection_bar`, `grid_nav_row`, `grid_input_box`, `grid_compact_input_row`, `grid_status_dot`, `grid_dropdown_surface`, `grid_check_option_row`, `grid_progress_track` | `grid_brand_plate` (`grid_cells.rs:433-456`) hardcoding `crate::assets::BRAND_LOGO_SVG` and `"POSTHAZE"`; `grid_cells.rs:14` importing `crate::assets::BRAND_LOGO_SVG` |
| **Assets (`assets.rs`)** | Pattern for chaining custom app assets before `AllAssets` | `APP_ICON_BYTES`, `LOGO_SVG_BYTES`, `PosthazeAssets` (`assets.rs:23-63`) |
| **Motion (`motion.rs`)** | `PanelAnim`, `DETENT_FLASH_MS`, `decay_ease_out`, exponential smoothing (`1 - (-dt * k).exp()`), `animate_sidebar` / `animate_inspector` / `animate_table_scroll` / `sash_detent_flash` | `use posthaze_engine::{DownloadId, DownloadState, ProgressSnapshot}` (`motion.rs:6`), `AnimatedTelemetry`, `RowMotionSample`, `on_progress`, `snap_to_live`, `on_state_changed`, `sample` (`26-48, 156-360, 410-453`) |
| **Platform (`platform_hooks.rs`)** | `MIN_MICA_WINDOWS_BUILD`, `MicaFallbackReason`, `MicaProbeInputs`, `evaluate_mica_availability`, `windows_build_number`, `is_high_contrast_active`, `read_personalize_dword`, `is_transparency_enabled`, `initial_os_appearance`, `system_uses_light_theme`, `mica_availability`, `apply_window_backdrop`, `assert_window_dark_mode`, `spawn_mica_preference_listener` (after parameterizing thread/window-class names), `win32_hwnd`, `hide_window_to_tray`, `show_window_from_tray`, `is_window_visible`, `TaskbarProgressState` / `sync_taskbar_progress` (optional feature) | `format_startup_command` (`--minimized`), `sync_windows_startup_registry` (`Run\Posthaze`), `set_process_eco_qos`, `open_file_in_shell`, `open_url_in_browser`, `open_folder_in_shell`, `open_directory_in_shell`, `spawn_completion_hook`, `win32_power` (`"Posthaze is downloading\0"`) (`592-1032`) |

### 2.4 Evidence of Existing Drift Across Surfaces & Docs

Auditing the primary files exposes concrete drift that validates extracting a shared source of truth:
- `extensions/chrome/popup.html:7-24` defines `--ok: #4cc38a`, `--err: #e5484d`, `--border: rgba(234, 230, 223, 0.08)`, and a single `--radius: 4px`, whereas `CHRONOPHOTOGRAPHIC` in `theme.rs:186, 206-207` uses `graphite_border: #2A2825`, `completed_emerald: #22C55E`, `failed_vermilion: #EF4444`, and `RADIUS_BADGE` (`3px`) / `RADIUS_MODAL` (`6px`).
- `docs/DESIGN.md:75-76` references `{colors.paper-bg}` and `{colors.carbon-text}`, neither of which exists in `DESIGN.md:5-20` (`table-bg` / `silver-text`), and `DESIGN.md:152, 168, 182, 194` still mentions `#181715`, `#22201D`, `#2563EB`, and `#D97724` from the earlier HTML concept (`docs/ui-concept/posthaze_desktop_ui_concept.html`) rather than `theme.rs`'s `#141311`, `#1B1A17`, `#3B82F6`, and `#E58A35`.

---

## 3. Multi-Project Reuse & Evolution Architecture

### 3.1 Track A — Standalone Rust / GPUI Design Crate (`chronophoto-ui`)

A standalone crate (starting as a member of Posthaze's Cargo workspace, then optionally split into its own Git repository) lets any GPUI desktop app consume the Table-Grid Cell System, palettes, and Variant C Mica bridge without pulling `posthaze-engine`, `sqlx`, `reqwest`, `tokio`, or `ed25519-dalek`.

#### Cargo Dependency & Multi-Repo Workflow
Per [The Cargo Book — Specifying Dependencies](https://doc.rust-lang.org/cargo/reference/specifying-dependencies.html):
- **In-workspace phase:** Consumed via `chronophoto-ui = { path = "../chronophoto-ui" }` inside `posthaze-download-manager/Cargo.toml`.
- **Cross-repo phase:** Consumed via `chronophoto-ui = { git = "https://github.com/<owner>/chronophoto-ui.git", tag = "v0.1.0" }` (Cargo automatically discovers member crates inside a workspace Git repository without needing a sub-path). When iterating simultaneously on the design crate and an app, developers override the Git dependency locally with `[patch."https://github.com/<owner>/chronophoto-ui.git"]` pointing at a local `path` checkout without editing `Cargo.toml`.
- **Feature gating:**
  - `default = ["mica"]` (or `mica` enabled only on Windows targets via `[target.'cfg(target_os = "windows")'.dependencies]`).
  - `mica` pulls only `raw-window-handle = "0.6"` and `windows-sys = "0.60"` with the 8 minimal Win32 features needed for DWM/Registry/Accessibility/SystemInfo (`Wdk_System_SystemServices`, `Win32_Foundation`, `Win32_Graphics_Dwm`, `Win32_System_Registry`, `Win32_System_SystemInformation`, `Win32_System_Threading`, `Win32_UI_Accessibility`, `Win32_UI_WindowsAndMessaging`)—avoiding `windows` `0.62`, `windows-core`, `tray-icon`, and `Win32_System_Com` / `Win32_UI_Shell` (unless an opt-in `taskbar-progress` feature is requested).
  - Non-Windows targets compile cleanly with `#[cfg(not(target_os = "windows"))]` stubs returning `Err(MicaFallbackReason::UnsupportedOs)` (`platform_hooks.rs:239-248`).

#### Decoupling the Three Seams
1. **Brand Plate & Assets (`grid_brand_plate`):**
   - Replace `grid_brand_plate()` with `grid_brand_plate(spec: BrandPlate<'_>)` (or `BrandPlate { svg_path: Option<&'static str>, wordmark: SharedString, version: Option<SharedString> }`), eliminating the `crate::assets::BRAND_LOGO_SVG` and `"POSTHAZE"` literals (`grid_cells.rs:14, 433-456`).
   - Provide a generic `CompositeAssetSource<A: AssetSource>` helper (replacing `PosthazeAssets`) that checks an app-provided static slice `&[(&str, &[u8])]` (or delegate `AssetSource`) before falling back to `gpui_kit::assets::AllAssets` (`assets.rs:37-46`).
2. **Motion (`motion.rs`):**
   - Extract `DecayPulse` (`decay_ease_out`, start `Instant`, duration ms), `InterpolatedScalar` (`current`, `target`, `step(dt, rate)` using `1.0 - (-dt * rate).exp()`), and `PanelMotion` (managing sidebar/inspector/scroll `PanelAnim` + detent flash) into `chronophoto_ui::motion`.
   - Keep `DownloadMotion` (`HashMap<DownloadId, AnimatedTelemetry>` + `on_progress` / `snap_to_live`) inside `posthaze-ui`, composing `PanelMotion` and `DecayPulse` rather than duplicating math.
3. **Font & Scale Configuration:**
   - Replace hardcoded `MONO_FONT = "Consolas"` (`theme.rs:9`, `grid_cells.rs:140+`) with a configurable `ThemeTypography { mono_font: SharedString, sans_font: SharedString, scale: f32 }` stored with the active palette so non-Windows targets (e.g., macOS `Menlo` / Linux `DejaVu Sans Mono`, matching `gpui-component-0.10.0/src/theme/mod.rs:188-190`) and `Settings.ui_scale` (`table-grid-ui-and-customization.md:57`) work natively across projects.

#### State Architecture: `RwLock<ThemePalette>` vs. GPUI `Global` + `Theme::update`
| Dimension | `static RwLock<ThemePalette>` (Current `theme.rs:695`) | GPUI `Global` (`impl Global for ChronoTheme`) + `Theme::update` |
|---|---|---|
| **Call-site ergonomics** | Zero-arg helpers (`paper_bg()`, `selenium_amber()`) usable anywhere inside closures without passing `cx: &App` | Requires `cx.chronophoto()` / `ChronoTheme::global(cx)` or passing `&ThemePalette` |
| **Multi-window / test isolation** | Process-wide global mutable static; parallel `#[gpui_kit::test]` tests mutating `ACTIVE_THEME` can race unless serialized | Scoped to each GPUI `App` instance (`cx.set_global(...)`); parallel `TestAppContext` tests are 100% isolated (`gpui-component-0.10.0/src/theme/mod.rs:880-899`) |
| **Upstream `gpui_base::Theme` sync** | Currently bypassed (`Theme::global_mut` leaves `gpui_base::Theme` stale) | Calling `Theme::update(cx, |theme| { ... })` reconciles `theme.colors` ↔ `theme.tokens`, rebuilds `gpui_base::Theme` via `Theme::sync_base`, and refreshes all windows (`theme/mod.rs:308-356`) |
| **Recommendation** | Keep thin `active_theme()` / `paper_bg()` shims inside `posthaze-ui` only as a temporary migration bridge | **Adopt `Global` + `Theme::update(cx, ...)`** in the extracted crate (with an extension trait `ChronoActiveTheme for App`), and pass `&ThemePalette` or `cx` to grid-cell builders |

#### Win32 DWM Primary-Source Alignment
Per [Microsoft Learn — `DwmSetWindowAttribute`](https://learn.microsoft.com/en-us/windows/win32/api/dwmapi/nf-dwmapi-dwmsetwindowattribute) and [`DWM_SYSTEMBACKDROP_TYPE`](https://learn.microsoft.com/en-us/windows/win32/api/dwmapi/ne-dwmapi-dwm_systembackdrop_type):
- `DWMWA_USE_IMMERSIVE_DARK_MODE` (`20`, `BOOL`) controls caption/frame polarity; `DWMWA_SYSTEMBACKDROP_TYPE` (`38`, supported starting Windows 11 Build `22621`) selects `DWMSBT_NONE` (`1`), `DWMSBT_MAINWINDOW` (`2`, Mica), `DWMSBT_TRANSIENTWINDOW` (`3`, Acrylic), or `DWMSBT_TABBEDWINDOW` (`4`, Mica Alt).
- Because GPUI's Windows backend opens HWNDs with `WS_EX_NOREDIRECTIONBITMAP` and a premultiplied DirectComposition swap chain (`platform_hooks.rs:298-303`), `apply_window_backdrop` intentionally omits `DwmExtendFrameIntoClientArea` (`MARGINS { -1 }`), preventing Windows from drawing duplicate non-client caption buttons behind `gpui_kit::component::TitleBar`.
- Note on `platform_hooks.rs:435, 455`: `spawn_mica_preference_listener` currently hardcodes `"posthaze-mica-pref-listener"` and `"PosthazeMicaPrefListener\0"` for its hidden top-level `WS_OVERLAPPED` window class; parameterize the window class/thread prefix (or use a crate-scoped name `"ChronophotoMicaPrefListener\0"`).

---

### 3.2 Track B — Cross-Stack Token Source of Truth (Rust + Web / Extensions / CSS)

To keep GPUI desktop apps, Chrome extensions (`extensions/chrome/popup.html`), docs, and future web/Electron tools aligned without manual copy-paste:

#### Format Evaluation: W3C Design Tokens Format Module (DTCG)
Per the [W3C Design Tokens Community Group (DTCG) Format Module](https://www.designtokens.org/) (`tr.designtokens.org/format/`):
- Tokens are structured JSON using `$`-prefixed reserved metadata keys (`$type`, `$value`, `$description`, `$extensions`) so user token/group names never collide (names starting with `$` or containing `{`, `}`, `.` are forbidden).
- Token aliases use `{group.token}` syntax (e.g., `"{palette.chronophotographic.accent}"`), matching the `{colors.*}` reference syntax already used in `docs/DESIGN.md:45, 70-82`.
- Multi-theme / light-dark contexts map cleanly to the **DTCG Resolver Module** (or per-theme token sets under `themes/<id>.tokens.json` overriding semantic aliases over base primitive scales).
- **Variant C & DWM metadata:** Standard DTCG `$type: "color"` supports alpha channels (`#RRGGBBAA` / sRGB components), while GPUI/Win32-specific attributes (`backdrop: "mica" | "mica-alt" | "opaque"`, `tone: "dark" | "light"`, `fallback: "mica-dark-fallback"`) fit cleanly under DTCG's sanctioned `$extensions` namespace (e.g., `"$extensions": { "com.chronophoto": { "backdrop": "mica", "tone": "dark" } }`).

#### Codegen Tooling Comparison: Style Dictionary v4+ vs. Rust `xtask` / `build.rs`

| Criterion | Style Dictionary v4+ (`styledictionary.com`) | Rust `xtask` CLI (or `build.rs` + committed generated files) |
|---|---|---|
| **DTCG Support** | Native `usesDtcg: true` flag + `getReferences(..., { usesDtcg: true })` utilities ([Style Dictionary docs](https://styledictionary.com/)) | Simple recursive JSON resolver in ~150 lines using `serde_json` (already in `Cargo.lock`) |
| **Runtime / Toolchain** | Requires **Node.js + npm** in every consumer repo and CI runner | **Zero external runtime**—uses the existing Rust/Cargo toolchain |
| **Output Flexibility** | Built-in `css/variables` formatter + custom JS `registerFormat` for Rust & `DESIGN.md` | Directly emits typed Rust `const ThemePalette`, CSS custom properties, and `DESIGN.md` YAML frontmatter, plus `--check` mode for CI |
| **Best Fit** | Polyglot monorepos already running Node/Vite/Tailwind pipelines | **Recommended for Posthaze & Rust-first repos** (can still expose standard `.tokens.json` files if a downstream web repo wants to feed them into Style Dictionary later) |

> **Recommendation:** Store canonical tokens in **W3C DTCG `.tokens.json` files** (`tokens/base.tokens.json`, `tokens/themes/*.tokens.json`, `tokens/accents.tokens.json`) and generate checked-in artifacts via a Rust `xtask tokens` command (with `xtask tokens --check` in `scripts/check-repo.ps1` and CI) rather than `build.rs` alone—checked-in generated `.rs` and `.css` files keep `cargo doc`, IDE rust-analyzer, and zero-build Chrome extensions (`extensions/chrome/popup.html`) immediately inspectable without build-script surprises.

#### Three Emitted Targets from One `.tokens.json` Source

1. **Rust (`chronophoto-ui/src/tokens_generated.rs`):**
   - `pub const TYPE_*`, `RADIUS_*`, `SECTION_HEADER_HEIGHT`, `MODAL_FOOTER_HEIGHT`, `INDICATOR_BAR_WIDTH`.
   - `pub const` `ThemePalette` definitions for all 5 opaque themes, 2 Mica themes, and 2 opaque fallbacks, plus `BUILTIN_THEME_PRESETS` and `ACCENT_PRESETS` (`theme.rs:177-629`).
2. **CSS Custom Properties (`tokens/dist/chronophoto.css` for Web / Chrome Extension / Electron):**
   - Per [MDN CSS Custom Properties](https://developer.mozilla.org/en-US/docs/Web/CSS/Using_CSS_custom_properties), [`color-scheme`](https://developer.mozilla.org/en-US/docs/Web/CSS/color-scheme), and [`light-dark()`](https://developer.mozilla.org/en-US/docs/Web/CSS/color_value/light-dark):
     - Emit `:root` / `[data-theme="chronophotographic"]` .. `[data-theme="archival"]` variable blocks (`--cp-graphite-bg`, `--cp-paper-bg`, `--cp-accent`, `--cp-checkpoint-blue`, `--cp-completed-emerald`, `--cp-failed-vermilion`, `--cp-radius-badge: 3px`, `--cp-radius-control: 4px`, `--cp-radius-modal: 6px`).
     - Set `color-scheme: dark` or `light` per `[data-theme]` selector, and provide an automatic `chronophotographic-system` block using `color-scheme: light dark` + `light-dark(<archival>, <chronophotographic>)` so web/extension surfaces follow OS polarity automatically.
     - Directly eliminates the `#4cc38a` / `#e5484d` / `rgba(...)` drift in `extensions/chrome/popup.html:7-24`.
3. **Impeccable `DESIGN.md` Frontmatter (+ optional `.impeccable/design.json` v2 Sidecar):**
   - Per `.agents/skills/impeccable/reference/document.md:3-50, 253-306` (which follows `google-labs-code/design.md` spec), `DESIGN.md` YAML frontmatter is normative for `colors`, `typography`, `rounded`, `spacing`, and `components` (using `{colors.*}` refs and 8 component props), while `.impeccable/design.json` (`schemaVersion: 2`) carries tonal ramps, shadows, motion, and self-contained `.ds-*` HTML/CSS snippets. Generating the YAML frontmatter block from `.tokens.json` fixes the broken `{colors.paper-bg}` / `{colors.carbon-text}` references in `docs/DESIGN.md:75-76`.

---

### 3.3 Track C — Continuous Development & Refinement Loop

#### 1. Standalone GPUI Component Gallery (`examples/gallery.rs`)
- Adding `[[example]] name = "gallery"` inside `chronophoto-ui` compiles **only** `gpui-kit` + `chronophoto-ui` (no `posthaze-engine`, `sqlx`, `reqwest`, `tokio`, `ed25519-dalek`, or `tray-icon`).
- Interactive controls in the gallery sandbox:
  - Live switcher across all 8 `BUILTIN_THEME_PRESETS` + 5 `ACCENT_PRESETS` + simulated `mica_available: true/false` fallback toggle + OS `WindowAppearance` polarity toggle.
  - Matrix rendering every `grid_cells` primitive (`grid_cell_button` across all 5 `GridTone`s × `CellBorder` variants, `grid_boxed_button`, `grid_section_header`, `grid_kv_row`, `grid_compact_input_row`, `grid_nav_row` in both `NavRowTone`s, `grid_badge` across all 5 `BadgeTone`s, `grid_dropdown_surface` + `grid_check_option_row`, `grid_progress_track`, `grid_modal_footer`, and `grid_brand_plate`), plus live `MotionState` / `PanelMotion` sash-drag and flash triggers.
  - Because `chronophoto-ui` can enable `gpui-kit/inspector` in dev builds (`gpui-kit-0.10.0/Cargo.toml:58-63`), layout bounds and style refinements can be inspected live without running Posthaze's download engine.

#### 2. Automated Design-System Invariants
Extend the unit test suite (`theme.rs:925-1065`, `platform_hooks.rs:1101-1268`) with machine-enforced invariants run on every PR:
- **WCAG 2.1 contrast assertions:** Compute relative luminance over composited surfaces for every palette in `BUILTIN_OPAQUE_THEMES` + `MICA_*_FALLBACK` + `ACCENT_PRESETS`:
  - `silver_text` on `graphite_bg` and `carbon_text` on `paper_bg` $\ge 4.5:1$ (`docs/creating-themes.md:110`).
  - `accent_foreground` on `accent` $\ge 4.5:1$, `danger_foreground` on `failed_vermilion` $\ge 4.5:1$, and `silver_muted` / `carbon_muted` $\ge 3.0:1$ (large/UI label threshold).
- **Variant C Mica alpha-calibration assertions:**
  - `backdrop != Opaque` palettes must have `titlebar_bg.a == 0` (`0%` clear glass), `0.0 < graphite_bg.a < paper_bg.a < 1.0` (so work surface reads denser than structural rail), and `modal_bg.a == 255` (`100%` opaque to prevent background bleed-through, `docs/DESIGN.md:256`).
  - `MICA_*_FALLBACK` palettes must have `backdrop == ThemeBackdrop::Opaque` and `a == 255` on all non-scrim surfaces.
- **Headless GPUI `Theme` + `gpui_base::Theme` sync test (`#[gpui_kit::test]`):**
  - Enable `gpui-kit = { version = "0.10.0", features = ["test-support"] }` in `[dev-dependencies]` (`gpui-kit-0.10.0/README.md:60-65`) to assert that `apply_theme_with_accent` updates *both* `Theme::global(cx)` (including `theme.tokens.background == Hsla::transparent_black().into()` in Mica mode) *and* `gpui_base::Theme::global(cx)` (`radius`, `tokens.colors.primary`, `tokens.colors.border`).
- **Rustdoc & Repo Checks:**
  - Enforce `#![deny(missing_docs)]` in `chronophoto-ui` (matching `posthaze-engine`, `AGENTS.md:16`) and verify `$env:RUSTDOCFLAGS="-D warnings"; cargo doc --workspace --no-deps`.
  - Run `xtask tokens --check` in CI / `scripts/check-repo.ps1` so `.tokens.json`, generated Rust/CSS, and `DESIGN.md` frontmatter cannot drift.

#### 3. Cross-Project Impeccable (`DESIGN.md` + `SKILL.md`) Workflow
- Keep a canonical `DESIGN.md` template + `.impeccable/design.json` sidecar inside the design-system repository, documenting the **Dual-Ground Truth Rule**, **Light-on-Dark Compensation Rule**, **Hairline Partition Rule**, and **Single-Layer Variant C Surface Hierarchy** (`docs/DESIGN.md:133, 152, 168, 252`).
- Downstream projects (GPUI apps, web dashboards, browser extensions) copy or symlink the generated `DESIGN.md` + `.impeccable/design.json` at their project root and reference the shared token CSS/crate, so any AI agent running `$impeccable` (`init` / `polish` / `audit` / `document`) in those repos enforces identical visual rules without re-inventing tokens.

---

## 4. Phased Extraction Plan (Ordered by Effort & Payoff)

| Phase | Scope & Concrete Actions | Effort | Payoff |
|---|---|---|---|
| **Phase 0: Fix Drift & Sync Bugs In-Place** | 1. Switch `apply_theme_with_accent` (`theme.rs:733`) from `Theme::global_mut` to `Theme::update(cx, ...)` so `gpui_base::Theme` stays synchronized.<br>2. Add WCAG contrast + Variant C alpha-bound unit tests to `theme.rs`.<br>3. Align `docs/DESIGN.md:5-20, 75-76` frontmatter keys (`paper-bg`, `carbon-text`) and `extensions/chrome/popup.html:7-24` (`--ok`, `--err`, `--border`) with `theme.rs`. | **Low** (1–2 hours) | Eliminates existing scrollbar/handle theme desync, catches contrast regressions, and removes doc/extension token drift immediately. |
| **Phase 1: Workspace Crate Extraction (`crates/chronophoto-ui`)** | 1. Create `crates/chronophoto-ui` in the root `Cargo.toml` workspace with `#![deny(missing_docs)]` and `default = ["mica"]`.<br>2. Move `theme.rs`, `grid_cells.rs` (parameterizing `grid_brand_plate`), `PanelMotion`/`DecayPulse` from `motion.rs`, `CompositeAssetSource`, and the Mica/DWM subset of `platform_hooks.rs` (behind `#[cfg(target_os = "windows")]` + `windows-sys` only).<br>3. Migrate `ThemePalette` state to GPUI `Global` + `ChronoActiveTheme` trait.<br>4. Add `examples/gallery.rs` sandbox and `#[gpui_kit::test]` headless sync tests.<br>5. Point `crates/posthaze-ui` at `chronophoto-ui = { path = "../chronophoto-ui" }`, keeping `DownloadMotion`, `PosthazeAssets`, and tray/startup/EcoQoS/taskbar hooks in `posthaze-ui`. | **Medium** (1–2 days) | Unlocks fast isolated UI iteration (`examples/gallery.rs` without `posthaze-engine`/`sqlx`/`reqwest`) and clean Rust/GPUI crate boundaries. |
| **Phase 2: Multi-Project Standalone Repo** | 1. Split `crates/chronophoto-ui` into its own Git repository (`chronophoto-ui`) with SemVer tags (`v0.1.0`).<br>2. Consume via `git = "...", tag = "..."` in Posthaze and other GPUI projects, using `[patch]` for local co-development.<br>3. Bundle portable `DESIGN.md` + `.impeccable/design.json` starter docs for downstream repos. | **Low–Medium** (half day) | Enables reuse across multiple independent Rust/GPUI desktop repositories with versioned upgrades. |
| **Phase 3: W3C DTCG `.tokens.json` Source of Truth & Codegen** | 1. Author `tokens/base.tokens.json`, `tokens/themes/*.tokens.json`, and `tokens/accents.tokens.json` in W3C DTCG (`$type`/`$value`/`$extensions`) format.<br>2. Add a pure-Rust `xtask tokens` generator emitting Rust `ThemePalette` constants, `chronophoto.css` (`:root`, `[data-theme]`, `color-scheme`, `light-dark()`), and `DESIGN.md` YAML frontmatter.<br>3. Wire `chronophoto.css` into `extensions/chrome/popup.html` and future web/extension UIs; enforce `xtask tokens --check` in CI. | **Medium** (1–2 days) | Single normative token source across Rust/GPUI, Chrome extensions, web UIs, and AI-agent `DESIGN.md` files with zero Node.js requirement. |

---

## 5. Primary Sources Cited

### Posthaze Repository Files
- Workspace configuration & member crates: [Cargo.toml](file:///e:/Development/posthaze-download-manager/Cargo.toml#L1-L28), [crates/posthaze-ui/Cargo.toml](file:///e:/Development/posthaze-download-manager/crates/posthaze-ui/Cargo.toml#L1-L59), [crates/posthaze-engine/Cargo.toml](file:///e:/Development/posthaze-download-manager/crates/posthaze-engine/Cargo.toml#L1-L39)
- Theme tokens, palettes, and `gpui-kit` sync: [crates/posthaze-ui/src/theme.rs](file:///e:/Development/posthaze-download-manager/crates/posthaze-ui/src/theme.rs#L1-L1065)
- Table-Grid Cell primitives & `grid_brand_plate`: [crates/posthaze-ui/src/grid_cells.rs](file:///e:/Development/posthaze-download-manager/crates/posthaze-ui/src/grid_cells.rs#L1-L601)
- Brand assets & `AssetSource`: [crates/posthaze-ui/src/assets.rs](file:///e:/Development/posthaze-download-manager/crates/posthaze-ui/src/assets.rs#L1-L104)
- Win32 DWM Mica & OS hooks: [crates/posthaze-ui/src/platform_hooks.rs](file:///e:/Development/posthaze-download-manager/crates/posthaze-ui/src/platform_hooks.rs#L1-L1268)
- Motion & telemetry interpolation: [crates/posthaze-ui/src/motion.rs](file:///e:/Development/posthaze-download-manager/crates/posthaze-ui/src/motion.rs#L1-L585)
- Multi-window backdrop coordinator & `TitleBar` usage: [crates/posthaze-ui/src/main.rs](file:///e:/Development/posthaze-download-manager/crates/posthaze-ui/src/main.rs#L77-L245), [crates/posthaze-ui/src/main.rs](file:///e:/Development/posthaze-download-manager/crates/posthaze-ui/src/main.rs#L2747-L2820), [crates/posthaze-ui/src/table.rs](file:///e:/Development/posthaze-download-manager/crates/posthaze-ui/src/table.rs#L575-L620)
- Design docs, ADRs & Impeccable reference: [docs/DESIGN.md](file:///e:/Development/posthaze-download-manager/docs/DESIGN.md#L1-L260), [docs/creating-themes.md](file:///e:/Development/posthaze-download-manager/docs/creating-themes.md#L1-L117), [docs/table-grid-ui-and-customization.md](file:///e:/Development/posthaze-download-manager/docs/table-grid-ui-and-customization.md#L1-L60), [docs/adr/0008-gpui-and-gpui-kit-desktop-ui.md](file:///e:/Development/posthaze-download-manager/docs/adr/0008-gpui-and-gpui-kit-desktop-ui.md#L1-L16), [.agents/skills/impeccable/reference/document.md](file:///e:/Development/posthaze-download-manager/.agents/skills/impeccable/reference/document.md#L1-L350)
- Web / extension surfaces: [extensions/chrome/popup.html](file:///e:/Development/posthaze-download-manager/extensions/chrome/popup.html#L1-L300), [docs/ui-concept/posthaze_desktop_ui_concept.html](file:///e:/Development/posthaze-download-manager/docs/ui-concept/posthaze_desktop_ui_concept.html#L1-L150)

### Upstream `gpui-kit` `0.10.0` Cargo Registry Source
- `gpui-kit` facade, features, and `open_window`: [gpui-kit-0.10.0/Cargo.toml](file:///C:/Users/prage/.cargo/registry/src/index.crates.io-1949cf8c6b5b557f/gpui-kit-0.10.0/Cargo.toml#L39-L83), [gpui-kit-0.10.0/src/lib.rs](file:///C:/Users/prage/.cargo/registry/src/index.crates.io-1949cf8c6b5b557f/gpui-kit-0.10.0/src/lib.rs#L47-L215), [gpui-kit-0.10.0/README.md](file:///C:/Users/prage/.cargo/registry/src/index.crates.io-1949cf8c6b5b557f/gpui-kit-0.10.0/README.md#L1-L70)
- `gpui-component` `Theme`, `ThemeMode`, `Theme::update`, `Theme::sync_base`, `ThemeTokens`: [gpui-component-0.10.0/src/theme/mod.rs](file:///C:/Users/prage/.cargo/registry/src/index.crates.io-1949cf8c6b5b557f/gpui-component-0.10.0/src/theme/mod.rs#L156-L863), [gpui-component-0.10.0/src/theme/theme_color.rs](file:///C:/Users/prage/.cargo/registry/src/index.crates.io-1949cf8c6b5b557f/gpui-component-0.10.0/src/theme/theme_color.rs#L9-L407)
- `gpui-component` `RootPlugin` (`WindowState::style`) & `TitleBar`: [gpui-component-0.10.0/src/root.rs](file:///C:/Users/prage/.cargo/registry/src/index.crates.io-1949cf8c6b5b557f/gpui-component-0.10.0/src/root.rs#L21-L23), [gpui-component-0.10.0/src/root.rs](file:///C:/Users/prage/.cargo/registry/src/index.crates.io-1949cf8c6b5b557f/gpui-component-0.10.0/src/root.rs#L722-L759), [gpui-component-0.10.0/src/title_bar.rs](file:///C:/Users/prage/.cargo/registry/src/index.crates.io-1949cf8c6b5b557f/gpui-component-0.10.0/src/title_bar.rs#L24-L94), [gpui-component-0.10.0/src/title_bar.rs](file:///C:/Users/prage/.cargo/registry/src/index.crates.io-1949cf8c6b5b557f/gpui-component-0.10.0/src/title_bar.rs#L352-L360)
- `gpui-base` `Root`, `Theme`, and `SemanticThemeTokens`: [gpui-base-0.10.0/src/root.rs](file:///C:/Users/prage/.cargo/registry/src/index.crates.io-1949cf8c6b5b557f/gpui-base-0.10.0/src/root.rs#L74-L202), [gpui-base-0.10.0/src/theme.rs](file:///C:/Users/prage/.cargo/registry/src/index.crates.io-1949cf8c6b5b557f/gpui-base-0.10.0/src/theme.rs#L10-L46), [gpui-base-0.10.0/src/theme_tokens.rs](file:///C:/Users/prage/.cargo/registry/src/index.crates.io-1949cf8c6b5b557f/gpui-base-0.10.0/src/theme_tokens.rs#L10-L120)
- `gpui-kit-assets` `IconName`, `IconNamed`, `Assets`, `AllAssets`, `icon_assets!`: [gpui-kit-assets-0.10.0/src/lib.rs](file:///C:/Users/prage/.cargo/registry/src/index.crates.io-1949cf8c6b5b557f/gpui-kit-assets-0.10.0/src/lib.rs#L58-L91), [gpui-kit-assets-0.10.0/src/icon.rs](file:///C:/Users/prage/.cargo/registry/src/index.crates.io-1949cf8c6b5b557f/gpui-kit-assets-0.10.0/src/icon.rs#L5-L34), [gpui-kit-assets-0.10.0/src/native_assets.rs](file:///C:/Users/prage/.cargo/registry/src/index.crates.io-1949cf8c6b5b557f/gpui-kit-assets-0.10.0/src/native_assets.rs#L7-L42)

### External Official Specifications & Documentation
- Cargo Reference — Specifying Dependencies (`path`, `git`, `workspace`, `features`, `[patch]`): https://doc.rust-lang.org/cargo/reference/specifying-dependencies.html
- W3C Design Tokens Community Group (DTCG) Format Module & Resolver: https://www.designtokens.org/ (spec: https://tr.designtokens.org/format/)
- Style Dictionary v4 Documentation (DTCG `usesDtcg` & `getReferences` / custom formats): https://styledictionary.com/
- `DESIGN.md` Format Specification: https://raw.githubusercontent.com/google-labs-code/design.md/main/docs/spec.md
- Microsoft Learn — `DwmSetWindowAttribute`: https://learn.microsoft.com/en-us/windows/win32/api/dwmapi/nf-dwmapi-dwmsetwindowattribute
- Microsoft Learn — `DWM_SYSTEMBACKDROP_TYPE`: https://learn.microsoft.com/en-us/windows/win32/api/dwmapi/ne-dwmapi-dwm_systembackdrop_type
- MDN Web Docs — Using CSS custom properties (`--*`): https://developer.mozilla.org/en-US/docs/Web/CSS/Using_CSS_custom_properties
- MDN Web Docs — `color-scheme` & `light-dark()`: https://developer.mozilla.org/en-US/docs/Web/CSS/color_value/light-dark
