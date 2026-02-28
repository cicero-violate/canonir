(function () {
  // Inject main bridge
  function injectScript(src) {
    const s = document.createElement("script");
    s.src = chrome.runtime.getURL(src);
    (document.head || document.documentElement).appendChild(s);
    s.onload = () => s.remove();
  }

  injectScript("inject.js");

  // Inject the correct request hook based on path
  if (location.pathname.startsWith("/gg/")) {
    injectScript("request_hook_group.js");
  } else {
    injectScript("request_hook_private.js");
  }

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
      window.postMessage({ type: "OUTBOUND_SUBMIT", payload: message.payload }, "*");
      sendResponse({ ok: true });
      return true;
    }
    sendResponse({ ok: false });
    return true;
  });
})();
