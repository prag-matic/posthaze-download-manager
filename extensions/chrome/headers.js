function isHttpUrl(raw) {
  return Boolean(raw && (raw.startsWith("http://") || raw.startsWith("https://")));
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

async function resolveActiveTabUrl(chromeApi) {
  if (!chromeApi?.tabs?.query) {
    return null;
  }

  try {
    const tabs = await chromeApi.tabs.query({ active: true, lastFocusedWindow: true });
    const tabUrl = tabs?.[0]?.url;
    return isHttpUrl(tabUrl) ? tabUrl : null;
  } catch {
    return null;
  }
}

async function collectCookiesForUrl(url, topLevelSite, cookiesApi) {
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
    pushUnique(await cookiesApi.getAll({ url }));
  } catch {
    // Cookie access may be restricted on special origins; proceed without unpartitioned cookies.
  }

  if (topLevelSite) {
    try {
      pushUnique(await cookiesApi.getAll({ url, partitionKey: { topLevelSite } }));
    } catch {
      // Older browsers or restricted origins may reject partitionKey; keep unpartitioned cookies.
    }
  }

  return merged;
}

/**
 * Builds forwarded HTTP headers (`Referer`, `User-Agent`, and `Cookie` including CHIPS
 * partitioned cookies when a top-level site can be determined) for Browser Capture.
 */
export async function buildHeaders(url, referrer, options = {}) {
  const chromeApi = options.chromeApi ?? globalThis.chrome;
  const userAgent =
    options.userAgent !== undefined ? options.userAgent : globalThis.navigator?.userAgent;

  const activeTabUrl = isHttpUrl(referrer) ? null : await resolveActiveTabUrl(chromeApi);
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
    const cookies = await collectCookiesForUrl(url, topLevelSite, chromeApi.cookies);
    if (cookies.length > 0) {
      const cookieHeader = cookies.map((c) => `${c.name}=${c.value}`).join("; ");
      headers.push(["Cookie", cookieHeader]);
    }
  }

  return headers;
}
