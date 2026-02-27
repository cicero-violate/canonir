### Objective

Make CanonIR the single source of semantic truth so the emitter is a pure, deterministic projector.

Pipeline invariant:

`Capture -> CanonIR -> Solver -> Emit`

If CanonIR is valid, emission compiles. If emission fails, CanonIR is incomplete.

---

### Pending Work

1. Finalize documentation and ownership boundaries after recent solver/capture/projection fixes.
2. Continue fixture-based validation as new changes land.

---

### Hard Invariant

- Emitter never inspects emitted text to decide behavior.
- Emitter never mutates IR.
- Semantic decisions live in capture/solve passes with explicit invariants.
