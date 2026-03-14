Here's the implementation plan.

---

## Implementation Plan: `agent_consumer.rs` — Full Wiring

### Orientation

Before touching anything, the agent reads these files in full:

```bash
bat -n canon-utils/event-runtime/src/consumers/agent_consumer.rs
bat -n canon-agent-v2/src/capability_types.rs
bat -n canon-agent-v2/src/planner_session.rs | head -100
bat -n canon-agent-v2/src/dag.rs
rg '(PipelineCapability|Llm|Analysis)' canon-agent-v2/src/capability_types.rs
```

Then confirm the `PipelineSnapshot` shape before writing any JSON parsing code:

```bash
bat -n canon-agent-v2/src/state_snapshot.rs
python3 -c "import json; d=json.load(open('agent_logs/state_snapshot.json')); print(list(d.keys()))"
```

---

### Change 1 — `capability_name_for_node`: Add `Llm` and `Analysis` arms

**File**: `canon-utils/event-runtime/src/consumers/agent_consumer.rs`

**What**: The `_` arm currently swallows `PipelineCapability::Llm` and `PipelineCapability::Analysis`. Both must map to `"llm.call"`.

**How**: In the match block inside `capability_name_for_node`, replace the catch-all with explicit arms. First confirm the exact variant names:

```bash
rg '(Llm|Analysis|LlmCall)' canon-agent-v2/src/capability_types.rs
```

Then apply:

```
PipelineCapability::Llm => return Some("llm.call"),
PipelineCapability::Analysis => return Some("llm.call"),
```

Keep the `_ => {}` catch-all for all other variants that genuinely have no mapping.

---

### Change 2 — `build_capability_args`: Handle `"llm.call"` nodes

**File**: same

**What**: When `capability == "llm.call"`, the function currently returns `None`. It must return:

```json
{"prompt": "<node description or extracted prompt>", "raw": false}
```

**How**: In `build_capability_args`, before the existing match, add a branch:

```rust
if capability == "llm.call" {
    let prompt = node.description.clone();
    return Some(serde_json::json!({
        "prompt": prompt,
        "raw": false
    }));
}
```

The node description is the natural source for the prompt — it contains the task the LLM node is meant to execute. Do not attempt to parse a structured prompt field out of the description at this stage; that can be a follow-up.

---

### Change 3 — `on_capability_result`: Parse `CapabilityCompleted` payload correctly

**File**: same

**What**: When a `CapabilityCompleted` event arrives for an `llm.call` node, the payload is:

```json
{"status": 0, "success": true, "duration_ms": 12161, "result": {"ok": true}}
```

The current `apply_result` impl calls `apply_result` on the graph but may not extract and store the result value into `node.result`. Confirm by reading the current `apply_result` body in the file.

**What to produce**: After applying status transitions, extract `payload["result"]` and write it into the node's `result` field as `Some(serde_json::to_string(&result_val).unwrap_or_default())`. Then call `plan_if_stalled` to re-evaluate.

**Shape of the event**: Confirm the `RuntimeEvent::CapabilityCompleted` variant carries `node_id`, `capability`, and `payload` fields by reading `lib.rs`. Use `rg 'CapabilityCompleted' canon-utils/event-runtime/src/lib.rs`.

---

### Change 4 — `seed_orchestration`: Real bootstrap when graph is empty

**File**: same

**What**: Currently a no-op stub. When the graph is empty (no snapshot loaded, no nodes), the agent needs to emit a minimal first node to bootstrap execution.

**Approach — keep it minimal and correct**. Do not wire `PlannerController` here — that requires a live WS bridge and config. Instead produce a single bootstrap node using `GraphPatch` that requests an LLM analysis of the current system state. This is analogous to what `planner_controller_seed_orchestration_if_empty` does in `planner_session.rs` — read that function first:

```bash
rg -n 'seed_orchestration_if_empty' canon-agent-v2/src/planner_session.rs
perl -ne 'print if /fn planner_controller_seed_orchestration_if_empty/../^fn /' \
  canon-agent-v2/src/planner_session.rs
```

Replicate its logic without the WS dependency: construct a `DecomposeTaskSpec`-shaped `ExecutionNode` directly — id `"seed_0"`, description `"Analyse system state and produce initial task decomposition"`, `required_capabilities: vec![PipelineCapability::Llm]`, no deps, `NodeStatus::Pending`. Add it to `update.new_nodes`.

---

### Change 5 — `plan_if_stalled`: Real stall detection and replan trigger

**File**: same

**What**: Currently retries one failed node. Real stall conditions are:

1. Graph is empty after snapshot load failed
2. All nodes are `Completed` (graph done, but no new seed issued)
3. All remaining nodes are `Failed` with no retries left
4. All nodes are `Blocked` (deadlock)

**How**:

First read `graph_analysis_compute_graph_signals` signature:

```bash
rg -n 'fn graph_analysis_compute_graph_signals' canon-agent-v2/src/graph_algo.rs
rg -n 'fn compute_graph_features_parallel' canon-agent-v2/src/graph_algo.rs
```

In `plan_if_stalled`:

1. Call `compute_graph_features_parallel(&self.graph)` → `features`
2. Call `graph_analysis_compute_graph_signals(&self.graph)` → `signals`
3. Check stall conditions using `features.failed_fraction`, `features.ready_fraction`, `features.blocked_fraction`, and `self.graph.nodes.is_empty()`
4. If stalled: construct a `GraphPatch` with one new `Llm` node (description derived from stall reason), call `apply_graph_patch(&mut self.graph, patch)`, return `true`
5. If not stalled: return `false`

Do not call `PlannerController::build_prompt` or make any LLM calls inside this function — this is a local graph manipulation only. The new node will be picked up on the next `Tick` and dispatched normally.

Read `apply_graph_patch` signature first:

```bash
rg -n 'fn apply_graph_patch' canon-agent-v2/src/planner_update.rs
```

---

### Import hygiene

After all edits, check that all newly referenced symbols are imported. The consumer file already imports from `canon_agent_v2` — confirm the crate name and module paths:

```bash
rg '^use ' canon-utils/event-runtime/src/consumers/agent_consumer.rs
rg '^canon-agent-v2' canon-utils/event-runtime/Cargo.toml
```

Then run:

```bash
cargo check -p event-runtime 2>&1 | head -60
```

Fix all errors before committing. Do not run `cargo build` or any other cargo subcommands.

---

### Commit

```bash
git add canon-utils/event-runtime/src/consumers/agent_consumer.rs
git commit -m "wire agent_consumer: llm.call capability mapping, result ingestion, real stall planning, seed bootstrap"
```

---

### What this plan explicitly defers

- Wiring `PlannerController` into the consumer (requires config/WS bridge plumbing that doesn't exist in the consumer's thread context yet)
- Multi-node replan via LLM (that's a follow-on after the graph can complete one round-trip)
- Snapshot persistence from the consumer (the graph state is in-memory; persistence belongs in a separate pass)

These are not regressions — the smoke test path (`LlmExecutorConsumer`) is untouched. The five changes above are purely additive to the existing consumer skeleton.
