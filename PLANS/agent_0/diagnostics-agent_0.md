# Diagnostics Report — agent_0

## Inputs

- Spec: `PLANS/SPEC.md`
- Invariants: `PLANS/INVARIANTS.md`
- Violations: `VIOLATIONS.md`
- Canonical event log: `state/event_log/event.tlog.d`
- Core source files reviewed:
  - `canon-utils/canon-runtime/src/lib.rs`
  - `canon-utils/canon-runtime/src/invariants.rs`
  - `canon-utils/canon-runtime/src/bin/event_runtime.rs`
  - `canon-utils/canon-route/src/policy.rs`
  - `canon-utils/canon-route/src/executor.rs`
  - `canon-utils/canon-loop/src/executor.rs`
  - `canon-utils/canon-loop/src/stage/plan.rs`
  - `canon-utils/canon-mini-agent/src/main.rs`

## Event-log evidence

- `state/event_log/event.tlog.d`
  - `.log` count: `1962`
  - `.idx` count: `1962`
  - `.time` count: `1962`
  - segment gaps: `1366`
  - median log size: `6302.0`
  - max log size: `2255656`
- Recent keyword counts from canonical segments:
  - `parent_ids`: `907`
  - `invariant violation`: `70`
  - `capabilityfailed`: `8`
  - `capabilitycompleted`: `4`
- Recent non-rustc counts:
  - `capabilityfailed`: `6`
  - `capabilitycompleted`: `2`

Interpretation: the canonical log is structurally present and active, but recent printable evidence still contains invariant-failure signals. Recent log strings are noisy because rustc/capture traffic is mixed into the same surface, so source-backed control-flow evidence is required to isolate root cause.

## Verifier reconciliation

Latest verifier summary already says:

- verified: targeted `validate_before_append` ordering, EventBus wiring
- unverified: global `validate -> append -> dispatch`, end-to-end fail-fast, routing strictly from `SemanticStateSummary`
- false: consistent fail-fast emission, full invariant enforcement, guaranteed successful event processing per tick

This diagnostics pass confirms that direction.

## Ranked failures

### 1. Global lawful-admission ordering is still not proven across all live paths (CRITICAL)

**Root problem:** Canon still has multiple live dispatch surfaces and multiple append/persistence surfaces, but only limited evidence of `validate_before_append`. The control law from the spec — `state -> decision -> transition -> event log` — is therefore not globally proven across all event paths.

**Concrete evidence:**

- `canon-utils/canon-runtime/src/lib.rs`
  - `bus.dispatch(...)` site at line `460`
  - `validate_before_append(...)` site at line `532`
  - `append_runtime_event(...)` site at line `539`
  - `runtime_event_to_wire(...)` and `invariant_engine.observe(...)` remain in the append path
  - same file contains the writer/consumer divergence warning around the control-write drop case
- correlation totals:
  - `bus_dispatch`: `7`
  - `validate_before_append`: `3`
  - `append_runtime_event`: `7`
  - `runtime_event_to_wire`: `2`

**Diagnosis:** the targeted fix exists, but global coverage is not established. The system still appears vulnerable to paths where live control advancement and lawful persistence are not uniformly sequenced.

**Repair target:** `canon-utils/canon-runtime/src/lib.rs`

**Required repair:** make one global rule true everywhere: validate lawful admission first, durably admit next, dispatch live control state only after admission succeeds.

### 2. Fail-fast propagation remains inconsistent (CRITICAL)

**Root problem:** the runtime still contains at least one ignored `emit_tick` result, and prior scans found ignored dispatch results. This means critical event-processing failures can still be silently tolerated.

**Concrete evidence:**

- `canon-utils/canon-runtime/src/bin/event_runtime.rs`
  - line `264`: `runtime.emit_tick()?;`
  - line `684`: `let _ = runtime.emit_tick();`
- correlation totals from source scans:
  - `emit_tick`: `7`
  - `ignored_emit_tick`: present
  - `bus_dispatch`: `7`

**Diagnosis:** fail-fast semantics are not uniform. This matches `VIOLATIONS.md` and the verifier false-item on consistent fail-fast emission.

**Repair target:** `canon-utils/canon-runtime/src/bin/event_runtime.rs`, plus any critical runtime dispatch site still discarding result paths.

### 3. Invariant rejection semantics remain partial rather than globally hard-preventive (HIGH)

**Root problem:** invariant enforcement still emits an error and returns false, but that is not yet equivalent to a globally proven prevention of unlawful live-state advancement.

**Concrete evidence:**

- `canon-utils/canon-runtime/src/invariants.rs`
  - `pub fn observe(...)`
  - `ErrorOccurred`
  - multiple `return false;` sites
- `canon-utils/canon-runtime/src/lib.rs`
  - append-path `invariant_engine.observe(...)`
  - `LoopObserved` remains a special-case surface in runtime handling

**Diagnosis:** invariant failure still behaves like a localized rejection mechanism, not a single globally uniform control law.

**Repair targets:**
- `canon-utils/canon-runtime/src/invariants.rs`
- `canon-utils/canon-runtime/src/lib.rs`

### 4. Route policy shows semantic-only intent, but queue-counter terminology still exists across loop/control files (HIGH)

**Root problem:** the route layer appears improved, but global routing authority is still not proven exclusive to `SemanticStateSummary` because queue-state vocabulary remains present in adjacent control layers.

**Concrete evidence:**

- `canon-utils/canon-route/src/policy.rs`
  - semantic-only routing comments and `SemanticStateSummary` references are present
  - also contains duplicate/dispatch control rules
- source totals:
  - `semantic_state_summary`: `58` in correlation scan / `194` in broader synthesis
  - `planned_pending`: `10` in correlation scan / `85` in broader synthesis
  - `scheduler_len`: `9` in correlation scan / `80` in broader synthesis
  - `pending_act`: `1` in correlation scan / `20` in broader synthesis

**Diagnosis:** this is no longer a simple “route policy missing semantic state” bug. The more likely problem is mixed authority across route, loop, and runtime boundaries.

**Repair targets:**
- `canon-utils/canon-loop/src/executor.rs`
- `canon-utils/canon-loop/src/stage/plan.rs`
- `canon-utils/canon-runtime/src/lib.rs`

### 5. Observe-boundary and duplicate-control handling remain brittle (HIGH)

**Root problem:** `LoopObserved` and duplicate suppression remain heavily defended surfaces, which indicates the observe/control boundary is still fragile.

**Concrete evidence:**

- `canon-utils/canon-loop/src/executor.rs`
  - `LoopObserved` single-source-of-truth comments
  - panic paths on observe bypass / Deferred / Noop
  - duplicate `LoopObserved` tolerance downstream
- source totals:
  - `loop_observed`: `53` in correlation scan / `94` in broader synthesis
  - `duplicate`: `48` in correlation scan / `110` in broader synthesis
  - `fanout`: `2` in correlation scan / `20` in broader synthesis
  - `observe_noop`: observed in source scans

**Diagnosis:** the system is carrying substantial defensive logic at the observe boundary. Even if intended, this is still a hotspot for successor discharge and duplicate-control distortion.

**Repair targets:**
- `canon-utils/canon-loop/src/executor.rs`
- `canon-utils/canon-runtime/src/lib.rs`
- `canon-utils/canon-route/src/policy.rs`

### 6. Fallback seeding still exists on planning surfaces (MEDIUM)

**Concrete evidence:**

- source totals:
  - `fallback`: `24` in focused scan / `95` in broader synthesis
- especially relevant file:
  - `canon-utils/canon-loop/src/stage/plan.rs`

**Diagnosis:** not every fallback site is a live runtime bug because some belong to harness flows, but fallback still exists in real planning surfaces and cannot yet be ruled out as synthetic work seeding.

**Repair target:** `canon-utils/canon-loop/src/stage/plan.rs`

### 7. Mini-agent prompt/response contract remains a secondary mismatch surface (MEDIUM)

**Concrete evidence:**

- `canon-utils/canon-mini-agent/src/main.rs`
  - `json array` contract surfaces
  - `submit_ack`
  - `lane_submit_in_flight`
- source totals:
  - `json_array_contract`: present

**Diagnosis:** this is not the primary canonical control failure, but it remains a real executor-orchestration friction source that can masquerade as planning/execution breakage.

**Repair target:** `canon-utils/canon-mini-agent/src/main.rs`

## True root problem

The root problem is not one isolated symptom like `planned_pending` or one ignored `emit_tick` site.

The deeper issue is:

> Canon still lacks one globally enforced control law across runtime, loop, and route boundaries:
>
> semantic truth decides -> event is validated as lawful -> event is durably admitted -> only then may live control state advance

Current code contains parts of that law, but not one unified implementation that is clearly global.

## Highest-priority repair order

1. **Enforce global lawful admission before live control advancement**
   - target: `canon-utils/canon-runtime/src/lib.rs`
2. **Make fail-fast truly global**
   - target: `canon-utils/canon-runtime/src/bin/event_runtime.rs`
3. **Normalize invariant rejection semantics**
   - targets: `canon-utils/canon-runtime/src/invariants.rs`, `canon-utils/canon-runtime/src/lib.rs`
4. **Prove queue counters are non-authoritative**
   - targets: `canon-utils/canon-loop/src/executor.rs`, `canon-utils/canon-loop/src/stage/plan.rs`, runtime integration points
5. **Simplify observe boundary and duplicate suppression**
   - targets: loop/runtime/route observe surfaces
6. **Strip live fallback seeding and tighten mini-agent contract**
   - targets: `canon-utils/canon-loop/src/stage/plan.rs`, `canon-utils/canon-mini-agent/src/main.rs`

## Bottom line

Canon is still not globally compliant with its own event-sourced judgment contract.

The strongest confirmed diagnosis is:

> global lawful event admission before live control advancement is not yet proven across all live paths

The strongest confirmed secondary diagnosis is:

> fail-fast propagation is still inconsistent
