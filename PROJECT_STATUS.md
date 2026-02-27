# PROJECT_STATUS.md

## CANONICAL_HEADER
- project: `canon`
- status_epoch: `2026-02-27`
- policy: `No heuristics. Structural invariants only.`
- active_plan: `Canon-Capture Compression Model`

## CURRENT_STATE
- Engine/rules path is active for metadata projection.
- MIR statement classification uses `project/mir_patterns.rs` dispatcher.
- MIR structural input gate uses `project/mir_engine.rs::structural_guard`.
- Suppressed-binding emission now uses shared primitive:
- `project/mir_engine.rs::emit_suppressed_binding`.
- `item.rs` legacy duplicates removed:
- guard helper duplicates removed,
- statement candidate helper duplicates removed.

## VALIDATION_STATE
- `cargo check -p canon-capture`: pass.
- `cargo check` workspace: pass.
- `repomap` fixture pipeline:
- capture: pass
- orchestration: pass
- emitted `cargo build`: pass

## COMPRESSION_PROGRESS
- Completed in this slice:
- MIR statement pattern dispatch integrated in active stmt loop.
- Structural guard ownership unified in `mir_engine`.
- Suppression emission deduplicated into shared helper.
- Pending for compression target:
- Call-terminator pattern table/dispatcher extraction.
- Operand/path labeling unification into a single MIR label API.
- Further shrink of `item.rs` to CFG walker + dispatcher calls only.

## NEXT_PENDING_PHASES
1. Extract call-terminator pattern table (`filtered` / `method` / `plain` / `fallback`) into `mir_patterns`.
2. Move MIR operand/path labeling utilities to shared API surface.
3. Remove remaining branch duplication in `item.rs` after parity validation on fixtures.
