import test from "node:test";
import assert from "node:assert/strict";

import { buildHeaders } from "./headers.js";

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
    ["User-Agent", "TestAgent/1.0"],
    ["Cookie", "chips_tok=xyz"],
  ]);
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
