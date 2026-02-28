(function () {
  if (window.__BridgeInstalled) return;
  window.__BridgeInstalled = true;

  // ── Globals for prompt injection (set by request hooks) ──────────────────
  window.__pendingPromptInjection  = window.__pendingPromptInjection  || null;
  window.__promptInjectionMode     = window.__promptInjectionMode     || "auto";
  window.__promptInjectionQueue    = window.__promptInjectionQueue    || [];

  // ── WebSocket hook (Calpico transport) ───────────────────────────────────
  const __OrigWS = window.WebSocket;
  window.WebSocket = function (url, protocols) {
    const ws = protocols ? new __OrigWS(url, protocols) : new __OrigWS(url);
    ws.addEventListener("message", (ev) => {
      let data = typeof ev.data === "string"
        ? ev.data
        : ev.data instanceof ArrayBuffer
          ? new TextDecoder().decode(ev.data)
          : "";
      if (data) window.postMessage({ type: "INBOUND_MESSAGE", payload: data }, "*");
    });
    return ws;
  };
  window.WebSocket.prototype = __OrigWS.prototype;

  // ── Fetch hook (SSE + Calpico streaming) ─────────────────────────────────
  const TARGETS = [
    { origin: "https://chatgpt.com",     path: "/backend-api/f/conversation" },
    { origin: "https://chat.openai.com", path: "/backend-api/f/conversation" },
    { origin: "https://chatgpt.com",     path: "/backend-api/calpico" },
    { origin: "https://chat.openai.com", path: "/backend-api/calpico" },
  ];

  function matchesTarget(input) {
    try {
      const url = new URL(input, location.href);
      for (const t of TARGETS) {
        if (url.origin === t.origin && url.pathname.startsWith(t.path)) return true;
      }
    } catch {}
    return false;
  }

  const __origFetch = window.fetch;
  window.fetch = async function (input, init) {
    // Silence Datadog beacons
    if (typeof input === "string" && input.includes("browser-intake-datadoghq.com"))
      return new Response(null, { status: 204 });

    const isTarget = matchesTarget(typeof input === "string" ? input : input?.url);
    const response = await __origFetch(input, init);
    if (!isTarget || !response.body) return response;

    const [toPage, toCapture] = response.body.tee();

    (async () => {
      const reader  = toCapture.getReader();
      const decoder = new TextDecoder();
      let   buffer  = "";
      try {
        while (true) {
          const { value, done } = await reader.read();
          if (done) break;
          const chunk = decoder.decode(value, { stream: true });
          if (chunk.includes("\n")) {
            buffer += chunk;
            const lines = buffer.split("\n");
            buffer = lines.pop();
            for (const line of lines)
              window.postMessage({ type: "INBOUND_MESSAGE", payload: line }, "*");
          } else {
            window.postMessage({ type: "INBOUND_MESSAGE", payload: chunk }, "*");
          }
        }
      } catch {}
    })();

    return new Response(toPage, {
      status:     response.status,
      statusText: response.statusText,
      headers:    response.headers,
    });
  };

  // ── Signal content.js that bridge is installed — wait for editor ─────────
  function signalReadyWhenEditorExists() {
    const editor = document.querySelector('div[contenteditable="true"]');
    if (editor) {
      console.log("[INJ] editor found, signalling BRIDGE_READY");
      window.postMessage({ type: "BRIDGE_READY" }, "*");
      return;
    }
    console.log("[INJ] editor not yet in DOM, waiting via MutationObserver");
    const observer = new MutationObserver(() => {
      const ed = document.querySelector('div[contenteditable="true"]');
      if (ed) {
        observer.disconnect();
        console.log("[INJ] editor appeared, signalling BRIDGE_READY");
        window.postMessage({ type: "BRIDGE_READY" }, "*");
      }
    });
    observer.observe(document.body, { childList: true, subtree: true });
  }
  signalReadyWhenEditorExists();

  // ── OUTBOUND_SUBMIT handler (Rust → ChatGPT input) ───────────────────────
  function waitForEditor(callback) {
    const editor = document.querySelector('div[contenteditable="true"]');
    if (editor) { callback(editor); return; }
    const observer = new MutationObserver(() => {
      const ed = document.querySelector('div[contenteditable="true"]');
      if (ed) { observer.disconnect(); callback(ed); }
    });
    observer.observe(document.body, { childList: true, subtree: true });
  }

  function waitForSendBtn(callback) {
    const btn = document.querySelector('button[data-testid="send-button"]');
    if (btn && !btn.disabled) { callback(btn); return; }
    const observer = new MutationObserver(() => {
      const b = document.querySelector('button[data-testid="send-button"]');
      if (b && !b.disabled) { observer.disconnect(); callback(b); }
    });
    observer.observe(document.body, { childList: true, subtree: true, attributes: true });
  }

  function submitViaEnter() {
    const editor = document.querySelector('div[contenteditable="true"]');
    if (!editor) return;

    editor.dispatchEvent(
      new KeyboardEvent("keydown", {
        key: "Enter",
        code: "Enter",
        which: 13,
        keyCode: 13,
        bubbles: true,
        cancelable: true
      })
    );
  }

  window.addEventListener("message", (event) => {
    if (event.source !== window) return;
    if (event.data?.type !== "OUTBOUND_SUBMIT") return;

    const { text, mode } = event.data.payload || {};
    if (typeof text !== "string") return;
    console.log("[INJ] OUTBOUND_SUBMIT received, text length:", text.length, "mode:", mode);

    window.__promptInjectionMode = mode || "auto";

    if (mode === "buffer") {
      window.__promptInjectionQueue ||= [];
      if (text) window.__promptInjectionQueue.push(text);
      const editor = document.querySelector('div[contenteditable="true"]');
      if (editor && editor.textContent !== "<PROMPT>") {
        editor.textContent = "<PROMPT>";
        editor.dispatchEvent(new Event("input", { bubbles: true }));
      }
      return;
    }

    // AUTO MODE
    if (text) {
      window.__pendingPromptInjection = text;
      const editor = document.querySelector('div[contenteditable="true"]');
      if (editor) {
        editor.textContent = "<PROMPT>";
        editor.dispatchEvent(new Event("input", { bubbles: true }));
      }

      setTimeout(() => {
        if (window.__promptInjectionQueue?.length > 0) {
          window.__pendingPromptInjection =
            window.__promptInjectionQueue.join("\n\n");
          window.__promptInjectionQueue = [];
        }

        const sendBtn =
          document.querySelector('button[data-testid="send-button"]');

        if (sendBtn && !sendBtn.disabled) {
          sendBtn.click();
        } else {
          submitViaEnter();
        }
      }, 100);
    }
  });
})();
