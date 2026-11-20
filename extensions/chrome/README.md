# Posthaze Browser Extension (Chrome / Edge / Brave)

Manifest V3 extension for **Browser Capture**, forwarding intercepted downloads and link/media context-menu captures—along with session `Cookie`, `Referer`, and `User-Agent` headers—to Posthaze via Chromium Native Messaging (`com.posthaze.host`).

## Setup

1. Open `chrome://extensions` (or `edge://extensions` / `brave://extensions`), enable **Developer mode**, click **Load unpacked**, and select this `extensions/chrome/` folder.
2. Copy the 32-character **Extension ID** shown on the extension card.
3. Open **Posthaze → Settings → Integration**, paste the **Extension ID**, and click **REGISTER NATIVE HOST**.
   *(CLI fallback: run `.\extensions\chrome\install-native-host.ps1 -ExtensionId <YOUR_EXTENSION_ID> -Profile release`.)*
4. Click the **Posthaze** toolbar icon to confirm the status pill shows **UI ONLINE** or **HOST READY**.

## Features & Safety

- **Safe Pause-Before-Cancel Handoff:** Intercepted downloads are paused first (`chrome.downloads.pause`) and only cancelled in the browser once Posthaze confirms receipt (`ok: true`). If the Native Messaging Host is unreachable or unregistered, the browser automatically resumes the download so transfers are never lost.
- **Capture Filters:** Open **CAPTURE FILTERS** in the toolbar popup to configure minimum file size (MiB), excluded extensions (default `svg`), and excluded domains (default `localhost, 127.0.0.1`).
- **Context Menu:** Right-click any link, video, audio, or image and choose **Download with Posthaze**.
- **Toolbar Badge:** Shows `OFF` when interception is paused, `!` if the last handoff failed, or stays clear when healthy.
- **Open Posthaze:** Click **OPEN POSTHAZE** in the popup to restore the running desktop window from the System Tray (or launch `posthaze-ui.exe` if closed).
