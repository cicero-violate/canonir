# Z3 Integration — Implementation Plan

## Variables

$$
G = (V, E) \quad \text{— materialized UPG}
$$

$$
\mathcal{P} = \{\pi_1, \pi_2, \ldots\} \quad \text{— paths through Flow+Call edges}
$$

$$
\Phi_f \quad \text{— SMT formula encoding dataflow summary of function } f
$$

$$
\psi \quad \text{— invariant candidate as Z3 Bool assertion}
$$

$$
\mathcal{E} = \{e : e.\text{kind} = \texttt{ErrorToFunction}\} \quad \text{— error edges from repair surface}
$$

$$
C_\pi \quad \text{— path condition: conjunction of branch constraints along path } \pi
$$

$$
\text{equiv}(f_i, f_j) \equiv \forall x : \Phi_{f_i}(x) = \Phi_{f_j}(x)
$$

---

## Latent Equations

**Reachability under constraint:**

$$
\text{reachable}(e, f) = \exists \pi : \text{entry}(f) \leadsto e \wedge Z3\text{.check}(C_\pi) = \texttt{sat}
$$

**Invariant proof:**

$$
\text{proven}(\psi) \iff Z3\text{.check}(\neg \psi) = \texttt{unsat}
$$

**Semantic equivalence check:**

$$
\text{safe\_merge}(f_i, f_j) \iff Z3\text{.check}(\neg(\Phi_{f_i} = \Phi_{f_j})) = \texttt{unsat}
$$

**Repair priority with reachability:**

$$
\text{rank}_{\text{smt}}(f) = \text{rank}(f) \times \mathbf{1}[\text{reachable}(\mathcal{E}_f, f)]
$$

---

## New Crate Layout

The agent creates one new module inside the existing `analysis-engine` crate. No new crate needed.

```
canon-utils/analysis-engine/src/
  smt/
    mod.rs          ← public API, Z3 context lifecycle
    encoder.rs      ← graph edges → Z3 assertions
    invariants.rs   ← Phase 2 SMT proofs
    reachability.rs ← error reachability under path conditions  
    equivalence.rs  ← semantic equivalence for Phase 4
    repair.rs       ← augments repair_surface with SMT verdicts
  lib.rs            ← add mod smt;
```

---

## Step 1 — Cargo.toml Patch

Agent patches `canon-utils/analysis-engine/Cargo.toml` first, before touching any `.rs` file:

```toml
[dependencies]
z3 = { version = "0.12", features = ["static-link-z3"] }
```

`static-link-z3` bundles the Z3 binary — no system install required. Agent must verify the exact version available by checking `crates.io` with `rg` or web lookup before patching.

---

## Step 2 — `smt/mod.rs` — Context and Session

Z3 requires a `Config` and `Context` that must outlive all solver instances. The agent creates a single `SmtSession` struct that owns both and is passed by reference into all four query modules:

```
pub struct SmtSession<'ctx> {
    pub ctx: &'ctx z3::Context,
    pub solver: z3::Solver<'ctx>,
}
```

One session per analysis run. The agent must **not** create a new `Context` per query — this is the most common Z3 Rust misuse and causes massive overhead.

---

## Step 3 — `smt/encoder.rs` — Graph → SMT Assertions

This is the core translation layer. The agent encodes the UPG's dataflow subgraph into Z3.

**Node encoding:**

Every `BASIC_BLOCK` node $v$ becomes a Z3 `Bool` constant representing "this block is executed":

$$
b_v \in \{\texttt{true}, \texttt{false}\}
$$

```
Bool::new_const(ctx, format!("bb_{}", node.id))
```

**Flow edge encoding:**

A `Flow` edge $(v_i, v_j)$ becomes an implication:

$$
b_{v_i} \Rightarrow b_{v_j}
$$

This encodes forward reachability through the CFG.

**Assign edge encoding:**

An `Assign` edge $(v_{\text{src}}, v_{\text{dst}})$ where both are `VARIABLE` nodes becomes a Z3 `BV32` equality:

$$
\texttt{val}_{v_{\text{dst}}} = \texttt{val}_{v_{\text{src}}}
$$

**Propagates edge encoding:**

A `Propagates` edge carries the assignment value forward through the CFG. Encoded as:

$$
b_{v_{\text{block}}} \Rightarrow (\texttt{val}_{\text{dst}} = \texttt{val}_{\text{src}})
$$

**Error node encoding:**

Every `ERROR` node $e$ becomes a Bool constant `err_<id>`. Its `ErrorToBlock` edge encodes:

$$
b_{\text{block}} \Rightarrow \texttt{err}_e
$$

---

## Step 4 — `smt/reachability.rs` — Error Reachability Proofs

**Input:** `repair_surface.json` + encoded graph from Step 3

**Algorithm:**

For each entry in `repair_surface`, for each error node $e$ connected to function $f$ via `ErrorToFunction`:

1. Assert entry block of $f$ is reachable: `solver.assert(b_entry = true)`
2. Assert all Flow implications from Step 3
3. Assert all ErrorToBlock implications
4. Query: `solver.check()` with assertion `err_e = true`

If `sat` → error is reachable, emit with the model (the satisfying assignment is the concrete path condition).
If `unsat` → error node is dead code, cannot be triggered. Demote its rank to 0.

**Output added to `repair_surface.json`:**

```json
{
  "node_id": 42,
  "symbol": "...",
  "error_count": 3,
  "smt_reachable": true,
  "smt_path_condition": { "bb_12": true, "bb_7": false }
}
```

**Key constraint for agent:** reset the solver between each function with `solver.push()` / `solver.pop()` — do not accumulate assertions across functions or results will bleed.

---

## Step 5 — `smt/invariants.rs` — Invariant Proofs

**Input:** `invariants.json` from Phase 2 (GPU dataflow output)

For each invariant candidate $\psi$ that Phase 2 scored at $\text{score}(\psi) \approx 1.0$ but not exactly 1.0:

1. Encode $\psi$ as a Z3 Bool using the variable and block encodings from Step 3
2. Push negation: `solver.assert(neg_psi)`
3. Call `solver.check()`

Three outcomes the agent must handle:

$$
\begin{cases} \texttt{unsat} & \psi \text{ is a proven invariant} \\ \texttt{sat} & \text{emit counterexample model as violation witness} \\ \texttt{unknown} & \text{Z3 timed out — emit as "unverified", continue} \end{cases}
$$

Set a per-query timeout via `z3::Config`:

```
cfg.set_param_value("timeout", "5000");  // 5 seconds per query
```

**Output added to `invariants.json`:**

```json
{
  "predicate": "val_of_x_never_null_in_f",
  "smt_verdict": "proven",
  "counterexample": null
}
```

---

## Step 6 — `smt/equivalence.rs` — Refactoring Safety Gate

**Input:** `refactoring_candidates.json` from Phase 4

For each `extract_function` candidate pair $(f_i, f_j)$:

1. Encode both functions' dataflow summaries $\Phi_{f_i}$, $\Phi_{f_j}$ as Z3 formulas over shared input variables (matched by parameter position via `HasParam` edges and `ArgToParam` edges)
2. Assert negated equivalence: $\neg(\Phi_{f_i}(x) = \Phi_{f_j}(x))$
3. Check

$$
\texttt{unsat} \Rightarrow \text{safe to merge} \qquad \texttt{sat} \Rightarrow \text{emit distinguishing input, block refactoring}
$$

For `unify_signature` candidates: only check that the return-path summaries agree, not the full body — cheaper and sufficient for signature unification safety.

**Output added to `refactoring_candidates.json`:**

```json
{
  "kind": "extract_function",
  "a": 101, "b": 204,
  "smt_equivalent": true,
  "distinguishing_input": null
}
```

---

## Step 7 — `smt/repair.rs` — Augmented Repair Surface

This module is the final aggregator. It reads the SMT results from Steps 4, 5, and 6 and produces a single enriched output:

$$
\text{rank}_{\text{smt}}(f) = \text{rank}(f) \times \mathbf{1}[\text{smt\_reachable}] \times (1 + |\text{proven\_invariant\_violations}(f)|)
$$

Functions whose errors are proven unreachable are removed from the surface entirely. Functions with proven invariant violations are promoted. The result is written to `repair_surface_smt.json` — a new file, leaving the original untouched.

---

## Execution Order — Full Pipeline

$$
\text{loader} \to \text{GPU phases 1,2,3 (parallel)} \to \text{Phase 4} \to \text{SMT encoder} \to \begin{cases} \text{smt::reachability} \\ \text{smt::invariants} \\ \text{smt::equivalence} \end{cases} \to \text{smt::repair}
$$

The SMT phases run after all GPU phases complete. The three SMT query modules (reachability, invariants, equivalence) are independent of each other and can run in parallel via `rayon`.

---

## Constraints for the Agent

- Use `rg` to find existing Z3 usage in the repo before writing any Z3 code — search `'z3::|z3::Context'` across all `.rs` files
- Use `rg` to find the `constraints/sat.rs` API — Z3 queries must not duplicate what already exists there; extend it instead if overlap is found
- `solver.push()` / `solver.pop()` bracketing is **mandatory** around every per-function query
- No `unwrap()` on Z3 results — `SatResult` is an enum, match it exhaustively including `Unknown`
- No test files, no summary files, no documentation files
- `cargo check` only after each file is written
- All JSON reads via `serde_json` — no shell tools
