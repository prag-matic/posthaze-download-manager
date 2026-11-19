---
version: 1
slug: "crates-posthaze-ui-src-main-rs"
primary_target: "crates/posthaze-ui/src/main.rs"
related_targets: []
---

# Posthaze Desktop UI (`crates/posthaze-ui/src/main.rs`)

- **Scope & Visitor Mode**: Single-window Windows 10/11 desktop download manager (`crates/posthaze-ui`), **Operate** mode.
- **Audience & Job**: Technical power users, systems engineers, local-LLM practitioners, and modders inspecting remote URLs pre-flight, managing a FIFO concurrency-governed `Queue`, and supervising live multi-`Segment` work-stealing and writer-acked `Checkpoint` progress.

## Direction contract

- **THESIS**: A high-density, calibrated Chronophotographic Interval Plate that replaces generic dark-neon downloaders and bloated shareware grids with a graphite-and-silver-halide workspace where every `Download` and `Segment` exposes its exact disk `Checkpoint` vs in-flight byte state on a shared vertical grid.
- **OWN-WORLD**: Warm graphite emulsion rail & bottom inspector (`#181715`, rules `#2C2A27`, text `#EAE6DF`) framing a silver-halide paper `Downloads` table (`#F2EFE9`, carbon ink `#141413`, rules `#DFDBD2`); Selenium Amber (`#D97724`) for in-flight bytes, Cyanotype Blue (`#2563EB`) for writer-acked `Checkpoint`s, Halide Emerald (`#15803D`) for `Completed`, Errata Vermilion (`#DC2626`) for `Failed` / `ResourceChanged`; workhorse UI sans + tabular mono figures (`Cascadia Mono` / `Consolas`); 90ms two-frame crisp state snaps; signature move **Graduated Interval Byte-Map Ruler**.
- **STORY**: The user pastes or enters a URL, verifies filename/size/range-capability in a Pre-Flight Probe Modal before committing disk space, admits transfers into a FIFO `Queue` (`#1 Next`, `#2`), and inspects live work-stealing bisections and `Checkpoint` safety in the docked bottom ruler.
- **FIRST VIEWPORT**: `1440×900` single-window shell (`220px` left `#181715` filter rail + aggregate throughput/slot telemetry + Settings trigger; `56px` top URL dispatch toolbar with auto-focused URL input, `Paste & Probe`, `Resume All`, `Pause All`, `Clear Completed`; `flex-1` `#F2EFE9` sortable `Downloads` table with fixed-cell geometric state marks, `#N` queue badges, dual-tone progress bars, and clipboard-ready empty state; `240px` bottom `#181715` inspector deck).
- **FORM**: Candidate #4 **Chronophotographic Interval Plate**, `seed:a6d67de4e1140bf8`, raised with `rw-manual-acetate-tab-board` (90ms state snaps + vermilion `ResourceChanged` errata strip), `rw-timetable-slide-rack` (single reversed-ink focal budget + fixed-cell geometric state marks), `crouwel-grid-specimen` (shared vertical construction-grid armature), and `pop-culture-shelf-character-goods-catalog` (live `#1`/`#2`/`#3` FIFO queue rank badges).
- **FINISH**: unreviewed and undocumented is unfinished; this build ends with the finish review, the verdict, DESIGN.md, and every shipping raster carrying its provenance.
