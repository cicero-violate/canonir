# Canon Control Layer — Implementation Plan v2

> Generated 2026-03-23. Audited against current codebase.
> Previous plan: `FEATURES_IMPL_control_layer.md`.

---

## Status Summary

| Plan   | Title                         | Status          | Notes                                                     |
|--------+-------------------------------+-----------------+-----------------------------------------------------------|
| CTRL-1 | Priority Scheduler            | IMPLEMENTED     | BinaryHeap, infer_priority, agent capacity partial        |
| CTRL-2 | Dependency-Aware Dispatch     | NOT IMPLEMENTED | depends_on missing from LoopPlanned, no DependencyTracker |
| CTRL-3 | Causal Event Graph            | NOT IMPLEMENTED | causal.rs not created                                     |
| CTRL-4 | Cross-Agent Provenance        | NOT IMPLEMENTED | Depends on CTRL-3 + MAGENT-2                              |
| CTRL-5 | File-Level Conflict Detection | IMPLEMENTED     | FileWriteTracker exists; claim() not called pre-dispatch  |
| CTRL-6 | Context Merge (batch_acted)   | IMPLEMENTED     | ContextMerger works; prompt section included              |
| CTRL-7 | State Reconciliation          | PARTIAL         | LoopContext uses tracker; RouteContext still uses bool    |

---

## CTRL-1: Priority Scheduler — IMPLEMENTED

**What exists:**
- `canon-loop/src/scheduler.rs` — `TaskPriority` enum, `ScheduledTask`, `Scheduler` with `BinaryHeap`
- `infer_priority()` considers `action_kind == "done"`, `goodness`, `delta_g`
- `canon-loop/src/context.rs` — `scheduler: Scheduler` replaces old `act_queue: VecDeque`
- `canon-loop/src/executor.rs` — `LoopPlanned` events pushed to scheduler with computed priority

**What's missing:**
- Per-agent capacity limits (`agent_capacity: HashMap`, `agent_active: HashMap`) not implemented
- `pop_for_agent(agent_id)` exists as `pop_for_llm()` with optional request_id filter — not true agent-scoped dispatch

**Pending work:**

1. Add capacity maps to `Scheduler` in `canon-loop/src/scheduler.rs`:
   ```rust
   agent_capacity: HashMap<String, usize>,
   agent_active:   HashMap<String, usize>,
   ```

2. Add `has_capacity(agent_id) -> bool` and `complete(agent_id)` to decrement active count.

3. On `AgentRegistered` event in `canon-loop/src/executor.rs`, populate `scheduler.agent_capacity`
   from the `capacity` field.

**Priority:** Low — current single-agent setup does not need this. Prerequisite for MAGENT-4.

---

## CTRL-2: Dependency-Aware Dispatch — NOT IMPLEMENTED

**What's missing:** Everything. `LoopPlanned` has no `depends_on` field. `DependencyTracker`
does not exist. `GoalEdgeDefined` is never emitted.

**Pending work:**

1. Add `depends_on` to `LoopPlanned` in `canon-runtime-events/src/events.rs`:
   ```rust
   #[serde(default)]
   pub depends_on: Vec<String>,  // action_ids that must complete before dispatch
   ```

2. Add `DependencyTracker` to `canon-loop/src/scheduler.rs`:
   ```rust
   pub struct DependencyTracker {
       waiting:  HashMap<String, HashSet<String>>,    // action_id → deps not yet done
       unblocks: HashMap<String, Vec<ScheduledTask>>, // action_id → tasks it unblocks
   }

   impl DependencyTracker {
       pub fn add(&mut self, task: ScheduledTask);
       /// Returns tasks newly unblocked by this completion.
       pub fn complete(&mut self, action_id: &str) -> Vec<ScheduledTask>;
   }
   ```

3. Wire in `canon-loop/src/executor.rs`:
   - On `LoopPlanned`: if `plan.depends_on` is non-empty call `dep_tracker.add(task)`
     instead of `scheduler.push(task)`
   - On `LoopActed`: call `dep_tracker.complete(action_id)`, push returned tasks to scheduler

4. Emit `GoalEdgeDefined` from `canon-loop/src/stage/plan.rs` when LLM response contains
   `"depends_on": ["<action_id>"]`.

**Priority:** Medium — required for correct ordering of parallel multi-step plans.

---

## CTRL-3: Causal Event Graph — NOT IMPLEMENTED

**What's missing:** `canon-route/src/causal.rs` does not exist. No causal graph is built.
`canon-storage-graph` is not a dependency of `canon-route`.

**Pending work:**

1. Add `canon-storage-graph` to `canon-route/Cargo.toml`.

2. Create `canon-route/src/causal.rs`:
   ```rust
   #[derive(Debug, Clone)]
   pub enum CausalNodeKind {
       LlmCall     { request_id: String, role: Option<String>, agent_id: Option<String> },
       LoopPlanned { action_id: String, action_kind: String },
       ToolCall    { tool_call_id: String, kind: String },
       ToolResult  { tool_result_id: String, success: bool },
       LoopActed   { action_id: Option<String>, success: bool },
       LoopVerified{ passed: bool },
   }

   pub struct CausalGraph {
       builder:    GraphBuilder<CausalNodeKind, &'static str>,
       node_index: HashMap<String, NodeId>,
       frozen:     Option<CsrGraph<CausalNodeKind, &'static str>>,
   }

   impl CausalGraph {
       pub fn record_llm_call(&mut self, request_id: &str, role: Option<&str>, agent_id: Option<&str>);
       pub fn record_planned(&mut self, action_id: &str, llm_request_id: &str, action_kind: &str);
       pub fn record_tool_call(&mut self, tool_call_id: &str, action_id: &str);
       pub fn record_tool_result(&mut self, tool_result_id: &str, tool_call_id: &str, success: bool);
       pub fn record_acted(&mut self, action_id: Option<&str>, tool_result_id: Option<&str>);
       pub fn checkpoint(&mut self) -> &CsrGraph<CausalNodeKind, &'static str>;
       pub fn causal_chain_for_failure(&self, tool_result_id: &str) -> String;
   }
   ```

3. Add `causal_graph: CausalGraph` to `RouteContext` in `canon-route/src/context.rs`.
   Populate on each relevant event inside `update_from_event()`.
   The existing `action_meta` HashMap already has `action_id → (action_kind, llm_request_id)` —
   use it to draw edges.

4. Add `pub mod causal;` to `canon-route/src/lib.rs`.

**Priority:** Medium — high debugging value once sub-agents run. Not blocking for correctness.

---

## CTRL-4: Cross-Agent Provenance Tracking — NOT IMPLEMENTED

**What's missing:** CTRL-3 does not exist. `RequestDispatch` is never emitted (MAGENT-2).

**Pending work** (after CTRL-3 and MAGENT-2 land):

1. Add variants to `CausalNodeKind` in `canon-route/src/causal.rs`:
   ```rust
   RequestDispatch { dispatch_id: String, from_agent: String, to_agent: String },
   SubTaskResult   { dispatch_id: String, agent_id: String, success: bool },
   ```

2. In `update_from_event()` in `canon-route/src/context.rs`, handle:
   - `RuntimeEvent::RequestDispatch` → `causal_graph.record_dispatch(...)`
   - `RuntimeEvent::SubTaskResult` → `causal_graph.record_sub_result(...)`
   - Draw edge: `RequestDispatch → LoopPlanned` via shared `llm_request_id`

3. Emit `GoalGraphCheckpointed` on each `CausalGraph::checkpoint()` call.

**Priority:** Low — depends on CTRL-3 + MAGENT-2. Observability only, not blocking.

---

## CTRL-5: File-Level Conflict Detection — IMPLEMENTED (pre-dispatch check missing)

**What exists:**
- `canon-loop/src/merge.rs` — `FileWriteTracker` with `claim()`, `release()`, `release_agent()`
- `canon-loop/src/context.rs` — `file_write_tracker: FileWriteTracker` field
- `canon-loop/src/executor.rs` — `release()` called on `LoopActed`; `release_agent()` on verify

**What's missing:**
- `claim()` is defined but **never called before dispatch**. A second agent can be dispatched
  to write the same file before the first write is verified.

**Pending work:**

1. In `canon-loop/src/executor.rs`, before dispatching a `ScheduledTask`, extract target
   file paths from the action payload and call `file_write_tracker.claim()`:
   ```rust
   for path in extract_written_paths(&task.plan.action_kind, &task.plan.action_payload) {
       if let Some((conflict_agent, conflict_action)) = self.ctx.file_write_tracker.claim(&path, agent_id, &action_id) {
           // Apply conflict policy: Block / Queue / Merge
           // Default: re-enqueue task with depends_on: [conflict_action_id]
       }
   }
   ```

2. Add `extract_written_paths()` helper to `canon-loop/src/merge.rs` (may already exist —
   verify at lines 110-129).

**Priority:** High — without this, concurrent writes from parallel sub-agents will corrupt files.

---

## CTRL-6: Context Merge (batch_acted) — IMPLEMENTED

**What exists:**
- `canon-loop/src/merge.rs` — `MergedActionEntry`, `ContextMerger` with `absorb()` and `prompt_section()`
- `canon-loop/src/context.rs` — `context_merger: ContextMerger`
- `canon-loop/src/executor.rs` line 82 — `self.ctx.context_merger.absorb(r, &r.agent_id)` on SubTaskResult
- `canon-loop/src/stage/plan.rs` — `sub_agent_section` passed to `build_prompt()` and included in prompt

**What's missing:**
- Nothing structural. Works end-to-end once `SubTaskResult` events actually arrive (blocked by MAGENT-4).

**No pending work** for this item beyond what MAGENT-4 enables.

---

## CTRL-7: State Reconciliation — PARTIAL

**What exists:**
- `canon-loop/src/merge.rs` — `WorkspaceDirtyTracker` with `mark_dirty()`, `mark_verified()`, `any_dirty()`, `all_clean()`
- `canon-loop/src/context.rs` — `dirty_tracker: WorkspaceDirtyTracker` (replaces scalar bool)
- `canon-loop/src/executor.rs` — `mark_dirty("orchestrator", ...)` on LoopActed; `mark_verified("orchestrator")` on LoopVerified

**What's missing:**
- `canon-route/src/context.rs` still uses `workspace_dirty: bool` (line 16), updated with raw assignments at lines 124, 146, 210
- The tracker is not in RouteContext — routing decisions use a stale scalar

**Pending work:**

1. In `canon-route/src/context.rs`, import `WorkspaceDirtyTracker` from `canon_loop::merge`
   (or move the struct to a shared crate like `canon-runtime-events`):
   ```rust
   pub workspace_dirty_tracker: WorkspaceDirtyTracker,
   ```
   Derive `workspace_dirty` as `self.workspace_dirty_tracker.any_dirty()` in `signals()` and
   `snapshot_text()`.

   Alternatively (simpler for now): keep the bool but derive it from events that carry
   agent context:
   - On `LoopActed`: `self.workspace_dirty = true` only when `agent_id == "orchestrator"` or unknown
   - On `SubTaskResult`: `self.workspace_dirty |= result.actions_taken > 0`

2. Update `finish_ready` derivation in `canon-route/src/context.rs` to use
   `dirty_tracker.all_clean()` once the tracker is wired.

**Priority:** Medium — correctness issue when running multiple agents. Low urgency with single-agent today.

---

## Recommended Implementation Order (v2)

1. **CTRL-5** (claim pre-dispatch) — one method call, prevents file corruption. Do now.
2. **CTRL-7** (RouteContext dirty tracker) — small change, correctness fix for routing.
3. **CTRL-2** (DependencyTracker + depends_on) — enables structured parallel plans.
4. **CTRL-1** (agent capacity maps) — needed once MAGENT-4 spawns multiple agents.
5. **CTRL-3** (causal graph) — after multi-agent runs, for debugging.
6. **CTRL-4** (cross-agent provenance) — after CTRL-3 + MAGENT-2.
