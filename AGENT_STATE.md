# AGENT_STATE.md

## CANONICAL_HEADER
- state_id: `CAPTURE_REFACTOR_MODEL_EXECUTION_SLICE_30`
- date: `2026-02-27`
- mode: `execution`
- invariant: `No heuristics. Structural invariants only.`

### 1) Investigate the problem
- `capture/mir/lower.rs` still embedded switch/suppression graph construction and reachability analysis.

### 2) Gather facts
- The following pre-analysis logic remained in `lower.rs`:
- successor/pred graph construction
- switch-source discovery
- switch-reachable closure
- switch-arm block fixpoint classification
- switch-source return-write propagation

### 3) Break down the facts
- This is a distinct analysis phase and should be isolated from CFG emission orchestration.
- Extracting it reduces coupling and makes `mir_body_structural` primarily sequencing logic.

### 4) Write it to a state file
- State overwritten for this slice.

### 5) Sort structural and categorical patterns
- Pattern A: pre-analysis extraction (`capture/mir/analysis.rs`).
- Pattern B: orchestration simplification (`lower.rs` consumes analysis result object).
- Pattern C: clear phase boundary (analyze first, then emit).

### 6) Write it to state file
- Files changed:
- `canon-capture/src/capture/mir/analysis.rs` (new)
- `canon-capture/src/capture/mir/lower.rs`
- `canon-capture/src/capture/mir/mod.rs`
- `STRUCTURAL_INVARIANTS_REPORT.md`
- `AGENT_STATE.md`

### 7) Solve the state file
- Added `capture/mir/analysis.rs`:
- `SwitchAnalysis` struct
- `analyze_switch_structure(body)`
- Moved complete switch/suppression pre-analysis logic out of `lower.rs`.
- Rewired `lower.rs` to consume `switch_analysis.*` fields.
- Exported module in `capture/mir/mod.rs`.

### 8) Emit and project the solution incrementally
- Validation:
- `cargo check -p canon-capture`: pass.
- `./run_script.sh repomap`: pass.
- `STRUCTURAL_INVARIANTS_REPORT.md` regenerated.
- LOC snapshot:
- `capture/mir/lower.rs`: 393 LOC (down from 465 in previous slice).

### 9) Repeat step 3
- Next structural slice:
- extract filtered-call feeder-local computation and suppressed-destination pre-scan into dedicated analysis helpers so `lower.rs` is reduced further to block-walk orchestration plus dispatch calls.
