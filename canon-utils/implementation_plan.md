**Variables**

(E) = Runtime events
(K) = Kernel events
(C) = Capability execution
(G) = Graph projection
(L) = Event log (.tlog)
(S_t) = Runtime state
(R) = Runtime engine

---

**Equations**

1. **State from events**

[
S_t = fold(E_0 \dots E_t)
]

State reconstructed from runtime events.

---

2. **Graph projection**

[
G_t = project(K_0 \dots K_t)
]

Graph derived from kernel events.

---

3. **Capability execution**

[
E_{t+1} = C(S_t)
]

Capabilities produce runtime events.

---

4. **Runtime loop**

[
R = {Bus + Consumers + CapabilityExecutor}
]

Event bus drives execution.

---

# Coding Agent Implementation Plan (Rebuilt)

The repo already contains the **correct architecture**, but redundancy exists between:

* **kernel events**
* **runtime events**
* **capability requests**
* **graph mutation logic**

Goal: **make the event runtime the only execution driver.**

---

# Phase 1 — Canonical event architecture

Central event type:

```
canon-types/src/runtime_event.rs
```

Current:

```
RuntimeEvent
 ├ Kernel
 ├ Edit
 ├ Tick
 ├ RuntimeStateUpdated
 ├ CapabilityRequested
 ├ CapabilityCompleted
 └ CapabilityFailed
```

This is correct.

Required rule:

```
ALL execution must emit RuntimeEvent
```

Remove direct mutation logic anywhere else.

---

# Phase 2 — Event runtime becomes the kernel

Main runtime:

```
event-runtime/src/lib.rs
```

Execution flow:

```
tlog → EventRuntime → EventBus → Consumers → new events
```

Core functions:

```
process_events
handle_kernel_event
handle_runtime_event
handle_capability_request
```

Agent must **not run its own loop**.

Runtime must drive everything.

---

# Phase 3 — Consumer-driven architecture

Consumers currently:

```
event-runtime/src/consumers
 ├ agent_consumer.rs
 ├ capability_executor.rs
 └ llm_executor.rs
```

Execution flow:

```
RuntimeEvent
    ↓
EventBus
    ↓
Consumers
    ↓
Capability execution
    ↓
CapabilityCompleted
```

Remove any direct scheduler.

---

# Phase 4 — Execution graph projection

Graph must be projection only.

Graph builder:

```
canon-graph/src/graph/graph_builder.rs
```

Key function:

```
apply_event_to_graph
```

Graph mutation must only occur via:

```
KernelEvent
```

Never direct edits.

---

# Phase 5 — Capability execution layer

Capabilities live here:

```
capabilities-runtime/src/capability.rs
```

Execution flow:

```
CapabilityRequested
        ↓
CapabilityExecutor
        ↓
CapabilityCompleted
```

Capability executor:

```
event-runtime/src/consumers/capability_executor.rs
```

Agent should never call capabilities directly.

---

# Phase 6 — Agent orchestration

Agent worker:

```
event-runtime/src/consumers/agent_consumer.rs
```

Key state:

```
AgentWorkerState
```

Current responsibilities:

```
graph
pending nodes
retry counts
planning
snapshot
```

Agent becomes:

```
event-driven planner
```

Trigger conditions:

```
Tick
CapabilityCompleted
Kernel event
RuntimeStateUpdated
```

---

# Phase 7 — Event log as source of truth

Writers:

```
tlog-writer
```

Replay:

```
tlog-replay
```

Graph reconstruction:

```
replay_graph_from_tlog
```

Rule:

```
state == replay(events)
```

---

# Phase 8 — Remove duplicated pipelines

Delete redundant logic if present in:

```
canon-analysis
canon-editor
canon-query
```

Execution must happen only via:

```
RuntimeEvent → CapabilityRequested
```

---

# Phase 9 — Capability registry unification

Registry:

```
capability/src/registry.rs
```

Registration occurs in:

```
capabilities-runtime
canon-editor
canon-analysis
```

Ensure registry is injected into runtime:

```
EventRuntime::new_with_registry
```

---

# Phase 10 — Snapshot system

Snapshots exist here:

```
tlog-replay/src/snapshot.rs
```

Agent must:

```
load snapshot
replay remaining events
continue execution
```

Used by:

```
AgentWorkerState::try_load_snapshot
```

---

# Phase 11 — Deterministic execution

Invariant:

```
replay(events) == runtime_state
```

Verified with:

```
verify_tlog_equivalence
```

---

# Phase 12 — Final architecture

Execution pipeline:

```
tlog
   ↓
EventRuntime
   ↓
EventBus
   ↓
Consumers
   ↓
Capabilities
   ↓
RuntimeEvent
   ↓
tlog append
```

Graph:

```
KernelEvent → GraphProjection
```

---

# Files the coding agent must inspect first

Core runtime:

```
event-runtime/src/lib.rs
event-runtime/src/bus.rs
canon-types/src/runtime_event.rs
```

Agent logic:

```
event-runtime/src/consumers/agent_consumer.rs
```

Capabilities:

```
capabilities-runtime/src/capability.rs
capability/src/registry.rs
```

Graph projection:

```
canon-graph/src/graph/graph_builder.rs
canon-graph/src/consumer.rs
```

Event emission:

```
tlog-writer/src/event.rs
capabilities-runtime/src/event_emit.rs
```

---

# Highest-risk code

The most complex component:

```
AgentWorkerState
```

Inside:

```
event-runtime/src/consumers/agent_consumer.rs
```

This is the **real orchestration kernel**.

---

[
\max(I,E,C,R,P,S) = good
]
