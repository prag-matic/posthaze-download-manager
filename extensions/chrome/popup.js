const toggle = document.getElementById("intercept-toggle");
const pingBtn = document.getElementById("ping-btn");
const statusBox = document.getElementById("status-box");

function renderStatus(lastStatus) {
  if (!lastStatus) {
    statusBox.className = "status";
    statusBox.textContent = "Ready.";
    return;
  }
  statusBox.className = `status ${lastStatus.ok ? "ok" : "err"}`;
  statusBox.textContent = lastStatus.message || (lastStatus.ok ? "Connected." : "Host error.");
}

async function init() {
  const { interceptEnabled, lastStatus } = await chrome.storage.local.get({
    interceptEnabled: true,
    lastStatus: null
  });
  toggle.checked = Boolean(interceptEnabled);
  renderStatus(lastStatus);
}

toggle.addEventListener("change", async () => {
  await chrome.storage.local.set({ interceptEnabled: toggle.checked });
});

pingBtn.addEventListener("click", async () => {
  statusBox.className = "status";
  statusBox.textContent = "Pinging native host...";
  const resp = await chrome.runtime.sendMessage({ action: "ping" });
  renderStatus({
    ok: Boolean(resp && resp.ok),
    message: (resp && (resp.message || resp.error)) || "No response from native host."
  });
});

init();
