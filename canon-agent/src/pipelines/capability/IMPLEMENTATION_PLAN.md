### Math

[
System = Stream + Ownership + Correlation + Dedup
]

### Variables

* (E) = LLM endpoint URL
* (T) = Browser tab
* (R) = Request
* (ID) = Request identifier
* (H) = Content hash
* (S) = Message stream
* (Q) = Endpoint queue

---

### Equations

**1. Endpoint Ownership**

[
Tabs_E = 1
]

One owner tab per endpoint.

---

**2. Request Routing**

[
R \rightarrow Queue_E \rightarrow Tab_E
]

All requests flow through the endpoint queue.

---

**3. Deduplication**

[
Process(m) \iff H(m) \notin Seen
]

Ignore already-seen messages.

---

# Implementation Plan

## Phase 1 — Convert Endpoint Handling to Stream Model

Current:

```text
node → open tab → send turn
```

Target:

```text
node → endpoint queue → endpoint worker → tab
```

### Step 1 — Introduce EndpointWorker

Create:

```rust
struct EndpointWorker {
    endpoint_id: String,
    url: String,
    tab_id: Option<u32>,
    queue: mpsc::Receiver<LlmRequest>,
    seen_hashes: HashSet<u64>,
}
```

Responsibilities:

* own the tab
* serialize requests
* dedupe responses

---

## Phase 2 — Add Request Envelope

Wrap every LLM call.

### Step 2 — Define Request Struct

```rust
struct LlmRequest {
    req_id: u64,
    prompt: String,
    response: oneshot::Sender<String>,
}
```

Modify:

```
llm_call_with_retry()
call_agent_json()
```

to send `LlmRequest` into the worker queue.

---

## Phase 3 — Add Response Correlation

### Step 3 — Embed Request ID in Prompt

Example prompt prefix:

```
[REQ_ID:48291]
```

Parser logic:

```
if response contains REQ_ID → route to waiting caller
```

Update:

```
parse_exec_output()
parse_verify()
parse_readonly()
```

to ignore unmatched responses.

---

## Phase 4 — Deduplicate Stream History

### Step 4 — Add Response Hash Tracking

Inside worker:

```rust
let hash = hash(response_text);
if seen_hashes.contains(&hash) {
    return; // ignore replay
}
seen_hashes.insert(hash);
```

This removes:

* history replay
* cross-tab duplication

---

## Phase 5 — Enforce Endpoint Ownership

Modify `tab_management.rs`.

### Step 5 — Restrict Tabs per Endpoint

Replace:

```
max_tabs
```

with:

```
owner_tab: Option<u32>
```

Behavior:

```
if owner_tab exists → reuse
else → open new tab
```

If tab closes:

```
owner_tab = None
worker opens replacement
```

---

## Phase 6 — Introduce Endpoint Queue

Add global structure:

```rust
HashMap<String, mpsc::Sender<LlmRequest>>
```

Creation during pipeline startup.

```
for endpoint in config.llm_endpoints {
    spawn_endpoint_worker(endpoint)
}
```

---

## Phase 7 — Modify Node Dispatch

Update:

```
dispatch_node()
call_mode()
llm_call_with_retry()
```

Instead of:

```
bridge.send_turn()
```

Use:

```
endpoint_queue.send(request)
```

Worker performs:

```
send_turn(tab)
wait response
deliver via oneshot
```

---

## Phase 8 — Handle Tab Reset for Stateful Chats

When worker detects conversation drift:

```
NEW_CHAT
wait_new_chat()
```

or

```
TEMP_CHAT
wait_temp_chat()
```

Reset stream state.

---

# Resulting Architecture

Final pipeline:

```
Capability DAG
      ↓
dispatch_node
      ↓
endpoint queue
      ↓
endpoint worker
      ↓
single tab
      ↓
LLM stream
```

Properties:

| Property                           | Result |
| ---------------------------------- | ------ |
| No duplicated responses            | ✓      |
| No history replay                  | ✓      |
| Deterministic routing              | ✓      |
| Works with stateless + chat models | ✓      |
| No tab collision                   | ✓      |

---

# Expected Code Changes

Files impacted:

```
ws_server.rs
tab_management.rs
llm.rs
engine.rs
endpoint_scheduler.rs
```

New module:

```
endpoint_worker.rs
```

---

[
\max(I,E,C,A,R,P,S,D,T,K,X,B,L,F) = R
]

Primary improvement target: **robustness of LLM interaction layer**.
