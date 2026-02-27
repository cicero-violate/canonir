### Objective

Make CanonIR the single source of semantic truth so the emitter is a pure, deterministic projector.

Pipeline invariant:

`Capture -> CanonIR -> Solver -> Emit`

If CanonIR is valid, emission compiles. If emission fails, CanonIR is incomplete.

---

---

### Hard Invariant

- Emitter never inspects emitted text to decide behavior.
- Emitter never mutates IR.
- Semantic decisions live in capture/solve passes with explicit invariants.
