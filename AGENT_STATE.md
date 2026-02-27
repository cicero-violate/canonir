# AGENT_STATE.md

## CANONICAL_HEADER
- state_id: `CAPTURE_REFACTOR_MODEL_EXECUTION_SLICE_37`
- date: `2026-02-27`
- mode: `execution`
- invariant: `No heuristics. Structural invariants only.`

### 1) Investigate the problem
- Pass primitives were still embedded in `lower.rs`, keeping pipeline semantics mixed with orchestration wiring.

### 2) Gather facts
- The following pass concerns were local to `lower.rs`:
- emitted block role model
- special-block emission for switch regions
- block normalization passes
- emitted-structure predicates (`has_ret_match` / `has_ret_binding`)

### 3) Break down the facts
- Move pass primitives into a dedicated module so `lower.rs` is primarily stage orchestration + statement lowering.
- Keep behavior frozen; only responsibility relocation.

### 4) Write it to a state file
- State overwritten for this slice.

### 5) Sort structural and categorical patterns
- Pattern A: pass-module extraction (`capture/mir/passes.rs`).
- Pattern B: pipeline wiring in `lower.rs` uses pass API only.
- Pattern C: preserve deterministic transform ordering.

### 6) Write it to state file
- Files changed:
- `canon-capture/src/capture/mir/passes.rs` (new)
- `canon-capture/src/capture/mir/lower.rs`
- `canon-capture/src/capture/mir/mod.rs`
- `STRUCTURAL_INVARIANTS_REPORT.md`
- `AGENT_STATE.md`

### 7) Solve the state file
- Added `capture/mir/passes.rs` with:
- `BlockRole`
- `EmittedBlock`
- `emit_special_block`
- `normalize_blocks`
- `make_normal_block`
- `blocks_have_ret_match`
- `blocks_have_ret_binding`
- Rewired `lower.rs` to use `mir_passes::...` for role/special/normalize/predicate operations.
- Removed local pass primitive ownership from `lower.rs`.

### 8) Emit and project the solution incrementally
- Validation:
- `cargo check -p canon-capture`: pass.
- `./run_script.sh repomap`: pass.
- `STRUCTURAL_INVARIANTS_REPORT.md` regenerated.
- LOC snapshot:
- `capture/mir/lower.rs`: 373 LOC
- `capture/mir/passes.rs`: 108 LOC

### 9) Repeat step 3
- Next structural slice (behavior frozen):
- formalize remaining lowering flow into explicit pass data transitions by introducing a small `BodyDraft` stage type (analysis output + emitted stream), then keep `mir_body_structural` as composition:
  `B0(raw) -> P1(plan) -> P2(emit_draft) -> P3(normalize) -> Bn(final)`.
