export const DEFAULT_EXCLUDED_EXTENSIONS = "svg, crx, xpi, html, htm";
export const DEFAULT_EXCLUDED_DOMAINS = "localhost, 127.0.0.1";

export function isHttpUrl(raw) {
  return Boolean(raw && (raw.startsWith("http://") || raw.startsWith("https://")));
}

export function parseCsvList(raw) {
  return String(raw || "")
    .split(",")
    .map((s) => s.trim().toLowerCase().replace(/^\./, ""))
    .filter(Boolean);
}

export function hostMatchesCsv(hostname, rawCsv) {
  const host = String(hostname || "").toLowerCase();
  if (!host) {
    return false;
  }
  const rules = parseCsvList(rawCsv);
  return rules.some((rule) => host === rule || host.endsWith(`.${rule}`));
}

export function isBlockedInternalUrl(raw) {
  if (!isHttpUrl(raw)) {
    return true;
  }
  if (raw.includes("colab-runtime-proxy-token")) {
    return true;
  }
  try {
    const parsed = new URL(raw);
    const host = parsed.hostname.toLowerCase();
    // Telegram Web decrypts service-worker media streams in-browser; external clients cannot fetch them.
    if (
      host === "web.telegram.org" &&
      (parsed.pathname.includes("/stream/") || parsed.pathname.includes("/progressive/"))
    ) {
      return true;
    }
    return false;
  } catch {
    return true;
  }
}

export function shouldSkipDownload(item, url, referrer, settings = {}) {
  let parsedUrl;
  try {
    parsedUrl = new URL(url);
  } catch {
    return true;
  }

  if (hostMatchesCsv(parsedUrl.hostname, settings.excludedDomains)) {
    return true;
  }

  if (isHttpUrl(referrer)) {
    try {
      const refUrl = new URL(referrer);
      if (hostMatchesCsv(refUrl.hostname, settings.excludedDomains)) {
        return true;
      }
    } catch {
      // Ignore malformed referrer.
    }
  }

  const rawCandidate = String((item && item.filename) || parsedUrl.pathname || "");
  const baseName = (rawCandidate.split(/[\\/]/).pop() || "").toLowerCase();
  const dotIdx = baseName.lastIndexOf(".");
  if (dotIdx !== -1) {
    const ext = baseName.slice(dotIdx + 1).replace(/[^a-z0-9]/g, "");
    const excludedExts = parseCsvList(settings.excludedExtensions);
    if (ext && excludedExts.includes(ext)) {
      return true;
    }
  }

  const minBytes = Math.max(0, Number(settings.minSizeMb) || 0) * 1024 * 1024;
  if (minBytes > 0 && item && item.totalBytes > 0 && item.totalBytes < minBytes) {
    return true;
  }

  return false;
}

export function resolveStoreId(item, tab) {
  if (item && item.cookieStoreId) {
    return item.cookieStoreId;
  }
  if ((item && item.incognito) || (tab && tab.incognito)) {
    return "1";
  }
  return undefined;
}

export function redactStatusForIncognito(status, incognito) {
  if (!incognito || !status) {
    return status;
  }
  return {
    ...status,
    url: "",
    message: status.ok ? "Sent to Posthaze (Private)" : status.message || "Host error",
  };
}

function toOrigin(raw) {
  if (!isHttpUrl(raw)) {
    return null;
  }
  try {
    return new URL(raw).origin;
  } catch {
    return null;
  }
}

async function resolveActiveTabUrl(chromeApi, incognito = undefined) {
  if (!chromeApi?.tabs?.query) {
    return null;
  }

  try {
    const tabs = await chromeApi.tabs.query({ active: true, lastFocusedWindow: true });
    const tab = tabs?.[0];
    if (
      incognito !== undefined &&
      typeof tab?.incognito === "boolean" &&
      tab.incognito !== incognito
    ) {
      return null;
    }
    const tabUrl = tab?.url;
    return isHttpUrl(tabUrl) ? tabUrl : null;
  } catch {
    return null;
  }
}

async function collectCookiesForUrl(url, topLevelSite, cookiesApi, storeId = undefined) {
  const merged = [];
  const seenNames = new Set();

  const pushUnique = (cookies) => {
    for (const cookie of cookies || []) {
      if (cookie?.name && !seenNames.has(cookie.name)) {
        seenNames.add(cookie.name);
        merged.push(cookie);
      }
    }
  };

  try {
    const query = storeId !== undefined ? { url, storeId } : { url };
    pushUnique(await cookiesApi.getAll(query));
  } catch {
    // Cookie access may be restricted on special origins; proceed without unpartitioned cookies.
  }

  if (topLevelSite) {
    try {
      const partQuery =
        storeId !== undefined
          ? { url, partitionKey: { topLevelSite }, storeId }
          : { url, partitionKey: { topLevelSite } };
      pushUnique(await cookiesApi.getAll(partQuery));
    } catch {
      // Older browsers or restricted origins may reject partitionKey; keep unpartitioned cookies.
    }
  }

  return merged;
}

/**
 * Builds forwarded HTTP headers (`Referer`, `User-Agent`, and `Cookie` including CHIPS
 * partitioned cookies and incognito `storeId` isolation) for Browser Capture.
 */
export async function buildHeaders(url, referrer, options = {}) {
  const chromeApi = options.chromeApi ?? globalThis.chrome;
  const userAgent =
    options.userAgent !== undefined ? options.userAgent : globalThis.navigator?.userAgent;

  const activeTabUrl = isHttpUrl(referrer)
    ? null
    : await resolveActiveTabUrl(chromeApi, options.incognito);
  const effectiveReferrer = isHttpUrl(referrer) ? referrer : activeTabUrl;

  const headers = [];
  if (effectiveReferrer) {
    headers.push(["Referer", effectiveReferrer]);
  }
  if (userAgent) {
    headers.push(["User-Agent", userAgent]);
  }

  if (chromeApi?.cookies?.getAll) {
    const topLevelSite = toOrigin(options.topLevelUrl) || toOrigin(effectiveReferrer);
    const cookies = await collectCookiesForUrl(
      url,
      topLevelSite,
      chromeApi.cookies,
      options.storeId,
    );
    if (cookies.length > 0) {
      const cookieHeader = cookies.map((c) => `${c.name}=${c.value}`).join("; ");
      headers.push(["Cookie", cookieHeader]);
    }
  }

  return headers;
}

/**
 * Computes optional `totalBytes` and `singleStream` / `skipProbe` hints from a Chrome
 * `DownloadItem` and extension settings.
 */
export function buildEnqueueHints(url, item, settings = {}) {
  const totalBytes =
    item && Number.isFinite(item.totalBytes) && item.totalBytes > 0 ? item.totalBytes : null;

  let singleStream = false;
  try {
    const parsed = new URL(url);
    const params = parsed.searchParams;
    const hasExpiryMac = params.has("expiry") && params.has("mac");
    const host = parsed.hostname.toLowerCase();
    const configured = String(settings?.singleStreamDomains || "")
      .split(",")
      .map((s) => s.trim().toLowerCase())
      .filter(Boolean);
    const matchesDomain = configured.some((d) => host === d || host.endsWith(`.${d}`));
    singleStream = hasExpiryMac || matchesDomain;
  } catch {
    singleStream = false;
  }

  return {
    totalBytes,
    singleStream,
    skipProbe: singleStream,
  };
}
