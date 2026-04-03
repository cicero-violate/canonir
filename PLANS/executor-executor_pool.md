# EXECUTOR PLAN (executor_pool)

## READY NOW (MAX 8)

1. **Patch `PLANS/SPEC.md` and `PLANS/INVARIANTS.md` for strict append-time rejection.**
   - Read `PLANS/SPEC.md` and `PLANS/INVARIANTS.md`.
   - Patch them so invariant violation at the append boundary rejects event persistence.
   - Re-read both files to confirm the rule is explicit and mandatory.

2. **Patch the real append boundary in `canon-utils/canon-runtime/src/lib.rs`.**
   - Read `handle_runtime_event_located_with_parents(...)` and `append_runtime_event(...)`.
   - Patch `append_runtime_event(...)` so invariant-engine rejection is authoritative at write-time.
   - Rebuild and verify invalid events no longer enter the log.

3. **Remove the `LoopObserved` append bypass unless it is explicitly lawful.**
   - Read the `LoopObserved` override in `append_runtime_event(...)`.
   - Patch it out unless SPEC + INVARIANTS are updated to allow it explicitly.
   - Rebuild and verify `LoopObserved` no longer bypasses append-time rejection by default.

4. **Patch dispatch-time handling so invalid append state does not propagate.**
   - Read `canon-utils/canon-runtime/src/lib.rs` and `canon-utils/canon-runtime/src/bus.rs`.
   - Patch dispatch handling so invalid events are not treated as lawful progress when append-time enforcement rejects them.
   - Rebuild and verify invalid state is blocked rather than merely logged.

5. **Add append-boundary tests.**
   - Read `canon-utils/canon-invariant/src/control_harness.rs` and `canon-utils/canon-invariant/src/request_lifecycle_harness.rs`.
   - Add tests for append-time rejection, no silent `LoopObserved` bypass, and no invalid-state propagation after rejection.
   - Run the relevant test targets.

6. **Inspect fresh canonical artifacts with Python.**
   - Read only the newest files under `state/event_log/event.tlog.d`.
   - Verify current artifacts show enforcement at append-time, not merely warnings or probes.
   - Do not advance if invalid events still appear to persist.

7. **Re-check semantic-state-only routing after append enforcement is live.**
   - Read `canon-utils/canon-route/src/policy.rs` and `canon-utils/canon-route/src/executor.rs`.
   - Patch route choice so queue-local mirrors cannot alter outcome unless proven semantic-state-derived.
   - Rebuild and run route-policy tests or targeted runtime checks.

8. **Only then restore broader runtime/control-flow progression.**
   - Read/patch runtime and route entry surfaces as needed.
   - Verify fresh artifacts show lawful decision, route, observe, and downstream progression under enforced append-time invariants.

## BLOCKED / NOT READY YET
- Any broader pipeline repair before append-time invariant rejection is mandatory.
- Any queue-derived routing behavior not proven from `SemanticStateSummary`.
- Any reliance on observational logging as a substitute for enforcement.
