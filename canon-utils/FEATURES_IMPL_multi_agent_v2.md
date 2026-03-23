# Canon Multi-Agent Features — Implementation Plan v2

> Generated 2026-03-23. Audited against current codebase.
> Previous plan: `FEATURES_IMPL_multi_agent.md`.

---

## Status Summary

| Plan     | Title                              | Status      | Notes                                                     |
|----------|------------------------------------|-------------|-----------------------------------------------------------|
| MAGENT-1 | agent_id on LlmCall                | IMPLEMENTED | Field exists; always None in practice                     |
| MAGENT-2 | RequestDispatch fan-out            | PARTIAL     | Structs + consumer exist; nothing emits RequestDispatch   |
| MAGENT-3 | DecomposeStage                     | NOT IMPLEMENTED | No decompose.rs, no RouteKind::Decompose              |
| MAGENT-4 | Sub-Agent Loop Spawning            | NOT IMPLEMENTED | No spawn logic anywhere                                   |
| MAGENT-5 | Agent Registry Consumer            | IMPLEMENTED | Consumer works; not registered in runtime bootstrap       |
| MAGENT-6 | GoalNode DAG wiring                | NOT IMPLEMENTED | Events defined, never emitted, no consumer                |
| MAGENT-7 | capability_config.toml agents      | PARTIAL     | Agent cards exist; no role-based dispatch routing         |
| MAGENT-8 | Result merging into orchestrator   | PARTIAL     | ContextMerger exists; no actual SubTaskResult flow        |

---

## MAGENT-1: agent_id on LlmCall — IMPLEMENTED

**What exists:**
- `canon-runtime-events/src/events.rs` — `LlmCall { request_id, prompt, role, agent_id }` with
  `#[serde(default)] agent_id: Option<String>`
- `canon-route/src/executor.rs` — emits `LlmCall { role: Some("router"), agent_id: None }`
- `canon-loop/src/stage/plan.rs` — emits `LlmCall { agent_id: None }`

**What's missing:**
- `agent_id` is always `None`. It should be set to the actual agent identifier once
  sub-agent loops are spawned (MAGENT-4).

**Pending work** (after MAGENT-4):
- In sub-agent `plan.rs`, set `agent_id: Some(self.agent_id.clone())` when emitting LlmCall.
- In `canon-exec` capability routing: filter LLM endpoint by `agent_id` or `role` so that
  specialist agents can use different models or system prompts.

**Priority:** Low — trivial change once sub-agent identities exist.

---

## MAGENT-2: RequestDispatch fan-out — PARTIAL

**What exists:**
- `canon-runtime-events/src/events.rs`:
  - `RequestDispatch { dispatch_id, parent_request_id, agent_id, task_prompt, task_kind, deps, workspace_scope }`
  - `SubTaskResult { dispatch_id, agent_id, parent_request_id, success, output, actions_taken, error }`
  - Both added to `RuntimeEvent` enum (lines 413-414)
- Wire protocol: serialization supported in `canon-runtime-events/src/wire.rs`
- `canon-runtime/src/consumers/agent_registry.rs` — `AgentRegistryConsumer` handles both events to update agent status

**What's missing:**
- `RequestDispatch` is **never emitted** anywhere in the codebase
- `SubTaskResult` is **never emitted** anywhere in the codebase
- No `DispatchConsumer` that routes a `RequestDispatch` to a sub-agent loop

**Pending work:**

1. Create `canon-loop/src/consumers/dispatch_consumer.rs`:
   ```rust
   pub struct DispatchConsumer {
       agent_registry: AgentRegistryHandle,
       sub_agent_tx:   HashMap<String, mpsc::Sender<RequestDispatch>>,
   }

   impl EventConsumer for DispatchConsumer {
       fn on_event(&mut self, event: &RuntimeEvent) {
           if let RuntimeEvent::RequestDispatch(req) = event {
               if let Some(tx) = self.sub_agent_tx.get(&req.agent_id) {
                   let _ = tx.send(req.clone());
               }
           }
       }
   }
   ```

2. Emit `RequestDispatch` from `canon-loop/src/stage/decompose.rs` (MAGENT-3) when the
   orchestrator decides to delegate a sub-task.

3. Emit `SubTaskResult` from the sub-agent loop (MAGENT-4) when a sub-task completes
   (i.e., when its own `finish_ready` becomes true or `done` action succeeds).

**Priority:** High — gate to all remaining multi-agent work. Nothing moves without this.

---

## MAGENT-3: DecomposeStage — NOT IMPLEMENTED

**What's missing:**
- `canon-loop/src/stage/decompose.rs` does not exist
- `RouteKind::Decompose` not present in `canon-decision` crate
- No decompose route handler in `canon-route`

**Pending work:**

1. Add `Decompose` to `RouteKind` enum in `canon-decision/src/lib.rs`:
   ```rust
   pub enum RouteKind { Plan, Act, Verify, Conclude, Decompose }
   ```
   Update `as_str()` and `from_str()` accordingly.

2. Handle `Decompose` in `canon-route/src/helpers.rs` `heuristic_route_json()` and
   `canon-route/src/decision.rs` gatekeeper rules.

3. Create `canon-loop/src/stage/decompose.rs`:
   ```rust
   pub struct DecomposeStage { /* LLM client, agent_registry */ }

   impl DecomposeStage {
       /// Called when route = Decompose.
       /// Asks LLM to split current goal into parallel sub-tasks.
       /// Emits one RequestDispatch per sub-task.
       pub async fn handle_decompose(&self, ctx: &LoopContext, emitter: &EventEmitterHandle);
   }
   ```
   The LLM prompt should include: current mission, current state snapshot, available
   agent roles from `capability_config.toml`, and an instruction to produce a JSON array
   of sub-tasks with `{ task_prompt, agent_id, depends_on }`.

4. Wire `DecomposeStage` into `canon-loop/src/executor.rs`:
   - On `RouteSelected { approved_route: "decompose" }`, call `decompose_stage.handle_decompose()`
   - Emitted `RequestDispatch` events flow back through the event bus to `DispatchConsumer`

**Priority:** High — this is the orchestrator's decompose capability.

---

## MAGENT-4: Sub-Agent Loop Spawning — NOT IMPLEMENTED

**What's missing:** No code spawns a sub-agent. This is the core missing piece of the
multi-agent system.

**Pending work:**

1. Create `canon-loop/src/sub_agent.rs`:
   ```rust
   pub struct SubAgentConfig {
       pub agent_id:      String,
       pub role:          String,
       pub workspace:     PathBuf,
       pub tlog_path:     PathBuf,
       pub parent_emitter: EventEmitterHandle,
   }

   pub fn spawn_sub_agent(
       config: SubAgentConfig,
       rx: mpsc::Receiver<RequestDispatch>,
   ) -> JoinHandle<()> {
       std::thread::spawn(move || {
           // Own LoopContext
           let mut ctx = LoopContext::new();
           ctx.agent_id = config.agent_id.clone();
           // Own tlog
           let tlog = BinarySegmentWriter::new(config.tlog_path, ...);
           // Own event bus (forwards select events to parent_emitter)
           let (bus, emitter) = EventBus::new(tlog, Some(config.parent_emitter.clone()));

           loop {
               let req = rx.recv().expect("dispatch channel closed");
               // Inject RequestDispatch as goal into this loop's context
               ctx.set_mission(&req.task_prompt);
               // Run until done, then emit SubTaskResult to parent emitter
               run_agent_loop(&mut ctx, &bus, &emitter);
               config.parent_emitter.emit(SubTaskResult {
                   dispatch_id:       req.dispatch_id.clone(),
                   agent_id:          config.agent_id.clone(),
                   parent_request_id: req.parent_request_id.clone(),
                   success:           ctx.finish_ready,
                   output:            ctx.last_output.clone(),
                   actions_taken:     ctx.total_actions,
                   error:             None,
               });
           }
       })
   }
   ```

2. In `canon-runtime/src/lib.rs` bootstrap: after `DispatchConsumer` is registered,
   for each agent card in `capability_config.toml`:
   ```rust
   let (tx, rx) = mpsc::channel();
   dispatch_consumer.register_agent(card.agent_id.clone(), tx);
   spawn_sub_agent(SubAgentConfig { agent_id: card.agent_id, ... }, rx);
   ```

3. Sub-agent event forwarding policy — which events should be forwarded to the parent bus:
   - Always forward: `SubTaskResult`, `LoopActed`, `LoopVerified`
   - Never forward: `LoopObserved` (parent has its own observe cycle)
   - Optional: `LoopPlanned` (for full tracing)

**Priority:** Critical — without this, no sub-agents exist and all remaining MAGENT items are inert.

---

## MAGENT-5: Agent Registry Consumer — IMPLEMENTED (not registered)

**What exists:**
- `canon-runtime/src/consumers/agent_registry.rs` (119 lines) — fully implemented:
  - `AgentRegistry` with `agents: HashMap<String, AgentCard>`
  - `AgentCard { agent_id, agent_url, role, tool_capabilities, status }`
  - `AgentStatus { Idle, Busy { dispatch_id }, Failed { reason } }`
  - `AgentRegistryConsumer` handles `AgentRegistered`, `RequestDispatch`, `SubTaskResult`

**What's missing:**
- `AgentRegistryConsumer` is **not registered** in the runtime bootstrap in `canon-runtime/src/lib.rs`

**Pending work:**

1. In `canon-runtime/src/lib.rs` bootstrap, create and register the consumer:
   ```rust
   let registry_handle = AgentRegistryHandle::default();
   let registry_consumer = AgentRegistryConsumer::new(registry_handle.clone());
   bus.register(Box::new(registry_consumer));
   ```

2. Pass `registry_handle` to `DispatchConsumer` and `DecomposeStage` so they can query
   available agents when dispatching work:
   ```rust
   let available = registry_handle.read().available_agents("builtin:exec");
   ```

3. On startup, emit `AgentRegistered` for each card in `capability_config.toml` so the
   registry is populated before any work arrives.

**Priority:** Medium — trivial to register; needed for agent selection in MAGENT-3.

---

## MAGENT-6: GoalNode DAG wiring — NOT IMPLEMENTED

**What's missing:**
- `GoalNodeCreated`, `GoalEdgeDefined`, `GoalGraphCheckpointed`, `GoalNodeRetracted`,
  `GoalNodeRewritten` are defined in events.rs and in the wire protocol but **never emitted**.
- No consumer builds or maintains a goal graph.

**Pending work:**

1. Emit `GoalNodeCreated` from `canon-loop/src/stage/decompose.rs` when a sub-task is
   created for dispatch:
   ```rust
   emitter.emit(GoalNodeCreated {
       node_id:   dispatch_id.clone(),
       parent_id: Some(parent_goal_id.clone()),
       label:     task.task_prompt.clone(),
       criteria:  task.success_criteria.clone(),
   });
   ```

2. Emit `GoalEdgeDefined` when a dependency between tasks is established (from CTRL-2):
   ```rust
   emitter.emit(GoalEdgeDefined {
       parent_id: dep_action_id.clone(),
       child_id:  action_id.clone(),
       kind:      "depends_on".to_string(),
   });
   ```

3. Create `canon-runtime/src/consumers/goal_graph_consumer.rs`:
   ```rust
   pub struct GoalGraphConsumer {
       nodes: HashMap<String, GoalNodeCreated>,
       edges: Vec<GoalEdgeDefined>,
   }
   impl EventConsumer for GoalGraphConsumer { ... }
   ```
   On `GoalGraphCheckpointed` events, serialize the graph to a file for external tooling.

4. Emit `GoalNodeRetracted` from the sub-agent loop when a task is abandoned or superseded.

5. Emit `GoalGraphCheckpointed` from `CausalGraph::checkpoint()` once CTRL-3 lands.

**Priority:** Low — valuable for visualization and audit, not needed for correctness.

---

## MAGENT-7: capability_config.toml specialist agents — PARTIAL

**What exists:**
- `canon-agent-prompts/capability_config.toml` defines agent cards:
  - `decompose` — role `builtin:planner`
  - `planner` — role `builtin:planner`
  - `executor` — role `builtin:exec`, tool_capabilities: `[apply_patch, bash]`
  - `verifier` — role `builtin:exec`
- Wire protocol supports `AgentRegistered { agent_id, role, capacity }`

**What's missing:**
- Nothing reads the agent cards from config and emits `AgentRegistered` at startup
- `DecomposeStage` doesn't query the registry for available agents by role
- No role-based LLM model selection (e.g., executor uses a faster model)

**Pending work:**

1. In `canon-runtime/src/lib.rs` startup, read `capability_config.toml` agent cards and
   emit `AgentRegistered` for each:
   ```rust
   for card in config.agents.cards {
       emitter.emit(AgentRegistered {
           payload: serde_json::to_value(&card).unwrap(),
       });
   }
   ```

2. In `DecomposeStage`, use `registry.available_agents(role)` to select the right agent
   for each sub-task kind (plan tasks → `builtin:planner`, exec tasks → `builtin:exec`).

3. (Optional) Add `model` field to agent card in `capability_config.toml` so that
   `canon-exec` selects a different LLM endpoint per agent role.

**Priority:** Medium — needed once MAGENT-3 and MAGENT-4 exist to select which agents
receive which tasks.

---

## MAGENT-8: Result merging into orchestrator — PARTIAL

**What exists:**
- `canon-loop/src/merge.rs` — `ContextMerger` with `absorb()` and `prompt_section()`
- `canon-loop/src/context.rs` — `context_merger: ContextMerger`
- `canon-loop/src/executor.rs` line 82 — calls `absorb()` on `SubTaskResult`
- `canon-loop/src/stage/plan.rs` — passes `sub_agent_section` to `build_prompt()`

**What's missing:**
- `SubTaskResult` is never emitted (MAGENT-4 blocker), so `absorb()` is never called
- No merging of sub-agent workspace state into orchestrator `workspace_dirty` / `acted_unverified`
- No merging of sub-agent compiler errors into orchestrator's error feed

**Pending work** (after MAGENT-4):

1. Verify `absorb()` correctly truncates output and maintains the 32-entry bound.

2. After `absorb()`, propagate sub-agent dirty state into the orchestrator:
   ```rust
   if result.actions_taken > 0 {
       self.ctx.dirty_tracker.mark_dirty(&result.agent_id, &result.dispatch_id);
   }
   ```

3. (Optional) Merge sub-agent `compiler_errors` into orchestrator `recent_compiler_errors`
   so the orchestrator's planner can see compile failures from sub-agent writes.

**Priority:** Unblocked after MAGENT-4. No structural changes needed — just integration.

---

## Recommended Implementation Order (v2)

The critical path to a working multi-agent system:

1. **MAGENT-5** (register AgentRegistryConsumer) — one line in bootstrap. Do now.
2. **MAGENT-7** (emit AgentRegistered from config) — startup initialization. Do now.
3. **MAGENT-3** (DecomposeStage + RouteKind::Decompose) — orchestrator splits goals.
4. **MAGENT-2** (DispatchConsumer) — routes RequestDispatch to sub-agent channels.
5. **MAGENT-4** (SubAgent spawn) — the core execution engine. Unblocks everything below.
6. **MAGENT-8** (result merge integration) — automatically works after MAGENT-4.
7. **MAGENT-1** (set agent_id on LlmCall) — trivial, do alongside MAGENT-4.
8. **MAGENT-6** (GoalNode DAG) — observability layer, add last.
