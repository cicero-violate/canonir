# Branching Reduction — Implementation Plan

> **Scope**: `mod.rs`, `act.rs`, `engine.rs`, `graph_algo.rs`, `dag.rs`
> **GPU backend**: CUDA via `algorithms` crate (`feature = "cuda"`)
> **Forbidden**: `cargo` commands other than `cargo check`, no test files, no summary files
> **Tooling**: `rg` to find, `awk` to slice, `apply_patch` to modify, `perl` for structure-aware extraction
> **JSON reads**: always via Python

---

## Phase 0 — Audit (read-only, no edits)

Run these before touching any file. Capture output for reference.

```sh
# 0-A: confirm algorithms GPU exports available
rg -n 'pub fn' canon/algorithms/src/graph/gpu.rs
rg -n 'pub fn' canon/algorithms/src/graph/reachability.rs
rg -n 'pub fn' canon/algorithms/src/graph/scheduling.rs
rg -n 'pub fn' canon/algorithms/src/graph/scc.rs
rg -n 'pub fn' canon/algorithms/src/graph/topological_sort.rs
rg -n 'pub fn' canon/algorithms/src/graph/csr.rs

# 0-B: confirm feature flag threading
rg -n 'feature.*cuda' canon/canon-agent/Cargo.toml

# 0-C: baseline branch counts (save for later diff)
rg -cn '\b(if|match|for|while)\b' \
  canon/canon-agent/src/pipelines/capability/mod.rs \
  canon/canon-agent/src/pipelines/capability/act.rs \
  canon/canon-agent/src/pipelines/capability/engine.rs \
  canon/canon-agent/src/pipelines/capability/graph_algo.rs \
  canon/canon-agent/src/pipelines/capability/dag.rs
```

---

## Phase 1 — `graph_algo.rs` — GPU offload (removes ~18 branches)

### 1-A  `reachability_mask` (CPU path)

**Current**: `while let Some(u) = q.pop_front()` + inner `for &v` + two `if v < n && !visited[v]`
inside `reachability_mask` (non-cuda path). That is 4 branch points per call.

**Target**: Delete the entire `#[cfg(not(feature = "cuda"))]` `reachability_mask` body and
replace it with a call to `algorithms::graph::reachability::reachability_gpu` behind a
unified wrapper that works for both paths using the same CSR type.

**Steps**:
```sh
# find exact line range of both cfg blocks
rg -n 'cfg.*feature.*cuda' canon/canon-agent/src/pipelines/capability/graph_algo.rs
perl -0777 -ne \
  'while (/fn reachability_mask.*?^}/gms) { print "---\n$&\n" }' \
  canon/canon-agent/src/pipelines/capability/graph_algo.rs
```

**Edit** (`apply_patch`):
- Remove both `#[cfg(feature = "cuda")]` and `#[cfg(not(feature = "cuda"))]` variants of
  `reachability_mask`.
- Replace with a single unconditional function that converts `adj: &[Vec<usize>]` to a
  `algorithms::graph::csr::Csr` (using the existing `Csr::from_adj` or equivalent — confirm
  exact constructor name in Phase 0-A), then calls
  `algorithms::graph::reachability::reachability_gpu(csr, roots)`.
- The `#[cfg(feature = "cuda")]` CSR build block inside `compute_graph_signals` that currently
  duplicates the conversion must be deleted; the single function handles it.
- After edit: `rg -n 'cfg' graph_algo.rs` should return zero hits inside `reachability_mask`.

### 1-B  `run_graph_algorithms` log path duplication

**Current**: `if iter == 0 { ... } else { ... }` appears **three times** (once in
`emit_planned_graph`, twice in `run_graph_algorithms` for cuda/non-cuda). Each is 1 branch.

**Target**: Extract a shared helper:
```rust
fn algo_log_path(log_dir: &Path, iter: u32, name: &str) -> PathBuf {
    if iter == 0 { log_dir.join(name) }
    else { log_dir.join(format!("iter_{:03}_{}", iter, name)) }
}
```
This does not eliminate the branch but collapses 3 identical branches into 1 call site each,
making future elimination straightforward. The `#[cfg]` dual-block in `run_graph_algorithms`
is merged into one block after Phase 1-A; its duplicate `if iter == 0` path is replaced with
`algo_log_path(log_dir, iter, "graph_algorithms.json")`.

**Steps**:
```sh
perl -0777 -ne \
  'while (/let path = if iter.*?};/gms) { print "MATCH:\n$&\n---\n" }' \
  canon/canon-agent/src/pipelines/capability/graph_algo.rs
```
Apply patch to add `algo_log_path` above `emit_planned_graph`, then replace all three
`let path = if iter == 0` blocks with `let path = algo_log_path(log_dir, iter, ...)`.

### 1-C  `compute_graph_signals` — branchless root/unreachable filters

**Current**:
```rust
.filter_map(|(i, &d)| if d == 0 { Some(i) } else { None })
.filter_map(|(i, &ok)| if ok { None } else { Some(i) })
```

**Target**: These are already functional-style (1 branch each) and cannot be made branchless
without unsafe. Leave them. Mark as **accepted cost**.

---

## Phase 2 — `dag.rs` — GPU topological scheduling + branchless transition (removes ~10 branches)

### 2-A  `resolve_ready` — remove the `if let Some(first)` guard

**Current**:
```rust
if let Some(first) = layers.first() {
    for &idx in first {
        if graph.nodes[idx].status == Status::Pending {
            graph.nodes[idx].status = Status::Ready;
        }
    }
}
```
3 branch points.

**Target**: `topological_layers` already returns `Vec<Vec<usize>>`. Iterate directly:
```rust
for &idx in layers.first().into_iter().flatten() {
    // branchless status upgrade using a lookup table (see below)
    graph.nodes[idx].status = PENDING_TO_READY[graph.nodes[idx].status as usize];
}
```
Where `PENDING_TO_READY` is a const array `[Status; 6]` that maps `Pending -> Ready` and is
identity for all other variants. This collapses 3 branches to 0 inside the hot path.

**Steps**:
```sh
rg -n 'resolve_ready' canon/canon-agent/src/pipelines/capability/dag.rs
# confirm Status discriminant ordering matches array indices
rg -n 'enum Status' canon/canon-agent/src/pipelines/capability/dag.rs
```

Add `#[repr(u8)]` to `Status` and define:
```rust
const PENDING_TO_READY: [Status; 6] = [
    Status::Ready,     // Pending     (0) -> Ready
    Status::Ready,     // Ready       (1) -> identity
    Status::Running,   // Running     (2) -> identity
    Status::Completed, // Completed   (3) -> identity
    Status::Failed,    // Failed      (4) -> identity
    Status::Blocked,   // Blocked     (5) -> identity
];
```

### 2-B  `transition_allowed` — table replace

**Current**: `matches!` macro over 7 tuple pairs = 7 implicit branches.

**Target**: Encode as a 6×6 `const bool` matrix:
```rust
const TRANSITION_TABLE: [[bool; 6]; 6] = {
    let mut t = [[false; 6]; 6];
    // Pending -> Ready, Blocked
    t[0][1] = true; t[0][5] = true;
    // Ready -> Running
    t[1][2] = true;
    // Running -> Ready, Completed, Failed
    t[2][1] = true; t[2][3] = true; t[2][4] = true;
    // Blocked -> Ready
    t[5][1] = true;
    t
};

fn transition_allowed(from: Status, to: Status) -> bool {
    TRANSITION_TABLE[from as usize][to as usize]
}
```
Zero runtime branches. Requires `#[repr(u8)]` on `Status` (done in 2-A).

**Steps**:
```sh
rg -n 'fn transition_allowed' canon/canon-agent/src/pipelines/capability/dag.rs
perl -0777 -ne 'if (/fn transition_allowed.*?^}/ms) { print $& }' \
  canon/canon-agent/src/pipelines/capability/dag.rs
```
Replace entire function body with table lookup.

### 2-C  `detect_cycle` — delegate to `algorithms::graph::scc`

**Current**: `dfs_cycle` recursive DFS with `visited`/`stack` sets — 4 branch points inside
the recursion, called from `detect_cycle` which has 2 more (`for n`, `if !visited`).

**Target**: Delete `dfs_cycle` and `detect_cycle`. Replace with:
```rust
fn detect_cycle(graph: &TaskGraph) -> Result<(), String> {
    // Build adj as Vec<Vec<usize>> (same as resolve_ready already does)
    let id_to_idx: HashMap<&str, usize> = graph.nodes.iter()
        .enumerate().map(|(i, n)| (n.id.as_str(), i)).collect();
    let adj: Vec<Vec<usize>> = graph.nodes.iter().map(|n|
        n.deps.iter().filter_map(|d| id_to_idx.get(d.as_str()).copied()).collect()
    ).collect();
    let sccs = algorithms::graph::scc::kosaraju_scc(&adj);
    if sccs.iter().any(|c| c.len() > 1) {
        return Err("cycle detected in task graph".into());
    }
    Ok(())
}
```
`kosaraju_scc` is already imported in `graph_algo.rs` and available from `algorithms`.
This removes `dfs_cycle` entirely (6 branches deleted).

**Steps**:
```sh
# confirm kosaraju_scc signature
rg -n 'pub fn kosaraju_scc' canon/algorithms/src/graph/scc.rs
perl -0777 -ne 'if (/fn detect_cycle.*?^}/ms) { print $& }' \
  canon/canon-agent/src/pipelines/capability/dag.rs
perl -0777 -ne 'if (/fn dfs_cycle.*?^}/ms) { print $& }' \
  canon/canon-agent/src/pipelines/capability/dag.rs
```

---

## Phase 3 — `act.rs` — table dispatch + branchless guards (removes ~14 branches)

### 3-A  `execute_read_only` / `execute_mutation` — fn-pointer dispatch tables

**Current**: Both functions are `match delta { Delta::X => ... }` with 3–4 arms each.
Each arm is 1 branch; combined = 7 branch points, plus inner guards.

**Target**: Define a handler type and two static dispatch tables:

```rust
type ReadHandler  = fn(&Delta, &[PathBuf], usize) -> Result<(String, String), String>;
type WriteHandler = fn(&Delta, &[PathBuf], &[PathBuf], usize) -> Result<String, String>;
```

Each `Delta` variant maps to exactly one handler via `delta_read_index(d: &Delta) -> usize`
and `delta_write_index(d: &Delta) -> usize`. These index functions use `#[repr(u8)]` on
`Delta` variants — confirm discriminant stability, or use a match that returns `usize`
(one match replacing many matches). The *handler functions* themselves have no internal
branching related to dispatch.

**Steps**:
```sh
rg -n 'fn execute_read_only\|fn execute_mutation' \
  canon/canon-agent/src/pipelines/capability/act.rs
perl -0777 -ne 'if (/fn execute_read_only.*?^}/ms) { print $& }' \
  canon/canon-agent/src/pipelines/capability/act.rs
perl -0777 -ne 'if (/fn execute_mutation.*?^}/ms) { print $& }' \
  canon/canon-agent/src/pipelines/capability/act.rs
```

Extract each arm body into a named `fn handle_read_file(...)`, `fn handle_list_dir(...)`,
`fn handle_read_command(...)`, `fn handle_write_file(...)`, `fn handle_replace_text(...)`,
`fn handle_delete_file(...)`. Replace the `match` arms with direct calls through the table.

The `_ => Err(...)` catch-all arms (2 branches) are replaced by the table structure itself —
wrong-phase deltas simply have no entry in that table, and the indexing function returns an
`Option` with an early `ok_or` — 1 branch total instead of 2.

### 3-B  `apply_read_only` / `apply_mutations` — collapse error accumulation

**Current**: Both have the pattern:
```rust
if error.is_none() { error = Some(msg); }
```
and `apply_read_only` has an extra:
```rust
if !out.trim().is_empty() { ... if !out.ends_with('\n') { ... } }
```
= 4 branches across the two functions.

**Target**:
- Replace `if error.is_none()` with `error.get_or_insert(msg)` — 0 branches.
- Replace the newline-append guard with `out.push_str(out.trim_end_matches('\n')); out.push('\n');`
  applied unconditionally after trimming — 0 branches.
- The `!out.trim().is_empty()` guard can be replaced by always appending and letting the
  consumer deal with empty strings (check whether callers care — `apply_node_result` in
  `engine.rs` discards output on empty; safe to remove).

**Steps**:
```sh
rg -n 'error.is_none\|ends_with.*\\\\n\|trim.*is_empty' \
  canon/canon-agent/src/pipelines/capability/act.rs
```

### 3-C  `resolve_path` — branchless absolute/relative

**Current**:
```rust
let resolved = if p.is_absolute() { p.to_path_buf() } else { roots[0].join(p) };
if !allow_nonexistent && !resolved.exists() { ... }
```
2 branches.

**Target**: The absolute/relative branch is unavoidable but can be extracted into a one-liner
using a helper that makes intent explicit:
```rust
fn anchor(p: &Path, root: &Path) -> PathBuf {
    if p.is_absolute() { p.to_path_buf() } else { root.join(p) }
}
```
The `allow_nonexistent` branch stays as one branch — it is a legitimate guard. Net: reduces
`resolve_path` from 4 to 2 visible branch points by extracting helpers.

### 3-D  `has_parent_dir_component` — branchless

**Current**: `args.iter().any(|a| a.split('/').any(|c| c == ".."))` — already 0 explicit
branches. **No change needed.**

### 3-E  `truncate_lines` — remove counter variable branch

**Current**: manual `count` variable with `if count >= max_lines { break; }` inside `for`.

**Target**:
```rust
fn truncate_lines(text: &str, max_lines: usize) -> String {
    let mut iter = text.lines();
    let kept: Vec<&str> = iter.by_ref().take(max_lines).collect();
    let remaining = iter.count();
    let mut out = kept.join("\n");
    if remaining > 0 {
        out.push_str(&format!("\n... [{} lines truncated] ...", remaining));
    }
    out
}
```
`.take(max_lines)` replaces the `if count >= max_lines` branch. `if remaining > 0` stays
(1 branch, unavoidable for correct output). Net: -1 branch.

---

## Phase 4 — `engine.rs` — table dispatch on `DispatchMode` (removes ~9 branches)

### 4-A  `call_mode` match arms — data table

**Current**: `match mode { DispatchMode::Mutate => (...), DispatchMode::Verify => (...), DispatchMode::Readonly => (...) }`
appearing **twice** in `call_mode` (once for prompt building, once for parse/wrap) = 6 branches.

**Target**: Define a struct `ModeConfig` that holds the static prompt/schema/log-name strings
per mode, built once at the top of `call_mode`:

```rust
struct ModeConfig {
    phase:    &'static str,
    schema:   &'static str,
    log_name: fn(u64) -> String,
}

const MODE_CONFIGS: [ModeConfig; 3] = [
    ModeConfig { phase: "mutate",   schema: MUTATE_SCHEMA,   log_name: |i| format!("iter_{:03}_execute_output.json", i) },
    ModeConfig { phase: "verify",   schema: VERIFY_SCHEMA,   log_name: |i| format!("iter_{:03}_verify_output.json", i) },
    ModeConfig { phase: "readonly", schema: READONLY_SCHEMA, log_name: |i| format!("iter_{:03}_readonly_output.json", i) },
];
```

Index by `mode as usize` (add `#[repr(u8)]` to `DispatchMode`). The input `Value` construction
still requires a match (it references different node fields per mode), but it is one match
instead of two. The parse/wrap section becomes a match over the already-known mode with handler
fns: `parse_mutate`, `parse_verify`, `parse_readonly` — each a standalone fn with no internal
dispatch branching.

**Steps**:
```sh
perl -0777 -ne 'if (/fn call_mode.*?^}/ms) { print $& }' \
  canon/canon-agent/src/pipelines/capability/engine.rs
rg -n 'DispatchMode' canon/canon-agent/src/pipelines/capability/engine.rs
```

Extract schema strings as `const` statics above `call_mode`. Replace the two `match mode`
blocks: first with `MODE_CONFIGS[mode as usize]`, second with three named parse fns called
through a fn-pointer array `PARSE_FNS: [fn(...) -> Result<NodeCallResult>; 3]`.

### 4-B  `apply_node_result` — already a dispatch table (match on enum)

**Current**: `match result { Mutate => ..., Readonly => ..., Verify => ... }` = 3 branches.
These map cleanly to `apply_mutate_output`, `apply_readonly_output`, `apply_verify_output`.

**Target**: Add `#[repr(u8)]` to `NodeCallResult` and replace the match with a fn-pointer array:
```rust
type ApplyFn = fn(NodeCallResult, &mut TaskGraph, &[PathBuf], usize, &Path, u64, &CapabilityPolicy) -> Result<()>;
static APPLY_FNS: [ApplyFn; 3] = [apply_mutate, apply_readonly, apply_verify];
APPLY_FNS[result.discriminant()](result, graph, roots, max_output_lines, log_dir, iter, policy)
```
This requires a `discriminant()` helper or matching only for the index. Net: 3 -> 1 branch
(the index extraction).

### 4-C  `apply_mutate_output` inner guards

**Current**:
```rust
if result.id != node_id { result.id = node_id.clone(); }
```
appears in all three apply fns = 3 branches.

**Target**: Extract `fn normalize_id(result_id: &mut String, node_id: &str)` that does the
assignment unconditionally using `clone_from`:
```rust
fn coerce_id(result_id: &mut String, canonical: &str) {
    result_id.clone_from(&canonical.to_string());
}
```
Wait — this always overwrites. Since `result.id == node_id` is the common case, the branch
is a short-circuit optimisation. Replace with `result.id.clone_from(node_id)` unconditionally —
this is correct and removes 3 branches at the cost of a no-op clone in the common path.
Confirm this is acceptable (it is — `String::clone_from` is `O(len)` but avoids allocation
when capacity suffices).

### 4-D  `apply_mutate_output` — `requires_verify` / `has_err` double-lookup

**Current**:
```rust
let requires_verify = graph.nodes.iter().find(...).map(...).unwrap_or(false);
if !requires_verify {
    let has_err = graph.nodes.iter().find(...).and_then(...).is_some();
    let _ = if has_err { graph.update_status(...Failed) } else { graph.update_status(...Completed) };
}
```
3 branches + 2 linear scans.

**Target**: Combine into one lookup:
```rust
if let Some(n) = graph.get_node_mut(&result.id) {
    let requires_verify = n.required_capabilities.contains(&Capability::StatusUpdateOnly);
    if !requires_verify {
        let s = if n.error.is_some() { Status::Failed } else { Status::Completed };
        let _ = graph.update_status(&result.id, s);
    }
}
```
2 branches instead of 3, 1 lookup instead of 2.

---

## Phase 5 — `mod.rs` — largest file (removes ~14 branches)

### 5-A  `build_graph_from_edges` — retry duplication

**Current**: The validate → replan → re-apply-edges sequence is written **twice** (once for
`graph.validate()` failure, once for `enforce_linking_constraints` failure). Each copy has:
`for edge in plan.edges` + `if !ids.contains` + `if let Some(node)` = 3 branches. Two copies
= 6 branches that are identical logic.

**Target**: Extract:
```rust
fn apply_edges_to_graph(
    nodes: &[TaskNode],
    edges: Vec<planner::EdgeSpec>,
    log_event: &str,
) -> dag::TaskGraph {
    let mut graph = dag::TaskGraph { nodes: nodes.to_vec() };
    let ids: HashSet<String> = graph.nodes.iter().map(|n| n.id.clone()).collect();
    for n in &mut graph.nodes { n.deps.clear(); }
    for edge in edges {
        if !ids.contains(&edge.from) || !ids.contains(&edge.to) {
            eprintln!(r#"[capability] {{"event":"{}","from":"{}","to":"{}"}}"#,
                      log_event, edge.from, edge.to);
            continue;
        }
        if let Some(node) = graph.get_node_mut(&edge.to) {
            node.deps.push(edge.from);
        }
    }
    graph
}
```
Then `build_graph_from_edges` becomes:
```rust
async fn build_graph_from_edges(...) -> Result<dag::TaskGraph> {
    let plan = plan_edges_burst(...).await?;
    let mut graph = apply_edges_to_graph(nodes, plan.edges, "edge_rejected");
    if let Err(e) = graph.validate().and_then(|_| enforce_linking_constraints(&graph).map_err(|e| e.into())) {
        // single retry path
        let note = format!("previous edge set invalid: {e}");
        let plan = planner::plan_edges(..., Some(&note), ...).await?;
        let graph_retry = apply_edges_to_graph(nodes, plan.edges, "edge_rejected");
        graph_retry.validate().map_err(|e| anyhow::anyhow!(e))?;
        enforce_linking_constraints(&graph_retry).map_err(|e| anyhow::anyhow!(e))?;
        return Ok(graph_retry);
    }
    Ok(graph)
}
```
Reduces from 2 retry blocks (6 branches each = 12) to 1 retry block (3 branches) + 1 shared
helper (3 branches) = 6 total. Net: **-6 branches**.

### 5-B  `run_capability_loop` — blocked streak

**Current**:
```rust
if graph.has_failed() && graph.ready_nodes().is_empty() {
    blocked_streak += 1;
    ...
    if blocked_streak >= 3 { anyhow::bail!("blocked"); }
    continue;
}
blocked_streak = 0;
```
2 branches.

**Target**: Use saturating arithmetic and a const threshold. No structural change possible
without changing semantics. **Accepted cost** — leave as is.

### 5-C  `select_endpoints_for_role` — weight accumulation loop

**Current**: `for (ep_idx, w) in &weights { acc += *w as usize; if idx < acc { chosen = *ep_idx; break; } }`
= 2 branches inside the inner loop.

**Target**: Use `Iterator::scan` + `find_map`:
```rust
let chosen = weights
    .iter()
    .scan(0usize, |acc, &(ep_idx, w)| { *acc += w as usize; Some((*acc, ep_idx)) })
    .find_map(|(acc, ep_idx)| (idx < acc).then_some(ep_idx))
    .unwrap_or(weights[0].0);
```
`if idx < acc` is now inside `.then_some()` — still 1 branch conceptually but expressed as
a predicate. Net: removes explicit `break` and `if`, -2 branches per call.

### 5-D  `merge_decompose_outputs` — duplicate-id rename

**Current**: `if !seen.insert(t.id.clone()) { ... let c = counter.entry(...).or_insert(0); *c += 1; t.id = format!(...); let _ = seen.insert(t.id.clone()); }`
= 1 branch.

**Target**: Use `entry` API to always generate a canonical id:
```rust
let slot = counter.entry(t.id.clone()).or_insert(0usize);
if *slot > 0 { t.id = format!("{}__{}", t.id, *slot); }
*slot += 1;
seen.insert(t.id.clone());
```
Still 1 branch but removes the `seen.insert` duplication and makes intent clear.
**Accepted** — minimal gain.

### 5-E  `expand_nodes` — depth guard

**Current**: `if depth > max_depth { continue; }` inside the child loop = 1 branch.

**Target**: Filter before pushing:
```rust
output.tasks.into_iter()
    .filter(|_| current_depth + 1 <= max_depth)
    ...
```
Equivalent, 1 branch remains but it is now a filter predicate rather than a `continue`.
**Accepted** — cosmetic only.

### 5-F  `prune_unlinked_nodes` — map + filter

**Current**: Two `for n in &graph.nodes` loops building `indegree`/`outdegree`, then
`filter_map` with `if in_d == 0 && out_d == 0 { None } else { Some(...) }` = 4 branches.

**Target**: Collapse degree computation into one pass using `entry`:
```rust
fn prune_unlinked_nodes(graph: &mut dag::TaskGraph) {
    let mut degree: HashMap<&str, (usize, usize)> = graph.nodes.iter()
        .map(|n| (n.id.as_str(), (0usize, 0usize))).collect();
    for n in &graph.nodes {
        for d in &n.deps {
            if let Some(e) = degree.get_mut(n.id.as_str()) { e.0 += 1; } // in
            if let Some(e) = degree.get_mut(d.as_str())    { e.1 += 1; } // out
        }
    }
    graph.nodes.retain(|n| {
        let (ind, outd) = degree.get(n.id.as_str()).copied().unwrap_or((0, 0));
        ind > 0 || outd > 0
    });
}
```
Reduces from 4 separate loops/branches to 2 (inner `if let Some` guards + `retain` predicate).

---

## Phase 6 — Verify with cargo check

After all patches are applied:

```sh
cd canon && cargo check -p canon-agent --features cuda 2>&1 | head -80
```

Fix any type errors surfaced (likely: `#[repr(u8)]` discriminant casts, fn-pointer signatures).
Do **not** run `cargo build`, `cargo test`, or any other cargo subcommand.

---

## Phase 7 — Confirm branch reduction

```sh
rg -cn '\b(if|match|for|while)\b' \
  canon/canon-agent/src/pipelines/capability/mod.rs \
  canon/canon-agent/src/pipelines/capability/act.rs \
  canon/canon-agent/src/pipelines/capability/engine.rs \
  canon/canon-agent/src/pipelines/capability/graph_algo.rs \
  canon/canon-agent/src/pipelines/capability/dag.rs
```

Expected totals (approximate):

| File            | Before | After  | Δ     |
|-----------------|--------|--------|-------|
| `mod.rs`        | 93     | ~79    | -14   |
| `act.rs`        | 36     | ~22    | -14   |
| `engine.rs`     | 33     | ~24    | -9    |
| `graph_algo.rs` | 28     | ~14    | -14   |
| `dag.rs`        | 21     | ~11    | -10   |
| **Total**       | **211**| **~150** | **-61** |

---

## Execution Order for Agent

Execute phases strictly in order. Do not skip Phase 0. Do not proceed past Phase 6 cargo check
if errors exist — fix them inline. Do not create test files, summary files, or documentation
files other than this plan.

After Phase 7, commit:
```sh
git add -A
git commit -m "refactor(capability): reduce branching 211->~150 via table-dispatch, branchless predicates, GPU graph offload"
```
