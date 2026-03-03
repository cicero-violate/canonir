# _IMPLEMENTATION_PLAN_CAPABILITY.md
# Capability-Based DAG Execution Model

## 1. Architecture Overview

The fixed four-agent model (decompose / planner / executor / verifier) is
replaced by a capability-scoped execution model. Every DAG node declares which
capabilities it requires. The scheduler resolves ready nodes and grants a
scoped `AuthorityContext` containing only the declared capabilities. No agent
has global authority. No capability bundle overlaps across mutation and
verification.

Control flow:
```
Goal
 └─ Decompose (C_decomp | C_graph)
     └─ TaskGraph (nodes with required_capabilities)
         └─ Scheduler (ready-node resolution + authority grant)
             └─ ExecutionEngine (capability-gated dispatch)
                 ├─ Mutation path  (C_mut  | C_exec)       — no C_verify
                 └─ Verify  path  (C_verify | C_proof)     — no C_mut
                     └─ update_status (append-only, verify path only)
```

Global invariant:
```
C_mut ∩ C_verify = ∅
authority(node) ⊆ required_capabilities(node)
update_status callable only from verify path
```

---

## 2. Type Definitions (Rust)

### 2.1 Capability enum
```rust
// capability/capability.rs

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Capability {
    // Graph management
    CreateNode,
    AddEdge,
    UpdateStatus,     // granted only to verify path
    ReadDag,
    ScheduleReady,

    // Decomposition
    GoalToSubgoals,
    ConstraintAttach,

    // Planning
    RefineNode,
    DependencyRewrite,
    RadiusBudgetEval,

    // Mutation (filesystem writes)
    ApplyPatch,
    FileRead,
    FileWrite,

    // Execution (process)
    Bash,
    CargoBuild,
    CargoCheck,
    StdoutCapture,

    // Verification (read + status update only)
    ParseOrchestrationReport,
    DetectFailures,
    StatusUpdateOnly,

    // Telemetry
    ReadStructuralSurface,
    ComputeDelta,
    RewardSignalCompute,

    // Proof / invariant
    InvariantCheck,
    BoundaryGuard,

    // LLM invocation
    PromptContractEnforce,
    StatelessInvoke,
}

// Hard-coded disjoint enforcement.
// Called at scheduler grant time — panics in debug, returns Err in release.
pub fn assert_mut_verify_disjoint(caps: &HashSet<Capability>) -> Result<(), String> {
    let mut_caps   = mutation_caps();
    let verify_caps = verify_caps();
    let overlap: HashSet<_> = caps
        .intersection(&mut_caps)
        .collect::<HashSet<_>>()
        .intersection(&verify_caps.iter().collect())
        .cloned()
        .collect();
    if !overlap.is_empty() {
        return Err(format!("capability overlap violation: {:?}", overlap));
    }
    Ok(())
}

pub fn mutation_caps() -> HashSet<Capability> {
    [Capability::ApplyPatch, Capability::FileWrite, Capability::Bash,
     Capability::CargoBuild, Capability::CargoCheck, Capability::StdoutCapture]
        .into_iter().collect()
}

pub fn verify_caps() -> HashSet<Capability> {
    [Capability::ParseOrchestrationReport, Capability::DetectFailures,
     Capability::StatusUpdateOnly, Capability::UpdateStatus,
     Capability::InvariantCheck, Capability::BoundaryGuard]
        .into_iter().collect()
}
```

### 2.2 Node model
```rust
// capability/dag.rs  (replaces multi-dag/dag.rs)

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct TaskNode {
    pub id:                   String,
    pub description:          String,
    pub status:               Status,
    pub deps:                 Vec<String>,
    pub required_capabilities: Vec<Capability>,
    pub result:               Option<String>,
    pub error:                Option<String>,
}
```

`required_capabilities` is set by the decomposer and is immutable after the
node is created. The scheduler reads it to construct the `AuthorityContext`.

### 2.3 AuthorityContext
```rust
// capability/authority.rs

#[derive(Debug, Clone)]
pub struct AuthorityContext {
    pub node_id:      String,
    pub capabilities: HashSet<Capability>,
}

impl AuthorityContext {
    pub fn new(node_id: String, caps: HashSet<Capability>) -> Result<Self, String> {
        assert_mut_verify_disjoint(&caps)?;
        Ok(Self { node_id, capabilities: caps })
    }

    pub fn has(&self, cap: Capability) -> bool {
        self.capabilities.contains(&cap)
    }

    pub fn require(&self, cap: Capability) -> Result<(), String> {
        if self.has(cap) {
            Ok(())
        } else {
            Err(format!("node {} missing capability {:?}", self.node_id, cap))
        }
    }

    pub fn is_verify_context(&self) -> bool {
        self.capabilities.contains(&Capability::StatusUpdateOnly)
    }

    pub fn is_mutation_context(&self) -> bool {
        self.capabilities.contains(&Capability::FileWrite)
            || self.capabilities.contains(&Capability::ApplyPatch)
    }
}
```

### 2.4 ExecutionEngine dispatch
```rust
// capability/engine.rs

pub async fn dispatch_node(
    node: &TaskNode,
    ctx: &AuthorityContext,
    graph: &mut TaskGraph,
    bridge: &WsBridge,
    url: &str,
    roots: &[PathBuf],
    max_output_lines: usize,
    log_dir: &Path,
    iter: u64,
) -> Result<NodeOutcome> {
    // Gate: verify path cannot mutate, mutation path cannot update status.
    if ctx.is_verify_context() {
        ctx.require(Capability::StatusUpdateOnly)?;
        return dispatch_verify(node, ctx, graph, bridge, url, log_dir, iter).await;
    }
    if ctx.is_mutation_context() {
        ctx.require(Capability::FileWrite)?;
        return dispatch_mutate(node, ctx, graph, bridge, url, roots, max_output_lines, log_dir, iter).await;
    }
    // Planning / decomp / telemetry nodes — read-only by default.
    dispatch_readonly(node, ctx, bridge, url, log_dir, iter).await
}

pub struct NodeOutcome {
    pub node_id: String,
    pub result:  Option<String>,
    pub error:   Option<String>,
    // Status proposed by verify path only.
    // Mutation path NEVER sets this.
    pub status_update: Option<Status>,
}
```

### 2.5 Scheduler update
```rust
// capability/scheduler.rs

pub fn resolve_ready(graph: &mut TaskGraph) {
    // Unchanged dep-completion logic (from multi-dag/scheduler.rs).
    // Added: validate caps at grant time.
    let status_map: HashMap<String, Status> = graph.nodes.iter()
        .map(|n| (n.id.clone(), n.status)).collect();

    for node in &mut graph.nodes {
        if node.status == Status::Pending || node.status == Status::Blocked {
            let any_failed   = node.deps.iter().any(|d| status_map.get(d) == Some(&Status::Failed));
            let all_complete = node.deps.iter().all(|d| status_map.get(d) == Some(&Status::Completed));
            if any_failed {
                node.status = Status::Blocked;
            } else if all_complete {
                node.status = Status::Ready;
            }
        }
    }
}

pub fn grant_authority(node: &TaskNode) -> Result<AuthorityContext, String> {
    let caps: HashSet<Capability> = node.required_capabilities.iter().cloned().collect();
    AuthorityContext::new(node.id.clone(), caps)
}
```

### 2.6 Config format change

Old (`agent_config.toml`):
```toml
[[agents.cards]]
agent_id = "executor"
agent_url = "https://..."
role      = "role_executor.md"
goal      = "goal_executor.md"
tool_capabilities = ["write_file", "bash"]
```

New (`capability_config.toml`):
```toml
[system]
exit_check_command       = "cargo check 2>&1"
max_message_output_lines = 2000
goal_file                = "AGENT_GOAL.md"
max_iterations           = 50
llm_retry_count          = 3
llm_retry_delay_secs     = 5

[[llm.endpoints]]
id  = "primary"
url = "https://chatgpt.com/gg/69a5aa249554819e9ac25e2df27102f1"
role_markdown = "role_primary.md"

# No agent cards. No fixed roles.
# Nodes declare capabilities directly.
```

`AgentConfig` is replaced by `CapabilityConfig`:
```rust
pub struct CapabilityConfig {
    pub exit_check_command:  String,
    pub max_output_lines:    usize,
    pub goal_file:           String,
    pub max_iterations:      u64,
    pub llm_retry_count:     u32,
    pub llm_retry_delay_secs: u64,
    pub llm_endpoints:       Vec<LlmEndpoint>,
}

pub struct LlmEndpoint {
    pub id:            String,
    pub url:           String,
    pub role_markdown: String,
}
```

---

## 3. Refactor Phases

### Phase 1 — Create directory and copy files
```
capability/
  _IMPLEMENTATION_PLAN_CAPABILITY.md   ← this file
  capability.rs      (new)
  authority.rs       (new)
  engine.rs          (new)
  config.rs          (fork of multi-dag/config.rs — strip agent cards)
  dag.rs             (fork of multi-dag/dag.rs — add required_capabilities)
  scheduler.rs       (fork of multi-dag/scheduler.rs — add grant_authority)
  decompose.rs       (fork — remove fixed URL, use endpoint id)
  planner.rs         (fork — remove fixed URL)
  execute.rs         (fork — route via engine.rs)
  verify.rs          (fork — route via engine.rs, only path with UpdateStatus)
  llm.rs             (fork — tabs keyed by endpoint id, not role string)
  goal.rs            (copy — no changes)
  act.rs             (copy — no changes, still the delta executor)
  mod.rs             (new — capability loop replaces run_dag_loop)
```

Files deleted from capability/ (do not copy):
- `observe.rs` — not needed, no phase loop
- `plan.rs`    — not needed, no phase loop

Files preserved unchanged:
- `act.rs`  — delta whitelist is infrastructure, not role logic
- `goal.rs` — no role coupling

### Phase 2 — dag.rs: add required_capabilities to TaskNode

Change only:
```rust
pub struct TaskNode {
    // existing fields unchanged
    pub required_capabilities: Vec<Capability>,   // ADD
}
```

`TaskGraph::validate()` gains one new check:
```rust
for n in &self.nodes {
    let caps: HashSet<Capability> = n.required_capabilities.iter().cloned().collect();
    assert_mut_verify_disjoint(&caps)
        .map_err(|e| format!("node {}: {}", n.id, e))?;
}
```

### Phase 3 — capability.rs + authority.rs

Implement the full `Capability` enum, `mutation_caps()`, `verify_caps()`,
`assert_mut_verify_disjoint()`, and `AuthorityContext` as specified in §2.1
and §2.3. No LLM calls. Pure data.

### Phase 4 — scheduler.rs: add grant_authority

Add `grant_authority(node: &TaskNode) -> Result<AuthorityContext, String>` as
in §2.5. `resolve_ready` is unchanged from multi-dag.

### Phase 5 — engine.rs: dispatch

Implement `dispatch_node` as in §2.4. The three dispatch sub-functions:

`dispatch_verify(node, ctx, graph, bridge, url, log_dir, iter)`:
- Collects Running nodes.
- Calls LLM with verifier schema.
- Calls `graph.update_status` — the ONLY call site for Completed/Failed.
- Returns `NodeOutcome { status_update: Some(...) }`.

`dispatch_mutate(node, ctx, bridge, url, roots, max_output_lines, log_dir, iter)`:
- Calls LLM with executor schema.
- Calls `act::apply_mutations`.
- Sets node to Running via `graph.update_status(id, Running)`.
- Does NOT set Completed or Failed. Ever.
- Returns `NodeOutcome { status_update: None }`.

`dispatch_readonly(node, ctx, bridge, url, log_dir, iter)`:
- Calls LLM with decompose or planner schema based on caps.
- Calls `act::apply_read_only` if FileRead cap present.
- Returns `NodeOutcome { status_update: None }`.

### Phase 6 — llm.rs: keyed by endpoint id

Replace role-string tab routing with endpoint-id routing:
```rust
pub struct DagTabSlots {
    pub slots: HashMap<String, u32>,           // key = endpoint id
    pub system_sent: HashSet<String>,
}
```

`call_agent_json` takes `endpoint_id: &str` instead of `role: &str`.
The endpoint URL comes from `CapabilityConfig::llm_endpoints`.
All four original agents can map to the same endpoint id (single LLM) or
different ones — the config drives it, not hardcoded role strings.

### Phase 7 — config.rs: CapabilityConfig

Replace `AgentConfig` with `CapabilityConfig` as in §2.6.
Remove `card_by_role`. Remove `plan_example` / `plan_refactor`.
Add `endpoint_by_id(id: &str) -> Result<&LlmEndpoint>`.

### Phase 8 — mod.rs: capability loop

Replace `run_dag_loop` with the capability loop:
```
fn run_capability_loop(ctx):
    goal = GoalSpec::from_file(config.goal_file)
    log goal_spec.json

    // Decompose: nodes created with required_capabilities set by LLM response
    decomp_nodes = decompose(goal, engine)
    graph = TaskGraph { nodes: decomp_nodes }
    graph.validate()?          // checks caps disjoint + no cycles
    log planner_output.json

    for iter in 1..=max_iterations:
        scheduler::resolve_ready(&mut graph)

        if graph.all_completed(): return Ok

        if graph.has_failed() && graph.ready_nodes().is_empty():
            bail("blocked")

        for node in graph.ready_nodes():
            ctx = scheduler::grant_authority(node)?
            outcome = engine::dispatch_node(node, ctx, &mut graph, ...).await

            if let Some(status) = outcome.status_update:
                // Only verify path returns Some here.
                graph.update_status(&node.id, status)?

        log iter_NNN_task_graph_after.json

    bail("iteration limit exceeded")
```

### Phase 9 — decompose.rs: capability-aware output schema

The D_g agent response must now include `required_capabilities` per task:
```json
{
  "tasks": [
    {
      "id": "t1",
      "description": "write src/main.rs",
      "deps": [],
      "required_capabilities": ["file_write", "apply_patch"]
    },
    {
      "id": "t2",
      "description": "verify cargo check passes",
      "deps": ["t1"],
      "required_capabilities": ["status_update_only", "detect_failures", "cargo_check"]
    }
  ]
}
```

The prompt to D_g must include the full capability enum values as a reference
list so the LLM can select from a closed vocabulary.

### Phase 10 — delete from multi-dag (after capability/ passes cargo check)

Delete from multi-dag/:
- `observe.rs`
- `plan.rs`

Do not delete `act.rs`, `goal.rs`, `dag.rs`, `scheduler.rs` from multi-dag
until capability/ is fully wired and compiles.

---

## 4. Failure Modes

### F1 — LLM returns unknown capability string
Deserialization of `required_capabilities` fails on unknown variant.
Mitigation: `#[serde(other)]` catch-all variant `Unknown(String)` on
`Capability` enum. Scheduler rejects nodes with Unknown caps before grant.

### F2 — LLM returns overlapping mut+verify caps for one node
`assert_mut_verify_disjoint` fires at `TaskGraph::validate()` after planning.
Hard error before any execution begins.

### F3 — Verify path calls update_status with mutation caps present
`AuthorityContext::new` rejects the bundle at grant time. Node is set to
Failed by the scheduler with error "capability overlap violation".

### F4 — Nondeterminism from concurrent node dispatch
Ready nodes must be dispatched in deterministic order. Sort by node id
(lexicographic) before dispatch loop. Never dispatch in parallel within one
iteration — sequential only.

### F5 — Decomposer assigns verify caps to a node that also mutates
Caught at `validate()`. The LLM prompt must include an explicit constraint:
"A node may not have both file_write/apply_patch and status_update_only in
required_capabilities."

### F6 — Single LLM endpoint used for both mutation and verify nodes
Allowed. The authority is enforced by `AuthorityContext`, not by which tab
handles the call. Tab isolation (X ≠ V) was a proxy for capability isolation.
Capability isolation is the real invariant and is now enforced structurally.

### F7 — Scheduler grants authority to a Blocked node
`grant_authority` is only called for nodes where `status == Ready`.
The scheduler loop filters on `ready_nodes()` before calling `grant_authority`.

### F8 — update_status called outside verify path
`TaskGraph::update_status` is not gated at the type level (Rust has no
capability type system). Gated by call-site discipline: only
`dispatch_verify` and `run_capability_loop`'s outcome handler call it.
Enforce with a `#[doc = "VERIFY PATH ONLY"]` marker and rg audit in CI.

---

## 5. Final Invariant Model
```
I1:  ∀ node ∈ graph.nodes:
       required_capabilities(node) ∩ mutation_caps() = ∅
         ∨
       required_capabilities(node) ∩ verify_caps()   = ∅

I2:  update_status(id, Completed|Failed) callable only from dispatch_verify.

I3:  apply_mutations callable only from dispatch_mutate.

I4:  Status transitions are append-only:
       Pending  → Ready | Blocked
       Ready    → Running
       Running  → Completed | Failed
       Blocked  → Ready
       Completed, Failed → (terminal)

I5:  AuthorityContext.capabilities ⊆ node.required_capabilities
     (scheduler never grants more than declared)

I6:  Decomposition output is validated before any execution:
       TaskGraph::validate() called once, hard-errors on:
         - duplicate node id
         - unknown dep reference
         - cycle
         - mut ∩ verify overlap

I7:  Execution is sequential within an iteration.
     Ready nodes dispatched in lexicographic id order.

I8:  max_iterations cap is enforced unconditionally.
     No path through run_capability_loop can loop forever.
```

---

## 6. Migration Map: Old Agents → Capability Bundles

| Old agent | Old agent_id | New capability bundle                                                                                     |
|-----------+--------------+-----------------------------------------------------------------------------------------------------------|
| D_g       | decompose    | GoalToSubgoals, ConstraintAttach, ReadDag, StatelessInvoke                                                |
| P         | planner      | RefineNode, DependencyRewrite, RadiusBudgetEval, ReadDag, StatelessInvoke                                 |
| X         | executor     | FileWrite, ApplyPatch, FileRead, Bash, CargoCheck, StdoutCapture, StatelessInvoke                         |
| V         | verifier     | ParseOrchestrationReport, DetectFailures, StatusUpdateOnly, UpdateStatus, InvariantCheck, StatelessInvoke |

These bundles become the default `required_capabilities` for the four logical
node types. The decomposer LLM assigns them per-node. Nodes may use subsets.

---

## 7. Files to Create in capability/

Copy from multi-dag, then modify as described above:

| File               | Action                                      |
|--------------------|---------------------------------------------|
| capability.rs      | NEW — Capability enum, disjoint check       |
| authority.rs       | NEW — AuthorityContext                      |
| engine.rs          | NEW — dispatch_node, three sub-dispatchers  |
| config.rs          | FORK — replace AgentConfig with CapabilityConfig |
| dag.rs             | FORK — add required_capabilities field      |
| scheduler.rs       | FORK — add grant_authority                  |
| decompose.rs       | FORK — new schema with required_capabilities |
| planner.rs         | FORK — route via endpoint id not role       |
| execute.rs         | FORK — route via engine.rs                  |
| verify.rs          | FORK — route via engine.rs                  |
| llm.rs             | FORK — HashMap tab slots, endpoint id keys  |
| goal.rs            | COPY — unchanged                            |
| act.rs             | COPY — unchanged                            |
| mod.rs             | NEW — capability loop                       |

Files NOT copied (deleted from scope):
- observe.rs
- plan.rs
