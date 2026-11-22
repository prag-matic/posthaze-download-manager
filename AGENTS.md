# AGENTS.md

## Agent skills

### Issue tracker

Issues and specs are tracked as local markdown files under `.scratch/`. See `docs/agents/issue-tracker.md`.

### Triage labels

Uses the default five canonical triage labels (`needs-triage`, `needs-info`, `ready-for-agent`, `ready-for-human`, `wontfix`). See `docs/agents/triage-labels.md`.

### Domain docs

Single-context layout (`docs/GLOSSARY.md` and `docs/adr/`). See `docs/agents/domain.md`.

## Documentation Standards

Documentation is a core deliverable of this project, not an afterthought:

1. **Ticket & feature docs:** Every completed `.scratch/` ticket or new user-facing feature (not bugfixes) must add or update its corresponding documentation under `docs/` and keep `docs/README.md` and `README.md` links current.
2. **Rustdoc completeness:** `posthaze-engine` enforces `#![deny(missing_docs)]`. Every public module, struct, enum, variant, field, and function must have clear `///` or `//!` doc comments using canonical terms from `docs/GLOSSARY.md`.
3. **Zero doc warnings:** `cargo doc --workspace --no-deps` with `RUSTDOCFLAGS="-D warnings"` must pass before committing.
4. **Domain & ADR hygiene:** Update `docs/GLOSSARY.md` when new domain terms are introduced and record non-obvious architectural trade-offs in `docs/adr/`.

## Review and checks

Review against `docs/CODING_STANDARDS.md`. Run `scripts/check-repo.ps1` for version, migration, and Windows-API invariants; enable the hook with `git config core.hooksPath .githooks`. Release files that carry the version: `Cargo.toml`, `extensions/chrome/manifest.json`, `packaging/windows/posthaze.iss`.
