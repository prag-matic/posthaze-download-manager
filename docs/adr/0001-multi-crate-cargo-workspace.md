# Multi-crate Cargo workspace from day one

- **Status:** accepted

We will structure the repository as a multi-crate Cargo workspace (separating the core download engine, CLI harness, and desktop UI — `posthaze-ui`, built with GPUI + GPUI Kit per [ADR-0008](0008-gpui-and-gpui-kit-desktop-ui.md)) from the start rather than beginning with a single crate.

## Considered Options

- **Single crate with module boundaries:** Simpler initial setup, but relies on convention alone to prevent the UI from reaching into HTTP connections or segment state.
- **Multi-crate Cargo workspace:** Enforces the architectural rule that the UI must never directly manage HTTP connections or file segments at compile time, and guarantees the engine can be tested and benchmarked headlessly.

## Consequences

- Cross-layer changes require explicit public APIs between crates rather than ad-hoc internal imports.
- The headless CLI and benchmark suite can depend on the engine crate without pulling in GPUI, GPUI Kit, or Windows GUI dependencies.
