# PROJECT_STATUS

## Canon-Agent Orchestration

✔ Refactor pipeline wired (Observe→Reason→Prove→Judge→Mutate)
✔ Full agent loop running — ChatGPT responds via calpico conduit WS on 8787
✔ All 5 nodes fire per tick, pipeline completes with cargo check gate
✔ executor.rs: apply_patch + cargo check + git stash rollback
✔ runner.rs: workspace-aware, IR rolled back on executor failure
✔ sse.rs: calpico Shape 4 frame parsing fixed
✔ ir.json: real self-model IR — 44 modules, 65+ functions from repomap
✔ apply_admitted_deltas: full ChangePayload → CodeDelta::ApplyPatch emitter
✔ pipeline.rs: Reasoner change_payload → StateChange → ir.deltas before admit
✔ Judge admission_id fallback — no more unknown delta panics
✔ Shell emission module exists (emit_shell.rs)
✔ CLI: run-agent, run-pipeline, show-ledger, show-graph

## Remaining Gap

Reasoner LLM is not yet prompted to emit `change_payload` in its output JSON.
Without it the pipeline admits a no-op StateChange (payload: None) every tick —
IR mutates in memory, cargo check passes, but no files change on disk.

## What Needs to Be Done

Update the Reasoner node system prompt (calpico / graph.json) to require:
```json
{
  "rationale": "<why this change>",
  "change_payload": {
    "type": "<variant>",
    ...fields per ChangePayload variant...
  }
}
```
Valid types: add_module, add_struct, add_field, add_trait, add_trait_function,
add_impl, add_function, add_enum, add_enum_variant, update_struct_visibility,
remove_field, rename_artifact, add_module_edge, add_call_edge, record_reward

## Stability

Compiles. Loop runs. No panics. Deterministic.
Files mutate on disk only when Reasoner emits a valid change_payload.
