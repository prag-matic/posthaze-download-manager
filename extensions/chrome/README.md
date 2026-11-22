# Posthaze Browser Extension (Chrome / Edge / Brave)

Manifest V3 extension for **Browser Capture**, forwarding intercepted downloads and link/media context-menu captures—along with session `Cookie`, `Referer`, and `User-Agent` headers—to Posthaze via Chromium Native Messaging (`com.posthaze.host`).

## Setup

1. Open `chrome://extensions` (or `edge://extensions` / `brave://extensions`), enable **Developer mode**, click **Load unpacked**, and select this `extensions/chrome/` folder.
2. Copy the 32-character **Extension ID** shown on the extension card.
3. Open **Posthaze → Settings → Integration**, paste the **Extension ID**, and click **REGISTER NATIVE HOST**.
   *(CLI fallback: run `.\extensions\chrome\install-native-host.ps1 -ExtensionId <YOUR_EXTENSION_ID> -Profile release`.)*
4. Click the **Posthaze** toolbar icon to confirm the status pill shows **UI ONLINE** or **HOST READY**.

## Features & Safety

- **Instant Cancel-and-Erase Handoff:** When Posthaze is running (open or minimized to the System Tray), intercepted downloads are cancelled and erased immediately in the browser and handed off to Posthaze. Both unpartitioned and CHIPS-partitioned cookies (`partitionKey`) are forwarded in the `Cookie` header, and when `referrer` is empty the active HTTP/HTTPS tab URL is used as a fallback `Referer`. When Posthaze is closed, the browser keeps and downloads the file normally. If a handoff fails unexpectedly, the extension falls back to `chrome.downloads.download` with a one-shot bypass so the transfer is not lost.
- **Capture Filters:** Open **CAPTURE FILTERS** in the toolbar popup to configure minimum file size (MiB), excluded extensions (default `svg`), and excluded domains (default `localhost, 127.0.0.1`).
- **Context Menu:** Right-click any link, video, audio, or image and choose **Download with Posthaze**.
- **Toolbar Badge:** Shows `OFF` when interception is paused, `!` if the last handoff failed, or stays clear when healthy.
- **Open Posthaze:** Click **OPEN POSTHAZE** in the popup to restore the running desktop window from the System Tray (or launch `posthaze-ui.exe` if closed).
