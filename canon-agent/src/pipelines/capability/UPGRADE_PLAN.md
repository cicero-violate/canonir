# Capability System Upgrade Plan

## Objective

Five independent, ordered changes that increase reasoning depth, problem-solving
strength, and template quality across runs. Each change is self-contained and
can be merged separately.

---

## Change 1 — PlannerUpdate: Retract and Rewrite Operations

**Files:** `planner_session.rs`, `scheduler.rs`, `templates.rs`

**Problem:** The planner can only add nodes and edges. It cannot remove stale
nodes or sharpen vague ones. On hard problems the graph accumulates dead
branches that block convergence.

**Changes:**

In `planner_session.rs`, extend `PlannerUpdate`:

```rust
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct RetractSpec {
    pub id: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct RewriteSpec {
    pub id: String,
    pub new_description: String,
    pub new_capabilities: Vec<Capability>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct PlannerUpdate {
    #[serde(default)]
    pub new_nodes: Vec<TaskSpec>,
    #[serde(default)]
    pub new_edges: Vec<EdgeSpec>,
    #[serde(default)]
    pub retract_nodes: Vec<RetractSpec>,
    #[serde(default)]
    pub rewrite_nodes: Vec<RewriteSpec>,
}
```

Update the planner prompt string in `planner_iteration` to include the two new
operations in the schema comment and rules. Add rule: "retract nodes that are
Pending or Failed with no dependents. Rewrite nodes that are Pending with an
imprecise description."

In `scheduler.rs`, extend `apply_planner_update` to handle both new fields:

- `retract_nodes`: for each `RetractSpec`, skip if node status is not Pending
  or Failed. Remove the node from `graph.nodes`. Remove any edge in other
  nodes' `deps` that references the retracted id. Call `graph.rebuild_index()`
  after all retractions.
- `rewrite_nodes`: for each `RewriteSpec`, find the node by id, assert status
  is Pending, replace `description` and `required_capabilities`. Validate the
  new capability set with `assert_mut_verify_disjoint` before writing.

In `validate_planner_update` in `scheduler.rs`, add validation for both:
- Retract: node must exist and be Pending or Failed.
- Rewrite: node must exist and be Pending. New capabilities must pass
  `assert_mut_verify_disjoint`.

`TemplateStore::update` requires no changes — it calls `apply_planner_update`
which now handles all four operation types.

---

## Change 2 — TaskNode: Priority, Budget, Reasoning Trace

**Files:** `dag.rs`, `scheduler.rs`, `decompose.rs`, `planner_session.rs`

**Problem:** All nodes are equal. The scheduler sorts ready nodes
alphabetically. There is no per-node cost limit and no record of why a node
was created.

**Changes:**

In `dag.rs`, add three fields to `TaskNode`:

```rust
#[serde(default)]
pub priority: u8,              // 0 = normal, 255 = highest. Scheduler sorts descending.

#[serde(default)]
pub budget: Option<u32>,       // max LLM calls before auto-fail. None = use global max_node_retries.

#[serde(default)]
pub reasoning_trace: Option<String>,  // rationale from planner or decomposer at creation time.
```

All three have `#[serde(default)]` so existing serialized templates load
without error.

In `reset_for_execution`, do NOT clear `priority`, `budget`, or
`reasoning_trace` — these are structural properties, not execution state.

In `scheduler.rs` inside `execute_graph_loop`, replace:
```rust
ready_ids.sort();
```
with:
```rust
ready_ids.sort_by_key(|id| {
    std::cmp::Reverse(graph.get_node(id).map(|n| n.priority).unwrap_or(0))
});
```

In `engine.rs` inside `apply_readonly_output` and `apply_mutate_output`,
replace the global `max_node_retries` check with:
```rust
let effective_budget = graph.get_node(&node_id)
    .and_then(|n| n.budget)
    .unwrap_or(max_node_retries);
```
Use `effective_budget` in the `fail_count >= effective_budget` comparison.

In `decompose.rs`, populate `reasoning_trace` from the `rationale` field
already present in `ExecNodeResult` when constructing `TaskNode` from
decompose output.

In `planner_session.rs`, add `reasoning_trace`, `priority`, and `budget` to
the planner prompt schema so the LLM can optionally set them on new nodes.
Update `TaskSpec` in `decompose.rs` to include these three optional fields
with defaults.

---

## Change 3 — ContextNode: Causal and Failure Context

**Files:** `graph_runtime.rs`, `engine.rs`

**Problem:** Each node receives a flat list of ancestor nodes as context. It
cannot see what results prior nodes produced or why prior attempts failed. This
forces the LLM to re-derive information it could read directly.

**Changes:**

In `engine.rs`, add two fields to `ContextNode`:

```rust
#[serde(default)]
pub causal_summary: Option<String>,
// Concatenated result strings from direct completed dependencies.

#[serde(default)]
pub failure_summary: Option<String>,
// Concatenated error strings from any Failed nodes in the context window.
```

In `graph_runtime.rs`, populate these fields inside `build_context` after
collecting the reachable node set:

```rust
// For each ContextNode being built:
let causal_summary = node.deps.iter()
    .filter_map(|dep_id| by_id.get(dep_id))
    .filter(|dep| dep.status == dag::Status::Completed)
    .filter_map(|dep| dep.result.as_deref())
    .collect::<Vec<_>>()
    .join("\n---\n")
    .pipe(|s| if s.is_empty() { None } else { Some(s) });

let failure_summary = graph.nodes.iter()
    .filter(|n| n.status == dag::Status::Failed)
    .filter_map(|n| n.error.as_deref().map(|e| format!("{}: {}", n.id, e)))
    .collect::<Vec<_>>()
    .join("\n")
    .pipe(|s| if s.is_empty() { None } else { Some(s) });
```

These fields serialize into the prompt payload automatically because
`ContextNode` is serialized to JSON in `call_mode` in `engine.rs`. No prompt
template changes needed — the fields appear in the INPUT block.

---

## Change 4 — Graded Reward Signal

**Files:** `telemetry.rs`, `scheduler.rs`, `mod.rs`

**Problem:** `PipelineOutcome` returns binary `1.0` / `-1.0`. The system
cannot distinguish a clean 10-node run from a thrashing 64-node run that
barely converged. Templates cannot be compared.

**Variables:**

$$R = \frac{N_c}{N_t} - \alpha \cdot \frac{I_a}{I_{max}} - \beta \cdot \frac{F}{N_t}$$

where $N_c$ = completed nodes, $N_t$ = total nodes, $I_a$ = actual iterations
used, $I_{max}$ = max iterations allowed, $F$ = failed nodes, $\alpha = 0.2$,
$\beta = 0.3$.

**Changes:**

In `telemetry.rs`, add a `compute_reward` function:

```rust
pub fn compute_reward(graph: &TaskGraph, iterations_used: u64, max_iterations: u64) -> f64 {
    let n_total = graph.nodes.len() as f64;
    if n_total == 0.0 { return 0.0; }
    let n_completed = graph.nodes.iter().filter(|n| n.status == Status::Completed).count() as f64;
    let n_failed    = graph.nodes.iter().filter(|n| n.status == Status::Failed).count() as f64;
    let iter_ratio  = iterations_used as f64 / max_iterations.max(1) as f64;
    (n_completed / n_total) - 0.2 * iter_ratio - 0.3 * (n_failed / n_total)
}
```

Add `reward: f64` to `TelemetrySnapshot`:

```rust
pub struct TelemetrySnapshot {
    pub planner: PlannerMetrics,
    pub exec: ExecMetrics,
    pub runtime: RuntimeMetrics,
    pub reward: f64,
}
```

In `scheduler.rs`, after `execute_graph_loop` or `run_planner_execution_loop`
returns, compute the reward and store it in the snapshot before calling
`telemetry::record_snapshot`.

In `mod.rs`, replace the binary `run_tick` return:

```rust
match self.run_capability_loop(ctx).await {
    Ok(reward) => Ok(PipelineOutcome { reward, summary: "capability completed".into(), advanced: true }),
    Err(e)     => Ok(PipelineOutcome { reward: -1.0, summary: format!("capability error: {e}"), advanced: false }),
}
```

Change `run_capability_loop` return type from `Result<()>` to `Result<f64>`,
returning the computed reward at the end.

---

## Change 5 — Template Ratchet

**Files:** `templates.rs`

**Problem:** `store.save()` unconditionally overwrites the template. A bad run
that converges with many failures will overwrite a good prior template.

**Changes:**

Add a sidecar reward file alongside each template. The sidecar path is
`path_for(name)` with extension replaced by `.reward`.

Add to `TemplateStore`:

```rust
fn reward_path(&self, name: &str) -> PathBuf {
    self.path_for(name).with_extension("reward")
}

pub fn stored_reward(&self, name: &str) -> f64 {
    fs::read_to_string(self.reward_path(name))
        .ok()
        .and_then(|s| s.trim().parse::<f64>().ok())
        .unwrap_or(f64::NEG_INFINITY)
}

pub fn save_with_reward(&self, name: &str, graph: &TaskGraph, reward: f64) -> Result<()> {
    if reward <= self.stored_reward(name) {
        return Ok(());   // ratchet: only advance
    }
    self.save(name, graph)?;
    fs::write(self.reward_path(name), reward.to_string())?;
    Ok(())
}
```

In `scheduler.rs`, replace the final `store.save(template_name, graph)` call
with `store.save_with_reward(template_name, graph, reward)`. Pass `reward`
down from wherever `compute_reward` was called.

In `templates.rs`, `evict` should also remove the sidecar:

```rust
pub fn evict(&self, name: &str) {
    let _ = fs::remove_file(self.path_for(name));
    let _ = fs::remove_file(self.reward_path(name));
}
```

`load`, `exists`, and `update` require no changes.

---

## Execution Order

| Order | Change | Why first |
|-------|--------|-----------|
| 1 | PlannerUpdate retract + rewrite | Unblocks hard problems immediately |
| 2 | TaskNode priority + budget + trace | Improves scheduling and per-node control |
| 3 | ContextNode causal + failure context | Improves LLM reasoning quality on every call |
| 4 | Graded reward signal | Required before ratchet can work |
| 5 | Template ratchet | Depends on reward signal from Change 4 |

## Touched Files Summary

| File | Changes |
|------|---------|
| `dag.rs` | Add `priority`, `budget`, `reasoning_trace` to `TaskNode` |
| `decompose.rs` | Populate `reasoning_trace` from rationale; add fields to `TaskSpec` |
| `planner_session.rs` | Extend `PlannerUpdate` with `retract_nodes`, `rewrite_nodes`; update prompt schema |
| `scheduler.rs` | Handle retract/rewrite in `apply_planner_update` and `validate_planner_update`; priority sort; graded reward call |
| `engine.rs` | Per-node budget in `apply_readonly_output` and `apply_mutate_output`; add `causal_summary`, `failure_summary` to `ContextNode` |
| `graph_runtime.rs` | Populate `causal_summary` and `failure_summary` in `build_context` |
| `telemetry.rs` | Add `compute_reward`; add `reward` field to `TelemetrySnapshot` |
| `templates.rs` | Add `reward_path`, `stored_reward`, `save_with_reward`; extend `evict` |
| `mod.rs` | Return `f64` reward from `run_capability_loop`; pass to `PipelineOutcome` |
