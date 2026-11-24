(() => {
  function findHttpLink(event) {
    if (!event.isTrusted || !(event.target instanceof Element)) {
      return null;
    }
    const anchor = event.target.closest("a[href]");
    if (!anchor) {
      return null;
    }
    return /^https?:/i.test(anchor.href) ? anchor : null;
  }

  async function notifyBypass(url) {
    if (!chrome?.runtime?.id) {
      return false;
    }
    try {
      await chrome.runtime.sendMessage({ action: "alt_bypass", url });
      return true;
    } catch {
      return false;
    }
  }

  function onPointerDown(event) {
    if (!event.altKey || event.button > 1) {
      return;
    }
    const anchor = findHttpLink(event);
    if (anchor) {
      void notifyBypass(anchor.href);
    }
  }

  function onCaptureClick(event) {
    if (!event.altKey) {
      return;
    }
    const anchor = findHttpLink(event);
    if (anchor) {
      void notifyBypass(anchor.href);
    }
  }

  async function onBubbleClick(event) {
    if (!event.altKey || event.button !== 0 || event.ctrlKey || event.metaKey || event.shiftKey) {
      return;
    }
    if (event.defaultPrevented) {
      return;
    }
    const anchor = findHttpLink(event);
    if (!anchor) {
      return;
    }
    // Suppress Chrome's Alt+Click "Save link as..." and follow the link after registering bypass.
    event.preventDefault();
    await notifyBypass(anchor.href);
    anchor.click();
  }

  globalThis.addEventListener("pointerdown", onPointerDown, true);
  globalThis.addEventListener("click", onCaptureClick, true);
  globalThis.addEventListener("auxclick", onCaptureClick, true);
  globalThis.addEventListener("click", onBubbleClick, false);
})();
