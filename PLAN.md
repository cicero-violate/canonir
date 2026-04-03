# PLAN: Make Invariants Enforce Correctness at the Append Boundary, Then Restore Semantic-State-Only Control

## A. Authoritative Context

### Current State
- `SemanticStateSummary` is the single source of truth for routing and control-flow correctness.
- `scheduler_len`, `planned_pending`, and similar counters are not root truth for routing.
- The latest verifier says EventBus wiring is fixed, but invariants are still not enforced at write-time, so the system remains observational rather than correctness-enforcing.

### Canonical Evidence
- `PLANS/SPEC.md` says lawful control must pass through judgment and invariants before entering the event log.
- `PLANS/INVARIANTS.md` defines append-time-relevant constraints such as append-only log, monotonic time, causal integrity, uniqueness, successor obligations, no illegal transitions, payload validity, schema consistency, no hidden state, and idempotent consumption.
- `VIOLATIONS.md` says:
  - invariant checks are not rejecting invalid events at the append boundary,
  - enforcement is missing at the write path,
  - dispatch is not yet guarded by invariant validity,
  - semantic-state-only routing is still not guaranteed by enforcement.

### Source Evidence
- `canon-utils/canon-runtime/src/lib.rs`
  - `emit_event_located(...)`, `emit_event(...)`, and `emit_event_with_parents(...)` all flow into `handle_runtime_event_located_with_parents(...)` and then `append_runtime_event(...)`.
  - live dispatch currently happens before append.
  - replay dispatch already has a zero-consumer guard.
  - `append_runtime_event(...)` calls `runtime_event_to_wire(...)`.
  - `append_runtime_event(...)` already calls `self.invariant_engine.observe(&wire, &self.emitter)`.
  - invariant rejection is still bypassed for `RuntimeEvent::LoopObserved`, which means write-time rejection is not yet universal.
- `canon-utils/canon-invariant/src/lib.rs`
  - exposes `invariant_violation_delta(...)` and `invariant_violation_state()` primitives that can support hard enforcement.
- `canon-utils/canon-route/src/policy.rs`
  - route-policy branches are intended to be semantic-only, but diagnostics still require enforcement proving queue-local mirrors cannot change route choice without semantic-state proof.

### Planning Rule
- Prioritize the real append boundary over observational logging.
- Remove all write-time bypasses that allow invariant violations to persist.
- Make append/write rejection authoritative before further downstream restoration.
- Keep semantic-state authority ahead of any queue-derived routing behavior.

## B. Ranked Root Failures

### 0. APPEND-TIME INVARIANT REJECTION IS INCOMPLETE (PRIMARY BLOCKER)
Evidence:
- verifier says write-time enforcement is missing.
- `append_runtime_event(...)` already invokes the invariant engine.
- invariant rejection is explicitly bypassed for `RuntimeEvent::LoopObserved`.

Required outcome:
- every invariant violation at the append boundary must reject persistence unless the exception is explicitly redefined in SPEC + INVARIANTS and justified as lawful.

### 1. DISPATCH STILL OCCURS BEFORE AUTHORITATIVE APPEND VALIDATION
Evidence:
- `handle_runtime_event_located_with_parents(...)` dispatches first, then reaches append.
- current runtime therefore still allows pre-append propagation before append-time correctness is fully enforced.

Required outcome:
- the runtime must not treat dispatch-before-append as a substitute for correctness enforcement.
- invalid events must not be allowed to persist, and dispatch must be guarded by invariant validity.

### 2. WRITE-TIME ENFORCEMENT IS STILL PARTLY OBSERVATIONAL
Evidence:
- append path logs and probes exist.
- invariant engine rejects, but a special-case override remains.

Required outcome:
- append-time invariant evaluation must become mandatory rejection logic, not warning-plus-exception behavior.

### 3. SEMANTIC-STATE AUTHORITY IS STILL NOT PROTECTED BY ENFORCEMENT
Evidence:
- verifier still says semantic-state enforcement is not proven.
- diagnostics still document historical queue-local routing pressure.

Required outcome:
- route choice must be protected by invariant-backed enforcement so queue-local mirrors cannot become primary truth.

## C. Dependency Order
1. Bind SPEC and INVARIANTS to mandatory append-time rejection.
2. Remove write-boundary bypasses in `append_runtime_event(...)`.
3. Enforce invalid-event rejection before persistence.
4. Guard dispatch with invariant-valid state.
5. Re-test fresh canonical artifacts for current enforcement evidence.
6. Re-test semantic-state-only routing under enforced invariants.
7. Only then continue broader runtime/control-flow restoration.

## D. READY NOW

### Executor: executor_pool
1. **Patch `PLANS/SPEC.md` and `PLANS/INVARIANTS.md` for strict append-time rejection.**
   - Read `PLANS/SPEC.md` and `PLANS/INVARIANTS.md` together.
   - Patch `PLANS/SPEC.md` so it states explicitly that invariant violation at the write/append boundary rejects event persistence.
   - Patch `PLANS/INVARIANTS.md` so the rejection rule is explicit and universal unless a narrowly defined lawful exception is documented.
   - Re-read both files and verify the append-time rejection rule is explicit.

2. **Read and patch the true append boundary in `canon-utils/canon-runtime/src/lib.rs`.**
   - Read `handle_runtime_event_located_with_parents(...)` and `append_runtime_event(...)` in `canon-utils/canon-runtime/src/lib.rs`.
   - Patch the append path so `self.invariant_engine.observe(&wire, &self.emitter)` is authoritative.
   - Remove or justify any write-boundary bypass that allows invalid events to persist.
   - Rebuild and verify append-time invariant failures now reject persistence.

3. **Eliminate the `LoopObserved` write-time bypass unless it is explicitly lawful.**
   - Read the `RuntimeEvent::LoopObserved` special case inside `append_runtime_event(...)`.
   - Patch it so invariant rejection is no longer bypassed by default.
   - Only preserve a special case if it is documented in `PLANS/SPEC.md` and `PLANS/INVARIANTS.md` as a lawful exception.
   - Rebuild and verify the append path no longer silently persists invariant-rejected `LoopObserved` events.

4. **Guard dispatch with invariant-valid state.**
   - Read `canon-utils/canon-runtime/src/lib.rs` and `canon-utils/canon-runtime/src/bus.rs`.
   - Patch the runtime so dispatch cannot be treated as valid progress when the event fails append-time invariant enforcement.
   - Keep zero-consumer and invalid-state guarding explicit.
   - Rebuild and verify invalid state no longer propagates as if it were lawful progress.

5. **Add append-boundary tests.**
   - Read `canon-utils/canon-invariant/src/control_harness.rs`, `canon-utils/canon-invariant/src/request_lifecycle_harness.rs`, and any runtime tests covering append behavior.
   - Add tests that fail when:
     - invariant-rejected events still persist,
     - `LoopObserved` bypasses rejection without a lawful exception,
     - dispatch proceeds as if append-time invalid state were acceptable.
   - Run the relevant test targets after patching.

6. **Inspect fresh canonical evidence with Python after enforcement patches.**
   - Read only the newest files under `state/event_log/event.tlog.d`.
   - Verify current artifacts show append-time enforcement behavior, not just observational logs.
   - Do not advance if the newest evidence still suggests invalid events can persist.

7. **Re-check semantic-state-only routing after append enforcement is live.**
   - Read `canon-utils/canon-route/src/policy.rs` and `canon-utils/canon-route/src/executor.rs`.
   - Patch route choice so queue-local mirrors cannot affect route outcome unless proven derived from semantic state.
   - Rebuild and run route-policy tests or targeted runtime checks.

8. **Only then restore broader runtime/control-flow progression.**
   - Read/patch `canon-utils/canon-runtime/src/lib.rs`, `canon-utils/canon-runtime/src/bin/event_runtime.rs`, `canon-utils/canon-route/src/policy.rs`, and `canon-utils/canon-route/src/executor.rs` as needed.
   - Ensure fresh artifacts show lawful decision, route, observe, and downstream stage progression under enforced append-time invariants.

## E. BLOCKED / NOT READY YET
- Any downstream control-flow restoration that leaves append-time invariant rejection partial or bypassable.
- Any routing behavior justified only by queue-derived heuristics instead of `SemanticStateSummary`.
- Any reliance on probes, warnings, or ad-hoc logs as a substitute for actual append-time rejection.
