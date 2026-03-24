# Repair Plan: LLM Bridge — Reliable Prompt Injection on ChatGPT

## Root-cause analysis

### Bug 1 — `temp_chat` races with the next TURN dispatch (endpoint_worker.rs:142-151)

After every non-stateful ChatGPT turn the worker does:
```
new_chat(tab_id)      → waits → navigates tab to "/"
temp_chat(tab_id)     → clicks "Turn on temporary chat" button
                        → button click MAY redirect to "/?temporary-chat=true"
                        → page is mid-redirect
                        → wait_temp_chat times out or completes
```
The next `send_turn` call arrives while the page is still in the redirect.
`inject.js` has not re-injected yet — the `OUTBOUND_SUBMIT` listener is gone.
The TURN is delivered but the send never fires; `send_turn` eventually times out.

### Bug 2 — `new_chat` destroys custom GPT context (inject.js:246-248)

`clickNewChat()` always navigates to `href="/"`. The fallback is
`location.href = "/"`. For a custom GPT tab opened at
`https://chatgpt.com/gg/<id>`, `new_chat` lands the tab at `https://chatgpt.com/`.

After this:
- `content.js` re-runs for the `/` URL → injects `request_hook_private.js`
- The next TURN arrives → editor is the standard ChatGPT editor, NOT the GPT editor
- The API call goes to `/backend-api/f/conversation` (standard model) instead of
  `/backend-api/calpico/chatgpt/rooms` (Calpico/custom GPT protocol)
- The custom GPT system prompt, tools, and identity are completely lost

### Bug 3 — `request_hook_private.js` drops `fetch(Request)` calls

The private hook only handles `fetch(url, init)` where `init.body` is a string
(line 55). It has no case for `fetch(new Request(url, {body}))`.
`request_hook_group.js` handles both forms (see its `input instanceof Request`
block). If ChatGPT internal code changes to the Request-form, injection silently
fails with no error.

### Bug 4 — `__promptInjectionQueue` not cleared in private hook auto mode

After injecting in auto mode, `request_hook_private.js` sets
`window.__pendingPromptInjection = null` but does NOT reset
`window.__promptInjectionQueue = []`. The group hook clears both. A stale
queue leaks into the next turn.

### Bug 5 — inject.js AUTO mode: 100ms fixed delay is fragile

```js
setTimeout(() => {
    sendBtn?.click() ?? submitViaEnter();
}, 100);
```
If the editor input event causes ChatGPT to briefly disable the send button
(e.g., while it validates input), 100 ms may not be enough. The submit fires
on a disabled button or falls through to `submitViaEnter()` which dispatches
a raw `keydown Enter` event that ChatGPT may ignore.

---

## Repair plan

Five targeted changes across three files. No new message types, no ws_server.rs changes.

---

### Fix 1 — `endpoint_worker.rs`: Remove `temp_chat`; fix custom GPT reset

**File:** `canon-utils/canon-llm-runtime/src/endpoint_worker.rs`

Lines 132-151 — the non-stateful ChatGPT cleanup block.

```rust
// BEFORE (lines 132-151):
} else if !self.stateful && is_chatgpt_url(&self.url) {
    let _ = self.bridge.new_chat(tab_id).await;
    match self.bridge.wait_new_chat(tab_id, 20).await {
        Ok(()) => tab_manager_log_llm(format!("phase={} endpoint={} tab={} new_chat_done", ...)),
        Err(e) => {
            tab_manager_mark_tab_in_flight(&self.tabs, tab_id, true).await;
            tab_manager_log_llm(format!("phase={} endpoint={} tab={} new_chat_timeout={}", ...));
            return Err(anyhow::anyhow!("new_chat timeout"));
        }
    }
    let _ = self.bridge.temp_chat(tab_id).await;                    // ← remove this
    match self.bridge.wait_temp_chat(tab_id, 20).await {            // ← remove this
        Ok(()) => tab_manager_log_llm(format!("... temp_chat_done")), // ← remove this
        Err(e) => {                                                   // ← remove this
            tab_manager_mark_tab_in_flight(&self.tabs, tab_id, true).await; // ← remove
            tab_manager_log_llm(format!("... temp_chat_timeout={}",e)); // ← remove this
            return Err(anyhow::anyhow!("temp_chat timeout"));         // ← remove this
        }                                                             // ← remove this
    }                                                                 // ← remove this
}

// AFTER:
} else if !self.stateful && is_chatgpt_url(&self.url) {
    let _ = self.bridge.new_chat(tab_id).await;
    match self.bridge.wait_new_chat(tab_id, 20).await {
        Ok(()) => tab_manager_log_llm(format!("phase={} endpoint={} tab={} new_chat_done", phase, self.endpoint_id, tab_id)),
        Err(e) => {
            tab_manager_mark_tab_in_flight(&self.tabs, tab_id, true).await;
            tab_manager_log_llm(format!("phase={} endpoint={} tab={} new_chat_timeout={}", phase, self.endpoint_id, tab_id, e));
            return Err(anyhow::anyhow!("new_chat timeout"));
        }
    }
    // temp_chat removed: the UI redirect races with the next TURN dispatch.
    // new_chat alone is sufficient to reset the conversation context.
}
```

**Why:** `temp_chat` adds a navigation to `?temporary-chat=true` with no benefit
for an automated agent (there is no human who would see the chat history).
`new_chat` already resets the conversation. The redirect caused by `temp_chat`
puts the page mid-flight when the next TURN arrives, silently swallowing the prompt.

---

### Fix 2 — `background.js`: Track original tab URL; route `NEW_CHAT` correctly for custom GPTs

**File:** `canon-chromium-extension/background.js`

**2a.** Add a map at the top of the file, alongside `pendingOpenReqIds`:

```js
// BEFORE (after line 11):
const pendingOpenReqIds = new Map();

// AFTER:
const pendingOpenReqIds = new Map();
// tabId → URL the tab was originally opened with (from OPEN_TAB).
// Used to navigate back to the correct URL on NEW_CHAT for custom GPT tabs.
const tabOriginalUrls = new Map();
// tabId → true when a navigate-back NEW_CHAT is in flight; resolved on CONTENT_READY.
const pendingNewChatNavigations = new Map();
```

**2b.** In the `OPEN_TAB` handler (inside `handleRustMessage`), store the original URL:

```js
// BEFORE (line 130-131):
chrome.tabs.create({ url: msg.url, active: false }, (tab) => {
    if (!tab?.id) return;
    const newTabId = tab.id;

// AFTER:
chrome.tabs.create({ url: msg.url, active: false }, (tab) => {
    if (!tab?.id) return;
    const newTabId = tab.id;
    tabOriginalUrls.set(newTabId, msg.url);   // ← add this line
```

**2c.** In the `NEW_CHAT` handler (inside `handleRustMessage`), intercept custom GPT tabs:

```js
// BEFORE (line 158-163):
if (msg?.type === "NEW_CHAT") {
    const targetTabId = msg.tabId;
    if (!targetTabId) return;
    sendToTab(targetTabId, { type: "NEW_CHAT" });
    return;
}

// AFTER:
if (msg?.type === "NEW_CHAT") {
    const targetTabId = msg.tabId;
    if (!targetTabId) return;
    const originalUrl = tabOriginalUrls.get(targetTabId) ?? "";
    const isCustomGpt =
        originalUrl.includes("/gg/") ||
        originalUrl.includes("chatgpt.com/g/");
    if (isCustomGpt) {
        // For custom GPTs: navigate back to the original GPT URL rather than
        // sending NEW_CHAT to inject.js (which would navigate to "/" and destroy
        // the GPT context). Resolution comes via CONTENT_READY below.
        pendingNewChatNavigations.set(targetTabId, true);
        chrome.tabs.update(targetTabId, { url: originalUrl }, () => void chrome.runtime.lastError);
        return;
    }
    // Default ChatGPT: delegate to inject.js clickNewChat() as before.
    sendToTab(targetTabId, { type: "NEW_CHAT" });
    return;
}
```

**2d.** In the `CONTENT_READY` handler, check if this is a navigate-back resolution:

```js
// BEFORE (line 100-106):
if (message?.type === "CONTENT_READY") {
    const reqId = pendingOpenReqIds.get(tabId) ?? null;
    pendingOpenReqIds.delete(tabId);
    sendToRust({ type: "TAB_READY", tabId, url: message.url, reqId });
    sendResponse({ ok: true });
    return true;
}

// AFTER:
if (message?.type === "CONTENT_READY") {
    // Case A: this is the resolution of a custom GPT navigate-back.
    if (pendingNewChatNavigations.get(tabId)) {
        pendingNewChatNavigations.delete(tabId);
        console.log("[BG] CONTENT_READY after navigate-back, sending NEW_CHAT_DONE", { tabId });
        sendToRust({ type: "NEW_CHAT_DONE", tabId });
        sendResponse({ ok: true });
        return true;
    }
    // Case B: normal open-tab flow.
    const reqId = pendingOpenReqIds.get(tabId) ?? null;
    pendingOpenReqIds.delete(tabId);
    sendToRust({ type: "TAB_READY", tabId, url: message.url, reqId });
    sendResponse({ ok: true });
    return true;
}
```

**2e.** Clean up tracking maps when a tab is removed (after line 209):

```js
// BEFORE:
chrome.tabs.onRemoved.addListener((tabId) => {
    pendingOpenReqIds.delete(tabId);
    sendToRust({ type: "TAB_CLOSED", tabId });
});

// AFTER:
chrome.tabs.onRemoved.addListener((tabId) => {
    pendingOpenReqIds.delete(tabId);
    tabOriginalUrls.delete(tabId);
    pendingNewChatNavigations.delete(tabId);
    sendToRust({ type: "TAB_CLOSED", tabId });
});
```

**Why:** `new_chat` in inject.js unconditionally navigates to `href="/"` or
`location.href="/"`. For a tab that was opened at `chatgpt.com/gg/<id>`, this
destroys the GPT context permanently. Navigating back to the original URL gives
a fresh session on the correct GPT — exactly what non-stateful reset requires.

---

### Fix 3 — `inject.js`: Guard `TEMP_CHAT` for custom GPT paths

**File:** `canon-chromium-extension/inject.js`

**3a.** In the `TEMP_CHAT` message handler (line 280):

```js
// BEFORE (line 280-295):
if (event.data?.type === "TEMP_CHAT") {
    const deadline = Date.now() + 10000;
    const tryEnable = () => {
        if (isTempChatEnabled()) {
            window.postMessage({ type: "TEMP_CHAT_DONE" }, "*");
            return;
        }
        clickTempChat();
        if (Date.now() < deadline) {
            setTimeout(tryEnable, 300);
        } else {
            window.postMessage({ type: "TEMP_CHAT_DONE" }, "*");
        }
    };
    tryEnable();
}

// AFTER:
if (event.data?.type === "TEMP_CHAT") {
    // Custom GPT tabs (/gg/) do not support temporary chat mode.
    // Acknowledge immediately so Rust does not time out.
    if (location.pathname.startsWith("/gg/")) {
        window.postMessage({ type: "TEMP_CHAT_DONE" }, "*");
        return;
    }
    const deadline = Date.now() + 10000;
    const tryEnable = () => {
        if (isTempChatEnabled()) {
            window.postMessage({ type: "TEMP_CHAT_DONE" }, "*");
            return;
        }
        clickTempChat();
        if (Date.now() < deadline) {
            setTimeout(tryEnable, 300);
        } else {
            // Timed out — acknowledge anyway so Rust is not blocked.
            window.postMessage({ type: "TEMP_CHAT_DONE" }, "*");
        }
    };
    tryEnable();
}
```

**3b.** Replace the 100 ms fixed-delay send with `waitForSendBtn`:

```js
// BEFORE (lines 191-207, inside OUTBOUND_SUBMIT AUTO handler):
setTimeout(() => {
    if (window.__promptInjectionQueue?.length > 0) {
        window.__pendingPromptInjection = window.__promptInjectionQueue.join("\n\n");
        window.__promptInjectionQueue = [];
    }
    const sendBtn = document.querySelector('button[data-testid="send-button"]');
    if (sendBtn && !sendBtn.disabled) {
        sendBtn.click();
    } else {
        submitViaEnter();
    }
}, 100);

// AFTER:
// Merge queue synchronously (before observing — no race).
if (window.__promptInjectionQueue?.length > 0) {
    window.__pendingPromptInjection = window.__promptInjectionQueue.join("\n\n");
    window.__promptInjectionQueue = [];
}
// Wait for send button to be enabled rather than using a fixed delay.
waitForSendBtn((btn) => {
    btn.click();
});
```

**Why 3b:** `waitForSendBtn` uses a `MutationObserver` (already defined) and
only fires when the button exists AND is not disabled. The 100 ms heuristic
fails silently whenever ChatGPT's input validation takes longer (e.g., after
a page navigation or when temp_chat is active).

---

### Fix 4 — `request_hook_private.js`: Handle `fetch(Request)` form

**File:** `canon-chromium-extension/request_hook_private.js`

Add a `fetch(Request)` case at the top of the `window.fetch` override, mirroring
`request_hook_group.js`:

```js
// BEFORE (line 34):
window.fetch = async function(input, init) {
    const isTarget = matchesTarget(typeof input === 'string' ? input : input?.url);

// AFTER:
window.fetch = async function(input, init) {
    // ── Case A: fetch(new Request(url, init)) form ───────────────────────────
    if (input instanceof Request && matchesTarget(input.url) && input.method === 'POST') {
        const hasPending = window.__pendingPromptInjection ||
                           window.__promptInjectionQueue?.length > 0;
        if (hasPending) {
            try {
                const text = await input.clone().text();
                if (text) {
                    const payload = JSON.parse(text);
                    const msgs = payload?.messages;
                    if (Array.isArray(msgs) && msgs.length > 0) {
                        const lastMsg = msgs[msgs.length - 1];
                        const parts = lastMsg?.content?.parts;
                        if (Array.isArray(parts) &&
                            parts.some(p => typeof p === 'string' && p.includes('<PROMPT>'))) {
                            const combined = [
                                ...window.__promptInjectionQueue,
                                ...(window.__pendingPromptInjection ? [window.__pendingPromptInjection] : [])
                            ].join('\n\n');
                            lastMsg.content.parts = [combined];
                            window.__promptInjectionQueue = [];
                            if (window.__promptInjectionMode === 'auto') {
                                window.__pendingPromptInjection = null;
                            }
                            console.log('[RequestHookPrivate] ✅ INJECTED (Request form)');
                            return originalFetch(new Request(input, { body: JSON.stringify(payload) }));
                        }
                    }
                }
            } catch (e) {
                console.warn('[RequestHookPrivate] fetch(Request) parse failed', e);
            }
        }
        return originalFetch(input);
    }

    // ── Case B: fetch(url, init) form (original code below) ─────────────────
    const isTarget = matchesTarget(typeof input === 'string' ? input : input?.url);
```

**Fix 4b.** Also clear `__promptInjectionQueue` in the existing auto-mode block
(line 84-86):

```js
// BEFORE:
if (window.__promptInjectionMode === "auto") {
    window.__pendingPromptInjection = null;
}

// AFTER:
if (window.__promptInjectionMode === "auto") {
    window.__pendingPromptInjection = null;
    window.__promptInjectionQueue = [];    // ← add this
}
```

---

### Fix 5 — `inject.js`: `BRIDGE_READY` re-arm after redirect

If the page navigates (e.g., URL changes to a new custom GPT session),
`signalReadyWhenEditorExists()` runs once on inject. This is fine for the
initial load. But after a `chrome.tabs.update(url)` navigation (from Fix 2),
the page fully reloads and inject.js re-injects, so `signalReadyWhenEditorExists`
fires again automatically. No change needed here — it already works.

However: the `MutationObserver` in `signalReadyWhenEditorExists` observes
`document.body`. If the script injects before `document.body` exists (very
early injection timing), the observer can't attach. Add a null guard:

```js
// BEFORE (line 116-120):
const observer = new MutationObserver(() => {
    const ed = document.querySelector('div[contenteditable="true"]');
    if (ed) {
        observer.disconnect();
        console.log("[INJ] editor appeared, signalling BRIDGE_READY");
        window.postMessage({ type: "BRIDGE_READY" }, "*");
    }
});
observer.observe(document.body, { childList: true, subtree: true });

// AFTER:
const observeTarget = document.body || document.documentElement;
const observer = new MutationObserver(() => {
    const ed = document.querySelector('div[contenteditable="true"]');
    if (ed) {
        observer.disconnect();
        console.log("[INJ] editor appeared, signalling BRIDGE_READY");
        window.postMessage({ type: "BRIDGE_READY" }, "*");
    }
});
observer.observe(observeTarget, { childList: true, subtree: true });
```

---

## Summary table

| # | File | Lines changed | Root cause fixed |
|---|---|---|---|
| 1 | `endpoint_worker.rs` | Remove lines 142-151 (`temp_chat` block) | Bug 1: redirect races with next TURN |
| 2a–2e | `background.js` | ~30 lines added/modified | Bug 2: `new_chat` destroys custom GPT context |
| 3a | `inject.js` | 2 lines added at top of TEMP_CHAT handler | Bug 1 (extension-side guard) |
| 3b | `inject.js` | Replace `setTimeout(100)` with `waitForSendBtn` | Bug 5: fragile send timing |
| 4a | `request_hook_private.js` | ~25 lines added (fetch Request case) | Bug 3: Request-form fetch drops injection |
| 4b | `request_hook_private.js` | 1 line added (clear queue in auto mode) | Bug 4: stale queue |
| 5 | `inject.js` | 1 line (`document.body || document.documentElement`) | Defensive: early injection guard |

## What does NOT change

- `ws_server.rs` — no new message types, no ServerState changes
- `WsBridge` public API — `new_chat`/`wait_new_chat` used as before
- `temp_chat`/`wait_temp_chat` methods on `WsBridge` — kept for future use but not called
- `parsers.rs`, `tab_management.rs`, `llm.rs` — no changes
- Stateful endpoint flow — entirely unaffected (neither `new_chat` nor `temp_chat` run for stateful)

## Execution order for codex

1. `endpoint_worker.rs` Fix 1 (remove temp_chat block)
2. `background.js` Fix 2 (all 5 sub-fixes a–e, in order)
3. `inject.js` Fix 3a (TEMP_CHAT guard)
4. `inject.js` Fix 3b (replace setTimeout with waitForSendBtn)
5. `inject.js` Fix 5 (document.body null guard)
6. `request_hook_private.js` Fix 4a (fetch Request case)
7. `request_hook_private.js` Fix 4b (clear queue in auto mode)
