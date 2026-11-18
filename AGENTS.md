# AGENTS.md

## Agent skills

### Issue tracker

Issues and specs are tracked as local markdown files under `.scratch/`. See `docs/agents/issue-tracker.md`.

### Triage labels

Uses the default five canonical triage labels (`needs-triage`, `needs-info`, `ready-for-agent`, `ready-for-human`, `wontfix`). See `docs/agents/triage-labels.md`.

### Domain docs

Single-context layout (`GLOSSARY.md` and `docs/adr/` at repo root). See `docs/agents/domain.md`.

## Documentation Standards

Documentation is a core deliverable of this project, not an afterthought:

1. **Ticket & feature docs:** Every completed ticket or feature must add or update its corresponding documentation under `docs/` and keep `docs/README.md` and `README.md` links current.
2. **Rustdoc completeness:** `posthaze-engine` enforces `#![deny(missing_docs)]`. Every public module, struct, enum, variant, field, and function must have clear `///` or `//!` doc comments using canonical terms from `GLOSSARY.md`.
3. **Zero doc warnings:** `cargo doc --workspace --no-deps` with `RUSTDOCFLAGS="-D warnings"` must pass before committing.
4. **Domain & ADR hygiene:** Update `GLOSSARY.md` when new domain terms are introduced and record non-obvious architectural trade-offs in `docs/adr/`.
