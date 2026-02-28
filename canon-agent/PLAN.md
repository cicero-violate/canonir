# PLAN

## Objective
Wire Reasoner LLM output → real on-disk file mutation via change_payload.

## Completed

- apply_admitted_deltas: ChangePayload → CodeDelta::ApplyPatch (all variants)
- pipeline.rs: Reasoner change_payload deserialized → StateChange appended to IR
- Judge admission_id fallback to deterministic delta_id
- executor: apply_patch + cargo check + git stash rollback

## Active Gap

Reasoner emits `{ "rationale": "..." }` only.
It must also emit `change_payload` to produce a real CodeDelta.

## Next Step — Prompt the Reasoner

The Reasoner node system prompt must instruct the LLM to output:
```json
{
  "rationale": "<reasoning>",
  "change_payload": {
    "type": "add_module",
    "module_id": "new_module_id",
    "name": "new_module_name",
    "visibility": "public",
    "description": "what this module does"
  }
}
```

### ChangePayload variant → required fields

| type                      | required fields                                              |
|---------------------------|--------------------------------------------------------------|
| add_module                | module_id, name, visibility, description                     |
| add_struct                | module, struct_id, name                                      |
| add_field                 | struct_id, field { name, ty }                                |
| add_trait                 | module, trait_id, name                                       |
| add_trait_function        | trait_id, function { id, name }                              |
| add_impl                  | module, impl_id, struct_id, trait_id                         |
| add_function              | function_id, impl_id, signature { name, visibility, ... }   |
| add_enum                  | module, enum_id, name, visibility                            |
| add_enum_variant          | enum_id, variant { name }                                    |
| update_struct_visibility  | struct_id, visibility                                        |
| remove_field              | struct_id, field_name                                        |
| rename_artifact           | kind, old_id, new_id                                         |
| add_module_edge           | from, to, rationale                                          |
| add_call_edge             | caller, callee                                               |
| record_reward             | record { id, ... }                                           |

## FileTopology Convention
π(module_id) = src/<module_id>.rs
Full FileTopology from repomap deferred — convention sufficient for current phase.

## Success Criteria
- Reasoner emits change_payload on every tick
- At least one tick writes a new or modified src/ file
- cargo check passes after the patch
- IR written to disk with structural change recorded
- No manual patch content — all derived from ChangePayload fields
