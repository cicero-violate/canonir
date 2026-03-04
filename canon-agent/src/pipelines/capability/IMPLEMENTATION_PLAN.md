# IMPLEMENTATION PLAN
# Capability Pipeline — LOC Reduction + Algorithm Library Integration
#
# Target: LOC_current=3524 → LOC_target≤2800
# Agent must: cargo check after each phase. No tests. No summary files.

---

## PHASE 0 — Read Before Touching Anything

```sh
rg --files canon-agent/src/pipelines/capability/
rg "use super::" canon-agent/src/pipelines/capability/engine.rs
rg "use algorithms" canon-agent/src/pipelines/capability/mod.rs
```

Confirm imports compile before any edit:
```sh
cargo check -p canon-agent
```

---

## PHASE 1 — Collapse execute.rs + verify.rs into engine.rs

### Why
`execute.rs` (60 LOC) calls `engine::dispatch_node`.
`verify.rs` (49 LOC) calls `engine::dispatch_node` with verify ctx.
Both are single-function thin wrappers with no independent logic.

### Steps

1. Open `execute.rs` and `verify.rs` with bat for full context.
2. Move `execute_node` body inline into `engine.rs` as a private fn, or
   delete it entirely if the call site in `mod.rs` already calls
   `engine::dispatch_node` directly (it does — confirm with rg).
3. Use apply_patch to delete both files.
4. Remove `pub mod execute;` and `pub mod verify;` from `mod.rs`.
5. `cargo check`

### Verification
```sh
rg "execute_node|verify_node" canon-agent/src/ --type rust
# must return 0 results
```

---

## PHASE 2 — Unify call_mutate / call_readonly / call_verify → call_mode

### Why
Three functions share identical structure:
  - build input json
  - build schema string
  - call_agent_json_with_retry
  - parse output
  - retry on parse failure
  - log to file
  - eprintln phase metric

The only differences are:
| Field       | mutate          | readonly           | verify          |
|-------------|-----------------|-------------------|-----------------|
| schema      | write deltas    | read deltas only   | status updates  |
| phase label | "mutate"        | "readonly"         | "verify"        |
| parse fn    | parse_exec_output | parse_exec_output | from_value    |
| log file    | execute_output  | readonly_output    | verify_output   |
| guard       | Render check    | mutation delta check | none          |

### Steps

1. Add `DispatchMode` enum to `engine.rs`:
```rust
enum DispatchMode { Mutate, Readonly, Verify }
```

2. Extract shared skeleton into:
```rust
async fn call_mode(
    mode: DispatchMode,
    node: &TaskNode,
    // ... shared params ...
) -> Result<NodeCallResult>
```

3. Inside `call_mode`, use `match mode` only for:
   - schema string selection
   - phase label (&str)
   - input json shape
   - output parse branch
   - post-call guard (render check / mutation delta check)

4. Replace bodies of `call_mutate`, `call_readonly`, `call_verify`
   with delegation to `call_mode`, then delete the three originals.

5. Same collapse for `dispatch_mutate`, `dispatch_readonly`,
   `dispatch_verify` → single `dispatch_mode(mode: DispatchMode, ...)`.
   The apply logic differs — keep `apply_mutate_output`,
   `apply_readonly_output`, `apply_verify_output` as-is (they are
   legitimately different), called via match inside `dispatch_mode`.

6. Use apply_patch for all edits. cargo check after each file change.

### Expected savings: ~250 LOC from engine.rs

---

## PHASE 3 — Collapse scheduler.rs + authority.rs into dag.rs

### Why
`scheduler.rs` (25 LOC): `resolve_ready` and `grant_authority` operate
on `TaskGraph` and `TaskNode`. Both belong at the graph layer.
`authority.rs` (36 LOC): `AuthorityContext` is referenced in `engine.rs`
and `scheduler.rs`. Moving to `dag.rs` removes a module boundary.

### Steps

1. Read scheduler.rs and authority.rs with bat.
2. Use apply_patch to append both structs/fns to `dag.rs`.
3. Update all `use super::scheduler::` and `use super::authority::`
   imports throughout the capability directory:
```sh
rg "use super::scheduler|use super::authority" \
  canon-agent/src/pipelines/capability/ --type rust -l
```
   For each file returned, apply_patch to rewrite the import to
   `use super::dag::`.
4. Delete `scheduler.rs` and `authority.rs`.
5. Remove `pub mod scheduler;` and `pub mod authority;` from `mod.rs`.
6. `cargo check`

### Replace resolve_ready with topological_layers

`resolve_ready` does a manual indegree scan to mark Pending→Ready.
`algorithms::graph::scheduling::topological_layers` does the same
computation but returns parallel execution layers directly.

Replace the body of `resolve_ready` in dag.rs:

```rust
// Old: manual indegree loop over Status::Pending
// New:
pub fn resolve_ready(graph: &mut TaskGraph) {
    let id_to_idx: HashMap<&str, usize> = graph.nodes.iter()
        .enumerate().map(|(i, n)| (n.id.as_str(), i)).collect();
    let adj: Vec<Vec<usize>> = graph.nodes.iter().map(|n| {
        n.deps.iter()
            .filter_map(|d| id_to_idx.get(d.as_str()).copied())
            .collect()
    }).collect();
    // Use library: layers[0] = nodes with no unfinished deps
    let layers = algorithms::graph::scheduling::topological_layers(&adj);
    if let Some(first) = layers.first() {
        for &idx in first {
            if graph.nodes[idx].status == Status::Pending {
                graph.nodes[idx].status = Status::Ready;
            }
        }
    }
}
```

Add to `canon-agent/Cargo.toml` if not already present:
```toml
algorithms = { path = "../../algorithms" }
```
Confirm with:
```sh
rg "algorithms" canon-agent/Cargo.toml
```

---

## PHASE 4 — Collapse goal.rs + policy.rs into config.rs

### Why
`goal.rs` (18 LOC): 1 struct, 2 methods. `policy.rs` (36 LOC): 1 struct,
1 load fn. Both follow the exact same load-from-file pattern as config.

### Steps

1. Apply_patch to append `GoalSpec` impl and `CapabilityPolicy` struct
   to end of `config.rs`.
2. Update all imports:
```sh
rg "use super::goal|use super::policy" \
  canon-agent/src/pipelines/capability/ --type rust -l
```
3. Delete `goal.rs` and `policy.rs`.
4. Remove `pub mod goal;` and `pub mod policy;` from `mod.rs`.
5. `cargo check`

---

## PHASE 5 — Extract graph_algo.rs from mod.rs

### Why
`mod.rs` (1189 LOC) is a god file. The following functions are pure graph
computation with no orchestration dependency:

- `compute_graph_signals` (~50 LOC)
- `reachability_mask` (~20 LOC)  ← replace with CPU BFS or library call
- `run_graph_algorithms` (~55 LOC)
- `emit_planned_graph` (~40 LOC)
- `planner_signals_for_graph` (~20 LOC)
- `enforce_linking_constraints` (~10 LOC)
- `GraphSignals` struct + `to_json` (~30 LOC)

Total: ~225 LOC extracted → new file `graph_algo.rs`

### Replace reachability_mask

`reachability_mask` in `mod.rs` is a CPU BFS reimplementation.
`algorithms::graph::reachability` is GPU-only (requires cuda feature).

Strategy:
```rust
// In graph_algo.rs:
#[cfg(feature = "cuda")]
pub fn reachability_mask(adj_csr: &Csr, roots: &[usize]) -> Vec<bool> {
    algorithms::graph::reachability::reachability_gpu(adj_csr, roots)
}

#[cfg(not(feature = "cuda"))]
pub fn reachability_mask(adj: &[Vec<usize>], roots: &[usize]) -> Vec<bool> {
    // keep current CPU BFS — 20 LOC, correct
}
```

When cuda feature is active, `run_graph_algorithms` already calls
`algorithms::graph::gpu::bfs_gpu`. The CSR conversion is done via
`AdjList::to_csr()`. Wire `reachability_mask` to use this path.

### Steps

1. Create `graph_algo.rs` with apply_patch (Add File).
2. Move the 7 items above out of `mod.rs` using apply_patch (delete
   from mod.rs, add to graph_algo.rs).
3. Add `pub mod graph_algo;` to `mod.rs`.
4. Update call sites in `mod.rs` to `graph_algo::compute_graph_signals`,
   `graph_algo::run_graph_algorithms`, etc.
5. `cargo check`

---

## PHASE 6 — Unify decompose_goal + decompose_node

### Why
`decompose_goal` (line 40, ~108 LOC) and `decompose_node` (line 148,
~57 LOC) share the same LLM call pattern, schema, and output parse.
The only difference is the prompt preamble and whether the input is a
`GoalSpec` or a `TaskSpec`.

### Steps

1. Read `decompose.rs` with bat.
2. Extract shared prompt-build + LLM call + parse into:
```rust
async fn decompose_inner(
    prompt: String,
    // shared params
) -> Result<DecomposeOutput>
```
3. Rewrite `decompose_goal` and `decompose_node` as thin wrappers that
   build the prompt string and call `decompose_inner`.
4. `cargo check`

### Expected savings: ~60 LOC

---

## PHASE 7 — Final cargo check + git commit

```sh
cargo check -p canon-agent
cargo check -p algorithms
```

Then commit:
```sh
git add -A
git commit -m "refactor(capability): collapse 6 thin modules, unify dispatch modes, extract graph_algo

- delete execute.rs, verify.rs (thin wrappers, inlined into engine.rs)
- delete scheduler.rs, authority.rs (merged into dag.rs)
- delete goal.rs, policy.rs (merged into config.rs)
- unify call_mutate/call_readonly/call_verify → call_mode(DispatchMode)
- unify dispatch_mutate/dispatch_readonly/dispatch_verify → dispatch_mode
- extract graph_algo.rs from mod.rs (GraphSignals, compute_graph_signals,
  run_graph_algorithms, emit_planned_graph, planner_signals_for_graph)
- replace resolve_ready with algorithms::graph::scheduling::topological_layers
- wire reachability_mask to algorithms::graph::reachability (cuda) / CPU BFS fallback
- unify decompose_goal + decompose_node via decompose_inner

LOC: 3524 → ~2800 (-20%); file count: 16 → 11"
```

---

## FILE COUNT DELTA

| Action          | Files                                      |
|-----------------|--------------------------------------------|
| Deleted         | execute.rs, verify.rs, scheduler.rs,       |
|                 | authority.rs, goal.rs, policy.rs           |
| Created         | graph_algo.rs                              |
| Net             | 16 → 11 files                              |

## CONSTRAINT: DO NOT
- Run cargo build or cargo test
- Create summary or documentation files
- Edit files outside apply_patch
- Touch algorithms/ source files
- Use grep (use rg)
- Use cat/bat for automated extraction (use awk/perl)
