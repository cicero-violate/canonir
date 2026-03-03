(function () {
  // Guard against re-injection and invalidated extension context
  if (window.__ContentBridgeInstalled) return;
  if (!chrome?.runtime?.id) return;
  window.__ContentBridgeInstalled = true;

  // Inject main bridge
  function injectScript(src) {
    const s = document.createElement("script");
    s.src = chrome.runtime.getURL(src);
    (document.head || document.documentElement).appendChild(s);
    s.onload = () => s.remove();
  }

  const host = location.hostname;
  if (host === "gemini.google.com") {
    injectScript("request_gemini.js");
  } else {
    injectScript("inject.js");
    // Inject the correct request hook based on path
    if (location.pathname.startsWith("/gg/")) {
      injectScript("request_hook_group.js");
    } else {
      injectScript("request_hook_private.js");
    }
  }

  // inject.js → content.js: bridge installed signal
  window.addEventListener("message", (event) => {
    if (event.source !== window) return;
    if (event.data?.type === "BRIDGE_READY") {
      chrome.runtime.sendMessage(
        { type: "CONTENT_READY", url: location.href },
        () => void chrome.runtime.lastError
      );
    }
  });

  // Page → Background: stream captures
  window.addEventListener("message", (event) => {
    if (event.source !== window) return;
    if (event.data?.type === "INBOUND_MESSAGE") {
      chrome.runtime.sendMessage(event.data, () => void chrome.runtime.lastError);
    }
  });

  // Background → Page: prompt injection
  chrome.runtime.onMessage.addListener((message, _sender, sendResponse) => {
    if (message?.type === "OUTBOUND_SUBMIT") {
      console.log("[CS] OUTBOUND_SUBMIT received, posting to page");
      window.postMessage({ type: "OUTBOUND_SUBMIT", payload: message.payload }, "*");
      sendResponse({ ok: true });
      return true;
    }
    if (message?.type === "NEW_CHAT") {
      window.postMessage({ type: "NEW_CHAT" }, "*");
      sendResponse({ ok: true });
      return true;
    }
    if (message?.type === "TEMP_CHAT") {
      window.postMessage({ type: "TEMP_CHAT" }, "*");
      sendResponse({ ok: true });
      return true;
    }
    sendResponse({ ok: false });
    return true;
  });
})();
