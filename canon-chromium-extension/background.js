// Background: single shared WS relay between Rust and all content scripts.
// All messages tagged with tabId. TAB_READY (not TAB_OPENED) gates Rust TURN dispatch.

const RUST_WS = "ws://127.0.0.1:9100";

let ws   = null;
let queue = [];
let pingInterval = null;

// tabId → reqId: tracks which reqId to attach when content signals READY
const pendingOpenReqIds = new Map();
// tabId → original URL from OPEN_TAB (for custom GPT navigate-back on NEW_CHAT).
const tabOriginalUrls = new Map();
// tabId → true while a navigate-back NEW_CHAT is in flight.
const pendingNewChatNavigations = new Map();

function connect() {
  if (ws && (ws.readyState === WebSocket.OPEN || ws.readyState === WebSocket.CONNECTING)) return;

  ws = new WebSocket(RUST_WS);

  ws.onopen = () => {
    console.log("[BG] WS connected to Rust");
    while (queue.length) ws.send(queue.shift());
    if (pingInterval) clearInterval(pingInterval);
    pingInterval = setInterval(() => {
      if (ws && ws.readyState === WebSocket.OPEN) {
        ws.send(JSON.stringify({ type: "PING" }));
      }
    }, 20000);
  };

  ws.onmessage = (ev) => {
    try {
      console.log("[BG] ws.onmessage fired, data length:", ev.data?.length, "preview:", ev.data?.substring?.(0,80));
      const msg = JSON.parse(ev.data);
      handleRustMessage(msg);
    } catch (e) {
      console.warn("[BG] WS parse error", e);
    }
  };

  ws.onclose = () => {
    console.warn("[BG] WS closed — reconnecting in 1s");
    if (pingInterval) { clearInterval(pingInterval); pingInterval = null; }
    ws = null;
    setTimeout(connect, 1000);
  };

  ws.onerror = () => { try { ws.close(); } catch {} };
}

// ── Retry sendMessage with exponential backoff ───────────────────────────
function sendToTab(tabId, message, attempts = 6, delayMs = 150) {
  chrome.tabs.sendMessage(tabId, message, (response) => {
    const err = chrome.runtime.lastError;
    if (err || !response?.ok) {
      if (attempts <= 1) {
        console.warn(`[BG] sendToTab tab=${tabId} failed after all retries:`, err?.message);
        return;
      }
      console.warn(`[BG] sendToTab tab=${tabId} retry in ${delayMs}ms (${attempts - 1} left):`, err?.message);
      setTimeout(() => sendToTab(tabId, message, attempts - 1, delayMs * 2), delayMs);
    }
  });
}

function sendToRust(msg) {
  const raw = typeof msg === "string" ? msg : JSON.stringify(msg);
  if (ws && ws.readyState === WebSocket.OPEN) {
    ws.send(raw);
  } else {
    queue.push(raw);
    connect();
  }
}

// ── Content script → Background ──────────────────────────────────────────
chrome.runtime.onMessage.addListener((message, sender, sendResponse) => {
  const tabId = sender?.tab?.id;
  if (!tabId) { sendResponse({ ok: false, error: "no tabId" }); return true; }

  if (message?.type === "INBOUND_MESSAGE") {
    try {
      const payload = typeof message.payload === "string" ? message.payload : JSON.stringify(message.payload ?? "");
      if (payload.includes("\"limit_modal\"") || payload.includes("\"limit_modal_action\"")) {
        chrome.storage.local.set({
          last_limit_event: payload,
          last_limit_event_ts: Date.now()
        });
      }
    } catch {}
    sendToRust({
      type:    "INBOUND_MESSAGE",
      tabId,
      payload: typeof message.payload === "string"
        ? message.payload
        : JSON.stringify(message.payload ?? "")
    });
    sendResponse({ ok: true });
    return true;
  }

  if (message?.type === "CONTENT_READY") {
    if (pendingNewChatNavigations.get(tabId)) {
      pendingNewChatNavigations.delete(tabId);
      console.log("[BG] CONTENT_READY after navigate-back, sending NEW_CHAT_DONE", { tabId });
      sendToRust({ type: "NEW_CHAT_DONE", tabId });
      sendResponse({ ok: true });
      return true;
    }
    const reqId = pendingOpenReqIds.get(tabId) ?? null;
    pendingOpenReqIds.delete(tabId);
    sendToRust({ type: "TAB_READY", tabId, url: message.url, reqId });
    sendResponse({ ok: true });
    return true;
  }
  if (message?.type === "NEW_CHAT_DONE") {
    console.log("[BG] NEW_CHAT_DONE -> Rust", { tabId });
    sendToRust({ type: "NEW_CHAT_DONE", tabId });
    sendResponse({ ok: true });
    return true;
  }
  if (message?.type === "TEMP_CHAT_DONE") {
    console.log("[BG] TEMP_CHAT_DONE -> Rust", { tabId });
    sendToRust({ type: "TEMP_CHAT_DONE", tabId });
    sendResponse({ ok: true });
    return true;
  }

  sendResponse({ ok: false });
  return true;
});

// ── Rust → Content script ────────────────────────────────────────────────
function handleRustMessage(msg) {
  if (msg?.type === "OPEN_TAB") {
    if (!msg.url || typeof msg.url !== "string") return;
    const reqId = msg.reqId ?? null;

    chrome.tabs.create({ url: msg.url, active: false }, (tab) => {
      if (!tab?.id) return;
      const newTabId = tab.id;
      tabOriginalUrls.set(newTabId, msg.url);
      // Prevent Chrome from discarding background ChatGPT tabs.
      try {
        chrome.tabs.update(newTabId, { autoDiscardable: false });
      } catch {}
      if (reqId !== null) pendingOpenReqIds.set(newTabId, reqId);

      // Informational only
      chrome.tabs.onUpdated.addListener(function listener(id, changeInfo) {
        if (id !== newTabId) return;
        const url = changeInfo.url || "";
        if (!(url.startsWith("https://chatgpt.com") || url.startsWith("https://gemini.google.com"))) return;
        chrome.tabs.onUpdated.removeListener(listener);
        sendToRust({ type: "TAB_OPENED", tabId: newTabId, url, reqId });
      });
    });
    return;
  }

  if (msg?.type === "OUTBOUND_SUBMIT") {
    const targetTabId = msg.tabId;
    if (!targetTabId) return;
    sendToTab(targetTabId, { type: "OUTBOUND_SUBMIT", payload: msg.payload });
    return;
  }

  if (msg?.type === "NEW_CHAT") {
    const targetTabId = msg.tabId;
    if (!targetTabId) return;
    const originalUrl = tabOriginalUrls.get(targetTabId) ?? "";
    const isCustomGpt = originalUrl.includes("/gg/") || originalUrl.includes("chatgpt.com/g/");
    if (isCustomGpt) {
      pendingNewChatNavigations.set(targetTabId, true);
      chrome.tabs.update(targetTabId, { url: originalUrl }, () => void chrome.runtime.lastError);
      return;
    }
    sendToTab(targetTabId, { type: "NEW_CHAT" });
    return;
  }

  if (msg?.type === "TEMP_CHAT") {
    const targetTabId = msg.tabId;
    if (!targetTabId) return;
    sendToTab(targetTabId, { type: "TEMP_CHAT" });
    return;
  }

  if (msg?.type === "CLOSE_TAB") {
    const targetTabId = msg.tabId;
    if (!targetTabId) return;
    chrome.tabs.remove(targetTabId, () => void chrome.runtime.lastError);
    return;
  }

  if (msg?.type === "TURN") {
    const targetTabId = msg.tabId;
    if (!targetTabId) return;
    console.log("[BG] TURN → sendToTab", targetTabId, "text length:", msg.text?.length);
    const turnId = msg.turnId ?? null;
    // Ensure the target tab is active and its window focused before sending.
    chrome.tabs.get(targetTabId, (tab) => {
      const err = chrome.runtime.lastError;
      if (err || !tab) {
        console.warn("[BG] TURN tab lookup failed:", err?.message);
        sendToTab(targetTabId, { type: "OUTBOUND_SUBMIT", payload: { text: msg.text, mode: "auto", turn_id: turnId } });
        return;
      }
      if (tab.windowId) {
        chrome.windows.update(tab.windowId, { focused: true }, () => {
          chrome.tabs.update(targetTabId, { active: true }, () => {
            sendToTab(targetTabId, { type: "OUTBOUND_SUBMIT", payload: { text: msg.text, mode: "auto", turn_id: turnId } });
          });
        });
      } else {
        chrome.tabs.update(targetTabId, { active: true }, () => {
          sendToTab(targetTabId, { type: "OUTBOUND_SUBMIT", payload: { text: msg.text, mode: "auto", turn_id: turnId } });
        });
      }
    });
    return;
  }
}

// ── Tab lifecycle ────────────────────────────────────────────────────────
chrome.tabs.onRemoved.addListener((tabId) => {
  pendingOpenReqIds.delete(tabId);
  tabOriginalUrls.delete(tabId);
  pendingNewChatNavigations.delete(tabId);
  sendToRust({ type: "TAB_CLOSED", tabId });
});

// ── Re-inject content scripts into existing chatgpt tabs on startup ──────
chrome.tabs.query({ url: ["https://chatgpt.com/*", "https://chat.openai.com/*", "https://gemini.google.com/*"] }, (tabs) => {
  for (const tab of tabs) {
    chrome.scripting.executeScript({
      target: { tabId: tab.id },
      files: ["content.js"]
    }).catch(() => {});
  }
});

connect();
