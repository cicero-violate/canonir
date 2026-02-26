### Objective — Refactor Goal

**Goal:**
Make CanonIR the single source of semantic truth such that the emitter contains zero semantic logic and performs only deterministic projection.

---

### Explicit Outcomes

1. **Remove all emitter heuristics**

   * No string-based path rewriting
   * No import injection
   * No visibility inference
   * No semantic repair

2. **Push semantic completion upstream**

   * Imports must exist explicitly in CanonIR
   * Paths must be canonicalized before emission
   * Visibility must be resolved during capture/solve
   * Generics and bounds must be structurally encoded

3. **Guarantee emission correctness by structure**

   * If CanonIR is valid → emission compiles
   * If emission fails → CanonIR is incomplete

4. **Define a hard invariant**

   * Emitter never inspects emitted text to decide behavior
   * Emitter never modifies IR
   * Emitter never derives missing semantics

---

### Final Target Architecture

[
\text{Capture} \rightarrow \text{CanonIR} \rightarrow \text{Solver} \rightarrow \text{Emit}
]

Where:

* Capture = extract resolved rustc facts
* CanonIR = canonical semantic graph
* Solver = generate and enforce semantic laws
* Emit = pure projection

---

### Success Condition

You can delete all normalization heuristics from:

* `file.rs`
* `fmt.rs`
* any path rewrites

And nothing breaks.

---


