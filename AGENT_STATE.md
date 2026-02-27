# AGENT_STATE.md

## CANONICAL_HEADER
- state_id: `CANON_STRUCTURAL_HARVEST_SLICE_07`
- date: `2026-02-27`
- mode: `execution`
- invariant: `Structural invariants only. No heuristics.`

### 1) Investigate the problem
- Objective: implement MIR return-value capture on the smallest listed functions first, starting with `node_text`/`field_text` path and then `symbol::line`/`symbol::render`.

### 2) Gather facts
- Baseline at slice start: `canon suppressed __ret count = 12` (repomap).
- `node_text` suppression came from dropped method-call chain (`utf8_text` -> `unwrap_or`) due guard rejection of synthetic locals.
- `Option::<T>::unwrap_or*` calls were incorrectly filtered as internal via unresolved-generic path filtering.

### 3) Break down the facts
- Pattern A: call-arg values labeled as `_vN` were treated unknown even when structurally defined.
- Pattern B: projected operands in call args needed structural expression labeling.
- Pattern C: opaque aggregate locals (closure/coroutine) can flow into emitted calls and must have a binding.

### 4) Write it to a state file
- State overwritten for this execution slice.

### 5) Sort structural and categorical patterns
- Structural guard invariant: synthetic value names are valid if and only if present in `defined`.
- Operand-label invariant: projected place operands in call args must be renderable structurally (deref/field/downcast/index).
- Binding invariant: every destination used later must have a syntactic binding; opaque aggregates emit suppressed bindings.

### 6) Write it to state file
- Files touched in this slice:
- `canon-capture/src/capture/mir/guard.rs`
- `canon-capture/src/capture/mir/ops.rs`
- `canon-capture/src/capture/mir/expr.rs`
- `canon-capture/src/capture/mir/lower.rs`
- `canon-capture/src/capture/mir/filters.rs`
- `STRUCTURAL_INVARIANTS_REPORT.md`
- `AGENT_STATE.md`

### 7) Solve the state file
- Implemented structural changes:
- synthetic `_vN` inputs now pass guard when already in `defined`.
- call-operand labeling supports projected places.
- deref projection typing hardened for refs/raw pointers.
- opaque aggregate destinations now emit suppressed bindings (not silent define-only).
- removed unresolved-generic call filtering from internal-call suppression.

### 8) Emit and project the solution incrementally
- Validation executed:
- `cargo check -p canon-capture -p orchestration`
- `./run_script.sh repomap`
- Current repomap structural surface:
- `canon suppressed binding count: 11`
- `canon suppressed __ret count: 11`
- `canon suppressed non-__ret count: 0`
- `canon match gap count: 0`
- `unreachable count: 0`
- `// match count: 0`
- `// goto count: 0`
- Confirmed reduction: `node_text` removed from suppressed `__ret` site list.

### 9) Repeat step 3
- Next structural targets:
- `field_text` and `fn_signature` return capture completion.
- then `symbol::line` and `symbol::render` (switch/downcast return path).
