# Posthaze Chrome / Edge Extension Prototype

Manifest V3 extension that intercepts browser downloads (and adds a **"Download with Posthaze"** link context menu) and forwards URLs plus session headers (`Cookie`, `Referer`, `User-Agent`) to Posthaze via Chrome Native Messaging.

## Setup

1. Build the Native Messaging host binary (`posthaze-cli`) and desktop app (`posthaze-ui`):
   ```powershell
   cargo build -p posthaze-cli -p posthaze-ui
   ```
2. Open `chrome://extensions` (or `edge://extensions`), enable **Developer mode**, click **Load unpacked**, and select `extensions/chrome/`.
3. Copy the unpacked **Extension ID** shown on the card.
4. Register the Native Messaging host in `HKCU`:
   ```powershell
   .\extensions\chrome\install-native-host.ps1 -ExtensionId <YOUR_EXTENSION_ID>
   ```
5. Click the **Posthaze** extension icon in the toolbar and click **Ping Posthaze Host** to verify the connection.

## Behavior
- **When `posthaze-ui` is running**: `posthaze-cli native-host` forwards the download over `\\.\pipe\posthaze-browser-bridge`, opening the **Pre-Flight** modal with captured browser headers.
- **When `posthaze-ui` is closed**: `posthaze-cli native-host` falls back to enqueuing the download directly into `%LOCALAPPDATA%\posthaze\state.db`.
