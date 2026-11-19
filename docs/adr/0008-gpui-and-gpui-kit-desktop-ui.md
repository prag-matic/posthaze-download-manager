# GPUI and GPUI Kit for the desktop UI

- **Status:** accepted

We will build `posthaze-ui` using **GPUI** (`gpui.rs`) and **GPUI Kit** (`gpui-kit.com`, crate `gpui-kit`) rather than Slint (`.slint` DSL).

## Considered Options

- **Slint (`.slint` declarative DSL + Rust glue):** Mature declarative UI toolkit, but introduces a separate `.slint` markup language, a `build.rs` compilation step, and `slint::VecModel` / `invoke_from_event_loop` bridging boilerplate.
- **GPUI + GPUI Kit (`gpui-kit`):** Pure-Rust, GPU-accelerated desktop framework natively targeting Windows via Win32, DirectX, and DirectWrite, paired with 75+ built-in components (buttons, inputs, data tables, docking/layouts) and native async integration (`Context::spawn` / `AsyncApp` + `cx.notify()`).

## Consequences

- `posthaze-ui` stays 100% pure Rust with no separate UI DSL or `build.rs` markup compilation step.
- Building on Windows requires the MSVC C++ build toolchain alongside stable Rust.
- The compile-time crate boundary in [ADR-0001](0001-multi-crate-cargo-workspace.md) remains unchanged: `posthaze-ui` consumes `posthaze-engine` exclusively via its public async API and never accesses HTTP connections or `.part` files directly.
