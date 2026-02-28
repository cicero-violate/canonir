## Refactor Plan → Codex-Style Deterministic Loop
Turn the pipeline into a pure state transition machine with deterministic phases and structured IO.

---

# Phase 1 — Separate the Loop Engine

### Goal

Decouple orchestration logic from LLM logic.

### Actions

1. Extract loop driver:

   * `fn codex_loop(state: S) -> S`
2. Move:

   * plan_via_llm
   * act
   * verification
     into separate modules:

   ```
   pipeline/
     loop.rs
     prompt.rs
     model.rs
     apply.rs
     verify.rs
   ```

---

# Phase 2 — Enforce Structured Model Contract

### Replace free-form prompt with schema-bound output

Define:

```rust
struct CodexResponse {
    patches: Vec<Patch>,
    commands: Vec<String>,
    rationale: String,
}
```

Enforce:

```
M(prompt) → STRICT JSON ONLY
```

Equation:

```
Valid(J) = schema_check(J)
If ¬Valid → reject → retry
```

This eliminates heuristic extraction and makes the loop mechanical.

---

# Phase 3 — Make State Explicit

Current pipeline is partially implicit via filesystem.

Refactor into:

```rust
struct LoopState {
    surface_before: StructuralSurface,
    surface_after: Option<StructuralSurface>,
    build_status: BuildResult,
    last_diff_summary: String,
    gap_delta: i64,
}
```

Equation:

```
Sᵢ₊₁ = verify(applyᵢ, Sᵢ)
```

No hidden state in logs. Everything becomes state transition.

---

# Phase 4 — Deterministic Feedback Builder

Replace ad-hoc diff text with structured feedback object:

```
F = {
   gap_delta: Δ,
   per_file_delta: Vec<(file, delta)>,
   per_fn_delta: Vec<(fn, delta)>,
   build_ok: bool
}
```

Prompt becomes:

```
promptᵢ = TEMPLATE(context, Fᵢ₋₁)
```

Not concatenated strings. Structured embedding.

---

# Phase 5 — Remove Multi-Step Nested Loop

Current design:

```
for step in 0..MAX_STEPS
```

Replace with single atomic Codex tick:

```
One model call per iteration.
If failure → next iteration.
```

Equation:

```
Iteration = atomic
No inner retry loop.
```

Codex pipelines work best with short atomic cycles.

---

# Phase 6 — Guardrails as Validators, Not Prompt Rules

Move:

```
if patch.contains("canon suppressed binding") → reject
```

Into:

```
fn validate(J) -> Result
```

Equation:

```
J_valid = Valid_schema(J) ∧ Valid_semantics(J)
```

This keeps prompt clean and enforcement mechanical.

---

# Phase 7 — Explicit Stop Conditions

Define convergence formally:

```
Stop if:
  unresolved_ret_gap_count == 0
OR
  i == max_iters
OR
  stagnation_count > k
```

Where:

```
stagnation = (Δ == 0)
```

---

# Phase 8 — Clean Architecture

Final architecture:

```
CodexLoop
  ├── PromptBuilder
  ├── ModelClient
  ├── ResponseParser
  ├── Validator
  ├── Applicator
  ├── Verifier
  └── FeedbackBuilder
```

Each component pure and testable.

---

# Final System Equation

```
Good = max(Intelligence, Determinism, Transparency)

LoopGoodness =
  (Δ > 0) * 0.5 +
  (build_ok) * 0.2 +
  (schema_valid) * 0.3
```

Maximize:

```
max(Intelligence, Efficiency, Correctness, Alignment,
    Robustness, Performance, Scalability,
    Determinism, Transparency, Collaboration,
    Empowerment, Benefit, Learning, FutureProofing)
```

= Good

---

If desired, next step: I can generate the exact module boundaries and Rust skeleton for this refactor.
