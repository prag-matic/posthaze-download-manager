# Auto Updater

Posthaze checks GitHub Releases for newer stable versions and lets installed Windows builds upgrade in place with cryptographic verification.

## How It Works

1. **Schedule:** When `auto_check_updates` is enabled (default: `true`), `posthaze-ui` queries `https://api.github.com/repos/prag-matic/posthaze-download-manager/releases/latest` on startup and every 24 hours via the background Tokio runtime. Users can also click **Check Now** under **Settings → Updates**.
2. **Release filtering:** Drafts, prereleases, and tags older than or equal to `CARGO_PKG_VERSION` are ignored (strict SemVer, no downgrades).
3. **Notification:** When a newer release is found, an `UPDATE v{x}` chip appears in the titlebar next to the version label (and a Windows toast is sent if the window is inactive). Clicking the chip opens **Settings → Updates**.
4. **Portable vs. Installed:**
   - **Installed builds:** Clicking **Install v{x}** streams the Inno Setup installer (`*-windows-x64-setup.exe`), its `.sig` file, and `SHA256SUMS.txt` into `%TEMP%\posthaze-update\` (capped at 200 MiB). Only after **both** the SHA-256 checksum and the Ed25519 **Update Signature** verify against an embedded public key does Posthaze launch the installer silently (`/SILENT /CLOSEAPPLICATIONS /RESTARTAPPLICATIONS`) and exit.
   - **Portable builds** (`posthaze.portable` beside `posthaze-ui.exe`): Never self-overwrite. They show the update notice and a **Download from GitHub** link that opens the release page in the default browser.

## Signing & Key Management

Because Posthaze Windows binaries do not currently use a paid Authenticode certificate, the updater verifies every downloaded installer with an Ed25519 signature:

- **`crates/posthaze-keytool`** (`publish = false`, not shipped):
  - `cargo run -p posthaze-keytool -- gen` generates a base64 Ed25519 keypair (`UPDATER_SIGNING_KEY` and `UPDATER_PUBLIC_KEY`).
  - `cargo run -p posthaze-keytool -- sign <file>` reads `UPDATER_SIGNING_KEY` from the environment and writes `<file>.sig`.
- **CI (`.github/workflows/release.yml`):** Tagged releases require the `UPDATER_SIGNING_KEY` GitHub Actions secret and publish `<setup>.exe.sig` alongside `SHA256SUMS.txt`.
- **Key rotation:** `ACCEPTED_PUBLIC_KEYS` in `crates/posthaze-ui/src/updater.rs` is a slice so a new public key can be pre-announced in one release before retiring the old key. Always keep an offline backup of `UPDATER_SIGNING_KEY`.

## Bootstrapping & Local Testing

- **Bootstrapping:** Releases prior to the updater (such as `v0.1.1`) must be upgraded manually once. Every subsequent release updates in place.
- **Local testing (debug builds only):** Set `POSTHAZE_UPDATE_URL=http://127.0.0.1:<port>/latest` to point a debug build at a local release server. Release builds ignore this variable and enforce the GitHub Releases download URL prefix.
