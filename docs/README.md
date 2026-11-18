# Documentation Index

Documentation is a first-class part of Posthaze Download Manager.

## Architecture & Decisions

- [Architecture Proposal — Windows Download Manager](Architecture%20Proposal%20-%20Windows%20Download%20Manager.md)
- [Domain Glossary](../GLOSSARY.md)
- [Product Context](../PRODUCT.md)
- Architecture Decision Records (`docs/adr/`):
  - [0001 — Multi-crate Cargo workspace from day one](adr/0001-multi-crate-cargo-workspace.md)
  - [0002 — Single pre-allocated `.part` file with positional writes](adr/0002-preallocated-single-part-file.md)
  - [0003 — Dynamic segment splitting (work stealing)](adr/0003-dynamic-segment-splitting.md)
  - [0004 — HTTP capability probing with HEAD and Range GET fallback](adr/0004-range-capability-probing.md)
  - [0005 — Dedicated blocking writer task over a bounded channel](adr/0005-dedicated-writer-channel.md)

## Implemented Milestones & Tickets

- [Ticket 01 — Single-Stream HTTP Download Engine & CLI](ticket-01-single-stream-engine.md)
- [Ticket 02 — `HEAD`-Blocked Range Probe Fallback & Chunked Handling](ticket-02-range-probe-fallback.md)
- [Ticket 03 — Multi-`Segment` Concurrent `Download`](ticket-03-multi-segment-download.md)
- [Ticket 04 — Dynamic `Segment` Splitting (Work Stealing)](ticket-04-dynamic-splitting.md)
- [Ticket 05 — Transient `Worker` Retry with Exponential Backoff & Jitter](ticket-05-worker-retry.md)

## Benchmarks

- [Benchmarking & Stress-Testing Guide](benchmarking.md)
- [Ticket 03 Baseline — Static Multi-`Segment` Scaling (`c=2..32`)](benchmarks/ticket-03-baseline.md)
- [Ticket 04 Benchmark — Dynamic `Segment` Splitting (`c=2..32`)](benchmarks/ticket-04-work-stealing.md)

## Agent Workflow Configuration

- [Agent Guidelines](../AGENTS.md)
- [Issue Tracker Conventions](agents/issue-tracker.md)
- [Triage Labels](agents/triage-labels.md)
- [Domain Docs Rules](agents/domain.md)

## Building API Docs

`posthaze-engine` enforces `#![deny(missing_docs)]`. Verify all rustdocs and intra-doc links build cleanly:

```powershell
$env:RUSTDOCFLAGS="-D warnings"
cargo doc --workspace --no-deps --open
```
