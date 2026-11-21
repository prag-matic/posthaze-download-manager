# ADR-0012: Ed25519-Signed In-App Auto Updater

- **Status:** Accepted
- **Date:** 2026-11-21

## Context

Posthaze ships unsigned Windows x64 Inno Setup installers and portable ZIP archives on GitHub Releases. Users need a low-friction way to stay up to date without silently downloading and executing unverified binaries if a release mirror or GitHub account is tampered with, and without breaking portable ZIP deployments.

## Decision

1. **Ed25519 + SHA-256 verification:** Every tagged release signs `posthaze-<ver>-windows-x64-setup.exe` with an Ed25519 private key stored in the GitHub Actions secret `UPDATER_SIGNING_KEY` (via the unshipped `posthaze-keytool` workspace crate) and publishes `<setup>.exe.sig` alongside `SHA256SUMS.txt`. The UI embeds `ACCEPTED_PUBLIC_KEYS` (a slice to support planned key rotation) and refuses to execute the installer unless both SHA-256 and signature verification pass.
2. **User-triggered silent upgrade for installed builds only:** Checking happens automatically at startup and every 24 hours (when `auto_check_updates` is enabled), but downloading and installing always requires an explicit click. Installed builds run Inno Setup with `/SILENT /CLOSEAPPLICATIONS /RESTARTAPPLICATIONS` and `[Run]` `skipifnotsilent` to relaunch; portable builds (`posthaze.portable`) only link to the GitHub release page.

## Consequences

- **Positive:** Protects against corrupted downloads and compromised release assets even without a paid Authenticode certificate; keeps portable archives self-contained.
- **Trade-off:** Losing `UPDATER_SIGNING_KEY` requires shipping a manual update with a new embedded public key, so the signing key must be backed up offline.
