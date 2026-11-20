const HOST_NAME = "com.posthaze.host";
const CONTEXT_MENU_ID = "posthaze-download-link";

const DEFAULT_SETTINGS = {
  interceptEnabled: true,
  minSizeMb: 0,
  excludedExtensions: "svg",
  excludedDomains: "localhost, 127.0.0.1",
  lastStatus: null
};

function ensureContextMenus() {
  chrome.contextMenus.removeAll(() => {
    chrome.contextMenus.create({
      id: CONTEXT_MENU_ID,
      title: "Download with Posthaze",
      contexts: ["link", "video", "audio", "image"]
    });
  });
}

async function refreshActionBadge(overrideFailed = null) {
  const { interceptEnabled, lastStatus } = await chrome.storage.local.get({
    interceptEnabled: true,
    lastStatus: null
  });

  if (!interceptEnabled) {
    await chrome.action.setBadgeText({ text: "OFF" });
    await chrome.action.setBadgeBackgroundColor({ color: "#6b675f" });
    return;
  }

  const failed = overrideFailed !== null ? overrideFailed : Boolean(lastStatus && !lastStatus.ok);
  if (failed) {
    await chrome.action.setBadgeText({ text: "!" });
    await chrome.action.setBadgeBackgroundColor({ color: "#ef4444" });
  } else {
    await chrome.action.setBadgeText({ text: "" });
  }
}

chrome.runtime.onInstalled.addListener(async () => {
  const existing = await chrome.storage.local.get(DEFAULT_SETTINGS);
  await chrome.storage.local.set(existing);
  ensureContextMenus();
  await refreshActionBadge();
});

chrome.runtime.onStartup.addListener(async () => {
  ensureContextMenus();
  await refreshActionBadge();
});

async function buildHeaders(url, referrer) {
  const headers = [];
  if (referrer && (referrer.startsWith("http://") || referrer.startsWith("https://"))) {
    headers.push(["Referer", referrer]);
  }
  if (navigator.userAgent) {
    headers.push(["User-Agent", navigator.userAgent]);
  }
  try {
    const cookies = await chrome.cookies.getAll({ url });
    if (cookies.length > 0) {
      const cookieHeader = cookies.map((c) => `${c.name}=${c.value}`).join("; ");
      headers.push(["Cookie", cookieHeader]);
    }
  } catch {
    // Cookie access may be restricted on special origins; proceed without cookies.
  }
  return headers;
}

function parseCsvList(raw) {
  return String(raw || "")
    .split(",")
    .map((s) => s.trim().toLowerCase().replace(/^\./, ""))
    .filter(Boolean);
}

function shouldSkipDownload(item, url, settings) {
  let parsedUrl;
  try {
    parsedUrl = new URL(url);
  } catch {
    return true;
  }

  const host = parsedUrl.hostname.toLowerCase();
  const excludedDomains = parseCsvList(settings.excludedDomains);
  if (excludedDomains.some((d) => host === d || host.endsWith(`.${d}`))) {
    return true;
  }

  const candidateName = (item.filename || parsedUrl.pathname || "").toLowerCase();
  const dotIdx = candidateName.lastIndexOf(".");
  if (dotIdx !== -1) {
    const ext = candidateName.slice(dotIdx + 1).replace(/[^a-z0-9]/g, "");
    const excludedExts = parseCsvList(settings.excludedExtensions);
    if (ext && excludedExts.includes(ext)) {
      return true;
    }
  }

  const minBytes = Math.max(0, Number(settings.minSizeMb) || 0) * 1024 * 1024;
  if (minBytes > 0 && item.totalBytes > 0 && item.totalBytes < minBytes) {
    return true;
  }

  return false;
}

async function sendToPosthaze(payload, recordStatus = true) {
  try {
    const response = await chrome.runtime.sendNativeMessage(HOST_NAME, payload);
    const ok = Boolean(response && response.ok);
    if (recordStatus) {
      await chrome.storage.local.set({
        lastStatus: {
          ok,
          mode: (response && response.mode) || "",
          url: payload.url || "",
          message: (response && (response.message || response.error)) || "Sent to Posthaze",
          at: new Date().toISOString()
        }
      });
      await refreshActionBadge(!ok);
    }
    return response;
  } catch (err) {
    const message = err && err.message ? err.message : String(err);
    if (recordStatus) {
      await chrome.storage.local.set({
        lastStatus: {
          ok: false,
          mode: "",
          url: payload.url || "",
          message,
          at: new Date().toISOString()
        }
      });
      await refreshActionBadge(true);
    }
    return { ok: false, error: message };
  }
}

chrome.contextMenus.onClicked.addListener(async (info, tab) => {
  if (info.menuItemId !== CONTEXT_MENU_ID) {
    return;
  }
  const url = info.linkUrl || info.srcUrl || "";
  if (!url.startsWith("http://") && !url.startsWith("https://")) {
    return;
  }
  const referrer = info.pageUrl || (tab && tab.url) || "";
  const headers = await buildHeaders(url, referrer);
  await sendToPosthaze({
    type: "enqueue",
    url,
    filename: null,
    headers
  });
});

const bypassedUrls = new Map();

function markBypassedUrl(url) {
  bypassedUrls.set(url, Date.now() + 15000);
}

function consumeBypassedUrl(url) {
  const expiresAt = bypassedUrls.get(url);
  if (!expiresAt) {
    return false;
  }
  bypassedUrls.delete(url);
  return Date.now() <= expiresAt;
}

chrome.downloads.onCreated.addListener(async (item) => {
  const settings = await chrome.storage.local.get(DEFAULT_SETTINGS);
  if (!settings.interceptEnabled) {
    return;
  }
  const url = item.finalUrl || item.url || "";
  if (!url.startsWith("http://") && !url.startsWith("https://")) {
    return;
  }
  if (consumeBypassedUrl(url)) {
    return;
  }
  if (shouldSkipDownload(item, url, settings)) {
    return;
  }

  // Cancel & erase immediately so Chrome does not animate or keep a duplicate download item.
  try {
    await chrome.downloads.cancel(item.id);
    await chrome.downloads.erase({ id: item.id });
  } catch {
    // Ignore if already completed/canceled.
  }

  const headers = await buildHeaders(url, item.referrer || "");
  const resp = await sendToPosthaze({
    type: "enqueue",
    url,
    filename: item.filename || null,
    headers
  });

  // Fall back to Chrome's own downloader only if the native host handoff failed.
  if (!resp || !resp.ok) {
    markBypassedUrl(url);
    try {
      await chrome.downloads.download({ url });
    } catch {
      bypassedUrls.delete(url);
    }
  }
});

chrome.runtime.onMessage.addListener((message, _sender, sendResponse) => {
  if (!message) {
    return false;
  }
  if (message.action === "ping") {
    (async () => {
      const resp = await sendToPosthaze({ type: "ping" }, false);
      sendResponse(resp);
    })();
    return true;
  }
  if (message.action === "show_ui") {
    (async () => {
      const resp = await sendToPosthaze({ type: "show" }, false);
      sendResponse(resp);
    })();
    return true;
  }
  if (message.action === "refresh_badge") {
    (async () => {
      await refreshActionBadge();
      sendResponse({ ok: true });
    })();
    return true;
  }
  return false;
});
