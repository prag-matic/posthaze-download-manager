# ADR-0013: Extract Reusable Design System into `chronophoto-ui`

- **Status:** Accepted
- **Date:** 2026-11-28

## Context

Posthaze's **"Chronophotographic Interval Plate"** visual language—tokens, opaque and Windows 11 Variant C Mica palettes, GPUI `Theme::change` + `Theme::update` synchronization, Table-Grid Cell UI primitives, overlays, motion interpolation, composite asset routing, and the Win32 DWM Mica coordinator—originally lived inside `crates/posthaze-ui` (`theme.rs`, `grid_cells.rs`, `motion.rs`, `assets.rs`, `platform_hooks.rs`, and `main.rs`). Coupling domain-free UI primitives to `posthaze-ui` prevented reuse across other GPUI applications, required compiling `posthaze-engine`, `sqlx`, and `reqwest` during visual iteration, and embedded Windows-specific `windows-sys` calls directly in UI code.

## Decision

1. **Extract `crates/chronophoto-ui` with domain- and polarity-neutral APIs:** Move all reusable tokens (`tokens.rs`), semantic palettes and WCAG 2.1 / Variant C alpha calibration (`palette.rs`), GPUI `ChronoTheme` global (`theme.rs`, accessed via `cx.chrono()`), fluent `RenderOnce` component builders (`components/`), domain-free motion primitives (`DecayPulse`, `InterpolatedScalar`, `PanelMotion`), `CompositeAssetSource` (`assets.rs`), and the feature-gated DWM Mica coordinator (`platform.rs`, `default = ["mica"]` with cross-platform opaque stubs) into `crates/chronophoto-ui` (`edition = "2024"`, `#![deny(missing_docs)]`, independent SemVer `0.1.0`).
2. **Stage in-workspace first (Phase 1) and migrate `posthaze-ui` directly:** Delete `crates/posthaze-ui/src/grid_cells.rs` and `crates/posthaze-ui/src/theme.rs` with zero compatibility wrappers. `posthaze-ui` retains only download-manager domain views, `DownloadMotion` (composing `PanelMotion`, `DecayPulse`, and `InterpolatedScalar`), `PosthazeAssets` (wrapping `CompositeAssetSource`), and app-specific Windows hooks (tray, `Run\Posthaze` startup registry, EcoQoS, `SetThreadExecutionState`, `ITaskbarList3`, `ShellExecuteW`).
3. **Automated design-system quality gates:** Enforce WCAG 2.1 contrast ratios, Variant C alpha-calibration bounds, `DESIGN.md` YAML frontmatter parity, and displayless `#[gpui_kit::test]` theme-sync checks inside `chronophoto-ui`, alongside an isolated `examples/gallery.rs` workbench.

## Consequences

- **Positive:** Visual iteration in `examples/gallery.rs` compiles without `posthaze-engine`, `sqlx`, or `reqwest`; `chronophoto-ui` compiles cleanly on Windows, macOS, and Linux; and contrast/alpha/doc invariants are enforced by `cargo test`.
- **Trade-off:** Phase 1 keeps `crates/chronophoto-ui` as a workspace member with its own independent SemVer (`0.1.0`) before splitting it to a standalone Git repository in Phase 2.
