# Canon — Remaining Implementation Plan v1

> Generated 2026-03-23.
> Covers the four items left after the multi-agent control layer landed in commit 28ecd5c.
> Build target: `cargo check -p canon-runtime -p canon-loop -p canon-route`

---

## Status Summary

| Item    | Title                          | Status          | Blocking?              |
|---------|--------------------------------|-----------------|------------------------|
| MAGENT-1 | agent_id threaded to LlmCall  | NOT IMPLEMENTED | No — cosmetic/tracing  |
| MAGENT-6 | GoalNode DAG wiring           | NOT IMPLEMENTED | No — observability     |
| CTRL-3   | CausalGraph → CSR upgrade     | REASSESSED      | Not needed — see below |
| CTRL-4   | Cross-agent provenance        | PARTIAL         | Needs CTRL-3 decision  |

---

## CTRL-3: CausalGraph / canon-storage-graph — REASSESSED, NO ACTION NEEDED

The original plan assumed `canon-storage-graph` exposed a generic `GraphBuilder<N,E>` type.
The actual API is:

```
canon_graph::CsrGraph  =  { row_ptr: Vec<u32>, col_idx: Vec<u32> }
canon_graph::CodeGraph =  { nodes: Vec<Node>, edges: Vec<Edge>, adjacency: CsrGraph, ... }
```

This is a **code-symbol CSR adjacency matrix**, not a generic typed graph library.
It has no `GraphBuilder`, no typed edge data, and uses `u32` node IDs with no metadata.

The current `CausalGraph` in `canon-route/src/causal.rs`:
```rust
pub struct CausalGraph {
    pub nodes: HashMap<String, CausalNodeKind>,
    pub edges: Vec<CausalEdge>,
}
```
is the **correct implementation** for this use case. It is:
- Typed (`CausalNodeKind` enum with rich per-variant fields)
- Queryable by string key (request_id, action_id, tool_call_id)
- Already populated in `RouteContext::update_from_event()`

**Conclusion:** No migration to `canon-storage-graph` is needed or desirable.
The `canon-storage-graph` dependency was planned based on a misread of its API.
`CTRL-3` is **COMPLETE** as-is.

---

## CTRL-4: Cross-Agent Provenance — PARTIAL

**What exists:**
- `CausalGraph` has `RequestDispatch` and `SubTaskResult` node kinds
- `update_causal_graph()` in `causal.rs` handles `RuntimeEvent::RequestDispatch` and
  `RuntimeEvent::SubTaskResult` (lines 94–95)
- `RouteContext::update_from_event()` calls `update_causal_graph` on LoopPlanned,
  LoopActed, ToolCall, ToolResult

**What's missing:**
- `update_from_event()` in `canon-route/src/context.rs` does **not** call
  `update_causal_graph` on `RequestDispatch` or `SubTaskResult` events — those arms
  are absent from the `match event` block

**Pending work — 2 lines in `canon-route/src/context.rs`:**

In `update_from_event()`, inside the `match event` block, add handlers for the two
remaining events. Currently lines for LoopPlanned (line 124), LoopActed (line 146),
ToolCall (line 204), ToolResult (line 212) call `update_causal_graph`. Add:

```rust
RuntimeEvent::RequestDispatch(_) | RuntimeEvent::SubTaskResult(_) => {
    update_causal_graph(&mut self.causal_graph, event);
}
```

That is the entire change. `update_causal_graph` already handles both variants.

**Files to touch:**
- `canon-route/src/context.rs` — add two-line arm to `update_from_event()` match

**Priority:** Low — tracing only, no correctness impact.

---

## MAGENT-1: agent_id Threaded to LlmCall — NOT IMPLEMENTED

**What exists:**
- `LlmCall { request_id, prompt, role, agent_id: Option<String> }` — field exists
- `CausalGraph` already stores `agent_id` in `CausalNodeKind::LlmCall`
- `RequestDispatch.agent_id` carries the agent identity to the sub-agent worker
- `SubTaskResult.agent_id` carries it back

**What's missing:**
- `LoopContext` has no `agent_id` field — the sub-agent has no identity
- `plan.rs` emits `LlmCall { agent_id: None }` (line ~315)
- `decompose.rs` emits `LlmCall { agent_id: None }` (line 30)
- `dispatch_consumer.rs::run_sub_agent()` primes the sub-agent with `LoopObserved`
  but does not set any agent_id on the context

**Pending work:**

**Step 1 — Add `agent_id` to `LoopContext`** in `canon-loop/src/context.rs`:
```rust
pub struct LoopContext {
    // ...existing fields...
    pub agent_id: Option<String>,   // None = orchestrator, Some(id) = sub-agent
}
```
Initialize to `None` in `LoopContext::new()`.

**Step 2 — Set agent_id in sub-agent** in `dispatch_consumer.rs::run_sub_agent()`:

The sub-agent's `LoopStageExecutor` is already constructed:
```rust
Box::new(LoopStageExecutor::new(workspace.clone(), tlog.clone())),
```
`LoopStageExecutor` owns a `LoopContext` internally. We need a way to set `agent_id`
on it before it starts processing events.

Two options:
- **Option A (preferred):** Add a `pub fn with_agent_id(mut self, id: String) -> Self`
  builder method to `LoopStageExecutor` that sets `self.ctx.agent_id`.
- **Option B:** Add a separate `AgentIdentity` event that `LoopStageExecutor` handles
  by setting `ctx.agent_id`.

Option A is simpler:
```rust
// In canon-loop/src/executor.rs
impl LoopStageExecutor {
    pub fn with_agent_id(mut self, id: String) -> Self {
        self.ctx.agent_id = Some(id);
        self
    }
}

// In dispatch_consumer.rs run_sub_agent():
Box::new(LoopStageExecutor::new(workspace.clone(), tlog.clone())
    .with_agent_id(req.agent_id.clone())),
```

**Step 3 — Use agent_id in plan.rs** when emitting `LlmCall`:
```rust
// In handle_observed(), replace:
agent_id: None,
// with:
agent_id: ctx.agent_id.clone(),
```

**Step 4 — Use agent_id in decompose.rs** when emitting `LlmCall`:
```rust
// Replace:
agent_id: None,
// with:
agent_id: ctx.agent_id.clone(),
```

**Files to touch:**
- `canon-loop/src/context.rs` — add `agent_id: Option<String>` field
- `canon-loop/src/executor.rs` — add `with_agent_id()` builder
- `canon-runtime/src/consumers/dispatch_consumer.rs` — call `.with_agent_id(req.agent_id.clone())`
- `canon-loop/src/stage/plan.rs` — `agent_id: ctx.agent_id.clone()`
- `canon-loop/src/stage/decompose.rs` — `agent_id: ctx.agent_id.clone()`

**Priority:** Low — tracing only. Useful for correlating sub-agent LLM calls in tlog.

---

## MAGENT-6: GoalNode DAG Wiring — NOT IMPLEMENTED

**What exists:**
- All five event types defined in `canon-runtime-events/src/events.rs`:
  ```rust
  GoalNodeCreated   { node_id, description, deps, caps, node_type, priority, budget }
  GoalNodeRetracted { node_id }
  GoalNodeRewritten { node_id, new_description, new_caps }
  GoalEdgeDefined   { from_node_id, to_node_id }
  GoalGraphCheckpointed { tlog_seq }
  ```
- `GoalEdgeDefined` **already emitted** from `canon-loop/src/executor.rs` lines 99–106
  whenever a `LoopPlanned` event has non-empty `depends_on`
- All variants in `RuntimeEvent` enum and wire protocol

**What's missing:**
- `GoalNodeCreated` never emitted
- `GoalNodeRetracted` never emitted
- `GoalGraphCheckpointed` never emitted
- No consumer builds a live in-memory goal graph

### Part A — Emit GoalNodeCreated from decompose stage

In `canon-loop/src/stage/decompose.rs`, after `parse_decompose_tasks()` returns the
dispatch list, emit a `GoalNodeCreated` for each task before emitting `RequestDispatch`:

```rust
// In execute_complete(), after dispatches are built:
let mut events: Vec<RuntimeEvent> = Vec::new();
for dispatch in &dispatches {
    events.push(RuntimeEvent::GoalNodeCreated(GoalNodeCreated {
        node_id:     dispatch.dispatch_id.clone(),
        description: dispatch.task_prompt.clone(),
        deps:        dispatch.deps.clone(),
        caps:        vec![dispatch.task_kind.clone()],
        node_type:   "sub_task".to_string(),
        priority:    128,
        budget:      None,
    }));
    // GoalEdgeDefined for each dep is already emitted by executor when LoopPlanned
    // with depends_on arrives — but for task-level deps, emit here:
    for dep_id in &dispatch.deps {
        events.push(RuntimeEvent::GoalEdgeDefined(GoalEdgeDefined {
            from_node_id: dep_id.clone(),
            to_node_id:   dispatch.dispatch_id.clone(),
        }));
    }
    events.push(RuntimeEvent::RequestDispatch(dispatch.clone()));
}
```

Note: `GoalNodeCreated` needs to be imported from `canon_event` in decompose.rs.

### Part B — Emit GoalNodeRetracted from DispatchConsumer on sub-agent failure

In `dispatch_consumer.rs::run_sub_agent()`, after the loop exits, if `!success`:
```rust
if !success {
    parent_emitter.emit(RuntimeEvent::GoalNodeRetracted(GoalNodeRetracted {
        node_id: req.dispatch_id.clone(),
    }));
}
```

### Part C — GoalGraphConsumer

Create `canon-runtime/src/consumers/goal_graph_consumer.rs`:

```rust
use std::collections::HashMap;
use canon_event::{
    EventConsumer, EventEmitterHandle, EventFilter, GoalGraphCheckpointed,
    GoalNodeCreated, GoalNodeRetracted, GoalNodeRewritten, GoalEdgeDefined, RuntimeEvent,
};

#[derive(Clone, Debug, Default)]
pub struct GoalNode {
    pub node_id:     String,
    pub description: String,
    pub deps:        Vec<String>,
    pub caps:        Vec<String>,
    pub node_type:   String,
    pub priority:    u8,
    pub retracted:   bool,
}

#[derive(Default)]
pub struct GoalGraph {
    pub nodes: HashMap<String, GoalNode>,
    pub edges: Vec<(String, String)>,   // (from, to)
}

impl GoalGraph {
    pub fn apply(&mut self, event: &RuntimeEvent) {
        match event {
            RuntimeEvent::GoalNodeCreated(e) => {
                self.nodes.insert(e.node_id.clone(), GoalNode {
                    node_id:     e.node_id.clone(),
                    description: e.description.clone(),
                    deps:        e.deps.clone(),
                    caps:        e.caps.clone(),
                    node_type:   e.node_type.clone(),
                    priority:    e.priority,
                    retracted:   false,
                });
            }
            RuntimeEvent::GoalNodeRetracted(e) => {
                if let Some(n) = self.nodes.get_mut(&e.node_id) {
                    n.retracted = true;
                }
            }
            RuntimeEvent::GoalNodeRewritten(e) => {
                if let Some(n) = self.nodes.get_mut(&e.node_id) {
                    n.description = e.new_description.clone();
                    n.caps = e.new_caps.clone();
                }
            }
            RuntimeEvent::GoalEdgeDefined(e) => {
                self.edges.push((e.from_node_id.clone(), e.to_node_id.clone()));
            }
            _ => {}
        }
    }

    pub fn active_nodes(&self) -> impl Iterator<Item = &GoalNode> {
        self.nodes.values().filter(|n| !n.retracted)
    }
}

pub struct GoalGraphConsumer {
    graph: GoalGraph,
    emitter: Option<EventEmitterHandle>,
    last_checkpoint_seq: u64,
}

impl GoalGraphConsumer {
    pub fn new() -> Self {
        Self { graph: GoalGraph::default(), emitter: None, last_checkpoint_seq: 0 }
    }

    pub fn graph(&self) -> &GoalGraph { &self.graph }
}

impl EventConsumer for GoalGraphConsumer {
    fn filter(&self) -> EventFilter { EventFilter::All }

    fn set_emitter(&mut self, emitter: EventEmitterHandle) {
        self.emitter = Some(emitter);
    }

    fn on_event(&mut self, event: &RuntimeEvent) {
        self.graph.apply(event);
        // Emit a checkpoint when any goal graph event lands.
        match event {
            RuntimeEvent::GoalNodeCreated(_)
            | RuntimeEvent::GoalNodeRetracted(_)
            | RuntimeEvent::GoalNodeRewritten(_)
            | RuntimeEvent::GoalEdgeDefined(_) => {
                self.last_checkpoint_seq += 1;
                if let Some(emitter) = &self.emitter {
                    emitter.emit(RuntimeEvent::GoalGraphCheckpointed(GoalGraphCheckpointed {
                        tlog_seq: self.last_checkpoint_seq,
                    }));
                }
            }
            _ => {}
        }
    }
}
```

### Part D — Register GoalGraphConsumer in event_runtime.rs

In `canon-runtime/src/bin/event_runtime.rs`, add to the consumer vec:
```rust
Box::new(GoalGraphConsumer::new()),
```
And export from `consumers/mod.rs`:
```rust
pub mod goal_graph_consumer;
```

**Files to touch:**
- `canon-loop/src/stage/decompose.rs` — emit `GoalNodeCreated` + `GoalEdgeDefined` per task
- `canon-runtime/src/consumers/dispatch_consumer.rs` — emit `GoalNodeRetracted` on failure
- `canon-runtime/src/consumers/goal_graph_consumer.rs` — new file
- `canon-runtime/src/consumers/mod.rs` — add `pub mod goal_graph_consumer;`
- `canon-runtime/src/bin/event_runtime.rs` — register consumer

**Priority:** Low — observability only. Implement after MAGENT-1.

---

## Recommended Implementation Order

All remaining items are non-blocking observability/tracing improvements:

1. **CTRL-4** — 2-line fix in `canon-route/src/context.rs`. Do now, trivially small.
2. **MAGENT-1** — Thread `agent_id` through LoopContext → LlmCall. 5 files, ~10 lines total.
3. **MAGENT-6 Part A+B** — Emit `GoalNodeCreated`/`GoalNodeRetracted` from existing stages.
4. **MAGENT-6 Part C+D** — `GoalGraphConsumer` new file + registration.

Total estimated changes: ~60 lines across 8 files. All correctness-safe additions with no
changes to existing behaviour.
