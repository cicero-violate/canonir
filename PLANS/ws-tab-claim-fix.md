# Analysis & Fix Plan: ws_server keeps opening new tabs instead of claiming existing ones

## Root cause chain (4 bugs)

---

### Bug 1 — Race: `wait_for_connection` returns before `TAB_READY` is delivered (PRIMARY BUG)

**Trace:**

1. Rust process starts. `ServerState::new()` → `preopened_tabs_by_url` is empty.
2. Extension `ws.onopen` fires. Rust's `wait_for_connection()` returns immediately.
3. Rust calls `tab_manager_get_or_open_tab` → `claim_tab_for_url(url)` → checks
   `preopened_tabs_by_url` → **empty** → falls through to `open_fresh_tab_with_url`.
4. Meanwhile (100–500ms later), `chrome.tabs.query` + `executeScript` at extension
   startup re-injects content scripts into existing ChatGPT tabs. Those content scripts
   send `CONTENT_READY` → background.js converts to `TAB_READY` (no reqId) → Rust
   adds to `preopened_tabs_by_url`. But it's too late — Rust already opened a new tab.

**Why `ws.onopen` sends nothing about existing tabs:**

```js
ws.onopen = () => {
  console.log(`[BG] WS connected to ${url}`);
  while (queue.length) ws.send(queue.shift());  // drain buffered messages only
  // ← NO tab inventory sent here
  ...
};
```

The extension never announces "here are the tabs I already have open" when connecting.

**Fix: Send a `TAB_INVENTORY` (or individual `TAB_READY`) for each already-known tab
in `ws.onopen`, BEFORE draining the queue.** The extension already has `tabWsOwner` and
`tabOriginalUrls` populated from any previous session. Enumerate all open ChatGPT/Gemini
tabs synchronously via `chrome.tabs.query` in `ws.onopen` and send `TAB_READY` for each,
with `reqId: null` so they land in `preopened_tabs_by_url`.

Alternatively: add a short grace-period wait in `tab_manager_get_or_open_tab` between
checking `claim_tab_for_url` (returns None) and calling `open_fresh_tab_with_url`, giving
the TAB_READY messages time to arrive. Something like:

```rust
// claim_tab_for_url returned None — wait briefly for TAB_READY to arrive
if let Some(id) = try_claim_with_grace_period(bridge, url, 800).await {
    // got it
} else {
    // open fresh
}
```

The ws.onopen fix is cleaner and eliminates the race entirely.

---

### Bug 2 — `CLAIM_TAB` doesn't set `tabOriginalUrls` in background.js

When Rust claims a pre-opened tab via `claim_tab_for_url`, the server sends:
```json
{ "type": "CLAIM_TAB", "tabId": 123 }
```

background.js handles it:
```js
if (msg?.type === "CLAIM_TAB") {
  const targetTabId = msg.tabId;
  tabWsOwner.set(targetTabId, sendFn);   // ownership set ✓
  // ← tabOriginalUrls NEVER set for claimed tabs ✗
  return;
}
```

Consequence: when `NEW_CHAT` is later sent for a custom GPT tab (url contains `/gg/`),
the navigate-back logic looks up `tabOriginalUrls.get(targetTabId)` → `undefined` →
`originalUrl` is empty → `isCustomGpt` is false → falls through to
`sendToTab(targetTabId, { type: "NEW_CHAT" })` (the in-page new-chat path, which may
not work on custom GPT pages).

**Fix: Rust must include the URL in the `CLAIM_TAB` message**, and background.js must
set `tabOriginalUrls` on receipt:

```rust
// ws_server.rs claim_tab_for_url
st.send(json!({ "type": "CLAIM_TAB", "tabId": tab_id, "url": url }))
```

```js
// background.js CLAIM_TAB handler
if (msg?.type === "CLAIM_TAB") {
  tabWsOwner.set(msg.tabId, sendFn);
  if (msg.url) tabOriginalUrls.set(msg.tabId, msg.url);
}
```

---

### Bug 3 — Unowned `TAB_READY` is broadcast to ALL mini-agent connections

In background.js `CONTENT_READY` handler:

```js
const owner = tabWsOwner.get(tabId);
if (owner) {
  owner(payload);
} else {
  runtimeConn.send(payload);          // goes to canon-loop
  for (const conn of miniAgentConns) {
    conn.send(payload);               // goes to EVERY mini-agent instance
  }
}
```

Every connected mini-agent instance receives the same `TAB_READY` for every unowned tab.
Each calls `claim_tab_for_url` — only one wins (pops the queue first). The others see
nothing in their `preopened_tabs_by_url` and fall through to `open_fresh_tab_with_url`,
causing N−1 duplicate new tabs to be opened.

**Fix:** The first instance to `CLAIM_TAB` wins. But the others must be told not to open
a new tab. Two options:

**Option A (preferred):** When background.js receives `CLAIM_TAB`, broadcast a
`TAB_CLAIMED { tabId }` message to all OTHER connections so they can evict that tabId
from their `preopened_tabs_by_url` queues.

```js
// background.js
if (msg?.type === "CLAIM_TAB") {
  tabWsOwner.set(msg.tabId, sendFn);
  if (msg.url) tabOriginalUrls.set(msg.tabId, msg.url);
  // notify all other connections that this tab is taken
  for (const conn of [runtimeConn, ...miniAgentConns]) {
    if (conn.send !== sendFn) conn.send({ type: "TAB_CLAIMED", tabId: msg.tabId });
  }
}
```

ws_server.rs handles `TAB_CLAIMED` by removing the tabId from `preopened_tabs_by_url`.

**Option B:** Only broadcast `TAB_READY` (no owner) to a single connection — whichever
is the "primary" one. But this doesn't work cleanly with multiple equal-priority mini-agents.

---

### Bug 4 — `llm_worker_init_workers` creates workers that are never reused

In `endpoint_worker.rs`, the worker cache key is:

```rust
// in llm_worker_init_workers (uses full URL Vec from config):
let worker_key = (endpoint.id.clone(), endpoint.url.clone(), endpoint.stateful, ptr);
// e.g. ("exec_pool", ["url1","url2","url3",...], true, 0x...)

// in llm_worker_send_request_with_req_id_timeout (single picked URL):
let worker_key = (endpoint_id.to_string(), vec![url.to_string()], stateful, ptr);
// e.g. ("exec_pool", ["url2"], true, 0x...)
```

These keys never match (`vec!["url1","url2"]` ≠ `vec!["url2"]`), so:
- `llm_worker_init_workers` pre-warms workers that are **immediately orphaned**
- Every `send_request` call creates a brand-new worker for the single picked URL instead

This means stateful workers never share tab state across requests, defeating the point
of the `tabs_with_role_sent` set (system prompt only sent once per tab).

**Fix:** Make both sites use the same key construction. The cleanest approach: the worker
key should use the **endpoint_id + tabs pointer only** (not the URL list), and the worker
owns the full URL Vec. Then `pick_url` happens inside the worker, not at the call site.

```rust
type WorkerKey = (String, bool, usize);  // (endpoint_id, stateful, tabs_ptr)
```

The `url` parameter to `llm_worker_send_request` becomes the full `Vec<String>` from
the endpoint config, and the worker selects from it per-request. All callers for the
same endpoint then reuse the same worker and its tab state.

---

## Fix summary table

| # | Location | Problem | Fix |
|---|---|---|---|
| 1 | `background.js` `ws.onopen` | No tab inventory sent on connect | Send `TAB_READY` for all open ChatGPT tabs in `ws.onopen` before draining queue |
| 2 | `background.js` `CLAIM_TAB` handler | `tabOriginalUrls` not set for claimed tabs | Include `url` in `CLAIM_TAB` message; set `tabOriginalUrls` in handler |
| 3 | `background.js` `CONTENT_READY` handler | Unowned `TAB_READY` broadcast to all instances | Broadcast `TAB_CLAIMED` to all other connections when a claim is made |
| 4 | `endpoint_worker.rs` worker key | `init_workers` key ≠ `send_request` key | Unify key to `(endpoint_id, stateful, tabs_ptr)`; pass full URL Vec to worker |

---

## Files to change

- `canon-chromium-extension/background.js`
  - `ws.onopen`: enumerate existing tabs and send `TAB_READY` for each
  - `CLAIM_TAB` handler: set `tabOriginalUrls`, broadcast `TAB_CLAIMED` to other connections
- `canon-utils/canon-llm-runtime/src/ws_server.rs`
  - `claim_tab_for_url`: include `url` in the `CLAIM_TAB` message
  - `handle_inbound`: add `TAB_CLAIMED` handler that removes tabId from `preopened_tabs_by_url`
- `canon-utils/canon-llm-runtime/src/endpoint_worker.rs`
  - Unify `WorkerKey` to not include URL list
  - Change `llm_worker_send_request*` signatures to accept `Vec<String>` URLs or let the worker own URL selection
