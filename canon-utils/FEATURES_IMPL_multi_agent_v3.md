# Canon Multi-Agent Features — Implementation Plan v3

> Generated 2026-03-23. Audited against current codebase.
> Previous plan: `FEATURES_IMPL_multi_agent_v2.md`.
> Build target: `cargo check -p canon-runtime -p canon-loop -p canon-route`

---

## Status Summary

| Plan     | Title                            | v2 Status       | v3 Status       | Delta                                              |
|----------+----------------------------------+-----------------+-----------------+----------------------------------------------------|
| MAGENT-1 | agent_id on LlmCall              | IMPLEMENTED     | IMPLEMENTED     | No change                                          |
| MAGENT-2 | RequestDispatch fan-out          | PARTIAL         | PARTIAL         | DispatchConsumer wired; sub-agent loop is stubbed  |
| MAGENT-3 | DecomposeStage                   | NOT IMPLEMENTED | PARTIAL         | decompose.rs exists; hardcoded tasks, no LLM parse |
| MAGENT-4 | Sub-Agent Loop Spawning          | NOT IMPLEMENTED | PARTIAL         | Worker thread spawns; loop body is one-shot stub   |
| MAGENT-5 | Agent Registry Consumer          | NOT IMPLEMENTED | IMPLEMENTED     | Registered in runtime bootstrap                    |
| MAGENT-6 | GoalNode DAG wiring              | NOT IMPLEMENTED | NOT IMPLEMENTED | Events defined, never emitted                      |
| MAGENT-7 | capability_config.toml agents    | PARTIAL         | PARTIAL         | Agent cards exist; not consulted during decompose  |
| MAGENT-8 | Result merging into orchestrator | PARTIAL         | PARTIAL         | ContextMerger absorbs SubTaskResult; flow is stub  |

---

## Compile Errors Fixed in This Session

| File                             | Issue                                           | Fix                        |
|----------------------------------+-------------------------------------------------+----------------------------|
| `canon-route/src/context.rs:306` | `LoopPlanned` test literal missing `depends_on` | Added `depends_on: vec![]` |

All three crates now build clean (`cargo check -p canon-runtime -p canon-loop -p canon-route`).

---

## MAGENT-1: agent_id on LlmCall — IMPLEMENTED

No change since v2. `LlmCall { agent_id: Option<String> }` exists in events.rs. Currently
always `None`. Will be populated once sub-agent identity flows through (MAGENT-4).

---

## MAGENT-2: RequestDispatch fan-out — PARTIAL

**What exists:**
- `RequestDispatch` / `SubTaskResult` structs fully defined in events.rs
- `DispatchConsumer` in `canon-runtime/src/consumers/dispatch_consumer.rs` (92 lines):
  - One persistent worker thread per `agent_id` (lazy-spawned on first dispatch)
  - Workers receive via `crossbeam_channel::unbounded::<RequestDispatch>()`
  - Per-dispatch isolated workspace at `/workspace/ai_sandbox/canon/state/sub_agents/{dispatch_id}`
  - Immediately emits `SubTaskResult { success: true, note: "sub-agent loop stub" }` — **no real work**
- `DispatchConsumer` and `AgentRegistryConsumer` both registered in runtime bootstrap

**What's missing:**

**Issue 1 — Sub-agent loop is a stub (one-shot PlanTrigger, no loop):**

Current code in `dispatch_consumer.rs` lines 40–65:
```rust
let plan_event = LoopStageEvent::PlanTrigger(...);
let _ = plan_event.execute(&mut loop_ctx);
// Simplified: emit SubTaskResult immediately; in a full impl we would run the loop.
let result = SubTaskResult { ..., success: true, output: json!({"note":"sub-agent loop stub"}) };
emitter.emit(RuntimeEvent::SubTaskResult(result));
```
This triggers one plan stage then quits. There is no observe → plan → act → verify cycle.

**Issue 2 — Sub-agent events not forwarded to parent:**

`LoopStageExecutor` and `RouteExecutor` are set up with `parent_emitter` but only
`SubTaskResult` is explicitly emitted. `LoopPlanned`, `LoopActed`, `LoopVerified` from the
sub-agent are silently dropped (they go into the sub-agent's tlog only, not the parent bus).

**Pending work:**

1. **Replace stub with a real event loop** in `dispatch_consumer.rs`:
   ```rust
   // Replace the one-shot plan_event block with:
   let bus_and_consumers = build_sub_agent_bus(
       workspace.clone(), tlog.clone(), parent_emitter.clone()
   );
   // Inject a synthetic LoopObserved to prime goal_text
   bus_and_consumers.emitter.emit(RuntimeEvent::LoopObserved(LoopObserved {
       tick: 0,
       goal_text: Some(req.task_prompt.clone()),
       error_count: 0,
       warning_count: 0,
       compiler_errors: vec![],
       workspace_facts: vec![],
   }));
   // Run until LoopRewarded(halt=true) or a timeout
   bus_and_consumers.run_until_halt(std::time::Duration::from_secs(120));
   let success = bus_and_consumers.ctx.finish_ready;
   emitter.emit(RuntimeEvent::SubTaskResult(SubTaskResult {
       dispatch_id: req.dispatch_id,
       agent_id: req.agent_id,
       parent_request_id: req.parent_request_id,
       success,
       output: serde_json::json!({}),
       actions_taken: bus_and_consumers.ctx.actions_taken_ids(),
       error: None,
   }));
   ```

2. **Forward sub-agent events to parent bus** — in the sub-agent's event bus, add a
   forwarding consumer that re-emits select events tagged with `agent_id`:
   ```rust
   struct ForwardConsumer {
       parent: EventEmitterHandle,
       agent_id: String,
       forward_kinds: &'static [&'static str],
   }
   ```
   Forward `LoopPlanned`, `LoopActed`, `LoopVerified` so the orchestrator's causal graph
   and `ContextMerger` see sub-agent activity in real time.

**Priority:** Critical — this is the gap between "skeleton" and "working multi-agent system".

---

## MAGENT-3: DecomposeStage — PARTIAL

**What exists:**
- `canon-loop/src/stage/decompose.rs` (46 lines)
- `RouteKind::Decompose` in `canon-decision/src/lib.rs` line 13
- Route → stage mapping in `canon-loop/src/stage/mod.rs` line 64
- `decompose::execute()` called on `LoopStageEvent::Decompose`

**Current behaviour — hardcoded two tasks:**
```rust
// decompose.rs lines 14-31
let impl_dispatch = RequestDispatch { agent_id: "exec", task_kind: "implement", ... };
let docs_dispatch = RequestDispatch { agent_id: "doc_writer", task_kind: "document",
    deps: vec![impl_dispatch.dispatch_id.clone()], ... };
```
This always produces exactly two tasks regardless of the goal. The LLM is never consulted
for decomposition. The `LlmCall` emitted (lines 35–43) is a marker only — its response
is never parsed.

**What's missing:**

**Issue 1 — No LLM output parsing:**

The decompose stage needs to ask the LLM how to split the goal, then parse the response
into an arbitrary list of `RequestDispatch` entries. The planner prompt format should be:

```
## Goal
{goal_text}

## Available Agents
{agent_cards from capability_config.toml}

Respond with a JSON array of tasks:
[
  { "agent_id": "exec", "task_kind": "implement", "task_prompt": "...", "deps": [] },
  { "agent_id": "doc_writer", "task_kind": "document", "task_prompt": "...", "deps": ["<impl_dispatch_id>"] }
]
```

**Issue 2 — Agent registry not consulted:**

`decompose.rs` hardcodes `agent_id: "exec"` and `agent_id: "doc_writer"`. It should call
`agent_registry.available_agents(role)` to dynamically select agents based on what is
currently idle and capable.

**Pending work:**

1. Replace the hardcoded two-task block with an LLM call that returns JSON task list:
   ```rust
   // In decompose.rs execute():
   let request_id = format!("decompose-llm-{}", Uuid::new_v4());
   canon_meta::canon_emit_meta!(emitter; Llm(LlmCall {
       request_id: request_id.clone(),
       prompt: build_decompose_prompt(&goal_text, &available_agents),
       role: Some("decompose".to_string()),
       agent_id: None,
   }));
   // Store request_id in LoopContext; parse response in a new on_event handler
   // that handles CapabilityCompleted matching request_id
   ```
   This requires decompose to be asynchronous (emit LLM call, wait for result, then emit
   RequestDispatch). Pattern: same as how plan stage works — emit LlmCall, handle
   CapabilityCompleted.

2. Add `build_decompose_prompt(goal: &str, agents: &[AgentCard]) -> String` to
   `canon-loop/src/stage/decompose.rs`.

3. Add `parse_decompose_response(json: &str, parent_id: &str) -> Vec<RequestDispatch>` to
   parse the LLM array response.

4. Pass `AgentRegistryHandle` into `LoopContext` so `decompose.rs` can query available agents.

**Priority:** High — the hardcoded stub works for demos but breaks on any non-trivial goal.

---

## MAGENT-4: Sub-Agent Loop Spawning — PARTIAL

**What exists:**
- `dispatch_consumer.rs` spawns one `thread::Builder` per unique `agent_id`
- Worker thread receives `RequestDispatch` via `crossbeam_channel`
- Isolated `LoopContext`, `LoopStageExecutor`, `RouteExecutor` are created per dispatch
- Isolated workspace directory created at `/workspace/ai_sandbox/canon/state/sub_agents/{dispatch_id}`
- `SubTaskResult` emitted via `parent_emitter`

**What's missing:**

**Issue 1 — The sub-agent loop body doesn't run:**

```rust
// Lines 40-65 of dispatch_consumer.rs
let plan_event = LoopStageEvent::PlanTrigger(...);
let _ = plan_event.execute(&mut loop_ctx);
// Immediately emits success without waiting for real work
```

One `PlanTrigger.execute()` call does not complete a full agent loop. The agent needs to
cycle through observe → plan → act → verify → reward until `LoopRewarded { halt: true }`.

**Issue 2 — LoopStageExecutor needs an event bus, not one-shot calls:**

`LoopStageExecutor` and `RouteExecutor` are `EventConsumer` implementors — they are designed
to receive events from a bus, not be called directly. The sub-agent needs its own
`EventBus` instance with both consumers registered.

**Issue 3 — Workspace scoping:**

Sub-agent workspace is set to `current_dir()`, not scoped to the sub-task. If two sub-agents
run concurrently they share the same working directory and will conflict.

**Pending work:**

1. Create `canon-loop/src/sub_agent.rs` with a `run_sub_agent_loop()` function:
   ```rust
   pub fn run_sub_agent_loop(
       req: RequestDispatch,
       parent_emitter: EventEmitterHandle,
       base_workspace: PathBuf,
   ) {
       let workspace = base_workspace.join("sub_agents").join(&req.dispatch_id);
       std::fs::create_dir_all(&workspace).ok();
       let tlog = workspace.join("event.tlog.d");

       // Build isolated event bus
       let (bus, emitter) = canon_runtime::EventBus::new(tlog.clone());

       // Register consumers
       let mut loop_exec = LoopStageExecutor::new(workspace.clone(), tlog.clone());
       loop_exec.set_emitter(emitter.clone());
       let mut route_exec = RouteExecutor::new(workspace.clone());
       route_exec.set_emitter(emitter.clone());
       let forward = ForwardConsumer::new(parent_emitter.clone(), &req.agent_id);

       bus.register(Box::new(loop_exec));
       bus.register(Box::new(route_exec));
       bus.register(Box::new(forward));

       // Prime with goal via LoopObserved
       emitter.emit(RuntimeEvent::LoopObserved(LoopObserved {
           goal_text: Some(req.task_prompt.clone()), ..Default::default()
       }));

       // Block until halt or timeout
       bus.run_until(|event| matches!(event, RuntimeEvent::LoopRewarded(r) if r.halt),
                     Duration::from_secs(120));
   }
   ```

2. In `dispatch_consumer.rs`, replace the stub body with a call to `run_sub_agent_loop()`.

3. Scope the workspace to the dispatch ID (prevents concurrent agent conflicts).

**Priority:** Critical — without this, sub-agents do no real work.

---

## MAGENT-5: Agent Registry Consumer — IMPLEMENTED

`AgentRegistryConsumer::new(agent_registry.clone())` and `DispatchConsumer::new()` are
both registered in the runtime bootstrap. `AgentRegistry` tracks `Idle / Busy / Failed`
per agent and correctly transitions on `RequestDispatch` and `SubTaskResult`.

**What's missing:**
- Nothing for basic operation. However, the registry is never queried by `decompose.rs`
  when selecting which agents to dispatch to (see MAGENT-3 Issue 2).

---

## MAGENT-6: GoalNode DAG wiring — NOT IMPLEMENTED

**What exists:** `GoalNodeCreated`, `GoalEdgeDefined`, `GoalNodeRetracted`, `GoalNodeRewritten`,
`GoalGraphCheckpointed` are defined in events.rs and in the wire protocol. Never emitted.

`GoalEdgeDefined` is emitted from `canon-loop/src/executor.rs` lines 99–106 when a
`LoopPlanned` event has non-empty `depends_on` — this is the one partial wire.

**What's missing:**
- `GoalNodeCreated` never emitted (no stage creates goal nodes)
- No consumer builds or maintains a live goal graph
- `GoalGraphCheckpointed` never emitted
- The causal graph in `canon-route/src/causal.rs` is a separate implementation —
  `GoalNode*` events and `CausalGraph` serve overlapping purposes but are not connected

**Pending work:**

1. Emit `GoalNodeCreated` from `decompose.rs` for each `RequestDispatch` created:
   ```rust
   emitter.emit(RuntimeEvent::GoalNodeCreated(GoalNodeCreated {
       node_id:   dispatch.dispatch_id.clone(),
       parent_id: Some(parent_request_id.clone()),
       label:     dispatch.task_prompt.clone(),
       criteria:  vec![],
   }));
   ```

2. Create `canon-runtime/src/consumers/goal_graph_consumer.rs` to maintain a live DAG.

3. Emit `GoalGraphCheckpointed` from `CausalGraph::checkpoint()` once it is wired
   to the goal graph (long-term: merge the two representations).

**Priority:** Low — observability only, not blocking.

---

## MAGENT-7: capability_config.toml agents — PARTIAL

**What exists:** Agent cards in `canon-agent-prompts/capability_config.toml`:
- `decompose` — `builtin:planner`
- `planner` — `builtin:planner`
- `executor` — `builtin:exec`, tool_capabilities: `[apply_patch, bash]`
- `verifier` — `builtin:exec`

**What's missing:**
- Nothing reads the config and emits `AgentRegistered` at startup, so the registry starts empty
- `decompose.rs` hardcodes `agent_id: "exec"` instead of querying the registry

**Pending work:**

1. In `canon-runtime/src/lib.rs` (or the binary bootstrap), after the event bus is set up,
   read agent cards from config and emit `AgentRegistered` for each:
   ```rust
   for card in &config.agents.cards {
       emitter.emit(RuntimeEvent::AgentRegistered(AgentRegistered {
           payload: serde_json::to_value(card).unwrap_or_default(),
       }));
   }
   ```

2. Pass `AgentRegistryHandle` into `LoopContext` and thread it to `decompose.rs` so it can
   call `registry.available_agents("builtin:exec")` when selecting dispatch targets.

**Priority:** Medium — registry starts empty, so agent selection cannot work until this lands.

---

## MAGENT-8: Result merging into orchestrator — PARTIAL

**What exists:**
- `ContextMerger::absorb()` called in `canon-loop/src/executor.rs` on `SubTaskResult`
- `prompt_section()` injected into planner prompt as `## Sub-Agent Actions`

**What's missing:**
- Sub-agents emit `SubTaskResult { success: true, actions_taken: [] }` — the stub returns
  no real action list, so `absorb()` produces empty entries
- Sub-agent workspace changes are not reconciled: if sub-agent wrote files, the orchestrator's
  `WorkspaceDirtyTracker` is not updated (only `ContextMerger` records the event)
- Compiler errors from sub-agent writes are not merged into orchestrator's error feed

**Pending work:**

1. After MAGENT-4 lands, ensure `SubTaskResult.actions_taken` is populated with actual
   `action_id` strings from the sub-agent's `LoopContext`.

2. In `canon-loop/src/executor.rs` `SubTaskResult` handler, propagate dirty state:
   ```rust
   RuntimeEvent::SubTaskResult(r) => {
       self.ctx.context_merger.absorb(r, &r.agent_id);
       if !r.actions_taken.is_empty() {
           for action_id in &r.actions_taken {
               self.ctx.dirty_tracker.mark_dirty(&r.agent_id, Some(action_id));
           }
       }
   }
   ```

3. (Optional) Merge sub-agent compiler errors by emitting a synthetic `LoopObserved` from
   the sub-agent's final verify state into the orchestrator's bus.

**Priority:** Unblocked only after MAGENT-4 produces real results.

---

## Known Issues

| # | File                      | Line   | Issue                                                          | Fix                                                                    |
|---+---------------------------+--------+----------------------------------------------------------------+------------------------------------------------------------------------|
| 1 | `dispatch_consumer.rs`    | 40–65  | Sub-agent loop is one-shot PlanTrigger + immediate stub result | Replace with `run_sub_agent_loop()`                                    |
| 2 | `decompose.rs`            | 14–31  | Hardcoded two tasks (exec + doc_writer), LLM not consulted     | Parse LLM response into arbitrary task list                            |
| 3 | `decompose.rs`            | 17, 26 | `agent_id` hardcoded; registry never queried                   | Query `AgentRegistryHandle` for available agents                       |
| 4 | `dispatch_consumer.rs`    | 28     | Workspace = `current_dir()`, not scoped to dispatch_id         | `workspace = base_workspace.join("sub_agents").join(&req.dispatch_id)` |
| 5 | Runtime bootstrap         | —      | `AgentRegistered` never emitted; registry always empty         | Emit on startup from capability_config.toml                            |
| 6 | `dispatch_consumer.rs`    | —      | Sub-agent LoopPlanned/Acted not forwarded to parent            | Add `ForwardConsumer` wrapping parent emitter                          |
| 7 | `context.rs` (route) test | 306    | `LoopPlanned` missing `depends_on: vec![]`                     | **Fixed in this session**                                              |

---

## Recommended Implementation Order (v3)

The three items that unlock everything else:

1. **MAGENT-4** — `run_sub_agent_loop()` with real event bus. This is the core execution engine.
   Without it, every other MAGENT item is dead code.
2. **MAGENT-7** — Emit `AgentRegistered` from config at startup. One loop in the bootstrap.
3. **MAGENT-3** — Parse LLM decompose response; query registry for agent selection.
4. **MAGENT-2** — `ForwardConsumer` to stream sub-agent events to parent bus.
5. **MAGENT-8** — Propagate dirty state and actions_taken after MAGENT-4 produces real results.
6. **MAGENT-1** — Set `agent_id` on `LlmCall` in sub-agent plan stage.
7. **MAGENT-6** — GoalNode DAG; emit `GoalNodeCreated` from decompose. Observability only.
