import {
  DEFAULT_EXCLUDED_DOMAINS,
  DEFAULT_EXCLUDED_EXTENSIONS,
  hostMatchesCsv,
  isHttpUrl,
  parseCsvList,
} from "./headers.js";

const toggle = document.getElementById("intercept-toggle");
const siteExcludeBtn = document.getElementById("site-exclude-btn");
const siteExcludeHost = document.getElementById("site-exclude-host");
const siteExcludeLabel = document.getElementById("site-exclude-label");
const openUiBtn = document.getElementById("open-ui-btn");
const pingBtn = document.getElementById("ping-btn");
const hostPill = document.getElementById("host-pill");
const statusBox = document.getElementById("status-box");
const minSizeInput = document.getElementById("min-size");
const skipExtInput = document.getElementById("skip-ext");
const skipDomInput = document.getElementById("skip-dom");

let currentTabHost = "";

function renderHostPill(resp) {
  if (!resp || !resp.ok) {
    hostPill.className = "pill offline";
    hostPill.textContent = "Offline";
    return;
  }
  if (resp.mode === "pong") {
    hostPill.className = "pill online";
    hostPill.textContent = "UI online";
  } else {
    hostPill.className = "pill host";
    hostPill.textContent = "Host ready";
  }
}

function renderStatus(lastStatus) {
  statusBox.innerHTML = "";
  if (!lastStatus) {
    statusBox.className = "status";
    statusBox.textContent = "Ready for browser capture";
    return;
  }
  statusBox.className = `status ${lastStatus.ok ? "ok" : "err"}`;
  if (lastStatus.url) {
    const urlEl = document.createElement("div");
    urlEl.className = "status-url";
    urlEl.textContent = lastStatus.url;
    statusBox.appendChild(urlEl);
  }
  const msgEl = document.createElement("div");
  msgEl.textContent = lastStatus.message || (lastStatus.ok ? "Connected" : "Host error");
  statusBox.appendChild(msgEl);
}

function renderSiteExclude(excludedDomains) {
  if (!currentTabHost) {
    siteExcludeBtn.hidden = true;
    return;
  }
  const excluded = hostMatchesCsv(currentTabHost, excludedDomains);
  siteExcludeBtn.hidden = false;
  siteExcludeBtn.classList.toggle("excluded", excluded);
  siteExcludeHost.textContent = currentTabHost;
  siteExcludeLabel.textContent = excluded ? "Excluded · Allow" : "Exclude site";
}

async function pingHost() {
  hostPill.className = "pill";
  hostPill.textContent = "Pinging";
  try {
    const resp = await chrome.runtime.sendMessage({ action: "ping" });
    renderHostPill(resp);
    return resp;
  } catch (err) {
    const resp = { ok: false, error: err && err.message ? err.message : String(err) };
    renderHostPill(resp);
    return resp;
  }
}

async function resolveCurrentTabHost() {
  try {
    const [tab] = await chrome.tabs.query({ active: true, lastFocusedWindow: true });
    if (tab && isHttpUrl(tab.url)) {
      return new URL(tab.url).hostname.toLowerCase();
    }
  } catch {
    // Ignore restricted/no-tab states.
  }
  return "";
}

async function init() {
  const [data, host] = await Promise.all([
    chrome.storage.local.get({
      interceptEnabled: true,
      minSizeMb: 0,
      excludedExtensions: DEFAULT_EXCLUDED_EXTENSIONS,
      excludedDomains: DEFAULT_EXCLUDED_DOMAINS,
      lastStatus: null,
    }),
    resolveCurrentTabHost(),
  ]);
  currentTabHost = host;
  toggle.checked = Boolean(data.interceptEnabled);
  minSizeInput.value = String(data.minSizeMb ?? 0);
  skipExtInput.value = data.excludedExtensions || "";
  skipDomInput.value = data.excludedDomains || "";
  renderSiteExclude(skipDomInput.value);
  renderStatus(data.lastStatus);
  await pingHost();
}

toggle.addEventListener("change", async () => {
  await chrome.storage.local.set({ interceptEnabled: toggle.checked });
  await chrome.runtime.sendMessage({ action: "refresh_badge" });
});

siteExcludeBtn.addEventListener("click", async () => {
  if (!currentTabHost) {
    return;
  }
  const existing = parseCsvList(skipDomInput.value);
  const excluded = hostMatchesCsv(currentTabHost, skipDomInput.value);
  const next = excluded
    ? existing.filter((rule) => !(currentTabHost === rule || currentTabHost.endsWith(`.${rule}`)))
    : [...new Set([...existing, currentTabHost])];
  const serialized = next.join(", ");
  skipDomInput.value = serialized;
  await chrome.storage.local.set({ excludedDomains: serialized });
  renderSiteExclude(serialized);
});

for (const [el, key, parse] of [
  [minSizeInput, "minSizeMb", (v) => Math.max(0, Number(v) || 0)],
  [skipExtInput, "excludedExtensions", (v) => v],
  [skipDomInput, "excludedDomains", (v) => v],
]) {
  el.addEventListener("change", async () => {
    const parsed = parse(el.value);
    await chrome.storage.local.set({ [key]: parsed });
    if (key === "excludedDomains") {
      renderSiteExclude(parsed);
    }
  });
}

openUiBtn.addEventListener("click", async () => {
  try {
    const resp = await chrome.runtime.sendMessage({ action: "show_ui" });
    renderStatus({
      ok: Boolean(resp && resp.ok),
      message:
        (resp && (resp.message || resp.error)) ||
        "Extension updated — click Reload in chrome://extensions",
    });
  } catch (err) {
    renderStatus({
      ok: false,
      message: err && err.message ? err.message : "Extension bridge error",
    });
  }
  await pingHost();
});

pingBtn.addEventListener("click", async () => {
  const resp = await pingHost();
  renderStatus({
    ok: Boolean(resp && resp.ok),
    message:
      (resp && (resp.message || resp.error)) ||
      "Extension updated — click Reload in chrome://extensions",
  });
});

init();
