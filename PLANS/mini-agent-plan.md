# Plan: Cross-Product Invariant Discovery Suite

## Problem Summary

The current invariant infrastructure is split across two disconnected layers:

- **Structural layer** (`canon-tools-analysis/src/invariants/`): mines `CodeGraph` edges for node-ownership rules (CONTAINS, CALL_SITE, EXPORT). Promotes candidates only when `support >= 0.99 && violation_rate <= 0.01`. No demotion. No convergence test. No cross-product of ControlState×ControlEvent.

- **Control layer** (`canon-invariant/src/control_harness.rs`): exhaustively seeds `ControlState` and walks traces at fixed depth but never feeds discovered patterns back into any promotion/demotion lifecycle. The two layers do not talk.

**What's missing (gap map):**

| Layer                         | Current                                                              | Missing                                                                                |
|-------------------------------+----------------------------------------------------------------------+----------------------------------------------------------------------------------------|
| T_S — seed classification     | `synthetic_control_seed_states()` enumerates all valid seeds         | Cross-dimension seeds: ConstraintState × ConstraintRoute seeds not enumerated          |
| T_SE — transition closure     | `synthetic_control_trace_metrics(depth)` walks tree, discards result | Full reachability table: (S, E) → S' not materialized; no coverage matrix              |
| T_C — constraint precedence   | None                                                                 | meta > discovered > deterministic priority ordering; conflict resolution rules         |
| T_I — invariant lifecycle     | `mine_candidates()` promote-only (no demote)                         | State machine: candidate → promoted → demoted → hard-banned; negative evidence path    |
| T_P — persistence round-trip  | History file written by `update_history()`, never read back          | Serde round-trip verification; idempotency under re-run                                |
| T_R — projection bisimilarity | None                                                                 | Control trace ≅ Constraint trace when projected onto shared events; bisimulation check |

---

## Mathematical Basis

Harness H = (S, E, D, I, P) where:

- **S** = seed set; cross-product of all boolean fields of `ControlState` ∪ `ConstraintState`, filtered by structural feasibility constraints
- **E** = event alphabet; `ControlEvent` (12 variants) ∪ `ConstraintRoute` (5) ∪ `ConstraintAction` (6)
- **D** = decision set; `ControlDecision` (5 variants) ∪ `ConstraintDecision` (to be defined)
- **I** = invariant candidate store; keyed by `(src_kind, edge_kind, dst_kind)` or `(state_bits, decision)` fingerprint
- **P** = persistence store; content-addressed by invariant fingerprint hash

Failure fingerprint **F** = set of (state, event, observed_decision) triples where observed ≠ expected.

Support threshold **θ** = configured per invariant tier (meta: 1.0, discovered: 0.98, deterministic: computed).

---

## Existing Infrastructure to Build On

### canon-invariant/src/control_harness.rs
- `ControlState` — 8 boolean fields, `Default`
- `ControlEvent` — 12 variants
- `step_control_state(state, event) -> ControlState` — pure transition function
- `evaluate_control_state(state) -> ControlDecision` — pure evaluator
- `synthetic_control_seed_states() -> Vec<ControlState>` — 512 seeds, filters infeasible
- `synthetic_control_trace_metrics(depth)` — tree walk, classifies terminal decisions

### canon-invariant/src/lib.rs
- `ConstraintState` — boolean fields (semantic_path_exists, actionable_failure, etc.)
- `ConstraintRoute` — Observe, Plan, Act, Verify, Conclude
- `ConstraintAction` — CargoInit, RepairLocalized, etc.

### canon-tools-analysis/src/invariants/
- `InvariantRule` trait — `name()`, `description()`, `evaluate(&graph, &features) -> InvariantResult`
- `discover_invariants()` — applies rules, returns `Vec<InvariantResult>`
- `mine_candidates()` — filters by coverage/violation thresholds
- `generate_candidates(patterns)` — filters `PatternRule` by support+confidence
- `run_invariant_pipeline()` — end-to-end orchestration

---

## Files to Change / Create

### 1. `canon-invariant/src/control_harness.rs`
**What:** Add `reachability_table()` — materializes the full (S×E → S') transition closure.

**How:**
- Return `HashMap<(ControlState, ControlEvent), ControlState>`
- Iterate `synthetic_control_seed_states()` × `synthetic_control_events()`
- For each (s, e): call `step_control_state(s, e)`, insert result
- Also return `HashMap<(ControlState, ControlEvent), ControlDecision>` for the post-step decision

This gives T_SE: the complete coverage matrix. Every reachable state is now enumerable.

---

### 2. `canon-invariant/src/constraint_harness.rs` (new file)
**What:** Mirror of `control_harness.rs` for `ConstraintState × ConstraintRoute`.

**How:**
- `ConstraintSeed` = `(ConstraintState, ConstraintRoute)` — all combinations, feasibility-filtered
- `step_constraint_state(state: ConstraintState, route: ConstraintRoute, action: ConstraintAction) -> ConstraintState`
  - Mirrors `evaluate_control_decision()` logic from `lib.rs` — extracts the implicit state machine into an explicit stepper
- `evaluate_constraint_state(state: ConstraintState, route: ConstraintRoute) -> ConstraintDecision`
  - New enum: `ConstraintDecision { Allow, Block(reason), Repair, Escalate }`
- `constraint_seed_states() -> Vec<ConstraintSeed>` — cross-product, filter invalid combos
- `constraint_reachability_table() -> HashMap<(ConstraintSeed, ConstraintAction), ConstraintSeed>`

This fills T_S for the constraint dimension and gives T_SE for constraint traces.

---

### 3. `canon-invariant/src/cross_product_harness.rs` (new file)
**What:** T_SE combined — joint transition closure over (ControlState, ConstraintState) × (ControlEvent ∪ ConstraintAction).

**How:**

```
JointState = (ControlState, ConstraintState)
JointEvent = ControlEvent(ControlEvent) | ConstraintEvent(ConstraintAction)

step_joint(js: JointState, je: JointEvent) -> JointState:
  match je:
    ControlEvent(e) => (step_control_state(js.0, e), js.1)
    ConstraintEvent(a) => (js.0, step_constraint_state(js.1, derive_route(js.0), a))

where derive_route(cs: ControlState) -> ConstraintRoute maps control decisions to routes:
  EmitRoute => Plan, ReplayCachedRoute => Observe, RequestFreshRoute => Plan,
  Suppress => Observe, InvariantViolation => Observe (safe fallback)
```

- `joint_seed_states() -> Vec<JointState>` — cross-product of control seeds × constraint seeds
- `joint_reachability_table()` — full closure; bounded by BFS to max depth (e.g., 6)
- `joint_projection(table, project_fn)` — collapses joint traces to single dimension for bisimulation

---

### 4. `canon-tools-analysis/src/invariants/invariant_lifecycle.rs` (new file)
**What:** T_I — invariant state machine with promotion and demotion.

**How:**

```
InvariantStatus: Candidate | Promoted | Demoted | HardBanned
InvariantEntry {
    fingerprint: u64,          // hash of (predicate, scope)
    description: String,
    status: InvariantStatus,
    support_samples: usize,
    violation_samples: usize,
    last_updated_epoch: u64,
}

Transitions:
  Candidate → Promoted:    support_samples / total >= θ_promote (0.98) AND violations == 0
  Promoted → Demoted:      any new violation observed (violation_samples >= 1)
  Demoted → Candidate:     support_samples reset after demotion cooldown (min_age = 5 cycles)
  Demoted → HardBanned:    violation_samples >= θ_hard_ban (6)
  HardBanned: terminal, never re-promoted
```

Methods:
- `record_support(fingerprint)` — increment support_samples
- `record_violation(fingerprint, context)` — increment violation_samples, trigger demotion
- `tick(epoch)` — apply transition rules, return list of status changes
- `promoted_invariants() -> Vec<&InvariantEntry>` — currently active constraints

This is the missing demote path. `mine_candidates()` currently has no demotion — it only promotes. `InvariantLifecycle` replaces the bare threshold filter.

---

### 5. `canon-tools-analysis/src/invariants/constraint_precedence.rs` (new file)
**What:** T_C — constraint priority ordering: meta > discovered > deterministic.

**How:**

Define three tiers:

```
ConstraintTier: Meta | Discovered | Deterministic

Meta:        kernel invariants from kernel_invariants.rs (structural: no orphan nodes, etc.)
             θ = 1.0, never demoted, hardcoded
Discovered:  from InvariantLifecycle (mine_candidates output that reached Promoted)
             θ = 0.98
Deterministic: from canon-invariant evaluate_control_state / evaluate_constraint_state
             θ = computed from reachability table coverage

PrecedenceMatrix: for any two constraints C1 (tier T1) and C2 (tier T2)
  if T1 > T2: C1 wins on conflict
  if T1 == T2: use support_samples as tiebreak
  if C1 and C2 conflict (C1 allows, C2 blocks same action): emit ConflictRecord
```

- `ConflictRecord { c1: InvariantFingerprint, c2: InvariantFingerprint, action: String, resolution: String }`
- `resolve_conflict(c1, c2, action) -> Resolution` — returns winning constraint + rationale
- Write conflict log to `state/invariants/conflicts.jsonl`

---

### 6. `canon-tools-analysis/src/invariants/persistence.rs` (new file)
**What:** T_P — persistence round-trip and idempotency verification.

**How:**
- `InvariantStore` wraps `HashMap<u64, InvariantEntry>` (keyed by fingerprint)
- `load(path: &Path) -> InvariantStore` — deserialize from `state/invariants/store.json`
- `save(path: &Path)` — serialize, atomic write (write to `.tmp`, rename)
- `round_trip_check(store: &InvariantStore) -> bool` — serialize to string, deserialize, compare; assert equality
- `idempotency_check(store: &InvariantStore) -> bool` — run `tick()` twice with same epoch; assert no state changes on second tick

Called from `run_invariant_pipeline()` after each cycle to verify persistence integrity.

---

### 7. `canon-tools-analysis/src/invariants/bisimulation.rs` (new file)
**What:** T_R — projection bisimilarity between control traces and constraint traces.

**How:**

Two LTS (Labelled Transition Systems):
- LTS_C: control layer, states = ControlState, labels = ControlEvent ∪ ControlDecision
- LTS_K: constraint layer, states = ConstraintState, labels = ConstraintRoute ∪ ConstraintDecision

Shared alphabet Σ = {route_selected, plan_completed, observe_completed} — the events both layers observe.

Bisimulation relation R ⊆ LTS_C × LTS_K:
- (s_C, s_K) ∈ R iff for all a ∈ Σ:
  - if s_C →a s_C' then ∃ s_K' such that s_K →a s_K' and (s_C', s_K') ∈ R
  - symmetric

**Implementation:**
- `project_control_trace(trace: &[(ControlState, ControlEvent)]) -> Vec<SharedEvent>`
  - maps ControlEvent to SharedEvent or filters (skips non-shared events)
- `project_constraint_trace(trace: &[(ConstraintSeed, ConstraintAction)]) -> Vec<SharedEvent>`
- `bisim_check(c_traces, k_traces) -> BisimResult`
  - For each shared-projected trace pair, check they produce the same decision sequence
  - If divergence found, emit `BisimViolation { control_state, constraint_state, shared_event, c_decision, k_decision }`
- `BisimResult { ok: bool, violations: Vec<BisimViolation> }`

Write results to `state/invariants/bisim_report.json`.

---

### 8. `canon-tools-analysis/src/invariants/invariant_validator.rs` — extend `run_invariant_pipeline()`
**What:** Wire all new modules into the existing pipeline.

**How:** Extend the pipeline in order:

```
1. load_code_graph()
2. T_S: joint_seed_states() — validate count and feasibility filter
3. T_SE: joint_reachability_table() — materialize, write coverage to state/invariants/coverage.json
4. run_kernel_invariants() [existing]
5. discover_invariants() [existing]
6. T_C: resolve_conflicts() — pass discovered + kernel invariants through precedence matrix
7. T_I: lifecycle.tick(epoch) — promote/demote based on current graph evidence
8. T_P: store.save() + round_trip_check() + idempotency_check()
9. T_R: bisim_check() — assert control and constraint traces agree on shared alphabet
10. write_report() [existing, extend with lifecycle + bisim sections]
```

---

## Persistence Layout

```
state/invariants/
  store.json             ← InvariantStore (all entries with status)
  coverage.json          ← T_SE reachability coverage matrix summary
  conflicts.jsonl        ← ConflictRecord log (append-only)
  bisim_report.json      ← BisimResult from most recent run
  history.jsonl          ← existing InvariantHistoryEntry (unchanged)
  violations.json        ← existing (unchanged)
  report.json            ← existing (extend with lifecycle + bisim)
```

---

## Threshold Configuration

Add to `capability_config.toml` under `[system]`:

```toml
invariant_promote_threshold = 0.98
invariant_hard_ban_threshold = 6
invariant_min_age_cycles = 5
bisim_shared_events = ["route_selected", "planning_completed", "observe_completed"]
```

---

## Verification Scenarios

### Scenario 1: T_S completeness
- `joint_seed_states().len()` > 0
- No seed violates feasibility constraints
- All seeds are distinct (no duplicates)

### Scenario 2: T_SE full coverage
- coverage.json shows all (ControlState, ControlEvent) pairs visited
- No (state, event) pair has a missing successor

### Scenario 3: T_C conflict resolution
- Insert two conflicting candidates (meta says "must have CONTAINS", discovered says "CONTAINS optional")
- Verify meta wins, ConflictRecord written

### Scenario 4: T_I demotion
- Promote a candidate by supplying sufficient support_samples
- Inject one violation via `record_violation()`
- Assert status = Demoted after `tick()`
- Assert status = HardBanned after 6 violations

### Scenario 5: T_P round-trip
- `round_trip_check()` returns true for any non-empty store
- `idempotency_check()` confirms no state mutations on second tick with same epoch

### Scenario 6: T_R bisimulation holds on normal path
- Walk control trace: `PendingRequestStarted → PromptDispatched → PromptCleared → RouteSelectedEmitted`
- Walk constraint trace: equivalent path through ConstraintState
- Assert projected traces agree on all shared events

### Scenario 7: T_R bisimulation detects divergence
- Inject a control trace that emits `route_selected(plan)` while constraint layer says `Block`
- Assert `BisimViolation` is recorded with correct states + shared_event

---

## What This Does NOT Fix

- The root cause of LLM relay timeouts (addressed in `mini-agent-plan.md`)
- The `consecutive_llm_plan_failures` counter wiring (addressed in `mini-agent-plan.md`)
- `event_repair_trigger` connection refused on 9102 (separate repair server concern)
- Semantic clustering or embedding quality (in `canon-tools-analysis/src/semantics/`)

---

## Acceptance Criteria

| Criterion                                      | Test                                                            |
|------------------------------------------------+-----------------------------------------------------------------|
| T_S: all joint seeds enumerated and classified | `joint_seed_states().len() == expected_count`                   |
| T_SE: full reachability table built            | `joint_reachability_table()` no missing entries                 |
| T_C: meta always wins conflicts                | conflict resolution scenario passes                             |
| T_I: demotion path fires on negative evidence  | lifecycle scenario 4 passes                                     |
| T_P: round-trip + idempotency                  | persistence scenarios 5 pass                                    |
| T_R: bisim check passes on normal path         | bisim scenario 6 passes                                         |
| T_R: bisim check detects divergence            | bisim scenario 7 records violation                              |
| Pipeline runs end-to-end without panic         | `run_invariant_pipeline()` completes on real graph              |
| No new clippy warnings                         | `cargo clippy -p canon-invariant -p canon-tools-analysis` clean |
