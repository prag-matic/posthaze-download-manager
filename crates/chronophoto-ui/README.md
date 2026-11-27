# chronophoto-ui

Pure-Rust GPUI design system implementing the **Chronophotographic Interval Plate** visual language on top of `gpui-kit`.

## Features

- `default = ["mica"]` — enables Windows 11 DWM Mica backdrop support (`windows-sys`, `raw-window-handle` on Windows targets).
- `test-support` — forwards `gpui-kit/test-support` for displayless `#[gpui_kit::test]` integration tests.
- `inspector` — forwards `gpui-kit/inspector` for UI inspection in examples and dev builds.

## Verification

```sh
cargo test -p chronophoto-ui --features test-support
cargo test -p chronophoto-ui --no-default-features
cargo clippy -p chronophoto-ui --all-targets -- -D warnings
RUSTDOCFLAGS="-D warnings" cargo doc -p chronophoto-ui --no-deps
```
