# Posthaze Browser Extension (Chrome / Edge / Brave)

Manifest V3 extension for **Browser Capture**, forwarding intercepted downloads and link/media context-menu captures—along with session `Cookie`, `Referer`, and `User-Agent` headers—to Posthaze via Chromium Native Messaging (`com.posthaze.host`).

## Setup (1-Click Unpacked Registration)

1. Open `chrome://extensions` (or `edge://extensions` / `brave://extensions`), enable **Developer mode**, click **Load unpacked**, and select this `extensions/chrome/` folder.
   - Because `manifest.json` embeds a pinned RSA public `"key"`, unpacked installs consistently receive the deterministic Extension ID `eiophahfcjidkfgdddgejjglglaopahi`.
2. Open **Posthaze → Settings → Integrations** and click **REGISTER NATIVE HOST** (the Extension ID is pre-filled automatically; edit only if overriding with a custom store ID).
   - *(CLI fallback: run `.\extensions\chrome\install-native-host.ps1 -Profile release`.)*
3. Click the **Posthaze** toolbar icon to confirm the status pill shows **UI ONLINE** or **HOST READY**.

## Features & Safety

- **Zero-Loss `onDeterminingFilename` Interception:** Chrome pauses eligible HTTP/HTTPS downloads after redirects and `Content-Disposition` filename/size resolution. When Posthaze UI is running, Posthaze opens Pre-Flight in the foreground and cancels/erases the browser download immediately. If Posthaze is closed, or if a transfer matches `POST` form submission, `Alt+Click` bypass, an excluded domain/referrer, or internal browser streams (`blob:`, `data:`, Telegram Web SW streams, Colab runtime proxy tokens), Chrome resumes the **original transfer stream untouched** with zero duplicate requests.
- **Session, CHIPS & Incognito Isolation:** Both unpartitioned and incognito-scoped (`storeId`) cookies—plus CHIPS partitioned cookies (`partitionKey`)—are collected, `sourcePageUrl` / `Referer` fallbacks survive cross-origin redirects, private window URLs are omitted from `lastStatus` persistence, and signed single-use URLs (`expiry` + `mac`) forward `singleStream` / `skipProbe` hints.
- **`Alt+Click` & 1-Click Site Exclude:** Hold **`Alt+Click`** on any link (via minimal `bypass.js`) for a one-shot 60-second bypass across redirects, or open the toolbar popup and click **EXCLUDE SITE** to toggle the active tab's hostname in **CAPTURE FILTERS** (`minSizeMb`, default extensions `svg, crx, xpi, html, htm`, and excluded hosts `localhost, 127.0.0.1`).
- **Context Menu:** Right-click any link, video, audio, or image and choose **Download with Posthaze** (stages Paused in `state.db` even when the UI is closed).
- **Toolbar Badge & Open Action:** Shows `OFF` when interception is paused, `!` if a host error occurred, or stays clear when healthy. Click **OPEN POSTHAZE** to restore the running desktop window or launch `posthaze-ui.exe` cleanly detached from the browser's Job Object.
