# SQLite persistence via `sqlx` with runtime queries

- **Status:** accepted

`posthaze-engine` persists `Download` metadata and `Segment` `Checkpoint`s in a local SQLite database using `sqlx` (bundled SQLite, `runtime-tokio`, and `sqlx::migrate!`) with runtime-checked `sqlx::query` calls rather than compile-time `sqlx::query!` macros or `rusqlite`.

## Considered Options

- **`rusqlite` (bundled) on `spawn_blocking` / dedicated DB thread:** Simple synchronous API with zero compile-time setup, but requires manual migration management and an extra actor/channel wrapper to integrate with async engine tasks.
- **`sqlx` with compile-time `query!` macros:** Checks SQL against a schema at compile time, but requires a live `DATABASE_URL` or `.sqlx/` offline cache during every build and CI run.
- **`sqlx` with runtime `sqlx::query` and embedded migrations:** Integrates natively with Tokio, ships schema migrations via `sqlx::migrate!`, and compiles cleanly out of the box without a `DATABASE_URL` environment variable.
