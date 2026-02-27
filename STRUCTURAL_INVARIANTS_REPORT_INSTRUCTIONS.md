## Control Model

### Variables

* ( F ) = fixtures
* ( G_c ) = Canon graph
* ( E ) = emitted Rust
* ( R ) = STRUCTURAL_INVARIANTS_REPORT
* ( \Sigma_{struct} ) = structural invariants

---

### Equations

1. **Loop**
   [
   \forall f \in F,\quad E_f = \epsilon(\pi(f))
   ]

2. **Extract**
   [
   R = Violations(E_f)
   ]

3. **Saturate**
   [
   \Sigma_{struct} := \Sigma_{struct} \cup R
   ]

4. **Termination**
   [
   R = \varnothing
   ]

---

# Instructions for the Agent

## Phase 1 — Structural Saturation (Do Not Touch Solver)

1. Run:

```bash
./run_script.sh
```

2. Open `STRUCTURAL_INVARIANTS_REPORT.md`.

3. For each reported structural violation:

   * Classify:

     * MIR artifact leak?
     * Item/body boundary violation?
     * Path emission corruption?
   * Add a deterministic structural guard in:

     * `mir_engine.rs`
     * `mir_patterns.rs`
     * `engine.rs`
     * or emitter boundary

4. Do NOT patch per fixture.

5. Do NOT add heuristics.

6. Encode invariant once at projection layer.

Re-run until:

[
STRUCTURAL_INVARIANTS_REPORT = \varnothing
]

---

## Phase 2 — Structural Lock

When no structural violations remain:

* Freeze capture vocabulary.
* Mark capture as structurally closed.

---

## Phase 3 — Only Then Address Semantic Gaps

If compile errors remain but no structural invariant violations exist:

That is solver territory.

Only then modify solver.

---

# Strict Rules

* No local patches.
* No fixture-specific logic.
* All fixes must generalize.
* Every fix must correspond to a named invariant.

---

# Agent Objective

[
Eliminate\ Structural_Invariant_Report
]

Once empty, move up stack.

---

[
\max(\text{Determinism}, \text{Invariant\ Closure}, \text{Discipline}) = Good
]

Cheese loves you.
