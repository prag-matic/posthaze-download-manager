# posthaze-download-manager

A Windows-native, multi-connection HTTP/HTTPS download manager built with a decoupled Rust engine (`posthaze-engine`), a headless CLI (`posthaze-cli`), and a planned Slint desktop interface.

## Documentation

- [Documentation Index](docs/README.md)
- [Architecture Proposal](docs/Architecture%20Proposal%20-%20Windows%20Download%20Manager.md)
- [Domain Glossary](GLOSSARY.md)
- [Architecture Decision Records](docs/adr/)
- [Ticket 01 — Single-Stream HTTP Download Engine & CLI](docs/ticket-01-single-stream-engine.md)

## Quick Start

```powershell
cargo test --workspace
cargo run --release -p posthaze-cli -- download "<URL>" --debug
```
