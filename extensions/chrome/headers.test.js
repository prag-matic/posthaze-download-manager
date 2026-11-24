import test from "node:test";
import assert from "node:assert/strict";

import {
  DEFAULT_EXCLUDED_EXTENSIONS,
  buildEnqueueHints,
  buildHeaders,
  hostMatchesCsv,
  isBlockedInternalUrl,
  redactStatusForIncognito,
  resolveStoreId,
  shouldSkipDownload,
} from "./headers.js";

test("merges unpartitioned and CHIPS-partitioned cookies without duplicates", async () => {
  const calls = [];
  const fakeChrome = {
    cookies: {
      async getAll(query) {
        calls.push(query);
        if (query.partitionKey) {
          return [
            { name: "cf_clearance", value: "part_val" },
            { name: "session", value: "dup_ignored" },
          ];
        }
        return [
          { name: "session", value: "unpart_val" },
          { name: "prefs", value: "dark" },
        ];
      },
    },
    tabs: {
      async query() {
        return [{ url: "https://kayoani.online/watch/123" }];
      },
    },
  };

  const headers = await buildHeaders(
    "https://drive.kayoani.online/download.aspx?file=abc",
    "https://kayoani.online/watch/123",
    {
      chromeApi: fakeChrome,
      userAgent: "TestAgent/1.0",
    },
  );

  assert.deepEqual(calls, [
    { url: "https://drive.kayoani.online/download.aspx?file=abc" },
    {
      url: "https://drive.kayoani.online/download.aspx?file=abc",
      partitionKey: { topLevelSite: "https://kayoani.online" },
    },
  ]);

  assert.deepEqual(headers, [
    ["Referer", "https://kayoani.online/watch/123"],
    ["User-Agent", "TestAgent/1.0"],
    ["Cookie", "session=unpart_val; prefs=dark; cf_clearance=part_val"],
  ]);
});

test("isolates cookies by incognito storeId and skips regular activeTab Referer", async () => {
  const calls = [];
  const fakeChrome = {
    cookies: {
      async getAll(query) {
        calls.push(query);
        if (query.partitionKey) {
          return [{ name: "part_incog", value: "p1" }];
        }
        return [{ name: "incog_sid", value: "secret" }];
      },
    },
    tabs: {
      async query() {
        return [{ url: "https://regular-tab.example.com/private", incognito: false }];
      },
    },
  };

  const headers = await buildHeaders("https://cdn.example.com/file.zip", "", {
    chromeApi: fakeChrome,
    userAgent: "TestAgent/1.0",
    storeId: "1",
    incognito: true,
    topLevelUrl: "https://incognito-tab.example.com/page",
  });

  assert.deepEqual(calls, [
    { url: "https://cdn.example.com/file.zip", storeId: "1" },
    {
      url: "https://cdn.example.com/file.zip",
      partitionKey: { topLevelSite: "https://incognito-tab.example.com" },
      storeId: "1",
    },
  ]);

  assert.deepEqual(headers, [
    ["User-Agent", "TestAgent/1.0"],
    ["Cookie", "incog_sid=secret; part_incog=p1"],
  ]);
});

test("resolves topLevelSite from active tab when referrer is empty", async () => {
  const calls = [];
  const fakeChrome = {
    cookies: {
      async getAll(query) {
        calls.push(query);
        if (query.partitionKey) {
          return [{ name: "chips_tok", value: "xyz" }];
        }
        return [];
      },
    },
    tabs: {
      async query() {
        return [{ url: "https://kayoani.online/episode/5" }];
      },
    },
  };

  const headers = await buildHeaders(
    "https://drive.kayoani.online/download.aspx?file=abc",
    "",
    {
      chromeApi: fakeChrome,
      userAgent: "TestAgent/1.0",
    },
  );

  assert.deepEqual(calls[1], {
    url: "https://drive.kayoani.online/download.aspx?file=abc",
    partitionKey: { topLevelSite: "https://kayoani.online" },
  });
  assert.deepEqual(headers, [
    ["Referer", "https://kayoani.online/episode/5"],
    ["User-Agent", "TestAgent/1.0"],
    ["Cookie", "chips_tok=xyz"],
  ]);
});

test("never sends non-HTTP(S) active tab URL as Referer", async () => {
  const fakeChrome = {
    cookies: {
      async getAll() {
        return [];
      },
    },
    tabs: {
      async query() {
        return [{ url: "chrome://downloads/" }];
      },
    },
  };

  const headers = await buildHeaders(
    "https://drive.kayoani.online/download.aspx",
    "",
    {
      chromeApi: fakeChrome,
      userAgent: "TestAgent/1.0",
    },
  );

  assert.deepEqual(headers, [["User-Agent", "TestAgent/1.0"]]);
});

test("degrades gracefully when partitioned cookie lookup throws", async () => {
  const fakeChrome = {
    cookies: {
      async getAll(query) {
        if (query.partitionKey) {
          throw new Error("partitionKey unsupported");
        }
        return [{ name: "sid", value: "abc" }];
      },
    },
    tabs: {
      async query() {
        return [{ url: "https://kayoani.online/watch/123" }];
      },
    },
  };

  const headers = await buildHeaders(
    "https://drive.kayoani.online/download.aspx",
    "https://kayoani.online/watch/123",
    {
      chromeApi: fakeChrome,
      userAgent: "TestAgent/1.0",
    },
  );

  assert.deepEqual(headers, [
    ["Referer", "https://kayoani.online/watch/123"],
    ["User-Agent", "TestAgent/1.0"],
    ["Cookie", "sid=abc"],
  ]);
});

test("buildEnqueueHints forwards totalBytes and sets singleStream/skipProbe for signed expiry+mac URLs", () => {
  const hints = buildEnqueueHints(
    "https://drive.kayoani.online/download.aspx?file=abc&expiry=12345&mac=deadbeef",
    { totalBytes: 52428800 },
    {},
  );
  assert.deepEqual(hints, {
    totalBytes: 52428800,
    singleStream: true,
    skipProbe: true,
  });

  const normalHints = buildEnqueueHints(
    "https://cdn.example.com/archive.zip",
    { totalBytes: -1 },
    {},
  );
  assert.deepEqual(normalHints, {
    totalBytes: null,
    singleStream: false,
    skipProbe: false,
  });
});

test("shouldSkipDownload checks target host, referrer host, basename extension, and minSizeMb", () => {
  const settings = {
    excludedDomains: "localhost, excluded.org",
    excludedExtensions: DEFAULT_EXCLUDED_EXTENSIONS,
    minSizeMb: 2,
  };

  assert.equal(hostMatchesCsv("sub.excluded.org", settings.excludedDomains), true);
  assert.equal(
    shouldSkipDownload(
      { filename: "C:\\v1.2\\Downloads\\archive.zip", totalBytes: 5 * 1024 * 1024 },
      "https://cdn.example.com/archive.zip",
      "https://app.excluded.org/page",
      settings,
    ),
    true,
  );
  assert.equal(
    shouldSkipDownload(
      { filename: "C:\\v1.2\\Downloads\\page.html", totalBytes: 5 * 1024 * 1024 },
      "https://cdn.example.com/export",
      "https://good.example.com/",
      settings,
    ),
    true,
  );
  assert.equal(
    shouldSkipDownload(
      { filename: "C:\\v1.2\\Downloads\\archive_noext", totalBytes: 5 * 1024 * 1024 },
      "https://cdn.example.com/dl/123",
      "https://good.example.com/",
      settings,
    ),
    false,
  );
  assert.equal(
    shouldSkipDownload(
      { filename: "small.zip", totalBytes: 1024 },
      "https://cdn.example.com/small.zip",
      "https://good.example.com/",
      settings,
    ),
    true,
  );
});

test("isBlockedInternalUrl and incognito helpers handle SW streams and redaction", () => {
  assert.equal(isBlockedInternalUrl("blob:https://example.com/uuid"), true);
  assert.equal(
    isBlockedInternalUrl("https://web.telegram.org/k/stream/12345"),
    true,
  );
  assert.equal(
    isBlockedInternalUrl("https://example.com/dl?colab-runtime-proxy-token=abc"),
    true,
  );
  assert.equal(isBlockedInternalUrl("https://cdn.example.com/archive.zip"), false);

  assert.equal(resolveStoreId({ incognito: true }, null), "1");
  assert.equal(resolveStoreId({ cookieStoreId: "firefox-private" }, null), "firefox-private");
  assert.equal(resolveStoreId({ incognito: false }, { incognito: false }), undefined);

  assert.deepEqual(
    redactStatusForIncognito({ ok: true, url: "https://secret.example/file", message: "ok" }, true),
    { ok: true, url: "", message: "Sent to Posthaze (Private)" },
  );
});
