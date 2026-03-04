I have everything I need. Here is the full implementation plan.

---

## Variables

$$
\text{Let } S_i = \text{solver}_i, \quad G_k = \text{graph}_k \text{ (from IR)}, \quad A_j = \text{GPU algorithm}_j
$$

## Latent Equations

$$
S_i' = A_j(G_k(\text{IR})) \quad \text{where } A_j \in \{\text{reachability\_gpu},\ \text{dominators},\ \text{ac3\_gpu\_apply}\}
$$

$$
\text{feature}(\text{cuda}) \Rightarrow A_j^{\text{GPU}}, \quad \neg\text{feature}(\text{cuda}) \Rightarrow A_j^{\text{CPU}}
$$

## Explanation

The `cuda` feature is already declared in `algorithms/Cargo.toml` and defaults to on. `canon-analyzer` already depends on `algorithms`. The GPU functions exist behind `#[cfg(feature = "cuda")]` guards. The solvers need to call those functions instead of their inline CPU implementations, using a `Csr` adapter to bridge `CsrGraph` → GPU buffers. No `Cargo.toml` changes are needed.

---

## Implementation Plan

### Phase 0 — Add the CSR bridge helper in `solver/mod.rs`

**Why first:** Every GPU call needs `row_ptr: &[i32]` + `col_idx: &[i32]`. The existing `csr_to_adj` produces `Vec<Vec<usize>>` (CPU adj-list). We need a parallel helper that produces an `algorithms::graph::csr::Csr` directly from a `CsrGraph`.

**Action:** Add `pub(crate) fn graph_to_csr<ND,ED>(graph: &CsrGraph<ND,ED>) -> algorithms::graph::csr::Csr` to `solver/mod.rs`. It maps `u32` row/col values to `i32`. This is the only shared plumbing all solvers rely on.

---

### Phase 1 — `cfg_solver.rs`

**Current state:** Inline CPU DFS for reachability + hand-rolled iterative dominator loop.

**Target algorithms:**
$$
\text{reach} = \text{reachability\_gpu}(\text{Csr}(G_{\text{cfg}}),\ [0])
$$
$$
\text{dom}[v] = \text{dominators}(G_{\text{cfg}},\ \text{preds},\ 0)
$$

**Steps:**
1. Import `algorithms::graph::reachability::reachability_gpu` behind `#[cfg(feature="cuda")]`.
2. Import `algorithms::control_flow::dominators::dominators`.
3. Replace the hand-rolled BFS/DFS reachability block with `reachability_gpu(&csr, &[0])` → `Vec<bool>`.
4. Replace the `while changed` dominator loop with `dominators(v, &preds_map, 0)` → `Vec<HashSet<usize>>`.
5. Keep the CPU DFS fallback path under `#[cfg(not(feature="cuda"))]` using the existing `dfs` import.
6. Derive `_dead` and `dom` from the new return types (shapes differ — `Vec<bool>` vs `HashSet`).

**Files touched:** `cfg_solver.rs`

---

### Phase 2 — `liveness_solver.rs`

**Current state:** Inline BFS (`reachability_mask`) using `VecDeque` on `call_graph`.

**Target algorithm:**
$$
\text{live}[v] = \text{reachability\_gpu}(\text{Csr}(G_{\text{call}}),\ \text{roots})
$$

**Steps:**
1. Convert `ir.call_graph` → `Csr` via the Phase 0 helper.
2. Under `#[cfg(feature="cuda")]`: call `reachability_gpu(&csr, &roots)` → `Vec<bool>`.
3. Under `#[cfg(not(feature="cuda"))]`: keep the existing `reachability_mask` function as CPU fallback.
4. Remove the `VecDeque` import (it becomes dead under cuda feature).
5. The `ir.emit_order.retain` logic is unchanged — it already consumes `Vec<bool>`.

**Files touched:** `liveness_solver.rs`

---

### Phase 3 — `type_solver.rs`

**Current state:** Calls `kosaraju_scc` (CPU) on `type_graph`. No constraint propagation.

**Target algorithm:** AC-3 is the right fit for type constraint propagation. SCC stays — it's the correct algorithm for cycle detection in type graphs.

**Steps:**
1. Add `algorithms::constraints::ac3::{ac3_gpu_apply, ConstraintGraph, Domain}` import under `#[cfg(feature="cuda")]`.
2. After the SCC pass, add a `build_type_constraint_graph(ir)` function that constructs a `ConstraintGraph` where variables are type node indices and constraints encode subtype/equality relations from `EdgeKind`.
3. Call `ac3_gpu_apply(&domains, &graph)` to prune inconsistent type variable domains.
4. Emit diagnostics for any empty domain (unresolvable type).
5. Keep `kosaraju_scc` — it stays for cycle detection, AC-3 is additive.

**Files touched:** `type_solver.rs`

---

### Phase 4 — `borrow_solver.rs`

**Current state:** Calls `outlives_cycles` (CPU SCC on `region_graph`).

**Target algorithm:** Reachability on the region graph catches lifetime constraint violations without full SCC enumeration.

$$
\text{conflicts} = \text{reachability\_gpu}(\text{Csr}(G_{\text{region}}),\ \text{roots}) \cap \text{cycles}
$$

**Steps:**
1. Convert `ir.region_graph` → `Csr` via Phase 0 helper.
2. Under `#[cfg(feature="cuda")]`: call `reachability_gpu` from all lifetime root nodes to find which lifetime nodes are in the closure.
3. Keep `outlives_cycles` for the actual cycle extraction — it's still the right tool. GPU reachability is a pre-filter to skip solvers on acyclic subgraphs quickly.
4. Add `#[cfg(not(feature="cuda"))]` guard preserving current behavior exactly.

**Files touched:** `borrow_solver.rs`

---

### Phase 5 — `dep_solver.rs` (read first)

**Likely state:** Topological sort on module/dependency graph. Need to read before prescribing exact steps.

**Planned target:**
$$
\text{order} = \text{topological\_sort}(G_{\text{dep}}) \quad \text{via } \texttt{algorithms::graph::topological\_sort}
$$

**Action for agent:** Run `bat canon-analyzer/src/solver/dep_solver.rs` then apply the same GPU-bridge pattern.

---

### Phase 6 — `gpu_algorithms.rs` (new file)

**Purpose:** Single re-export point so solvers import from one place, not scattered paths.

```
canon-analyzer/src/solver/gpu_algorithms.rs
```

Contents:
```rust
#[cfg(feature = "cuda")]
pub use algorithms::graph::reachability::reachability_gpu;
#[cfg(feature = "cuda")]
pub use algorithms::constraints::ac3::ac3_gpu_apply;
pub use algorithms::control_flow::dominators::dominators;
pub use algorithms::graph::scc::kosaraju_scc;
pub use algorithms::graph::dfs::dfs;
```

Register it in `solver/mod.rs` as `pub(crate) mod gpu_algorithms;`.

---

### Ordering for the agent

$$
\text{Phase } 0 \rightarrow 6 \rightarrow 1 \rightarrow 2 \rightarrow 3 \rightarrow 4 \rightarrow 5
$$

Do Phase 0 (bridge) and Phase 6 (re-exports) **before** touching any solver. Every subsequent phase depends on both. After each solver phase run `cargo check` to gate progress.

---

### What does NOT change

- `solver/mod.rs` solve pipeline order — untouched
- `algorithms/Cargo.toml` — already has `cuda` feature defaulting to on
- `canon-analyzer/Cargo.toml` — already depends on `algorithms`
- `kosaraju_scc` in `type_solver` — correct algorithm, stays
- `outlives_cycles` in `borrow_solver` — stays as the cycle extractor, GPU is a pre-filter only
