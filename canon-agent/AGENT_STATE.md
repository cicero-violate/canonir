# AGENT_STATE

## Current Phase
Refactor Pipeline → Structural Mutation → CodeDelta Emission

## System Condition
- Pipeline compiles
- Evolution returns (SystemState, Vec<CodeDelta>)
- CodeDelta currently empty
- Shell projection wired
- Execution authority structurally prepared

## Active Invariants
- No heuristic mutation
- Deterministic structural diff only
- Lyapunov bound enforced before mutation
- Admission required for evolution

## Immediate Next Move
Implement IR₀ → IR₁ structural diff → CodeDelta

## Constraint
Execution artifacts must originate from IR transition, not CLI surface.
# AGENT_STATE

## Current Phase
Implement `apply_admitted_deltas` — IR₀ → IR₁ structural diff → CodeDelta emission

## What Was Built This Session
- `src/executor.rs`: executes `Vec<CodeDelta>` — ApplyPatch via `apply_patch` stdin,
  Bash via `sh -c`, gates on `cargo check`, rolls back via `git stash pop` on failure
- `src/runner.rs`: `RunnerConfig` gains `workspace: PathBuf`, executor called in
  `Ok(result)` arm after IR promotion, IR rolled back in memory on executor failure
- `src/main.rs`: `run-agent <ir.json> <layout.json> <graph.json> <workspace>` subcommand
  fully wired to `run_agent()` loop, WsBridge on `127.0.0.1:8787`
- `src/sse.rs`: fixed Shape 4 frame handler — array-valued `{"v": [...]}` bare frames
  from calpico conduit were being dropped; now extracts content correctly
- `ir.json`: real self-model IR generated from repomap — 44 modules, 65+ functions
- `layout.json`: `null` (FileTopology is unit struct)
- `graph.json`: full 5-node Observer→Reasoner→Prover→Judge→Mutator topology
- `scripts/gen_ir.py`: generates ir.json from repomap symbol list

## System Condition
- Full agent loop is running and stable
- All 5 nodes fire per tick, ChatGPT responds via calpico conduit WS on 8787
- Pipeline completes: Observe→Reason→Prove→Judge→Mutate → `cargo check` passes
- `execute_deltas()` is called but receives empty `Vec<CodeDelta>` every tick
- No files are being changed on disk — loop is structurally valid but inert

## The Stub
`src/evolution/mod.rs` — `apply_admitted_deltas()` is a TODO stub:
```rust
pub fn apply_admitted_deltas(
    ir: &SystemState,
    _admission_ids: &[String],
) -> Result<(SystemState, Vec<CodeDelta>), EvolutionError> {
    let next = ir.clone();
    let code_deltas = Vec::new();  // ← THIS IS THE PROBLEM
    Ok((next, code_deltas))
}
```
The admission_ids passed in come from the Judge's payload field `admission_id`.
The IR has `deltas: Vec<StateChange>` — each StateChange has a `ChangePayload`
enum variant (AddModule, AddFunction, AddStruct, etc.) that describes a structural op.
The diff engine must: look up admitted deltas by id → apply each ChangePayload to
IR clone → diff IR₀ vs IR₁ → emit CodeDelta (ApplyPatch or Bash) per change.

## Immediate Next Move
Implement `apply_admitted_deltas` in `src/evolution/mod.rs`:
1. Look up `StateChange` entries in `ir.deltas` by admission_id match
2. Call `structural::apply_structural_delta(ir_mut, delta)` for each
3. Diff IR₀ vs IR₁ to produce `Vec<CodeDelta>`
4. Each structural change → one `CodeDelta::ApplyPatch` targeting the correct src/ file

## Key Files to Read First
- `src/evolution/mod.rs` — stub to implement (shown above, 44 lines)
- `src/evolution/structural/apply.rs` — `apply_delta_payload` — already handles ChangePayload variants
- `src/evolution/structural/mod.rs` — `apply_structural_delta` entry point
- `src/ir/delta.rs` — `StateChange`, `ChangePayload` enum — all mutation variants
- `src/ir/types.rs` — `CodeDelta` enum: `ApplyPatch { patch }` and `Bash { command }`
- `src/emit_shell.rs` — how CodeDelta → shell string (for reference)

## Active Invariants
- No heuristic mutation
- Deterministic structural diff only
- Lyapunov bound enforced before mutation (in pipeline.rs, already working)
- Admission required for evolution
- cargo check gates every delta application
- git stash rollback on any failure

## Constraint
Execution artifacts must originate from IR transition, not CLI surface.
CodeDelta must be derived from ChangePayload, not from LLM free-text.
