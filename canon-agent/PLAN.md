# PLAN

## Objective
Make `apply_admitted_deltas` emit real `CodeDelta::ApplyPatch` values
so the agent loop actually mutates files on disk.

## The Gap

```rust
// src/evolution/mod.rs
pub fn apply_admitted_deltas(
    ir: &SystemState,
    _admission_ids: &[String],
) -> Result<(SystemState, Vec<CodeDelta>), EvolutionError> {
    let next = ir.clone();
    let code_deltas = Vec::new(); // ← stub
    Ok((next, code_deltas))
}
```

## Data Flow (what already exists)

```
ir.deltas: Vec<StateChange>
  └─ StateChange { id, kind, payload: ChangePayload, ... }
       └─ ChangePayload variants:
            AddModule, AddStruct, AddField, AddTrait, AddTraitFunction,
            AddImpl, AddFunction, AddModuleEdge, AddCallEdge,
            AttachExecutionEvent, UpdateFunctionAst, AddEnum, AddEnumVariant,
            UpdateFunctionInputs, UpdateFunctionOutputs, UpdateStructVisibility,
            RemoveField, RenameArtifact, RecordReward

apply_structural_delta(ir_mut, delta) → Result<(), EvolutionError>
  already implemented in src/evolution/structural/mod.rs
  calls apply_delta_payload which handles all ChangePayload variants

CodeDelta::ApplyPatch { patch: String }
  patch is apply_patch format:
    *** Begin Patch
    *** Update File: src/foo.rs
    @@
    -old line
    +new line
    *** End Patch
```

## Implementation Steps

### Step 1 — Read `src/evolution/structural/apply.rs`
Understand what `apply_delta_payload` does per ChangePayload variant.
This determines what IR state changes, which drives what patch to emit.

### Step 2 — Read `src/ir/delta.rs`
Understand the full `ChangePayload` enum and what fields each variant carries.
Each variant maps to a specific file mutation.

### Step 3 — Implement `apply_admitted_deltas`

```
for each admission_id:
    find matching StateChange in ir.deltas (by id or by admission cross-ref)
    clone IR → ir_mut
    apply_structural_delta(&mut ir_mut, &delta) → updates IR in memory
    diff(ir_before, ir_after) → CodeDelta::ApplyPatch
return (ir_mut, code_deltas)
```

### Step 4 — Implement the diff → patch emitter

Map each ChangePayload variant to an apply_patch string:

| ChangePayload      | File target                        | Patch content                  |
|--------------------|------------------------------------|--------------------------------|
| AddModule          | src/<module_id>.rs (new file)      | *** Add File: src/<id>.rs      |
| AddFunction        | src/<module_id>.rs                 | *** Update File: + fn body     |
| AddStruct          | src/<module_id>.rs                 | *** Update File: + struct def  |
| AddField           | src/<module_id>.rs                 | *** Update File: + field line  |
| AddTrait           | src/<module_id>.rs                 | *** Update File: + trait def   |
| RenameArtifact     | src/<module_id>.rs                 | *** Update File: -old +new     |
| RemoveField        | src/<module_id>.rs                 | *** Update File: -field line   |

### Step 5 — Wire FileTopology
FileTopology maps module_id → file path.
Currently `FileTopology` is a unit struct (stub).
For now: derive path as `src/<module_id>.rs` by convention.
Later: populate FileTopology from repomap for precise mapping.

### Step 6 — Validate
Each emitted CodeDelta goes through `execute_deltas`:
  apply_patch → cargo check → git stash rollback on failure
No manual patch injection. All patches derived from ChangePayload fields only.

## Files to Read Before Writing Code

1. `src/evolution/structural/apply.rs`   — apply_delta_payload implementation
2. `src/evolution/structural/mod.rs`     — apply_structural_delta entry point
3. `src/ir/delta.rs`                     — ChangePayload enum + StateChange fields
4. `src/evolution/mod.rs`               — stub to replace
5. `src/ir/types.rs`                    — CodeDelta enum definition

## Success Criteria

- At least one tick produces a non-empty `Vec<CodeDelta>`
- `apply_patch` runs against a real src/ file
- `cargo check` passes after the patch
- IR is written to disk with the structural change recorded
- No manual patch content — all derived from ChangePayload fields
