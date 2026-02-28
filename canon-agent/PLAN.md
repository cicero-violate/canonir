# PLAN

## Objective
Verify first real on-disk file mutation from Reasoner change_payload.

## Completed

- apply_admitted_deltas: ChangePayload → CodeDelta::ApplyPatch (all variants)
- pipeline.rs: Reasoner change_payload deserialized → StateChange appended to IR
- Judge admission_id fallback to deterministic delta_id
- executor: apply_patch + cargo check + git stash rollback
- llm_provider.rs: Reasoner prompt fixed — single clean add_module example

## Verification Steps

Run ./run.sh, then after first tick check:
```bash
python3 -c "
import json
ir = json.load(open('ir.json'))
for d in ir.get('deltas', []):
    print(d['id'], d.get('payload'))
"
```
Expected: payload.type = "add_module" with module_id, name, visibility, description.

Then check disk:
```bash
git status src/
```
Expected: new untracked file src/<module_id>.rs

## FileTopology Convention
π(module_id) = src/<module_id>.rs
Full FileTopology from repomap deferred — convention sufficient for current phase.

## Success Criteria
- Reasoner emits parseable change_payload on every tick
- At least one tick writes a new src/<module_id>.rs file
- cargo check passes after the patch
- IR written to disk with structural change recorded
- No manual patch content — all derived from ChangePayload fields
