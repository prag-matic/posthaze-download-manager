---
world: "Chronophotographic Interval Plate"
seed: "seed:a6d67de4e1140bf8"
mode: "operate"
platform: "windows"
---

# Posthaze Design System (`DESIGN.md`)

**Visual World:** Chronophotographic Interval Plate (`seed:a6d67de4e1140bf8`)  
**Implementation:** `crates/posthaze-ui` (`gpui-kit = "0.10.0"`, `src/theme.rs`, `src/main.rs`, `src/inspector.rs`, `src/modals.rs`)

## Color Tokens

| Role | Hex | Usage |
| :--- | :--- | :--- |
| `graphite-bg` | `#181715` | Left filter rail, bottom inspector deck, modal plates |
| `graphite-elevated` | `#22201D` | Active filter pill, telemetry readout card, byte-map track |
| `graphite-border` | `#2C2A27` | Hairline dividers on dark graphite surfaces |
| `silver-text` | `#EAE6DF` | Primary text on dark graphite surfaces |
| `silver-muted` | `#9C968B` | Secondary labels and byte offsets on dark graphite |
| `paper-bg` | `#F2EFE9` | Center `Downloads` table work surface and top dispatch toolbar |
| `paper-selected` | `#E6E0D4` | Selected `Download` row fill |
| `paper-hover` | `#ECE8DF` | Hovered `Download` row fill |
| `paper-border` | `#DFDBD2` | Hairline table row & header rules |
| `carbon-text` | `#141413` | Primary ink on silver-halide paper |
| `carbon-muted` | `#5C5852` | Secondary metadata on silver-halide paper |
| `selenium-amber` | `#D97724` | Active state mark, aggregate speed, in-flight `Chunk` bar fill |
| `cyanotype-blue` | `#2563EB` | Writer-acknowledged disk `Checkpoint` bar fill, `206` range badge |
| `halide-emerald` | `#15803D` | `Completed` state mark and finished `Segment` bar fill |
| `errata-vermilion` | `#DC2626` | `Failed` state mark and `ResourceChanged` errata recovery strip |

## Typography & Density

- **UI Sans**: System Windows UI stack (`Segoe UI` / DirectWrite) for navigation, filenames, modal headers, and controls.
- **Tabular Telemetry Mono**: `Consolas` (`theme::MONO_FONT`) for byte intervals `[start..end)`, speeds, ETAs, FIFO `#1 NEXT` / `#N` badges, and HTTP `Validator`s (`ETag`, `Last-Modified`).
- **Role Scale (`theme::TYPE_*`)**:
  - `TYPE_DISPLAY` (`18px / 24px`, `Bold`): Empty-state title and sidebar aggregate `THROUGHPUT` readout.
  - `TYPE_HEADING` (`15px / 20–22px`, `Bold`/`Semibold`): Brand plate `POSTHAZE`, modal headings, selected inspector filename.
  - `TYPE_ROW_PRIMARY` (`13.5px / 18px`, `Semibold`): Table `filename` cells.
  - `TYPE_BODY` (`12px / 16–18px`): Speed, ETA, segment count, modal field labels and inputs (with `18px` line-height compensation on `#181715` dark surfaces).
  - `TYPE_MICRO` (`11px / 14–16px`): Byte progress sub-line, ruler ticks, `Validator` badges, `Segment` grid cells.
  - `TYPE_HEADER` (`10.5px / 14px`, `Semibold`): Uppercase table column headers and sidebar section kickers.
- **Row Cadence**: `40px` table rows, `32px` sortable headers, `220px` left rail, `56px` toolbar, `240px` bottom inspector (`1440×900` default, `960×640` minimum).

## Signature Move & State Grammar

- **Graduated Interval Byte-Map Ruler**: Proportional `[0..total_bytes)` horizontal bar with `0% / 25% / 50% / 75% / 100%` ticks, Cyanotype Blue disk `Checkpoint` fills, Selenium Amber in-flight fills, and `1px` vertical work-steal split dividers.
- **Fixed-Cell Geometric State Marks**: `◌` (`Queued`), `◐` (`Probing`), `●` (`Active`), `❚❚` (`Paused`), `✓` (`Completed`), `✕` (`Failed` / `Cancelled`).
