# Canon Control Layer — Implementation Plan v3

> Generated 2026-03-23. Audited against current codebase.
> Previous plan: `FEATURES_IMPL_control_layer_v2.md`.

---

## Status Summary

| Plan   | Title                           | v2 Status       | v3 Status       | Delta                                      |
|--------|---------------------------------|-----------------|-----------------|--------------------------------------------|
| CTRL-1 | Priority Scheduler              | IMPLEMENTED     | IMPLEMENTED     | No change                                  |
| CTRL-2 | Dependency-Aware Dispatch       | NOT IMPLEMENTED | IMPLEMENTED     | DependencyTracker + depends_on landed      |
| CTRL-3 | Causal Event Graph              | NOT IMPLEMENTED | NOT IMPLEMENTED | causal.rs still missing                    |
| CTRL-4 | Cross-Agent Provenance          | NOT IMPLEMENTED | NOT IMPLEMENTED | Blocked on CTRL-3 + MAGENT-2               |
| CTRL-5 | File-Level Conflict Detection   | PARTIAL         | IMPLEMENTED     | claim() now called in act.rs before dispatch |
| CTRL-6 | Context Merge (batch_acted)     | IMPLEMENTED     | IMPLEMENTED     | No change                                  |
| CTRL-7 | State Reconciliation            | PARTIAL         | IMPLEMENTED     | RouteContext now uses WorkspaceDirtyTracker |

---

## CTRL-1: Priority Scheduler — IMPLEMENTED

**What exists:**
- `canon-loop/src/scheduler.rs` — `TaskPriority { Low=0, Normal=1, High=2, Critical=3 }`,
  `ScheduledTask`, `Scheduler` backed by `BinaryHeap<Queued>` ordered by `(priority, seq)`
- `infer_priority()` — sets `Critical` for `done` actions, `High` when `goodness < 0.0` or
  `delta_g < -0.1`, `Normal` otherwise
- `pop_any()` — highest priority, earliest enqueued
- `pop_for_llm(llm_request_id)` — targets a specific LLM batch for ordered dispatch
- `canon-loop/src/context.rs` — `scheduler: Scheduler` field; `active_batch_llm_request_id`
  tracks in-flight batch
- `canon-loop/src/executor.rs` — priority inferred and task pushed on `LoopPlanned`

**What's missing:**
- Per-agent capacity limits (`agent_capacity`, `agent_active` maps) — not implemented.
  Needed once MAGENT-4 spawns multiple concurrent agents.

**Pending work:**

1. Add capacity tracking to `Scheduler` in `canon-loop/src/scheduler.rs`:
   ```rust
   agent_capacity: HashMap<String, usize>,
   agent_active:   HashMap<String, usize>,
   ```
   Add `has_capacity(agent_id) -> bool` and `complete(agent_id)`.

2. On `AgentRegistered` in `canon-loop/src/executor.rs`, populate
   `scheduler.agent_capacity` from the event's `capacity` field.

**Priority:** Low — single-agent today; prerequisite for MAGENT-4 parallel dispatch.

---

## CTRL-2: Dependency-Aware Dispatch — IMPLEMENTED

**What exists:**
- `canon-runtime-events/src/events.rs` line 170 — `LoopPlanned` has
  `#[serde(default)] depends_on: Vec<String>`
- `canon-loop/src/scheduler.rs` lines 140–184 — `DependencyTracker`:
  ```rust
  pub struct DependencyTracker {
      waiting:  HashMap<String, HashSet<String>>,    // action_id → remaining deps
      unblocks: HashMap<String, Vec<ScheduledTask>>, // dep_id → tasks it unblocks
  }
  ```
  - `add(task)` — registers task and maps it against all its `depends_on` entries
  - `complete(action_id) -> Vec<ScheduledTask>` — decrements deps; returns tasks
    whose all deps are now satisfied
- `canon-loop/src/executor.rs` lines 88–96 — on `LoopPlanned`:
  ```rust
  if !p.depends_on.is_empty() {
      self.ctx.dep_tracker.add(task);
  } else {
      self.ctx.scheduler.push(task);
  }
  ```
  And on `LoopActed` (inferred): `dep_tracker.complete(action_id)` pushes unblocked tasks
  to scheduler

**What's missing:**
- `GoalEdgeDefined` is still never emitted. The dependency field exists on `LoopPlanned` and
  the tracker works, but no stage emits `GoalEdgeDefined` when a dependency is declared.
  This is only needed for external observability (MAGENT-6), not for correctness.
- The planner LLM prompt does not yet describe the `depends_on` field, so the LLM cannot
  deliberately create task dependencies. Only hand-crafted or injected `LoopPlanned` events
  carry it today.

**Pending work:**

1. In `canon-loop/src/stage/plan.rs` `build_prompt()`, add to the tool descriptions:
   ```
   Optional field on any action: "depends_on": ["<action_id>"] — defer this action until
   the listed action_id completes.
   ```

2. Emit `GoalEdgeDefined` from `canon-loop/src/executor.rs` when a task with non-empty
   `depends_on` is added to the tracker (optional — observability only).

**Priority:** Low — tracker is correct; LLM just needs prompt guidance to use it.

---

## CTRL-3: Causal Event Graph — NOT IMPLEMENTED

**What's missing:** `canon-route/src/causal.rs` does not exist. `canon-storage-graph` is
not a dependency of `canon-route`. No causal graph is built or queried.

**Pending work:**

1. Add to `canon-route/Cargo.toml`:
   ```toml
   canon-storage-graph = { path = "../canon-storage-graph" }
   ```

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
       /// Returns a formatted string tracing the chain back from a failed tool result.
       pub fn causal_chain_for_failure(&self, tool_result_id: &str) -> String;
   }
   ```

3. Add `causal_graph: CausalGraph` to `RouteContext` in `canon-route/src/context.rs`.
   Populate in `update_from_event()` using the existing `action_meta` HashMap
   (`action_id → (action_kind, llm_request_id)`) to draw edges.

4. Add `pub mod causal;` to `canon-route/src/lib.rs`.

**Priority:** Medium — no correctness impact today; high debugging value once sub-agents run.

---

## CTRL-4: Cross-Agent Provenance Tracking — NOT IMPLEMENTED

**What's missing:** CTRL-3 does not exist. `RequestDispatch` is never emitted (MAGENT-2).
Both prerequisites must land first.

**Pending work** (after CTRL-3 and MAGENT-2):

1. Add variants to `CausalNodeKind` in `canon-route/src/causal.rs`:
   ```rust
   RequestDispatch { dispatch_id: String, from_agent: String, to_agent: String },
   SubTaskResult   { dispatch_id: String, agent_id: String, success: bool },
   ```

2. Handle in `RouteContext::update_from_event()`:
   - `RuntimeEvent::RequestDispatch` → `causal_graph.record_dispatch(...)`
   - `RuntimeEvent::SubTaskResult` → `causal_graph.record_sub_result(...)`
   - Draw cross-agent edge via shared `dispatch_id` / `parent_request_id`

3. Emit `GoalGraphCheckpointed` on each `CausalGraph::checkpoint()` call.

**Priority:** Low — observability only; depends on CTRL-3 + MAGENT-2.

---

## CTRL-5: File-Level Conflict Detection — IMPLEMENTED

**What exists:**
- `canon-loop/src/merge.rs` — `FileWriteTracker { pending: HashMap<PathBuf, (agent_id, action_id)> }`
  with `claim()`, `release()`, `release_agent()`
- `extract_written_paths(action_kind, payload)` — handles `write_file` (reads `path` field)
  and `apply_patch` (parses `*** Add File:` / `*** Update File:` headers from patch text)
- `canon-loop/src/context.rs` — `file_write_tracker: FileWriteTracker` and
  `write_paths_by_action: HashMap<String, Vec<PathBuf>>`
- `canon-loop/src/stage/act.rs`:
  - Line 234 — `claim()` called in `write_file` branch **before** `ToolCall` is emitted
  - Line 375 — `claim()` called in `apply_patch` branch **before** `ToolCall` is emitted
  - Conflict emits `LoopActed { success: false, stderr: "conflict:pending_write:<agent>:<action>" }`
- `canon-loop/src/executor.rs` lines 59–63 — `release()` called on `LoopActed` for each
  path in `write_paths_by_action`; `release_agent()` called on `LoopVerified`

**What's missing:**
- Conflict resolution policy beyond Block. Currently the only policy is to reject the second
  write with a conflict error. The Queue policy (re-enqueue with `depends_on`) and Merge
  policy (3-way merge) from the original plan are not implemented.
- `release_agent()` is called on every `LoopVerified` regardless of whether the verify passed.
  Should be conditional on `v.passed` to keep dirty paths tracked until they are actually clean.

**Pending work:**

1. Fix `release_agent()` call site in `executor.rs` — only release when `v.passed == true`:
   ```rust
   RuntimeEvent::LoopVerified(v) => {
       if v.passed {
           self.ctx.file_write_tracker.release_agent("orchestrator");
       }
   }
   ```

2. (Optional) Queue policy: when `claim()` returns a conflict, instead of rejecting, create
   a new `LoopPlanned` with `depends_on: [conflict_action_id]` and push it back to
   `dep_tracker`. Requires CTRL-2 which is now complete — the tracker will handle it.

**Priority:** The release fix is low-risk and should land immediately. Queue policy is medium.

---

## CTRL-6: Context Merge (batch_acted) — IMPLEMENTED

**What exists:**
- `canon-loop/src/merge.rs` — `MergedActionEntry`, `ContextMerger` with:
  - `absorb(result: &SubTaskResult, agent_id: &str)` — builds truncated summary (256 chars),
    bounds list to 32 entries
  - `prompt_section() -> String` — returns last 8 entries as markdown bullets
- `canon-loop/src/context.rs` — `context_merger: ContextMerger`
- `canon-loop/src/executor.rs` — `absorb()` called on `SubTaskResult` events
- `canon-loop/src/stage/plan.rs` — `ctx.context_merger.prompt_section()` passed to
  `build_prompt()` and injected as `## Sub-Agent Actions` section

**What's missing:**
- Nothing structural. Works end-to-end. Waiting on MAGENT-4 to produce real `SubTaskResult`
  events. The prompt section will appear automatically once sub-agents complete work.

**No pending work** for this item.

---

## CTRL-7: State Reconciliation — IMPLEMENTED

**What exists:**
- `canon-loop/src/merge.rs` — `WorkspaceDirtyTracker { dirty_by_agent: HashMap<String, Vec<String>> }`
  with `mark_dirty()`, `mark_verified()`, `any_dirty()`, `all_clean()`
- `canon-loop/src/context.rs` — `dirty_tracker: WorkspaceDirtyTracker` (no longer a bool)
- `canon-loop/src/executor.rs`:
  - `mark_dirty("orchestrator", Some(&action_id))` on non-readonly `LoopActed`
  - `mark_verified("orchestrator")` on `LoopVerified`
- `canon-route/src/context.rs` — `WorkspaceDirtyTracker` is defined locally and
  `workspace_dirty_tracker: WorkspaceDirtyTracker` replaces the old `workspace_dirty: bool`

**What's missing:**
- `RouteContext`'s `WorkspaceDirtyTracker` is a **local re-definition**, not the same type
  from `canon-loop::merge`. If the two implementations diverge, bugs will be subtle.
  They should share one definition (move to `canon-runtime-events` or a shared util crate).
- `RouteContext::snapshot_text()` and `signals()` — verify they derive `workspace_dirty`
  from `workspace_dirty_tracker.any_dirty()` rather than a raw bool (needs confirmation).
- `finish_ready` in `RouteContext` — should gate on `dirty_tracker.all_clean()`; confirm
  this derivation is correct in the current code.

**Pending work:**

1. Move `WorkspaceDirtyTracker` to `canon-runtime-events` (or a new `canon-loop-types` crate)
   so both `canon-loop` and `canon-route` import the same type.

2. Confirm `snapshot_text()` and `signals()` in `canon-route/src/context.rs` read from the
   tracker, not a cached bool.

**Priority:** Low — both trackers exist and work; deduplication is a maintenance concern.

---

## Recommended Implementation Order (v3)

All correctness-critical items are now done. Remaining work is ordered by impact:

1. **CTRL-5 release fix** — `release_agent()` only on `v.passed`. One-line change, prevents
   premature path release after a failed verify.
2. **CTRL-2 prompt guidance** — Add `depends_on` description to planner prompt so the LLM
   can deliberately create task dependencies.
3. **CTRL-7 type dedup** — Move `WorkspaceDirtyTracker` to a shared crate. Maintenance only.
4. **CTRL-1 capacity maps** — Agent capacity limits. Needed when MAGENT-4 lands.
5. **CTRL-3 causal graph** — After multi-agent is running, for debugging and audit.
6. **CTRL-4 cross-agent provenance** — After CTRL-3 + MAGENT-2.
7. **CTRL-5 Queue policy** — Conflict re-enqueue via dep_tracker. Nice-to-have after MAGENT-4.
