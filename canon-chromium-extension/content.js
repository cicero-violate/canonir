(function () {
  // Guard against re-injection and invalidated extension context
  if (window.__ContentBridgeInstalled) return;
  if (!chrome?.runtime?.id) return;
  window.__ContentBridgeInstalled = true;

  let lastTurnId = null;

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
    // Inject both hooks — SPA navigation can change /gg/ → /c/ after load,
    // so both must be present from the start. Targets don't overlap.
    injectScript("request_hook_private.js");
    injectScript("request_hook_group.js");
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
      const payload = event.data.payload;
      let patched = payload;
      if (payload && typeof payload === "object") {
        if (payload.turn_id == null && lastTurnId != null) {
          patched = { ...payload, turn_id: lastTurnId };
        }
      } else if (typeof payload === "string") {
        try {
          const obj = JSON.parse(payload);
          if (obj && obj.turn_id == null && lastTurnId != null) {
            obj.turn_id = lastTurnId;
            patched = obj;
          }
        } catch {}
      }
      chrome.runtime.sendMessage({ type: "INBOUND_MESSAGE", payload: patched }, () => void chrome.runtime.lastError);
    }
    if (event.data?.type === "NEW_CHAT_DONE") {
      console.log("[CS] NEW_CHAT_DONE from page");
      chrome.runtime.sendMessage({ type: "NEW_CHAT_DONE" }, () => void chrome.runtime.lastError);
    }
    if (event.data?.type === "TEMP_CHAT_DONE") {
      console.log("[CS] TEMP_CHAT_DONE from page");
      chrome.runtime.sendMessage({ type: "TEMP_CHAT_DONE" }, () => void chrome.runtime.lastError);
    }
    if (event.data?.type === "SUBMIT_ACK") {
      chrome.runtime.sendMessage(
        {
          type: "SUBMIT_ACK",
          turn_id: event.data.turn_id ?? lastTurnId ?? null,
          ts: event.data.ts ?? Date.now()
        },
        () => void chrome.runtime.lastError
      );
    }
  });

  // Background → Page: prompt injection
  chrome.runtime.onMessage.addListener((message, _sender, sendResponse) => {
    if (message?.type === "OUTBOUND_SUBMIT") {
      console.log("[CS] OUTBOUND_SUBMIT received, posting to page");
      const turnId = message?.payload?.turn_id;
      if (typeof turnId === "number") {
        lastTurnId = turnId;
      }
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
