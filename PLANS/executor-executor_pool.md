# EXECUTOR PLAN (executor_pool)

## READY NOW (MAX 10)

1. **Audit and patch all live event paths in `canon-utils/canon-runtime/src/lib.rs` for global ordering.**
   - Read every `bus.dispatch`, `append_runtime_event`, `validate_before_append`, `handle_runtime_event_located_with_parents`, replay handling, and any other emission path.
   - Patch the file so every live path obeys `validate -> append -> dispatch` or a clearly lawful atomic equivalent.
   - Remove any path where control/FSM state can advance before lawful write admission is known.
   - Rebuild and verify no live path can dispatch before validation and lawful write admission.

2. **Patch fail-fast semantics in the live runtime loop in `canon-utils/canon-runtime/src/bin/event_runtime.rs`.**
   - Read the main loop around the remaining `let _ = runtime.emit_tick();` and adjacent ignored flush behavior.
   - Replace ignored critical `Result` handling with fail-fast `?` propagation.
   - Rebuild and verify the live loop no longer ignores tick or flush failure.

3. **Audit all critical runtime progression calls for ignored `Result`s.**
   - Read `canon-utils/canon-runtime/src/bin/event_runtime.rs`, `canon-utils/canon-runtime/src/lib.rs`, and any related runtime helpers.
   - Search for ignored `Result` patterns on emission, flush, drain, dispatch, and related critical progression operations.
   - Patch all critical paths so failures terminate the runtime instead of being dropped.
   - Rebuild and verify no ignored critical path remains.

4. **Patch downstream invariant gating across dispatch, route, and loop layers.**
   - Read `canon-utils/canon-runtime/src/bus.rs`, `canon-utils/canon-runtime/src/invariants.rs`, `canon-utils/canon-route/src/executor.rs`, `canon-utils/canon-loop/src/executor.rs`, and `canon-utils/canon-loop/src/stage/plan.rs`.
   - Patch these surfaces so downstream consumers only execute on events that have already passed lawful validation and admission.
   - Add guards preventing invalid or rejected events from advancing route or loop state.
   - Rebuild and verify invalid state cannot propagate downstream.

5. **Patch invariant rejection semantics to be uniform and non-bypassable.**
   - Read `canon-utils/canon-runtime/src/invariants.rs` and the invariant-handling branches in `canon-utils/canon-runtime/src/lib.rs`.
   - Remove special-case persistence or execution behavior unless `PLANS/SPEC.md` and `PLANS/INVARIANTS.md` explicitly define it as lawful.
   - Re-evaluate the `LoopObserved` override under the same rule.
   - Rebuild and verify invariant rejection semantics are uniform.

6. **Add a per-tick successful control progression guarantee.**
   - Read the live loop in `canon-utils/canon-runtime/src/bin/event_runtime.rs` plus append and drain surfaces in `canon-utils/canon-runtime/src/lib.rs`.
   - Patch the runtime so each tick either produces at least one successfully appended and consumed control event or fails immediately.
   - Make the guarantee observable in canonical artifacts, runtime state, or tests.
   - Rebuild and verify silent zero-progress ticks are no longer possible.

7. **Patch `PLANS/SPEC.md` and `PLANS/INVARIANTS.md` to make global ordering and fail-fast law explicit.**
   - Read both files together.
   - Patch them so lawful runtime behavior explicitly requires global `validate -> append -> dispatch` ordering or a clearly defined lawful atomic equivalent.
   - Patch them so critical emission, append, dispatch, and per-tick control progression failures must terminate the runtime.
   - Re-read both files to confirm the law is explicit and mandatory.

8. **Add runtime ordering, fail-fast, and downstream-gating tests.**
   - Read runtime tests and invariant harnesses such as `canon-utils/canon-invariant/src/control_harness.rs` and `canon-utils/canon-invariant/src/request_lifecycle_harness.rs`.
   - Add tests that fail when:
     - any live path dispatches before validation and lawful append
     - critical `Result`s are ignored
     - downstream consumers execute on invalid state
     - a tick does not yield successfully appended and consumed control progress
     - invariant rejection is bypassed without lawful documentation
   - Run the relevant test targets.

9. **Inspect fresh canonical artifacts with Python after enforcement patches.**
   - Read only the newest files under `state/event_log/event.tlog.d`.
   - Verify current artifacts show lawful control progress, fail-fast behavior, and reduced invariant-violation noise.
   - Do not advance if fresh artifacts still indicate silent fail-open behavior or unlawful progression.

10. **Only then prove strict `SemanticStateSummary` routing authority and restore broader control flow.**
   - Read `canon-utils/canon-route/src/policy.rs`, `canon-utils/canon-route/src/executor.rs`, `canon-utils/canon-loop/src/executor.rs`, and any runtime surfaces that still carry queue-counter terminology.
   - Patch route choice so queue-local mirrors cannot alter outcome unless proven semantic-state-derived.
   - Rebuild and run route-policy tests or targeted runtime checks.

## BLOCKED / NOT READY YET
- Any broader pipeline restoration before global runtime correctness is proven.
- Any routing work that assumes downstream invariant gating while invalid-state propagation is still unproven.
- Any reliance on targeted-path success while global correctness is still unverified.
