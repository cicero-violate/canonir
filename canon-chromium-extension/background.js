/// Background: pure relay between Rust WS and content scripts

const RUST_WS = "ws://127.0.0.1:8787";

// tabId → { ws, queue }
const tabConnections = new Map();

function connectTab(tabId) {
  const existing = tabConnections.get(tabId);
  if (existing?.ws?.readyState === WebSocket.OPEN ||
      existing?.ws?.readyState === WebSocket.CONNECTING) return;

  const ws   = new WebSocket(RUST_WS);
  const conn = { ws, queue: [] };
  tabConnections.set(tabId, conn);

  ws.onopen = () => {
    // Flush queued messages
    while (conn.queue.length) ws.send(conn.queue.shift());
    // Report tab to Rust
    chrome.tabs.get(tabId, (tab) => {
      if (chrome.runtime.lastError) return;
      ws.send(JSON.stringify({ type: "TAB_OPENED", tabId, url: tab?.url || "" }));
    });
  };

  ws.onmessage = (ev) => {
    try {
      const msg = JSON.parse(ev.data);
      handleRustMessage(tabId, msg);
    } catch (e) {
      console.warn("[BG] WS parse error", e);
    }
  };

  ws.onclose = () => tabConnections.delete(tabId);
  ws.onerror = () => { try { ws.close(); } catch {} };
}

function sendToRust(tabId, payload) {
  const conn = tabConnections.get(tabId);
  const raw  = typeof payload === "string" ? payload : JSON.stringify(payload);
  if (conn?.ws?.readyState === WebSocket.OPEN) {
    conn.ws.send(raw);
  } else {
    conn?.queue.push(raw);
    connectTab(tabId);
  }
}

// Content script → Background
chrome.runtime.onMessage.addListener((message, sender, sendResponse) => {
  const tabId = sender?.tab?.id;
  if (!tabId) { sendResponse({ ok: false, error: "no tabId" }); return true; }

  if (message?.type === "INBOUND_MESSAGE") {
    const raw = typeof message.payload === "string"
      ? message.payload
      : JSON.stringify(message.payload ?? "");
    sendToRust(tabId, raw);
    sendResponse({ ok: true });
    return true;
  }

  sendResponse({ ok: false });
  return true;
});

// Rust → Content script
function handleRustMessage(tabId, msg) {
  if (msg?.type === "OPEN_TAB") {
    if (!msg.url || typeof msg.url !== "string") return;
    const reqId = msg.reqId ?? null;

    chrome.tabs.create({ url: msg.url, active: true }, (tab) => {
      if (!tab?.id) return;
      const newTabId = tab.id;

      chrome.tabs.onUpdated.addListener(function listener(id, changeInfo) {
        if (id !== newTabId) return;
        const url = changeInfo.url || "";
        if (!url.startsWith("https://chatgpt.com")) return;
        chrome.tabs.onUpdated.removeListener(listener);

        const requester = tabConnections.get(tabId);
        if (requester?.ws?.readyState === WebSocket.OPEN) {
          requester.ws.send(JSON.stringify({ type: "TAB_OPENED", tabId: newTabId, url, reqId }));
        }
        connectTab(newTabId);
      });
    });
    return;
  }

  if (msg?.type === "OUTBOUND_SUBMIT") {
    const targetTabId = msg.tabId ?? tabId;
    chrome.tabs.sendMessage(
      targetTabId,
      { type: "OUTBOUND_SUBMIT", payload: msg.payload },
      () => void chrome.runtime.lastError
    );
    return;
  }

  if (msg?.type === "TURN") {
    const targetTabId = msg.tabId ?? tabId;
    chrome.tabs.sendMessage(
      targetTabId,
      { type: "OUTBOUND_SUBMIT", payload: { text: msg.text, mode: "auto" } },
      () => void chrome.runtime.lastError
    );
    return;
  }
}

// Tab lifecycle
chrome.tabs.onRemoved.addListener((tabId) => {
  const conn = tabConnections.get(tabId);
  if (conn?.ws) {
    try {
      conn.ws.send(JSON.stringify({ type: "TAB_CLOSED", tabId }));
      conn.ws.close();
    } catch {}
  }
  tabConnections.delete(tabId);
});
