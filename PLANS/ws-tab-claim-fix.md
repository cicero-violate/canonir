# Analysis & Fix Plan: ws_server keeps opening new tabs instead of claiming existing ones

## What was fixed last time (now in code)
- `ws.onopen` calls `chrome.tabs.query` to send inventory ← **broken, see Bug 1**
- `CLAIM_TAB` sends URL, sets `tabOriginalUrls` ✓
- `TAB_CLAIMED` broadcast to other connections ✓
- `ws_server.rs` handles `TAB_CLAIMED` ✓

---

## Why it still doesn't work — three remaining bugs

---

### Bug 1 — `TAB_QUERY_PATTERNS` is undefined (CRITICAL)

In `ws.onopen`:
```js
chrome.tabs.query({ url: TAB_QUERY_PATTERNS }, (tabs) => {
```

`TAB_QUERY_PATTERNS` is **never defined** anywhere in background.js.
This throws a `ReferenceError` in the service worker, silently swallowed by the JS engine.
The `chrome.tabs.query` call never executes. No `TAB_READY` inventory messages are ever sent.
`preopened_tabs_by_url` stays empty. `claim_tab_for_url` always returns `None`.
The code falls through to `open_fresh_tab_with_url` every single time.

**Fix:** Inline the array literal (same one used at startup):
```js
ws.onopen = () => {
  chrome.tabs.query(
    { url: ["https://chatgpt.com/*", "https://chat.openai.com/*", "https://gemini.google.com/*"] },
    (tabs) => {
      for (const tab of tabs) {
        if (!tab?.id || !tab?.url) continue;
        const originalUrl = tabOriginalUrls.get(tab.id) ?? tab.url;
        send({ type: "TAB_READY", tabId: tab.id, url: tab.url, reqId: null, originalUrl });
      }
    }
  );
  while (queue.length) ws.send(queue.shift());
  ...
};
```

---

### Bug 2 — URL mismatch: tab navigates from `/gg/` to `/c/` and `tabOriginalUrls` is lost on restart

**How it happens:**

content.js sends `CONTENT_READY` with `location.href`. For a custom GPT tab that has
already had a conversation, ChatGPT SPA-navigates from
`https://chatgpt.com/gg/<id>` → `https://chatgpt.com/c/<chat-id>`.
So `tab.url` (and `location.href`) is the `/c/` URL, not the configured `/gg/` URL.

In `ws.onopen` inventory:
```js
const originalUrl = tabOriginalUrls.get(tab.id) ?? tab.url;
send({ type: "TAB_READY", ..., url: tab.url, originalUrl });
```

`tabOriginalUrls` is populated at OPEN_TAB and CLAIM_TAB time — but it's an **in-memory
Map** that is **cleared whenever the extension service worker restarts**. After a browser
restart or service worker eviction (Chrome evicts them after inactivity), `tabOriginalUrls`
is empty, so `originalUrl` falls back to `tab.url` (the `/c/` URL). The tab is stored in
`preopened_tabs_by_url` under the `/c/` URL only.

Meanwhile, Rust calls `claim_tab_for_url("https://chatgpt.com/gg/<id>")` — this key
doesn't exist in the map. Claim fails, new tab opens.

**Fix: Persist `tabOriginalUrls` in `chrome.storage.session` and restore on startup.**

`chrome.storage.session` persists across service worker restarts within the same browser
session (cleared only when the browser closes). This gives us the `/gg/` URLs back after
service worker eviction.

Changes to background.js:

```js
// On startup: restore tabOriginalUrls from session storage
chrome.storage.session.get("tabOriginalUrls", (result) => {
  if (result?.tabOriginalUrls) {
    for (const [k, v] of Object.entries(result.tabOriginalUrls)) {
      tabOriginalUrls.set(Number(k), v);
    }
  }
});

// Helper: persist tabOriginalUrls to session storage whenever it changes
function persistTabOriginalUrls() {
  const obj = {};
  for (const [k, v] of tabOriginalUrls) obj[String(k)] = v;
  chrome.storage.session.set({ tabOriginalUrls: obj });
}
```

Call `persistTabOriginalUrls()` after any write to `tabOriginalUrls`:
- After `tabOriginalUrls.set(newTabId, msg.url)` in OPEN_TAB handler
- After `tabOriginalUrls.set(targetTabId, msg.url)` in CLAIM_TAB handler
- After `tabOriginalUrls.delete(tabId)` in `onRemoved` listener

With this in place, after a service worker restart the `ws.onopen` inventory correctly
sends `originalUrl` as the `/gg/` URL even for tabs currently at a `/c/` URL.

---

### Bug 3 — Rust calls `claim_tab_for_url` before inventory messages are processed

Even with bugs 1 and 2 fixed, there is still a timing window:

1. WS connection established → Rust's `out_tx` set → `wait_for_connection()` returns
2. Simultaneously, JS `ws.onopen` fires, calls `chrome.tabs.query` (async callback)
3. Rust immediately calls `tab_manager_get_or_open_tab` → `claim_tab_for_url` →
   `preopened_tabs_by_url` is empty → claim fails → opens new tab
4. ...100–300ms later, `chrome.tabs.query` callback fires, sends TAB_READY messages
   → too late

The `wait_for_connection()` poll interval is 200ms. Rust returns from it almost
immediately after the WS connects. The JS `chrome.tabs.query` callback is async and
may take 50–300ms (depends on number of tabs and Chrome scheduler). The TAB_READY
messages also need to travel over loopback and be processed by `handle_inbound`.

**Fix: Add a grace-period claim retry in `tab_manager_get_or_open_tab`.**

Instead of immediately falling through to `open_fresh_tab_with_url` after a failed claim,
poll `claim_tab_for_url` for up to ~1500ms before giving up:

```rust
// tab_management.rs  tab_manager_get_or_open_tab
pub async fn tab_manager_get_or_open_tab(...) -> Result<u32> {
    tab_manager_wait_endpoint_cooldown(endpoint_id, tabs).await;
    if let Some(id) = tab_manager_get_owner_tab(endpoint_id, tabs).await {
        return Ok(id);
    }
    bridge.wait_for_connection().await;

    // Grace period: poll claim_tab_for_url for up to 1500ms
    // to allow the inventory TAB_READY messages from ws.onopen to arrive.
    const CLAIM_POLL_INTERVAL_MS: u64 = 100;
    const CLAIM_POLL_MAX_MS: u64 = 1500;
    let poll_start = std::time::Instant::now();
    loop {
        if let Some(id) = bridge.claim_tab_for_url(url).await {
            tab_manager_set_tab_id(endpoint_id, id, tabs, _max_tabs).await;
            tab_manager_mark_tab_in_flight(tabs, id, true).await;
            tab_manager_log_llm(format!("endpoint={} claimed_tab={} url={}", endpoint_id, id, url));
            return Ok(id);
        }
        if poll_start.elapsed().as_millis() as u64 >= CLAIM_POLL_MAX_MS {
            break;
        }
        tokio::time::sleep(std::time::Duration::from_millis(CLAIM_POLL_INTERVAL_MS)).await;
    }

    // No pre-opened tab found — open a fresh one.
    tab_manager_log_llm(format!("endpoint={} opening_new_tab url={}", endpoint_id, url));
    let open = bridge.open_fresh_tab_with_url(url.to_string());
    let id = match tokio::time::timeout(std::time::Duration::from_secs(20), open).await {
        Ok(Ok(id)) => id,
        Ok(Err(e)) => return Err(anyhow::anyhow!("failed to open tab: {e}")),
        Err(_) => return Err(anyhow::anyhow!("open tab timeout")),
    };
    tab_manager_set_tab_id(endpoint_id, id, tabs, _max_tabs).await;
    tab_manager_mark_tab_in_flight(tabs, id, true).await;
    Ok(id)
}
```

The 1500ms grace period is long enough for `chrome.tabs.query` to complete and TAB_READY
messages to arrive, but short enough not to significantly delay startup when there truly
are no existing tabs (the fallback opens a fresh tab after 1.5s in that case).

---

## Summary of all remaining fixes

| # | File | Change |
|---|---|---|
| 1 | `background.js` `ws.onopen` | Replace `TAB_QUERY_PATTERNS` with inline array literal |
| 2 | `background.js` | Add `chrome.storage.session` persistence for `tabOriginalUrls`; restore on startup; call `persistTabOriginalUrls()` after every write/delete |
| 3 | `tab_management.rs` `tab_manager_get_or_open_tab` | Add 1500ms grace-period poll loop around `claim_tab_for_url` before calling `open_fresh_tab_with_url` |

---

## Full corrected `ws.onopen` block

```js
ws.onopen = () => {
  console.log(`[BG] WS connected to ${url}`);
  chrome.tabs.query(
    { url: ["https://chatgpt.com/*", "https://chat.openai.com/*", "https://gemini.google.com/*"] },
    (tabs) => {
      for (const tab of tabs) {
        if (!tab?.id || !tab?.url) continue;
        const originalUrl = tabOriginalUrls.get(tab.id) ?? tab.url;
        send({ type: "TAB_READY", tabId: tab.id, url: tab.url, reqId: null, originalUrl });
      }
    }
  );
  while (queue.length) ws.send(queue.shift());
  if (pingInterval) clearInterval(pingInterval);
  pingInterval = setInterval(() => {
    if (ws && ws.readyState === WebSocket.OPEN) ws.send(JSON.stringify({ type: "PING" }));
  }, 20000);
};
```

## Full corrected `tab_manager_get_or_open_tab` flow

```
1. wait endpoint cooldown
2. check owner tab (already assigned) → return if found
3. wait for WS connection
4. LOOP up to 1500ms every 100ms:
     claim_tab_for_url(url) → return if claimed
5. open_fresh_tab_with_url(url) → wait 20s → return
```

## `tabOriginalUrls` persistence additions

```js
// At top of background.js (after tabOriginalUrls Map declaration):
chrome.storage.session.get("tabOriginalUrls", (result) => {
  if (result?.tabOriginalUrls) {
    for (const [k, v] of Object.entries(result.tabOriginalUrls)) {
      tabOriginalUrls.set(Number(k), v);
    }
  }
});

function persistTabOriginalUrls() {
  const obj = {};
  for (const [k, v] of tabOriginalUrls) obj[String(k)] = v;
  chrome.storage.session.set({ tabOriginalUrls: obj });
}

// Call persistTabOriginalUrls() after:
// - tabOriginalUrls.set(newTabId, msg.url)  in OPEN_TAB handler
// - tabOriginalUrls.set(targetTabId, msg.url) in CLAIM_TAB handler
// - tabOriginalUrls.delete(tabId)  in onRemoved listener
```
