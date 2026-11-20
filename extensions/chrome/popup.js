const toggle = document.getElementById("intercept-toggle");
const openUiBtn = document.getElementById("open-ui-btn");
const pingBtn = document.getElementById("ping-btn");
const hostPill = document.getElementById("host-pill");
const statusBox = document.getElementById("status-box");
const minSizeInput = document.getElementById("min-size");
const skipExtInput = document.getElementById("skip-ext");
const skipDomInput = document.getElementById("skip-dom");

function renderHostPill(resp) {
  if (!resp || !resp.ok) {
    hostPill.className = "pill offline";
    hostPill.textContent = "OFFLINE";
    return;
  }
  if (resp.mode === "pong") {
    hostPill.className = "pill online";
    hostPill.textContent = "UI ONLINE";
  } else {
    hostPill.className = "pill host";
    hostPill.textContent = "HOST READY";
  }
}

function renderStatus(lastStatus) {
  statusBox.innerHTML = "";
  if (!lastStatus) {
    statusBox.className = "status";
    statusBox.textContent = "Ready for browser capture.";
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
  msgEl.textContent = lastStatus.message || (lastStatus.ok ? "Connected." : "Host error.");
  statusBox.appendChild(msgEl);
}

async function pingHost() {
  hostPill.className = "pill";
  hostPill.textContent = "PING...";
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

async function init() {
  const data = await chrome.storage.local.get({
    interceptEnabled: true,
    minSizeMb: 0,
    excludedExtensions: "svg",
    excludedDomains: "localhost, 127.0.0.1",
    lastStatus: null
  });
  toggle.checked = Boolean(data.interceptEnabled);
  minSizeInput.value = String(data.minSizeMb ?? 0);
  skipExtInput.value = data.excludedExtensions || "";
  skipDomInput.value = data.excludedDomains || "";
  renderStatus(data.lastStatus);
  await pingHost();
}

toggle.addEventListener("change", async () => {
  await chrome.storage.local.set({ interceptEnabled: toggle.checked });
  await chrome.runtime.sendMessage({ action: "refresh_badge" });
});

for (const [el, key, parse] of [
  [minSizeInput, "minSizeMb", (v) => Math.max(0, Number(v) || 0)],
  [skipExtInput, "excludedExtensions", (v) => v],
  [skipDomInput, "excludedDomains", (v) => v]
]) {
  el.addEventListener("change", async () => {
    await chrome.storage.local.set({ [key]: parse(el.value) });
  });
}

openUiBtn.addEventListener("click", async () => {
  try {
    const resp = await chrome.runtime.sendMessage({ action: "show_ui" });
    renderStatus({
      ok: Boolean(resp && resp.ok),
      message:
        (resp && (resp.message || resp.error)) ||
        "Extension updated — click Reload in chrome://extensions."
    });
  } catch (err) {
    renderStatus({
      ok: false,
      message: err && err.message ? err.message : "Extension bridge error."
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
      "Extension updated — click Reload in chrome://extensions."
  });
});

init();
