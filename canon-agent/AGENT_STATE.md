# AGENT_STATE

## Current Phase
Pipeline operational — Reasoner change_payload → StateChange → CodeDelta emission

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

## System Condition
- Full agent loop running and stable
- All 5 nodes fire per tick via ChatGPT/calpico WS on 8787
- Pipeline: Observe→Reason→Prove→Judge→Mutate completes without panic
- `apply_admitted_deltas` emits non-empty Vec<CodeDelta> when Reasoner emits change_payload
- `execute_deltas` gates on cargo check, rolls back via git stash on failure
- IR written to disk after each successful tick

## Active Invariants
- No heuristic mutation
- Deterministic structural diff only — CodeDelta derived from ChangePayload fields
- Lyapunov bound enforced before mutation (pipeline.rs)
- Admission required for evolution
- cargo check gates every delta application
- git stash rollback on any failure

## Immediate Next Move
Update Reasoner LLM prompt (via calpico/graph.json system prompt) to emit:
```json
{
  "rationale": "...",
  "change_payload": {
    "type": "add_module",
    "module_id": "...",
    "name": "...",
    "visibility": "public",
    "description": "..."
  }
}
```

## Constraint
Execution artifacts must originate from IR transition, not CLI surface.
CodeDelta must be derived from ChangePayload, not from LLM free-text.
