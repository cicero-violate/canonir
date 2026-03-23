# Canon Control Layer — Implementation Plan

> Generated 2026-03-23. Covers three tightly coupled subsystems needed for true multi-agent
> parallelism: **Scheduler**, **Causal Graph**, and **Merge Logic**.
>
> Prerequisite: MAGENT-1 through MAGENT-5 from `FEATURES_IMPL_multi_agent.md` should be
> completed first, or implemented in parallel with this plan.

---

## Status Summary

| Plan   | Title                           | Status          |
|--------|---------------------------------|-----------------|
| CTRL-1 | Priority Scheduler              | NOT IMPLEMENTED |
| CTRL-2 | Dependency-Aware Dispatch       | NOT IMPLEMENTED |
| CTRL-3 | Causal Event Graph              | NOT IMPLEMENTED |
| CTRL-4 | Cross-Agent Provenance Tracking | NOT IMPLEMENTED |
| CTRL-5 | File-Level Conflict Detection   | NOT IMPLEMENTED |
| CTRL-6 | Context Merge (batch_acted)     | NOT IMPLEMENTED |
| CTRL-7 | State Reconciliation            | NOT IMPLEMENTED |

---

## Existing Infrastructure to Build On

Before describing what to add, here is the relevant infrastructure that already exists:

### Act Queue / Batch Tracker — `canon-loop/src/executor.rs`
```
act_queue: VecDeque<LoopPlanned>      — strict FIFO, one pending_act at a time
act_batch_tracker: HashMap<String, BatchStatus>  — per-llm_request_id completion tracking
PendingAct { plan_id, action_id, llm_request_id, ... } — active execution slot
```
Currently single-threaded: one `PendingAct` at a time, no priority, no dependency awareness.

### Graph Infrastructure — `canon-storage-graph/src/`
```
CsrGraph<N, E>   — compressed sparse row graph with typed node/edge data
GraphBuilder     — incremental edge insertion, then freeze to CSR
canon-tools-analysis/src/  — Tarjan SCC, GPU topological sort, reachability queries
```
Fully generic — can store any `(NodeData, EdgeData)` pair. Used for code structure today;
can be repurposed for causal event DAGs.

### Goal DAG Events — `canon-runtime-events/src/events.rs`
```
GoalNodeCreated   { node_id, parent_id, label, criteria }  — emitted, stub only
GoalNodeRetracted { node_id }
GoalNodeRewritten { node_id, new_label }
GoalEdgeDefined   { parent_id, child_id, kind }
GoalGraphCheckpointed { snapshot }
```
These structs exist and are in the wire protocol but nothing emits or consumes them yet.
They are the natural home for the task dependency graph.

### Agent Registry — `canon-runtime-events/src/events.rs`
```
AgentRegistered { agent_id, role, capacity }
```
Exists in wire protocol. No consumer tracks live agent state yet (MAGENT-5).

---

## CTRL-1: Priority Scheduler — NOT IMPLEMENTED

**Goal:** Replace `VecDeque<LoopPlanned>` with a priority queue that respects task urgency
and agent capacity.

### What to add

**1. `TaskPriority` enum** — `canon-loop/src/scheduler.rs` (new file):
```rust
#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord)]
pub enum TaskPriority {
    Critical = 3,   // blocking the critical path (no successors can proceed)
    High     = 2,   // on critical path but has parallel alternatives
    Normal   = 1,   // default
    Low      = 0,   // background, speculative
}
```

**2. `ScheduledTask` struct**:
```rust
pub struct ScheduledTask {
    pub priority: TaskPriority,
    pub enqueued_at: std::time::Instant,
    pub agent_id: Option<String>,   // target agent, None = any
    pub plan: LoopPlanned,
}
```

**3. `Scheduler` struct** replacing `act_queue`:
```rust
pub struct Scheduler {
    queue: BinaryHeap<ScheduledTask>,        // max-heap by (priority, enqueued_at)
    agent_capacity: HashMap<String, usize>,  // agent_id → max parallel tasks
    agent_active:   HashMap<String, usize>,  // agent_id → current active count
}

impl Scheduler {
    pub fn push(&mut self, task: ScheduledTask);
    pub fn pop_for_agent(&mut self, agent_id: &str) -> Option<ScheduledTask>;
    pub fn pop_any(&mut self) -> Option<ScheduledTask>;
    pub fn has_capacity(&self, agent_id: &str) -> bool;
    pub fn complete(&mut self, agent_id: &str);  // decrement active count
}
```

**4. Wire up in `canon-loop/src/executor.rs`**:
- Replace `self.act_queue: VecDeque<LoopPlanned>` with `self.scheduler: Scheduler`
- On `LoopPlanned` enqueue: compute priority from `plan.signals` (use `goodness`, `delta_g`
  if available; fall back to Normal)
- On dispatch: call `scheduler.pop_for_agent(agent_id)` or `pop_any()`

**5. Priority inference heuristic** (in `canon-loop/src/scheduler.rs`):
```rust
pub fn infer_priority(plan: &LoopPlanned, ctx: &LoopContext) -> TaskPriority {
    if plan.action_kind == "done" { return TaskPriority::Critical; }
    if ctx.compiler_errors > 0   { return TaskPriority::High; }
    TaskPriority::Normal
}
```

**Files to touch:**
- `canon-loop/src/scheduler.rs` — new file
- `canon-loop/src/executor.rs` — replace act_queue, dispatch logic
- `canon-loop/src/lib.rs` — `mod scheduler;`

**Priority:** High — prerequisite for CTRL-2 and for meaningful parallelism.

---

## CTRL-2: Dependency-Aware Dispatch — NOT IMPLEMENTED

**Goal:** Prevent dispatching task B until task A completes, when A → B is a declared dependency.

### What to add

**1. Dependency field on `LoopPlanned`** — `canon-runtime-events/src/events.rs`:
```rust
// Add to LoopPlanned struct:
pub depends_on: Vec<String>,  // list of action_ids that must complete first
```

**2. `DependencyTracker`** — `canon-loop/src/scheduler.rs`:
```rust
pub struct DependencyTracker {
    /// action_id → set of action_ids it is waiting for
    waiting: HashMap<String, HashSet<String>>,
    /// action_id → list of tasks unblocked when it completes
    unblocks: HashMap<String, Vec<ScheduledTask>>,
}

impl DependencyTracker {
    pub fn add(&mut self, task: ScheduledTask);
    /// Called when action_id completes. Returns newly unblocked tasks.
    pub fn complete(&mut self, action_id: &str) -> Vec<ScheduledTask>;
    pub fn is_ready(&self, action_id: &str) -> bool;
}
```

**3. Integration with GoalEdgeDefined events:**
When `GoalEdgeDefined { parent_id, child_id, kind: "depends_on" }` is received in
`LoopExecutor::on_event`, register the dependency in `DependencyTracker` before the child
task is dispatched.

**4. Emit `GoalEdgeDefined` from planner** — `canon-loop/src/stage/plan.rs`:
When the LLM response contains `"depends_on": ["<action_id>"]` in the action JSON,
parse it and emit a `GoalEdgeDefined` event before `LoopPlanned`.

**Files to touch:**
- `canon-runtime-events/src/events.rs` — add `depends_on` to `LoopPlanned`
- `canon-loop/src/scheduler.rs` — add `DependencyTracker`
- `canon-loop/src/stage/plan.rs` — parse `depends_on`, emit `GoalEdgeDefined`
- `canon-loop/src/executor.rs` — wire `DependencyTracker.complete()` on `LoopActed`

**Priority:** Medium — needed for correct ordering of multi-step parallel plans.

---

## CTRL-3: Causal Event Graph — NOT IMPLEMENTED

**Goal:** Build a runtime DAG that tracks the full provenance chain of every event:
`LlmCall → LoopPlanned → ToolCall → ToolResult → LoopActed`.

This enables replay, audit trails, and root-cause analysis for failures.

### What to add

**1. `CausalNode` and `CausalEdge` types** — `canon-route/src/causal.rs` (new file):
```rust
#[derive(Debug, Clone)]
pub enum CausalNodeKind {
    LlmCall     { request_id: String, role: Option<String>, agent_id: Option<String> },
    LoopPlanned { action_id: String, action_kind: String, plan_id: Option<String> },
    ToolCall    { tool_call_id: String, kind: String },
    ToolResult  { tool_result_id: String, success: bool },
    LoopActed   { action_id: Option<String>, success: bool },
    LoopVerified{ passed: bool },
}

#[derive(Debug, Clone)]
pub struct CausalEdge {
    pub kind: &'static str,  // "caused", "resolved", "triggered"
}
```

**2. `CausalGraph` backed by `CsrGraph`** — `canon-route/src/causal.rs`:
```rust
pub struct CausalGraph {
    builder: GraphBuilder<CausalNodeKind, CausalEdge>,
    node_index: HashMap<String, NodeId>,  // event_id / request_id → graph node
    frozen: Option<CsrGraph<CausalNodeKind, CausalEdge>>,
}

impl CausalGraph {
    pub fn record_llm_call(&mut self, request_id: &str, ...);
    pub fn record_planned(&mut self, action_id: &str, llm_request_id: &str, ...);
    pub fn record_tool_call(&mut self, tool_call_id: &str, action_id: &str);
    pub fn record_tool_result(&mut self, tool_result_id: &str, tool_call_id: &str, success: bool);
    pub fn record_acted(&mut self, action_id: Option<&str>, tool_result_id: Option<&str>);
    /// Checkpoint: freeze current graph state for query.
    pub fn checkpoint(&mut self) -> &CsrGraph<CausalNodeKind, CausalEdge>;
    /// Ancestors of a node (for root-cause tracing).
    pub fn ancestors(&self, node: NodeId) -> Vec<NodeId>;
}
```

**3. Populate from `RouteContext::update_from_event`** — `canon-route/src/context.rs`:
On each relevant event, call the matching `causal_graph.record_*()` method. The existing
`action_meta` HashMap already captures `action_id → (action_kind, llm_request_id)` — feed
that into causal graph edges.

**4. Query interface** — expose via a method on `RouteContext`:
```rust
pub fn causal_ancestors_of_action(&self, action_id: &str) -> Vec<CausalNodeKind>;
pub fn causal_chain_for_failure(&self, tool_result_id: &str) -> String; // formatted for prompt
```

**Files to touch:**
- `canon-route/src/causal.rs` — new file
- `canon-route/src/context.rs` — add `causal_graph: CausalGraph`, populate in `update_from_event`
- `canon-route/src/lib.rs` — `pub mod causal;`
- `Cargo.toml` — add `canon-storage-graph` as dependency of `canon-route`

**Priority:** Medium — high value for debugging; not blocking for correctness.

---

## CTRL-4: Cross-Agent Provenance Tracking — NOT IMPLEMENTED

**Goal:** Extend the causal graph to link events across agent boundaries so that
`SubTaskResult` from agent B can be traced back to the `RequestDispatch` from agent A.

**Prerequisite:** CTRL-3 (causal graph) and MAGENT-2 (RequestDispatch wiring).

### What to add

**1. New `CausalNodeKind` variants**:
```rust
RequestDispatch { dispatch_id: String, from_agent: String, to_agent: String },
SubTaskResult   { dispatch_id: String, agent_id: String, success: bool },
```

**2. Edge: `RequestDispatch → LoopPlanned (in sub-agent)`**
When the orchestrator emits `RequestDispatch`, record it in the causal graph.
When the sub-agent emits `LoopPlanned` in response, the `llm_request_id` on the plan
references back to the dispatch — use this to draw the cross-agent edge.

**3. `agent_id` tag on all nodes**
Add `agent_id: Option<String>` to `CausalNodeKind` variants so that graph queries can
filter by agent or show the full cross-agent chain.

**4. `GoalGraphCheckpointed` emission**
When `CausalGraph::checkpoint()` is called, emit a `GoalGraphCheckpointed` event
(already exists in wire protocol) with the serialized snapshot. This makes the causal
graph observable from outside the process.

**Files to touch:**
- `canon-route/src/causal.rs` — add new node kinds, agent_id tags
- `canon-loop/src/stage/decompose.rs` (MAGENT-3) — emit RequestDispatch with dispatch_id
- Sub-agent `canon-loop/src/executor.rs` — tag LoopPlanned with parent dispatch_id
- `canon-route/src/context.rs` — handle SubTaskResult in causal graph

**Priority:** Low — important for full multi-agent observability; depends on CTRL-3 + MAGENT-2.

---

## CTRL-5: File-Level Conflict Detection — NOT IMPLEMENTED

**Goal:** Detect when two sub-agents have both issued write operations targeting the same
file path, and block or defer the second write until the first is verified.

### What to add

**1. `FileWriteTracker`** — `canon-loop/src/merge.rs` (new file):
```rust
pub struct FileWriteTracker {
    /// path → (agent_id, action_id) that last wrote it, not yet verified
    pending_writes: HashMap<PathBuf, (String, String)>,
}

impl FileWriteTracker {
    /// Returns conflict info if another agent has an unverified write to this path.
    pub fn claim(&mut self, path: &Path, agent_id: &str, action_id: &str)
        -> Option<(String, String)>; // conflicting (agent_id, action_id)
    /// Called when LoopVerified(passed=true) lands for agent_id's writes.
    pub fn release_agent(&mut self, agent_id: &str);
}
```

**2. Populate from `LoopActed` events in orchestrator**:
When a `LoopActed { action_kind: "apply_patch" | "write_file", ... }` arrives from any
agent, parse the file path from `stdout`/`stderr` and call `tracker.claim(path, agent_id, action_id)`.

**3. Conflict resolution policy** (three options, choose per `GatePolicy`):
- `Block` — reject second write; emit `LoopActed { success: false, stderr: "conflict:pending_write" }`
- `Queue` — push the second write back into the scheduler with `depends_on: [first_action_id]`
- `Merge` — attempt 3-way merge (see CTRL-6)

**4. Path extraction helper**:
```rust
pub fn extract_written_paths(action_kind: &str, payload: &serde_json::Value) -> Vec<PathBuf>;
```
Parse the `file` field from `apply_patch` payloads, or the `path` field from `write_file`.

**Files to touch:**
- `canon-loop/src/merge.rs` — new file
- `canon-loop/src/executor.rs` — call `file_write_tracker.claim()` on LoopActed
- `canon-loop/src/lib.rs` — `mod merge;`

**Priority:** High — without this, parallel sub-agents will corrupt each other's edits.

---

## CTRL-6: Context Merge (batch_acted from Multiple Agents) — NOT IMPLEMENTED

**Goal:** When `SubTaskResult` arrives from a sub-agent, merge its recent action history
(`batch_acted`) into the orchestrator's `LoopContext` without losing either agent's state.

### What to add

**1. `MergedActionEntry`** — `canon-loop/src/merge.rs`:
```rust
pub struct MergedActionEntry {
    pub agent_id: String,
    pub action_kind: String,
    pub success: bool,
    pub stdout_summary: String,  // truncated to 256 chars
    pub ts: u64,
}
```

**2. `ContextMerger`** — `canon-loop/src/merge.rs`:
```rust
pub struct ContextMerger {
    pub merged_actions: Vec<MergedActionEntry>,  // bounded to 32 entries
}

impl ContextMerger {
    /// Absorb a sub-agent's SubTaskResult into the merged log.
    pub fn absorb(&mut self, result: &SubTaskResult, agent_id: &str);
    /// Produce a summary string suitable for injection into the orchestrator's planner prompt.
    pub fn prompt_section(&self) -> String;
}
```

**3. Wire into orchestrator `LoopContext`**:
Add `pub context_merger: ContextMerger` to `LoopContext`.
In `canon-loop/src/executor.rs`, on `RuntimeEvent::SubTaskResult`, call:
```rust
self.ctx.context_merger.absorb(&result, &result.agent_id);
```

**4. Expose in planner prompt** — `canon-loop/src/stage/plan.rs`:
```rust
if !ctx.context_merger.merged_actions.is_empty() {
    prompt.push_str(&format!("\n## Sub-Agent Actions\n{}", ctx.context_merger.prompt_section()));
}
```

**Files to touch:**
- `canon-loop/src/merge.rs` — `MergedActionEntry`, `ContextMerger`
- `canon-loop/src/context.rs` — add `context_merger: ContextMerger`
- `canon-loop/src/executor.rs` — call `absorb()` on SubTaskResult
- `canon-loop/src/stage/plan.rs` — add merged section to prompt

**Priority:** Medium — needed for the orchestrator to make informed routing decisions
after sub-agents complete work.

---

## CTRL-7: State Reconciliation — NOT IMPLEMENTED

**Goal:** When two sub-agents both report `workspace_dirty=true` (or one reports it and
the other has already triggered verify), reconcile which verify pass covers which writes.

### What to add

**1. `WorkspaceDirtyTracker`** — `canon-loop/src/merge.rs`:
```rust
pub struct WorkspaceDirtyTracker {
    /// agent_id → set of action_ids with unverified writes
    dirty_by_agent: HashMap<String, HashSet<String>>,
}

impl WorkspaceDirtyTracker {
    pub fn mark_dirty(&mut self, agent_id: &str, action_id: &str);
    pub fn mark_verified(&mut self, agent_id: &str);
    /// True if ANY agent has unverified writes (conservative: triggers verify)
    pub fn any_dirty(&self) -> bool;
    /// True if ALL agents are clean
    pub fn all_clean(&self) -> bool;
}
```

**2. Replace single `workspace_dirty: bool` in `LoopContext`** with `WorkspaceDirtyTracker`:
- `ctx.workspace_dirty` → `ctx.dirty_tracker.any_dirty()`
- On `LoopActed` from agent: `dirty_tracker.mark_dirty(agent_id, action_id)`
- On `LoopVerified` from agent: `dirty_tracker.mark_verified(agent_id)`

**3. Update `RouteContext` signals**:
- `workspace_dirty` field used in `RouteContext::signals()` — derive from `dirty_tracker.any_dirty()`
- `finish_ready` — only set when `dirty_tracker.all_clean() && system_satisfied`

**4. Reconcile `acted_unverified`** similarly:
```rust
pub acted_unverified_by_agent: HashMap<String, bool>,
```
Derive `ctx.acted_unverified` as `acted_unverified_by_agent.values().any(|&v| v)`.

**Files to touch:**
- `canon-loop/src/merge.rs` — `WorkspaceDirtyTracker`
- `canon-loop/src/context.rs` — replace `workspace_dirty`, `acted_unverified` with tracker
- `canon-route/src/context.rs` — same replacement in RouteContext, derived from tracker state

**Priority:** Medium — needed for correct routing when running parallel agents.

---

## Recommended Implementation Order

Based on dependencies and impact:

1. **CTRL-1** — Priority Scheduler. Prerequisite for parallel dispatch. Low risk, local to executor.
2. **CTRL-5** — File Conflict Detection. Must exist before any parallel writes land, otherwise
   correctness breaks immediately.
3. **CTRL-6** — Context Merge. Sub-agent results need to flow into orchestrator context.
4. **CTRL-7** — State Reconciliation. Correct workspace_dirty / acted_unverified tracking for N agents.
5. **CTRL-2** — Dependency-Aware Dispatch. Enables structured multi-step parallel plans.
6. **CTRL-3** — Causal Event Graph. High debug value, but not blocking.
7. **CTRL-4** — Cross-Agent Provenance. Depends on CTRL-3 + MAGENT-2.

---

## Integration Checklist

Before considering this plan complete:

- [ ] `Scheduler` replaces `act_queue` in `canon-loop/src/executor.rs`
- [ ] `DependencyTracker` blocks dispatch until declared deps complete
- [ ] `FileWriteTracker` gates parallel writes to the same path
- [ ] `ContextMerger` absorbs `SubTaskResult` entries into planner prompt
- [ ] `WorkspaceDirtyTracker` replaces scalar `workspace_dirty` in both LoopContext and RouteContext
- [ ] `CausalGraph` populates on every LoopPlanned / ToolCall / ToolResult / LoopActed
- [ ] `GoalGraphCheckpointed` emitted on each causal graph checkpoint
- [ ] `cargo test -p canon-loop -p canon-route` passes after each CTRL plan lands
