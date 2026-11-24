import {
  DEFAULT_EXCLUDED_DOMAINS,
  DEFAULT_EXCLUDED_EXTENSIONS,
  buildEnqueueHints,
  buildHeaders,
  isBlockedInternalUrl,
  isHttpUrl,
  redactStatusForIncognito,
  resolveStoreId,
  shouldSkipDownload,
} from "./headers.js";

const HOST_NAME = "com.posthaze.host";
const CONTEXT_MENU_ID = "posthaze-download-link";
const STALE_DOWNLOAD_AGE_MS = 60 * 1000;
const TRANSIENT_TTL_MS = 60 * 1000;
const LEGACY_DEFAULT_EXCLUDED_EXTENSIONS = "svg";

const DEFAULT_SETTINGS = {
  interceptEnabled: true,
  minSizeMb: 0,
  excludedExtensions: DEFAULT_EXCLUDED_EXTENSIONS,
  excludedDomains: DEFAULT_EXCLUDED_DOMAINS,
  lastStatus: null,
};

function ensureContextMenus() {
  chrome.contextMenus.removeAll(() => {
    chrome.contextMenus.create({
      id: CONTEXT_MENU_ID,
      title: "Download with Posthaze",
      contexts: ["link", "video", "audio", "image"],
    });
  });
}

async function refreshActionBadge(overrideFailed = null) {
  const { interceptEnabled, lastStatus } = await chrome.storage.local.get({
    interceptEnabled: true,
    lastStatus: null,
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
  if (existing.excludedExtensions === LEGACY_DEFAULT_EXCLUDED_EXTENSIONS) {
    existing.excludedExtensions = DEFAULT_EXCLUDED_EXTENSIONS;
  }
  await chrome.storage.local.set(existing);
  ensureContextMenus();
  await refreshActionBadge();
});

chrome.runtime.onStartup.addListener(async () => {
  ensureContextMenus();
  await refreshActionBadge();
});

// Ephemeral correlation maps mirrored into chrome.storage.session so MV3 ~30s SW idle
// restarts never lose Alt+Click bypasses, POST markers, or redirect-chain origins.
const altBypassUrls = new Map();
const recentPostUrls = new Map();
const redirectParents = new Map();
const requestOrigins = new Map();
let sessionHydrationPromise = null;

function pruneMap(map, now = Date.now()) {
  for (const [key, entry] of map.entries()) {
    if (!entry || entry.expiresAt <= now) {
      map.delete(key);
    }
  }
}

async function ensureSessionHydrated() {
  if (!chrome.storage?.session) {
    return;
  }
  if (!sessionHydrationPromise) {
    sessionHydrationPromise = chrome.storage.session
      .get({
        altBypassEntries: [],
        recentPostEntries: [],
        redirectEntries: [],
      })
      .then((data) => {
        const now = Date.now();
        for (const [url, expiresAt] of data.altBypassEntries || []) {
          if (expiresAt > now && !altBypassUrls.has(url)) {
            altBypassUrls.set(url, { expiresAt });
          }
        }
        for (const [url, expiresAt] of data.recentPostEntries || []) {
          if (expiresAt > now && !recentPostUrls.has(url)) {
            recentPostUrls.set(url, { expiresAt });
          }
        }
        for (const [childUrl, parentUrl, expiresAt] of data.redirectEntries || []) {
          if (expiresAt > now && !redirectParents.has(childUrl)) {
            redirectParents.set(childUrl, { parentUrl, expiresAt });
          }
        }
      })
      .catch(() => {});
  }
  await sessionHydrationPromise;
}

function persistSessionMaps() {
  if (!chrome.storage?.session) {
    return;
  }
  const now = Date.now();
  pruneMap(altBypassUrls, now);
  pruneMap(recentPostUrls, now);
  pruneMap(redirectParents, now);
  chrome.storage.session
    .set({
      altBypassEntries: Array.from(altBypassUrls.entries()).map(([u, v]) => [u, v.expiresAt]),
      recentPostEntries: Array.from(recentPostUrls.entries()).map(([u, v]) => [u, v.expiresAt]),
      redirectEntries: Array.from(redirectParents.entries()).map(([c, v]) => [
        c,
        v.parentUrl,
        v.expiresAt,
      ]),
    })
    .catch(() => {});
}

async function markAltBypass(url) {
  if (!isHttpUrl(url)) {
    return;
  }
  await ensureSessionHydrated();
  altBypassUrls.set(url, { expiresAt: Date.now() + TRANSIENT_TTL_MS });
  persistSessionMaps();
}

function collectUrlLineage(urls) {
  const seen = new Set();
  const queue = urls.filter(isHttpUrl);
  while (queue.length > 0) {
    const cur = queue.shift();
    if (!cur || seen.has(cur)) {
      continue;
    }
    seen.add(cur);
    const parent = redirectParents.get(cur)?.parentUrl;
    if (parent && !seen.has(parent)) {
      queue.push(parent);
    }
  }
  return seen;
}

async function consumeAltBypass(urls) {
  await ensureSessionHydrated();
  const now = Date.now();
  pruneMap(altBypassUrls, now);
  const lineage = collectUrlLineage(urls);
  let matched = false;
  for (const candidate of lineage) {
    const entry = altBypassUrls.get(candidate);
    if (entry && entry.expiresAt > now) {
      altBypassUrls.delete(candidate);
      matched = true;
    }
  }
  if (matched) {
    persistSessionMaps();
  }
  return matched;
}

async function isRecentPostDownload(urls) {
  await ensureSessionHydrated();
  const now = Date.now();
  pruneMap(recentPostUrls, now);
  const lineage = collectUrlLineage(urls);
  for (const candidate of lineage) {
    const entry = recentPostUrls.get(candidate);
    if (entry && entry.expiresAt > now) {
      return true;
    }
  }
  return false;
}

if (chrome.webRequest?.onBeforeRequest) {
  chrome.webRequest.onBeforeRequest.addListener(
    (details) => {
      const now = Date.now();
      const expiresAt = now + TRANSIENT_TTL_MS;
      const originUrl = details.documentUrl || details.initiator || "";
      if (isHttpUrl(originUrl)) {
        pruneMap(requestOrigins, now);
        requestOrigins.set(details.url, { originUrl, expiresAt });
      }
      if (details.method === "POST") {
        pruneMap(recentPostUrls, now);
        recentPostUrls.set(details.url, { expiresAt });
        persistSessionMaps();
      }
    },
    {
      urls: ["http://*/*", "https://*/*"],
      types: ["main_frame", "sub_frame", "other"],
    },
  );
}

if (chrome.webRequest?.onBeforeRedirect) {
  chrome.webRequest.onBeforeRedirect.addListener(
    (details) => {
      if (!details.redirectUrl || !isHttpUrl(details.redirectUrl)) {
        return;
      }
      const now = Date.now();
      const expiresAt = now + TRANSIENT_TTL_MS;
      pruneMap(redirectParents, now);
      redirectParents.set(details.redirectUrl, { parentUrl: details.url, expiresAt });

      const origin = requestOrigins.get(details.url)?.originUrl;
      if (origin && !requestOrigins.has(details.redirectUrl)) {
        requestOrigins.set(details.redirectUrl, { originUrl: origin, expiresAt });
      }
      if (recentPostUrls.has(details.url)) {
        recentPostUrls.set(details.redirectUrl, { expiresAt });
      }
      persistSessionMaps();
    },
    {
      urls: ["http://*/*", "https://*/*"],
      types: ["main_frame", "sub_frame", "other"],
    },
  );
}

async function sendToPosthaze(
  payload,
  { recordStatus = true, recordOnUiOffline = true, incognito = false } = {},
) {
  try {
    const response = await chrome.runtime.sendNativeMessage(HOST_NAME, payload);
    const ok = Boolean(response && response.ok);
    const uiOffline = response && response.mode === "ui-offline";
    if (recordStatus && (!uiOffline || recordOnUiOffline)) {
      const status = redactStatusForIncognito(
        {
          ok,
          mode: (response && response.mode) || "",
          url: payload.url || "",
          message: (response && (response.message || response.error)) || "Sent to Posthaze",
          at: new Date().toISOString(),
        },
        incognito,
      );
      await chrome.storage.local.set({ lastStatus: status });
      await refreshActionBadge(!ok);
    }
    return response;
  } catch (err) {
    const message = err && err.message ? err.message : String(err);
    if (recordStatus) {
      const status = redactStatusForIncognito(
        {
          ok: false,
          mode: "",
          url: payload.url || "",
          message,
          at: new Date().toISOString(),
        },
        incognito,
      );
      await chrome.storage.local.set({ lastStatus: status });
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
  if (isBlockedInternalUrl(url)) {
    return;
  }
  const incognito = Boolean(tab && tab.incognito);
  const referrer = info.pageUrl || (tab && tab.url) || "";
  const sourcePageUrl = (tab && tab.url) || info.pageUrl || "";
  const storeId = resolveStoreId(null, tab);
  const headers = await buildHeaders(url, referrer, {
    topLevelUrl: sourcePageUrl,
    storeId,
    incognito,
  });
  await sendToPosthaze(
    {
      type: "enqueue",
      requireUi: false,
      url,
      originalUrl: url,
      sourcePageUrl: isHttpUrl(sourcePageUrl) ? sourcePageUrl : null,
      filename: null,
      headers,
    },
    { incognito },
  );
});

function isFreshActiveDownload(item) {
  // Chrome can surface restored historical items; only intercept live in-progress downloads.
  if (!item || item.state !== "in_progress" || item.paused || item.endTime) {
    return false;
  }
  if (item.startTime) {
    const startedAt = Date.parse(item.startTime);
    if (Number.isFinite(startedAt) && Date.now() - startedAt > STALE_DOWNLOAD_AGE_MS) {
      return false;
    }
  }
  return true;
}

async function resolveDownloadTabContext(item, urls) {
  let tab = null;
  if (typeof item.tabId === "number" && item.tabId >= 0) {
    try {
      tab = await chrome.tabs.get(item.tabId);
      if (tab && !isHttpUrl(tab.url) && typeof tab.openerTabId === "number" && tab.openerTabId >= 0) {
        const opener = await chrome.tabs.get(tab.openerTabId).catch(() => null);
        if (opener && isHttpUrl(opener.url)) {
          tab = opener;
        }
      }
    } catch {
      tab = null;
    }
  }

  const now = Date.now();
  pruneMap(requestOrigins, now);
  let trackedOrigin = "";
  for (const candidate of urls) {
    const rec = requestOrigins.get(candidate);
    if (rec && isHttpUrl(rec.originUrl)) {
      trackedOrigin = rec.originUrl;
      break;
    }
  }

  const tabUrl = tab && isHttpUrl(tab.url) ? tab.url : "";
  const effectiveReferrer = isHttpUrl(item.referrer)
    ? item.referrer
    : trackedOrigin || tabUrl;
  const sourcePageUrl = tabUrl || trackedOrigin || (isHttpUrl(item.referrer) ? item.referrer : "");
  return { tab, effectiveReferrer, sourcePageUrl };
}

function noName(suggest) {
  // Chromium treats { filename: "..", conflictAction: "uniquify" } as an invalid filename and
  // discards it as a no-op so Chrome or another filename extension preserves the transfer name.
  try {
    suggest({ filename: "..", conflictAction: "uniquify" });
  } catch {
    // Ignore if already canceled or suggested.
  }
}

async function handleDeterminingFilename(item, suggest) {
  let suggested = false;
  const passToBrowser = () => {
    if (!suggested) {
      suggested = true;
      noName(suggest);
    }
  };

  try {
    if (!isFreshActiveDownload(item)) {
      return;
    }
    const settings = await chrome.storage.local.get(DEFAULT_SETTINGS);
    if (!settings.interceptEnabled) {
      return;
    }

    const finalUrl = item.finalUrl || item.url || "";
    const originalUrl = item.url || finalUrl;
    if (isBlockedInternalUrl(finalUrl)) {
      return;
    }

    const candidateUrls = [finalUrl, originalUrl].filter(Boolean);
    if (await consumeAltBypass(candidateUrls)) {
      return;
    }

    if (await isRecentPostDownload(candidateUrls)) {
      const { tab } = await resolveDownloadTabContext(item, candidateUrls);
      const incognito = Boolean(item.incognito || (tab && tab.incognito));
      if (!incognito) {
        await chrome.storage.local.set({
          lastStatus: {
            ok: true,
            mode: "browser-post",
            url: finalUrl,
            message: "Kept in browser (POST form download)",
            at: new Date().toISOString(),
          },
        });
        await refreshActionBadge(false);
      }
      return;
    }

    const { tab, effectiveReferrer, sourcePageUrl } = await resolveDownloadTabContext(
      item,
      candidateUrls,
    );

    if (shouldSkipDownload(item, finalUrl, effectiveReferrer, settings)) {
      return;
    }

    const incognito = Boolean(item.incognito || (tab && tab.incognito));
    const storeId = resolveStoreId(item, tab);
    const headers = await buildHeaders(finalUrl, effectiveReferrer, {
      topLevelUrl: sourcePageUrl,
      storeId,
      incognito,
    });
    const hints = buildEnqueueHints(finalUrl, item, settings);

    const resp = await sendToPosthaze(
      {
        type: "enqueue",
        requireUi: true,
        url: finalUrl,
        originalUrl: originalUrl !== finalUrl ? originalUrl : null,
        sourcePageUrl: isHttpUrl(sourcePageUrl) ? sourcePageUrl : null,
        filename: item.filename || null,
        headers,
        totalBytes: hints.totalBytes,
        singleStream: hints.singleStream,
        skipProbe: hints.skipProbe,
      },
      {
        incognito,
        recordOnUiOffline: false,
      },
    );

    if (!resp || !resp.ok) {
      return;
    }

    try {
      await chrome.downloads.cancel(item.id);
      await chrome.downloads.erase({ id: item.id });
    } catch {
      // Ignore if already canceled.
    }
  } finally {
    passToBrowser();
  }
}

chrome.downloads.onDeterminingFilename.addListener((item, suggest) => {
  void handleDeterminingFilename(item, suggest);
  return true;
});

chrome.runtime.onMessage.addListener((message, sender, sendResponse) => {
  if (!message || (sender && sender.id && sender.id !== chrome.runtime.id)) {
    return false;
  }
  if (message.action === "alt_bypass") {
    (async () => {
      await markAltBypass(message.url);
      sendResponse({ ok: true });
    })();
    return true;
  }
  if (message.action === "ping") {
    (async () => {
      const resp = await sendToPosthaze({ type: "ping" }, { recordStatus: false });
      sendResponse(resp);
    })();
    return true;
  }
  if (message.action === "show_ui") {
    (async () => {
      const resp = await sendToPosthaze({ type: "show" }, { recordStatus: false });
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
