### Equations

**1. Execution Load**

[
Load = N \times C
]

More nodes × concurrency → more tab pressure.

---

**2. Stream Collision**

[
Collision = Tabs_{same_url} \times Shared_Chat
]

Multiple tabs on same chat stream cause duplicated responses.

---

**3. Deterministic Execution**

[
Determinism = Queue + Ownership + Dedup
]

Single routing path removes race conditions.

---

# Architectural Observations

Your system already has strong components:

* `TaskGraph` → deterministic execution model
* `AuthorityContext` → capability gating
* `DispatchMode` → execution routing
* `TabSlots` → tab resource manager
* `EndpointScheduler` → endpoint selection

But **the LLM interaction layer is still tab-centric**, not **stream-centric**.

The root issue is:

[
LLM = Stream
]

but the code treats it as:

[
LLM = RequestResponse(tab)
]

---

# Additional Solutions (Architecture Level)

## 1. Endpoint Worker Model (Recommended)

Create **one async worker per endpoint URL**.

```
Agent nodes
     ↓
endpoint queue
     ↓
endpoint worker
     ↓
single tab
```

Rust model:

```rust
struct EndpointWorker {
    queue: mpsc::Receiver<Request>,
    tab_id: u32,
    in_flight: bool,
}
```

Execution rule:

```
only 1 message active per endpoint stream
```

Removes duplication entirely.

---

# 2. Message Ticket System

Every LLM request gets a **ticket id**.

```
ticket = atomic_counter++
```

Send prompt:

```
[TICKET: 4821]
payload...
```

Response parser:

```
if ticket already processed → drop
```

Works even with replayed history.

---

# 3. Stream Offset Tracking

Maintain a per-endpoint offset.

```
endpoint_state.last_seen_hash
```

On new tab:

```
ignore messages until new hash appears
```

Equivalent to Kafka consumer offsets.

---

# 4. Tab Lease System

Modify `TabSlots`.

Current:

```
slot → tab
```

New:

```
slot → tab → lease_owner
```

Only the **owner worker** may send messages.

If owner dies:

```
lease_expire → new owner
```

Prevents parallel tabs.

---

# 5. Stateless Envelope Protocol

Wrap every LLM message:

```json
{
 "req_id": "node_418",
 "phase": "mutate",
 "payload": {...}
}
```

Parser extracts only matching `req_id`.

Old messages ignored.

---

# 6. Local Response Journal

Persist responses:

```
~/.canon_llm_journal
```

Structure:

```
endpoint_id
req_id
content_hash
timestamp
```

Processing rule:

```
if hash exists → skip
```

Prevents replay across restarts.

---

# 7. Graph-Driven LLM Batching

Your DAG allows batching.

Instead of:

```
call_node(node)
```

Do:

```
call_nodes_batch([node1,node2,node3])
```

One LLM request:

```
{
 "nodes": [...]
}
```

Reduces tab traffic.

---

# Structural Recommendation

Given your pipeline:

```
CapabilityPipeline
   ↓
scheduler
   ↓
dispatch_node
   ↓
llm_call
```

Best fix is **insert endpoint workers here**:

```
dispatch_node
      ↓
endpoint_queue
      ↓
endpoint_worker
      ↓
tab
```

This converts your system from:

[
ConcurrentTabs
]

to

[
DeterministicStreamProcessing
]

---

# Codebase Health

Your branch stats:

```
ifs   132
loops  1
```

