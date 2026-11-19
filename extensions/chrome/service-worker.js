const HOST_NAME = "com.posthaze.host";
const CONTEXT_MENU_ID = "posthaze-download-link";

chrome.runtime.onInstalled.addListener(async () => {
  const { interceptEnabled } = await chrome.storage.local.get({ interceptEnabled: true });
  await chrome.storage.local.set({ interceptEnabled });

  chrome.contextMenus.removeAll(() => {
    chrome.contextMenus.create({
      id: CONTEXT_MENU_ID,
      title: "Download with Posthaze",
      contexts: ["link"]
    });
  });
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

async function sendToPosthaze(payload) {
  try {
    const response = await chrome.runtime.sendNativeMessage(HOST_NAME, payload);
    await chrome.storage.local.set({
      lastStatus: {
        ok: Boolean(response && response.ok),
        url: payload.url || "",
        message: (response && (response.message || response.error)) || "Sent to Posthaze",
        at: new Date().toISOString()
      }
    });
    return response;
  } catch (err) {
    const message = err && err.message ? err.message : String(err);
    await chrome.storage.local.set({
      lastStatus: {
        ok: false,
        url: payload.url || "",
        message,
        at: new Date().toISOString()
      }
    });
    return { ok: false, error: message };
  }
}

chrome.contextMenus.onClicked.addListener(async (info, tab) => {
  if (info.menuItemId !== CONTEXT_MENU_ID || !info.linkUrl) {
    return;
  }
  const url = info.linkUrl;
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

chrome.downloads.onCreated.addListener(async (item) => {
  const { interceptEnabled } = await chrome.storage.local.get({ interceptEnabled: true });
  if (!interceptEnabled) {
    return;
  }
  const url = item.finalUrl || item.url || "";
  if (!url.startsWith("http://") && !url.startsWith("https://")) {
    return;
  }

  try {
    await chrome.downloads.cancel(item.id);
    await chrome.downloads.erase({ id: item.id });
  } catch {
    // Ignore if already completed/canceled.
  }

  const headers = await buildHeaders(url, item.referrer || "");
  await sendToPosthaze({
    type: "enqueue",
    url,
    filename: item.filename || null,
    headers
  });
});

chrome.runtime.onMessage.addListener((message, _sender, sendResponse) => {
  if (message && message.action === "ping") {
    (async () => {
      const resp = await sendToPosthaze({ type: "ping" });
      sendResponse(resp);
    })();
    return true;
  }
  return false;
});
