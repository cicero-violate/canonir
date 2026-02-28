# AGENT_STATE

## Current Phase
Reasoner prompt fixed — awaiting first real on-disk file mutation

## What Was Built This Session
- `src/evolution/mod.rs`: `apply_admitted_deltas` fully implemented
  - looks up StateChange by admission_id in ir.deltas
  - calls `apply_structural_delta` per delta to mutate IR clone in memory
  - `payload_to_code_delta` (φ) maps every ChangePayload variant to CodeDelta
  - file-producing variants → `CodeDelta::ApplyPatch`
  - edges/events/rewards → `CodeDelta::Bash { "true" }` (no-op, cargo check still gates)
- `src/pipeline.rs`: IR delta injection before apply_admitted_deltas
  - Reasoner output `change_payload` field deserialized into `ChangePayload`
  - Wrapped into `StateChange` with deterministic `delta-tick-N` id
  - Pushed into ir_with_delta before Mutate stage runs
  - Judge admission_id resolved: matches delta_id if echoed, else falls back
  - No more unknown delta panics
- `src/llm_provider.rs`: Reasoner prompt schema fixed
  - Removed ambiguous uppercase example keys from change_payload block
  - Now emits one clean concrete add_module example
  - LLM will produce a parseable ChangePayload on every tick

## System Condition
- Full agent loop running and stable
- All 5 nodes fire per tick via ChatGPT/calpico WS on 8787
- Pipeline: Observe→Reason→Prove→Judge→Mutate completes without panic
- Reasoner prompt now instructs LLM to emit valid change_payload JSON
- Previous issue: payload deserialized as None due to ambiguous prompt schema
- Fix applied: single concrete add_module example, no extra noise keys

## Active Invariants
- No heuristic mutation
- Deterministic structural diff only — CodeDelta derived from ChangePayload fields
- Lyapunov bound enforced before mutation (pipeline.rs)
- Admission required for evolution
- cargo check gates every delta application
- git stash rollback on any failure

## Immediate Next Move
Run ./run.sh and verify:
1. ir.json delta has payload.type = "add_module"
2. A new src/<module_id>.rs file appears on disk
3. cargo check passes

## Constraint
Execution artifacts must originate from IR transition, not CLI surface.
CodeDelta must be derived from ChangePayload, not from LLM free-text.
