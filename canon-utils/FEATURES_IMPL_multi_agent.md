# Canon — True Multi-Agent Implementation Plan

> Date: 2026-03-23
> Based on audit of current codebase. All file paths are actual current locations.

---

## Current State — What Exists vs. What's Stubbed

### Already wired

| Mechanism                              | Location                                 | Notes                             |
|----------------------------------------+------------------------------------------+-----------------------------------|
| `role`-based endpoint selection        | `canon-exec/src/exec/llm.rs:75`          | Works — role selects URL          |
| 4 parallel exec tabs (`exec_a/b/c/d`)  | `capability_config.toml:75-105`          | Works — concurrent LLM calls      |
| `[llm.roles.decompose]` config section | `capability_config.toml:107`             | Defined, no emitter wired         |
| `AgentRegistered` event                | `canon-runtime-events/src/events.rs:260` | Observability only                |
| `RequestDispatch` wire variant         | `canon-runtime-events/src/wire.rs:36`    | **Stub — never emitted**          |
| `GoalNodeCreated/Retracted/Rewritten`  | `canon-runtime-events/src/events.rs:268` | Events exist, unused in loop      |
| `GoalEdgeDefined`                      | `canon-runtime-events/src/events.rs:284` | Events exist, unused in loop      |
| `agent_id` in `capability_config.toml` | `[[agents.cards]]`                       | Registration only, not in LlmCall |

### Not wired at all

- No `agent_id` field on `LlmCall` — dispatch is role-only
- No sub-task fan-out logic — one planner, one plan at a time
- No result aggregation from parallel agents
- No agent-to-agent messaging
- No sub-agent lifecycle tracking (spawned, running, finished, failed)
- The `decompose` agent is registered but never invoked

---

## Architecture Overview

```
                    ┌─────────────────────────────────────┐
                    │         Orchestrator Loop            │
                    │  Observe → Decompose → Dispatch      │
                    │       → Aggregate → Verify           │
                    └──────────────┬──────────────────────┘
                                   │ RequestDispatch (fan-out)
              ┌────────────────────┼────────────────────┐
              ▼                    ▼                    ▼
       ┌─────────────┐    ┌─────────────┐    ┌─────────────┐
       │  Sub-Agent A │    │  Sub-Agent B │    │  Sub-Agent C │
       │  (rust_impl) │    │  (doc_write) │    │  (test_write)│
       │  own loop    │    │  own loop    │    │  own loop    │
       └──────┬───────┘    └──────┬───────┘    └──────┬───────┘
              │ SubTaskResult      │ SubTaskResult      │ SubTaskResult
              └────────────────────┴────────────────────┘
                                   │
                    ┌──────────────▼──────────────────────┐
                    │       Aggregator Consumer            │
                    │  collects all SubTaskResults,        │
                    │  emits LoopPlanned per result        │
                    └─────────────────────────────────────┘
```

Each sub-agent is a full canon loop with its own observe→plan→act→verify cycle. The orchestrator decomposes the goal, dispatches sub-tasks, and aggregates results.

---

## MAGENT-1: Add `agent_id` to `LlmCall`

**Purpose:** Allow any stage to target a specific named agent instead of just a role.

**Files:**
- `canon-runtime-events/src/events.rs`
- `canon-exec/src/exec/llm.rs`

**Changes:**

### 1. Extend `LlmCall`

```rust
// canon-runtime-events/src/events.rs
canon_event_struct!(LlmCall {
    request_id: String,
    prompt: String,
    #[serde(default)]
    role: Option<String>,
    #[serde(default)]
    agent_id: Option<String>,   // new: targets a specific endpoint by id
});
```

### 2. Prefer `agent_id` in dispatch

```rust
// canon-exec/src/exec/llm.rs — in the endpoint selection block
let selected = if let Some(aid) = agent_id.as_deref() {
    // exact endpoint id match (e.g. "exec_chatgpt_a")
    config.llm_endpoints.iter().find(|e| e.id == aid)
        // fallback: agent card url match
        .or_else(|| config.llm_endpoints.iter().find(|e| e.url.contains(aid)))
} else if let Some(role_name) = role.as_deref() {
    config.llm_endpoints.iter().find(|e| e.role.as_deref() == Some(role_name))
        .or_else(|| if role_name == "router" {
            config.llm_endpoints.iter().find(|e| e.role.as_deref() == Some("planner"))
        } else { None })
} else {
    config.llm_endpoints.first()
};
```

**Acceptance criteria:**
- `LlmCall { agent_id: Some("exec_chatgpt_b"), ... }` routes to the `exec_chatgpt_b` tab.
- Existing calls without `agent_id` are unaffected.

---

## MAGENT-2: Wire `RequestDispatch` as Sub-Task Fan-Out

**Purpose:** Let the orchestrator emit `RequestDispatch` events to spawn parallel sub-agent work.

**Files:**
- `canon-runtime-events/src/events.rs` (structured type)
- `canon-runtime-events/src/wire.rs` (variant exists, needs structured type)
- New: `canon-runtime/src/consumers/dispatch_consumer.rs`
- `canon-runtime/src/lib.rs` (register consumer)

### 1. Define `RequestDispatch` as a proper struct

Currently `RequestDispatch(serde_json::Value)` in wire.rs. Add typed struct:

```rust
// canon-runtime-events/src/events.rs
canon_event_struct!(RequestDispatch {
    dispatch_id: String,          // unique ID for this fan-out batch
    parent_request_id: String,    // the LlmCall request_id that produced this
    agent_id: String,             // target agent from capability_config agents.cards
    task_prompt: String,          // full prompt for the sub-agent
    task_kind: String,            // "plan", "review", "test", "document", etc.
    #[serde(default)]
    deps: Vec<String>,            // dispatch_ids this task waits for (DAG ordering)
    #[serde(default)]
    workspace_scope: Option<String>, // sub-directory scope for the sub-agent
});
```

### 2. Define `SubTaskResult`

```rust
// canon-runtime-events/src/events.rs
canon_event_struct!(SubTaskResult {
    dispatch_id: String,
    agent_id: String,
    parent_request_id: String,
    success: bool,
    output: serde_json::Value,    // structured result from sub-agent
    actions_taken: Vec<String>,   // summary of what the sub-agent did
    #[serde(default)]
    error: Option<String>,
});
```

### 3. `DispatchConsumer` — handles fan-out and collection

```rust
// canon-runtime/src/consumers/dispatch_consumer.rs
pub struct DispatchConsumer {
    pending: HashMap<String, DispatchBatch>,  // parent_request_id → batch
    emitter: Option<EventEmitterHandle>,
}

struct DispatchBatch {
    parent_request_id: String,
    total: usize,
    results: Vec<SubTaskResult>,
}
```

On `RequestDispatch`:
- Register in `pending` batch
- Spawn sub-agent loop (MAGENT-4)
- Emit `CapabilityInvoked { capability_id: dispatch_id, ... }`

On `SubTaskResult`:
- Add to batch
- When `results.len() == total` → batch complete
- Emit `CapabilityCompleted` back to the plan stage with aggregated output

**Acceptance criteria:**
- Two `RequestDispatch` events with same `parent_request_id` both complete before `CapabilityCompleted` fires.
- A failed sub-task sets `any_failed=true` in the aggregated result.

---

## MAGENT-3: `DecomposeStage` — Task Splitter

**Purpose:** A new plan stage variant that calls the `decompose` agent to split a goal into parallel sub-tasks.

**Files:**
- New: `canon-loop/src/stage/decompose.rs`
- `canon-loop/src/executor.rs` (new route branch)
- `canon-decision/src/lib.rs` (add `RouteKind::Decompose`)

### 1. Add `RouteKind::Decompose`

```rust
// canon-decision/src/lib.rs
pub enum RouteKind {
    Observe, Plan, Act, Verify, Conclude,
    Decompose,   // new: calls decompose agent to split goal into sub-tasks
}
```

### 2. Decompose agent response format

The `decompose` agent receives the goal and current workspace state, and returns a JSON array of sub-tasks:

```json
[
  {
    "agent_id": "exec_chatgpt_a",
    "task_kind": "implement",
    "task": "Implement src/lib.rs with the core logic",
    "workspace_scope": "src/",
    "deps": []
  },
  {
    "agent_id": "exec_chatgpt_b",
    "task_kind": "document",
    "task": "Write README.md describing build and usage",
    "workspace_scope": ".",
    "deps": []
  },
  {
    "agent_id": "exec_chatgpt_c",
    "task_kind": "test",
    "task": "Write tests in src/tests.rs",
    "workspace_scope": "src/",
    "deps": ["exec_chatgpt_a"]
  }
]
```

### 3. Decompose stage

```rust
// canon-loop/src/stage/decompose.rs
pub fn execute_trigger(rs: RouteSelected, ctx: &mut LoopContext) -> anyhow::Result<LoopStageResult> {
    // Build decompose prompt from goal + workspace state
    // Fire LlmCall { role: Some("decompose"), ... }
    // On response: parse sub-task array
    // Emit RequestDispatch per sub-task
    // Emit LoopPlanned { action_kind: "decompose", ... } for tracking
}
```

### 4. Gatekeeper routing to decompose

Router gains the `decompose` option. Fires when:
- Goal has a `## Parallel Tasks` or `## Sub-Agents` section, OR
- `task_complexity_score > 0.8` in LLM signals, OR
- `planned_pending == 0 && !acted_unverified && goal_specifies_parallel_work`

**Acceptance criteria:**
- A goal with parallel requirements emits N `RequestDispatch` events.
- Orchestrator loop does not proceed to `act` until all `SubTaskResult` events arrive.

---

## MAGENT-4: Sub-Agent Loop Spawning

**Purpose:** Each `RequestDispatch` spawns an independent canon event loop for the target agent.

**Files:**
- New: `canon-runtime/src/sub_agent.rs`
- `canon-runtime/src/lib.rs`

### Design

Each sub-agent gets:
- Its own isolated tlog path: `{state_dir}/sub_agents/{dispatch_id}/event.tlog.d`
- Its own `LoopContext` scoped to `workspace_scope`
- Its own `RouteExecutor` instance
- A modified goal injected as `PromptLoaded` from `task_prompt`
- A result emitter that writes `SubTaskResult` to the parent tlog when it reaches `conclude`

```rust
// canon-runtime/src/sub_agent.rs
pub struct SubAgentConfig {
    pub dispatch_id: String,
    pub agent_id: String,
    pub parent_request_id: String,
    pub task_prompt: String,
    pub workspace_scope: PathBuf,
    pub parent_emitter: EventEmitterHandle,
    pub endpoint_override: Option<String>,  // pin to specific tab
}

pub fn spawn(cfg: SubAgentConfig) -> std::thread::JoinHandle<()> {
    std::thread::spawn(move || {
        let mut loop_executor = LoopExecutor::new(cfg.workspace_scope.clone());
        // inject task_prompt as the goal
        // run loop until LoopRewarded(halt=true)
        // emit SubTaskResult to parent_emitter
    })
}
```

### Sub-agent termination

When sub-agent's verify sets `finish_ready=true` and loop concludes:
1. Collect `batch_acted` summary
2. Emit `SubTaskResult { dispatch_id, success: true, actions_taken, output }`
3. Thread exits

If sub-agent exceeds `max_iterations` or halts on error:
1. Emit `SubTaskResult { success: false, error: Some(reason) }`

**Acceptance criteria:**
- Sub-agent runs in a separate thread with no shared mutable state.
- Sub-agent events written to its own tlog AND key events forwarded to parent tlog.
- Sub-agent `conclude` maps to `SubTaskResult` emitted to parent.

---

## MAGENT-5: Agent Registry Consumer

**Purpose:** Make `AgentRegistered` data queryable at runtime, not just observability events.

**Files:**
- New: `canon-runtime/src/consumers/agent_registry.rs`
- `canon-runtime/src/lib.rs`

### Registry struct

```rust
pub struct AgentRegistry {
    agents: HashMap<String, AgentCard>,
}

pub struct AgentCard {
    pub agent_id: String,
    pub agent_url: String,
    pub role: String,
    pub tool_capabilities: Vec<String>,
    pub status: AgentStatus,
}

pub enum AgentStatus {
    Idle,
    Busy { dispatch_id: String },
    Failed { reason: String },
}
```

On `AgentRegistered` → insert card with `status: Idle`.
On `RequestDispatch` → mark assigned agent as `Busy`.
On `SubTaskResult` → mark agent as `Idle` or `Failed`.

`DispatchConsumer` queries the registry to select an available agent when `agent_id` in the request is a role name rather than a specific tab (e.g., `"exec"` → pick any idle exec agent).

**Acceptance criteria:**
- `registry.available_agents("exec")` returns only agents not currently `Busy`.
- After `SubTaskResult`, the previously busy agent returns to `Idle`.

---

## MAGENT-6: `GoalNode` Graph as Decomposition DAG

**Purpose:** Wire the existing `GoalNodeCreated/GoalEdgeDefined` events (already in events.rs:268, never emitted) into the decompose stage so the task graph is persisted and observable.

**Files:**
- `canon-loop/src/stage/decompose.rs` (emit during decomposition)
- New: `canon-runtime/src/consumers/goal_graph.rs` (track DAG, enforce dep ordering)

### Emit during decompose

For each sub-task:
```rust
GoalNodeCreated {
    node_id: dispatch_id.clone(),
    description: task.task.clone(),
    deps: task.deps.clone(),
    caps: vec![task.task_kind.clone()],
    node_type: "sub_task".into(),
    priority: 128,
    budget: None,
}
```

For each dependency edge:
```rust
GoalEdgeDefined {
    from_node_id: dep_id.clone(),
    to_node_id: dispatch_id.clone(),
}
```

### `GoalGraphConsumer`

Tracks the DAG. Blocks dispatching a `RequestDispatch` until all deps have a corresponding `SubTaskResult { success: true }`. Enables **sequential sub-agent chaining** — e.g., "write tests" waits for "implement core".

**Acceptance criteria:**
- A 3-node DAG with a dependency chain dispatches in topological order.
- A cycle in the dep graph is detected and triggers `ErrorOccurred`.

---

## MAGENT-7: capability_config.toml Specialist Agents

**Purpose:** Add named specialist agents with different URLs and system prompts.

```toml
[llm.endpoints.rust_specialist]
id = "rust_specialist"
url = "https://chatgpt.com/gg/<rust-specialist-gpt-id>"
role_markdown = "builtin:planner"
role = "rust_specialist"
stateful = true
max_tabs = 2

[llm.endpoints.doc_writer]
id = "doc_writer"
url = "https://chatgpt.com/gg/<doc-writer-gpt-id>"
role_markdown = "builtin:planner"
role = "doc_writer"
stateful = true
max_tabs = 1

[llm.endpoints.test_writer]
id = "test_writer"
url = "https://chatgpt.com/gg/<test-writer-gpt-id>"
role_markdown = "builtin:planner"
role = "test_writer"
stateful = true
max_tabs = 1

[[agents.cards]]
agent_id = "rust_specialist"
agent_url = "https://chatgpt.com/gg/<rust-specialist-gpt-id>"
role = "rust_specialist"
goal = "AGENT_GOAL.md"
tool_capabilities = ["apply_patch", "bash"]

[[agents.cards]]
agent_id = "doc_writer"
agent_url = "https://chatgpt.com/gg/<doc-writer-gpt-id>"
role = "doc_writer"
goal = "AGENT_GOAL.md"
tool_capabilities = ["apply_patch"]

[[agents.cards]]
agent_id = "test_writer"
agent_url = "https://chatgpt.com/gg/<test-writer-gpt-id>"
role = "test_writer"
goal = "AGENT_GOAL.md"
tool_capabilities = ["apply_patch", "bash"]
```

---

## MAGENT-8: Result Merging into Orchestrator Context

**Purpose:** When all sub-tasks complete, merge their action histories into the orchestrator's context so the final verify and conclude have full visibility.

**Files:**
- `canon-loop/src/context.rs`
- `canon-route/src/context.rs`

When `DispatchConsumer` fires `CapabilityCompleted` with aggregated results, emit a synthetic `LoopActed` per sub-agent result into the orchestrator loop:

```rust
LoopActed {
    action_kind: format!("sub_agent:{}", result.agent_id),
    stdout: serde_json::to_string(&result.output).unwrap_or_default(),
    stderr: result.error.unwrap_or_default(),
    success: result.success,
    // ...
}
```

This flows into `batch_acted` so the planner's next prompt shows what each sub-agent did, and the router context's `recent_tool_results` reflects the merged outcome.

---

## Implementation Order

| Step | Plan                                                       | Effort      | Unblocks   |
|------+------------------------------------------------------------+-------------+------------|
|    1 | **MAGENT-1** — `agent_id` in `LlmCall`                     | Small       | Everything |
|    2 | **MAGENT-2** — `RequestDispatch` + `SubTaskResult` structs | Small       | 3, 4, 5    |
|    3 | **MAGENT-5** — Agent Registry Consumer                     | Small       | 4          |
|    4 | **MAGENT-4** — Sub-Agent Loop Spawning                     | Large       | 3, 6       |
|    5 | **MAGENT-3** — Decompose Stage + `RouteKind::Decompose`    | Medium      | 6          |
|    6 | **MAGENT-2** — `DispatchConsumer` aggregation logic        | Medium      | 8          |
|    7 | **MAGENT-6** — GoalNode DAG wiring                         | Medium      | —          |
|    8 | **MAGENT-8** — Result merging into orchestrator context    | Small       | —          |
|    9 | **MAGENT-7** — specialist agents in capability_config.toml | Config only | —          |

---

## Wire Protocol Changes Summary

### New events (`canon-runtime-events/src/events.rs`)

```rust
canon_event_struct!(RequestDispatch {
    dispatch_id: String,
    parent_request_id: String,
    agent_id: String,
    task_prompt: String,
    task_kind: String,
    deps: Vec<String>,
    workspace_scope: Option<String>,
});

canon_event_struct!(SubTaskResult {
    dispatch_id: String,
    agent_id: String,
    parent_request_id: String,
    success: bool,
    output: serde_json::Value,
    actions_taken: Vec<String>,
    error: Option<String>,
});
```

### Modified events

```rust
// LlmCall — add agent_id field
canon_event_struct!(LlmCall {
    request_id: String,
    prompt: String,
    role: Option<String>,
    agent_id: Option<String>,   // add
});
```

### New `CanonPayload` variants (`wire.rs`)

```rust
SubTaskResult(serde_json::Value),
// RequestDispatch already exists at wire.rs:36 — just needs to be emitted
```

---

## Observable Tlog Flow (Target State)

```
PromptLoaded(goal)
LoopObserved
RouteSelected(decompose)
  Llm(role=decompose, prompt=...)
  CapabilityCompleted(decompose response)
  GoalNodeCreated × N
  GoalEdgeDefined × M
  RequestDispatch × N                   ← fan-out

  [sub-agent A — own tlog]
    LoopObserved(scoped goal)
    RouteSelected(plan) → Llm(planner) → LoopPlanned → LoopActed
    RouteSelected(verify) → LoopVerified(passed)
    RouteSelected(conclude) → SubTaskResult(success=true)

  [sub-agent B — own tlog, parallel]
    ...

SubTaskResult × N                       ← aggregation
CapabilityCompleted(merged results)
LoopActed(action_kind=sub_agent:exec_a) ← synthetic merge
LoopActed(action_kind=sub_agent:exec_b)
RouteSelected(verify)
LoopVerified(passed=true)
RouteSelected(conclude)
LoopRewarded(halt=true)
```

---

## `agent_id` vs `role` — Distinction

| Concept             | Meaning                                                | When to use                                                   |
|---------------------+--------------------------------------------------------+---------------------------------------------------------------|
| `role`              | Capability type (`planner`, `exec`, `rust_specialist`) | When any available agent of that type will do; load-balanced  |
| `agent_id`          | Specific named instance (`exec_chatgpt_b`)             | When a sub-agent needs to continue a stateful browser session |
| `dispatch_id`       | Unique ID for one sub-task invocation                  | Correlates `RequestDispatch` → `SubTaskResult` → `GoalNode`   |
| `parent_request_id` | The LlmCall that produced the decomposition            | Groups all sub-tasks from one decompose call                  |
