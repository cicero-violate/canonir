# AGENT_STATE.md

## CANONICAL_HEADER
- state_id: `CANON_STRUCTURAL_SATURATION_RUN_SCRIPT_V1`
- date: `2026-02-27`
- mode: `execution`
- invariant: `No heuristics. Structural invariants only.`

### 1) Investigate the problem
- Structural violations were not fully visible because `run_script.sh` only extracted invariant facts from failed steps.
- Successful `orchestration` runs still contained solver-warning signatures indicating structural leakage candidates.

### 2) Gather facts
- `repomap` and `test_1` pipeline builds pass.
- `conversation` orchestration initially reported unresolved use paths with private helper segment `_serde`.
- `build_emit` failures for `conversation`, `semantic-lint`, and `canon` are offline dependency/network constraints, not capture/projection structure faults.

### 3) Break down the facts
- Gap A: invariant extraction coverage (tooling boundary).
- Gap B: capture-side use-path boundary accepted macro-private helper segments (e.g. `_serde`).
- Non-structural blocker: offline crate resolution in fixture builds.

### 4) Write it to a state file
- State overwritten for current slice.

### 5) Sort structural and categorical patterns
- Pattern A: successful steps can still emit structural warning signals.
- Pattern B: private helper use segments (prefix `_`) should not cross capture boundary.
- Pattern C: offline dependency failures are environment blockers, separate from structural invariants.

### 6) Write it to state file
- Files changed:
- `run_script.sh`
- `canon-capture/src/project/engine.rs`
- `STRUCTURAL_INVARIANTS_REPORT.md`
- `AGENT_STATE.md`

### 7) Solve the state file
- `run_script.sh` updated:
- Added extract mode (`always` / `on_fail`) for `run_step`.
- Set `orchestration` and `diff` to `always` extract invariants.
- Added extraction of solver warnings and liveness prune facts.
- Expanded invariant candidate detector to catch private helper segments (`::_...`) in paths.
- `engine.rs` updated:
- Added structural guard `use_path_has_private_helper_segment`.
- Suppresses `use` emission when any path segment starts with `_`.

### 8) Emit and project the solution incrementally
- Validation:
- `cargo check -p canon-capture`: pass.
- `./run_script.sh conversation` executed and report regenerated.
- Result: unresolved `_serde` use-solver warnings removed from report.
- Remaining report entries are impl/provenance/liveness warnings plus offline dependency blockers.

### 9) Repeat step 3
- Next structural slice:
- classify remaining orchestration warnings into capture-invariant vs solver-semantic categories,
- keep capture boundary strict,
- defer solver modifications until structural report contains no capture/projection-origin violations.
