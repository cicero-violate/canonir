# PLAN: Restore Canonical Event-Sourced Control by Proving the Live Runtime Binary First

## A. Authoritative Context

### Current State
- `SemanticStateSummary` remains the single source of truth for routing and control-flow correctness.
- Current canonical evidence shows the live runtime pipeline is inactive even though the reviewed source constructs a non-empty consumer set.
- Source evidence:
  - `canon-utils/canon-runtime/src/bus.rs` shows `register()` and `dispatch()` both operate on the same `EventBus.sync_consumers` vector.
  - `canon-utils/canon-runtime/src/lib.rs` shows `EventRuntime::new(consumers)` creates a fresh `EventBus`, registers every supplied consumer, and stores that same bus into `EventRuntime`.
  - `canon-utils/canon-runtime/src/bin/event_runtime.rs` constructs a non-empty consumer vector including `GoalGenConsumer`, `AnalystConsumer`, `RepairControlConsumer`, `RouteExecutor`, `DispatchConsumer`, `LoopStageExecutor`, `DiagnosticsConsumer`, `CapabilityExecutor`, and others before calling `EventRuntime::new(consumers)`.
- Canonical runtime evidence:
  - freshest canonical event-log segments contain only rustc events
  - zero `route_selected`, `loop_observed`, `planning_completed`, `loop_acted`, `loop_verified`, `verifier_policy_updated`, and `loop_rewarded`
  - zero `BUS REGISTER TRACE`, zero `BUS DISPATCH TRACE`, and zero `[RUNTIME NEW]` signals in the freshest canonical segments
  - `VIOLATIONS.md` reports `sync_consumers_len = 0` during dispatch
  - `runtime_debug.log` exists from an older run, but its mtime is stale relative to the freshest canonical event-log segments and therefore is not canonical evidence for the current runtime

### Canonical Control Law
semantic state -> judgment/decision -> lawful transition -> event log

### Planning Rule
- Prove the actual launched binary and live crate path before assuming the reviewed source is the executing runtime.
- Move startup evidence into the canonical event log instead of depending on stderr or ad-hoc debug files.
- Add fail-fast checks so a runtime with missing registration or zero-consumer dispatch cannot continue silently.
- Preserve `SemanticStateSummary` as routing authority and keep scheduler-derived counters behind this work.

## B. Ranked Root Failures

### 0. PROVE THE LAUNCHED `canon-runtime` BINARY MATCHES THE REVIEWED SOURCE (PRIMARY BLOCKER)
Evidence:
- source shows consumer construction plus immediate registration in `EventRuntime::new`
- live canonical evidence shows zero runtime startup traces and zero canonical control-flow events
- live violation evidence reports `sync_consumers_len = 0` during dispatch

Imperative repair:
1. Prove which executable path is actually launched in the failing runtime session.
2. Prove Cargo workspace resolution points to the intended `canon-runtime` crate and one canonical `EventBus` implementation.
3. Remove stale or mismatched build artifacts so the launched binary cannot come from an older or duplicate path.
4. Require a clean rebuild path when binary identity or crate linkage is ambiguous.
5. Record binary identity and crate identity in canonical runtime startup evidence.

Exit criteria:
- the launched binary path is proven
- the launched binary is proven to come from the reviewed `canon-runtime` crate
- one canonical `EventBus` implementation is proven live

### 1. MOVE STARTUP AND REGISTRATION EVIDENCE INTO THE CANONICAL EVENT LOG
Evidence:
- freshest canonical event-log segments show zero startup traces
- `runtime_debug.log` is stale and cannot serve as current canonical proof

Imperative repair:
1. Emit canonical startup evidence into the event log, not only stderr or ad-hoc files.
2. Record runtime bootstrap start, consumer registration count, and bus identity through canonical events or canonical debug events.
3. Make registration success observable from the same canonical log used for diagnostics.
4. Stop relying on stale external debug files as proof of current runtime behavior.

Exit criteria:
- canonical event log contains current runtime startup evidence
- consumer registration evidence is visible in canonical logs

### 2. FAIL FAST IF REGISTRATION OR DISPATCH STATE IS INVALID
Evidence:
- `VIOLATIONS.md` reports zero consumers at dispatch
- current runtime continues far enough to dispatch despite a dead control path

Imperative repair:
1. Add a runtime invariant that registration traces or equivalent canonical startup evidence must appear before normal dispatch begins.
2. Add a runtime invariant that consumer count must be greater than zero before dispatch.
3. Halt runtime immediately when registration evidence is missing or dispatch sees zero consumers.
4. Make these failures canonical and visible in the event log.

Exit criteria:
- runtime refuses to continue on zero-consumer dispatch
- missing registration evidence becomes a canonical hard failure

### 3. PROVE REGISTRATION AND DISPATCH USE THE SAME LIVE EVENTBUS INSTANCE
Evidence:
- `EventRuntime::new` source registers onto a local `EventBus`
- live dispatch evidence still sees `sync_consumers_len = 0`

Imperative repair:
1. Trace the `EventBus` instance created in `EventRuntime::new` through later runtime dispatch.
2. Remove any path that swaps, reconstructs, shadows, or dispatches on a different bus instance.
3. Prove the bus that receives registration is the same bus that later dispatches runtime events.

Exit criteria:
- registration and dispatch operate on one proven live bus instance
- live dispatch reports non-zero consumer count

### 4. RESTORE RUNTIME PARTICIPATION IN THE CANONICAL EVENT SYSTEM
Evidence:
- only rustc actor appears in canonical logs
- no `runtime_started` or canonical tick evidence in the freshest segments

Imperative repair:
1. Emit `runtime_started` exactly once per process.
2. Make runtime actor identity visible in canonical logs.
3. Ensure recurring tick emission reaches persistence through the live runtime path.
4. Add fail-fast if runtime starts and only rustc remains visible after bootstrap.

Exit criteria:
- runtime actor visible in canonical logs
- `runtime_started` present once
- recurring tick events present

### 5. RESTORE SEMANTIC-STATE DECISION ENTRY
Evidence:
- no decision events
- no semantic-state-driven transitions

Imperative repair:
1. Construct `SemanticStateSummary` at startup.
2. Recompute semantic state on each tick.
3. Emit one canonical decision every tick before downstream execution.
4. Add fail-fast if a tick completes without a decision event.
5. Keep `scheduler_len`, `planned_pending`, and similar counters out of control truth.

Exit criteria:
- decision events present
- each decision is traceable to semantic-state evaluation
- no tick completes without decision output

### 6. RESTORE DECISION -> ROUTE -> LOOP ONLY AFTER 0-5 HOLD
Evidence:
- no route events
- no observe or `LoopObserved`
- no downstream canonical stages

Imperative repair:
1. Restore lawful `RouteSelected` emission from semantic-state-driven decision output.
2. Ensure routing derives from `SemanticStateSummary`, policy, and invariants only.
3. Restore observe entry and `LoopObserved` only after registration, runtime events, and decisions exist.
4. Keep plan/act/verify/reward work blocked until these preconditions hold.

Exit criteria:
- route events present
- observe and `LoopObserved` present
- downstream canonical stages begin to execute lawfully

## C. Dependency Order
1. Prove launched binary identity and crate linkage
2. Move startup and registration evidence into the canonical event log
3. Fail fast on missing registration evidence or zero-consumer dispatch
4. Prove registration and dispatch share one live EventBus instance
5. Restore runtime actor participation and tick persistence
6. Restore semantic-state decision entry
7. Restore decision -> route -> loop
8. Only then repair downstream plan/act/verify and residual queue-truth cleanup

## D. READY NOW

### Executor: executor_pool
1. Prove the actual launched `canon-runtime` binary path, crate resolution, and linked `EventBus` implementation for the failing runtime session.
2. Add canonical startup logging so runtime bootstrap, consumer registration count, and live bus identity are visible in the event log rather than only stderr or stale debug files.
3. Add fail-fast invariants so the runtime halts when registration evidence is missing or dispatch sees zero consumers.
4. Trace and fix any path where registration and dispatch operate on different or stale `EventBus` instances.
5. After live non-zero consumer registration is proven, restore runtime actor visibility, `runtime_started`, recurring tick persistence, and then semantic-state decision emission.

## E. BLOCKED / NOT READY YET
- route repair before non-zero live consumer registration is proven
- loop entry repair before route events exist
- downstream plan/act/verify/reward work before observe exists
- local queue-symptom patches that preserve scheduler-first routing
