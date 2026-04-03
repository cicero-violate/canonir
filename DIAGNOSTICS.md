# Diagnostics Report

## Inputs Scanned
- event log segments reviewed:
  - freshest canonical segments under `state/event_log/event.tlog.d`: 00000000000000010269.log, 00000000000000010291.log, 00000000000000010292.log, 00000000000000010298.log, 00000000000000010300.log, 00000000000000010301.log, 00000000000000010302.log, 00000000000000010304.log
  - repeated recent-window scans over canonical `.log` segments
- violations reviewed:
  - `VIOLATIONS.md`
- spec and invariants reviewed:
  - `PLANS/SPEC.md`
  - `PLANS/INVARIANTS.md`
- source areas reviewed:
  - `canon-utils/canon-runtime/src/lib.rs`
  - `canon-utils/canon-runtime/src/invariants.rs`
  - `canon-utils/canon-runtime/src/bus.rs`
  - `canon-utils/canon-runtime/src/bin/event_runtime.rs`
  - `canon-utils/canon-route/src/policy.rs`
  - `canon-utils/canon-route/src/executor.rs`
  - `canon-utils/canon-loop/src/executor.rs`
  - `canon-utils/canon-loop/src/stage/plan.rs`
  - `canon-utils/canon-mini-agent/src/main.rs`
- commands run:
  - structured Python scans over canonical event-log segments
  - structured Python scans over `PLANS/SPEC.md`, `PLANS/INVARIANTS.md`, and `VIOLATIONS.md`
  - structured Python scans over runtime, route, loop, and mini-agent source files
  - source grep over runtime enforcement sites

## Ranked Failures

1. Impact: high
   Signal: invariant handling remains observational relative to live control flow because rejection happens in the append path after dispatch has already advanced runtime state.
   Evidence:
   - The freshest canonical log window still shows zero `route_selected`, `loop_observed`, `planning_completed`, `loop_acted`, `loop_verified`, `verifier_policy_updated`, and `loop_rewarded` events, with only rustc traffic visible.
   - `canon-utils/canon-runtime/src/lib.rs:701-708` shows append-guard rejection logs an error and returns from the append path.
   - `canon-utils/canon-runtime/src/lib.rs:713-720` shows `if !self.invariant_engine.observe(&wire, &self.emitter)` then most events return immediately, but `LoopObserved` is explicitly overridden and allowed to persist.
   - `canon-utils/canon-runtime/src/lib.rs:737-741` documents that `bus.dispatch` runs before append and warns that dropping a control event after dispatch causes writer/consumer divergence.
   - `canon-utils/canon-runtime/src/invariants.rs:124-136` shows enforced invariant failure emits `ErrorOccurred` and returns `false`, but does not itself reject or roll back dispatch state.
   - The verifier summary says correctness remains observational rather than enforced at write-time.
   Repair Targets:
   - `canon-utils/canon-runtime/src/lib.rs`
     - move invariant gating to occur before any live control dispatch mutates consumer/FSM state
     - treat failed invariant checks as hard rejections before state advancement, not post-dispatch observations
     - remove special-case persistence overrides that permit rejected control-semantic events to survive unless explicitly justified by SPEC/INVARIANTS
   - `canon-utils/canon-runtime/src/invariants.rs`
     - separate discovery/telemetry from hard enforcement semantics
     - make enforcement return a rejection artifact that the caller must honor before dispatch/write
   - runtime lifecycle
     - add explicit tests proving rejected control events never advance bus/consumer state or writer pending-FSM state

2. Impact: high
   Signal: SPEC and INVARIANTS are not yet bound by a root invariant that defines compliance and enforces EventBus identity and lawful transition ordering at runtime.
   Evidence:
   - `PLANS/SPEC.md` says Canon derives control from semantic truth, evaluates it through judgment and invariants, and records the lawful transition in the event log (`state -> decision -> transition -> event log`).
   - `PLANS/INVARIANTS.md` currently defines generic append-only/FSM/replay properties but does not establish a root invariant binding SPEC compliance to runtime enforcement.
   - `VIOLATIONS.md` explicitly states that invariants exist as documentation rather than binding system constraints and calls out “No root invariant enforcing EventBus identity (CRITICAL)”.
   - `VIOLATIONS.md` requires runtime checks at write-time and dispatch-time and proposes `WriterBus == DispatcherBus` style enforcement.
   Repair Targets:
   - `PLANS/SPEC.md`
     - add a normative section stating that invariant satisfaction is the runtime definition of compliance
     - make unlawful transitions and invariant failures explicitly non-admissible
   - `PLANS/INVARIANTS.md`
     - add a root invariant binding semantic truth, lawful transition, bus identity, and runtime enforcement into one compliance condition
     - add an invariant requiring dispatcher bus identity, non-zero canonical control consumer presence, and pre-dispatch validation
   - `canon-utils/canon-runtime/src/lib.rs`
     - enforce the root invariant at runtime write-time and dispatch-time rather than only surfacing observational errors

3. Impact: high
   Signal: route and control correctness still depend on queue-local mirrors such as `planned_pending`, `scheduler_len`, and pending queue emptiness in code that should derive from `SemanticStateSummary`.
   Evidence:
   - Canonical law says `SemanticStateSummary` is the single source of truth for routing and control-flow correctness.
   - Repeated source scans flagged `canon-utils/canon-route/src/policy.rs` and `canon-utils/canon-route/src/executor.rs` for `planned_pending`, queue-related gating, and semantic-summary co-existence.
   - Route-policy comments claim semantic-only routing and removal of queue-local dependence, yet the same regions still contain queue-local control branches.
   Repair Targets:
   - `canon-utils/canon-route/src/policy.rs`
     - remove queue-local routing authority from route proposal, event proposal, and dispatch decision paths
     - require every nontrivial route choice to justify itself from `SemanticStateSummary` or explicitly proven derived mirrors
   - `canon-utils/canon-route/src/executor.rs`
     - stop using queue emptiness and pending counters as primary route triggers unless semantic derivation is explicit and tested
   - `PLANS/INVARIANTS.md`
     - add an invariant forbidding queue-local counters from changing route choice unless they are proven mirrors of semantic state
   - tests/runtime checks
     - add assertions that route choice cannot change solely because `planned_pending`, `scheduler_len`, or pending-queue emptiness changed while semantic state remained equivalent

4. Impact: medium
   Signal: canonical observability is still dominated by rustc capture traffic, obscuring whether runtime lifecycle, dispatch, and stage enforcement are alive.
   Evidence:
   - Repeated recent-window scans show only `rustc` actors and kinds such as `code`, `rustc_capture_started`, `rustc_graph_artifact_written`, `rustc_capture_completed`, and `rustc_capture_failed`.
   - The freshest canonical segments still contain zero visible control-stage events and zero EventBus register/dispatch traces.
   Repair Targets:
   - `canon-utils/canon-runtime/src/lib.rs`
     - emit explicit runtime lifecycle, validation, dispatch, route, and loop heartbeat events into canonical logs
   - observability pipeline
     - separate or clearly label rustc capture noise versus canonical control-flow signals so enforcement failures are observable

5. Impact: medium
   Signal: repeated rustc capture failures generate invariant-violation text, but these appear secondary to missing contract enforcement and missing visible canonical control-flow execution.
   Evidence:
   - Recent canonical segments contain repeated rustc payloads with `invariant violation` text inside capture failures.
   - These violations appear while the canonical control pipeline remains absent from the log.
   Repair Targets:
   - `canon-rustc` capture path
     - investigate recurring assembler/capture failures after runtime contract enforcement and canonical control-flow visibility are restored
   - diagnostics prioritization
     - treat rustc capture invariant spam as secondary until runtime EventBus and semantic control-flow are visibly live again

## Planner Handoff
- ordered list of the highest-value repair targets
  1. Move invariant enforcement in `canon-utils/canon-runtime/src/lib.rs` to pre-dispatch/pre-state-advance so rejected events cannot mutate live runtime state.
  2. Add a root invariant binding SPEC to INVARIANTS and make invariant satisfaction the runtime definition of compliance.
  3. Add a root invariant enforcing EventBus identity, non-zero canonical control-consumer presence, and pre-dispatch validation.
  4. Remove queue-local routing authority from `canon-route/src/policy.rs` and `canon-route/src/executor.rs`; route decisions must derive from `SemanticStateSummary` unless mirrors are explicitly proven.
  5. Add runtime/tests that reject route changes or control progression driven only by `planned_pending`, `scheduler_len`, or queue emptiness without semantic-state change.
  6. Restore canonical runtime observability so lifecycle, dispatch, route, and loop stages remain visible even when rustc capture traffic is heavy.
  7. After runtime/control recovery, investigate repeated rustc capture invariant failures as secondary issues.
- blockers or missing evidence
  - the freshest canonical logs still contain no runtime-start or EventBus trace evidence, so exact live lifecycle behavior must still be inferred partly from source and verifier evidence
  - queue-local drift is clearly indicated by source scans, but final cleanup still requires line-by-line derivation review after the runtime pipeline becomes visible again
