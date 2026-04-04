# EXECUTOR PLAN (executor_pool)

## READY NOW (MAX 10 — STRICT ORDER)

1. FIX WRITER INITIALIZATION (ROOT PART A)
    - Read `canon-utils/canon-runtime/src/lib.rs`.
    - Locate runtime startup path.
    - Ensure `tlog_path` is set BEFORE runtime loop starts.
    - Ensure `tlog_writer` is constructed BEFORE any emission.
    - Verify emitter is bound to initialized writer.
    - Remove INIT GUARD silent drop and replace with hard failure.
    - In `append_runtime_event`: assert writer exists.

2. FIX PARENT_IDS PROPAGATION (ROOT PART B)
    - Run `rg -n "emit_located|emit_with_parents" canon-utils`.
    - Replace all `emit_with_parents(..., vec![])` with non-empty parent_ids.
    - Fix known violations in:
        - `canon-loop/src/context.rs`
        - `canon-runtime/src/bin/event_runtime.rs`
    - Ensure parent_id propagates through all emission paths.
    - Add assertion: non-root events must have parent_ids.

3. VERIFY EVENTS PERSIST TO TLOG
    - Run runtime.
    - Inspect latest tlog segment.
    - Confirm control events exist.
    - If zero: trace emitter → EventBus → wire → writer.

4. TRACE FINAL DROP LAYER IF NEEDED
    - Instrument boundaries:
        - emitter.emit
        - EventBus dispatch
        - runtime_event_to_wire
        - append_runtime_event
    - Add logs for each stage.
    - Remove ALL silent drop paths.

## BLOCKED (START ONLY AFTER ROUTETICK + ROUTESELECTED CONFIRMED)

8. ENFORCE PER-CYCLE RUNTIME GUARANTEES
    - Read `canon-utils/canon-runtime/src/lib.rs` and `canon-utils/canon-loop/src/executor.rs`.
    - Add cycle accounting for `Tick -> RouteTick -> Decision -> RouteSelected`.
    - Fail on zero or multiple decisions in a cycle.
    - Test:
      - `cargo test -p canon-runtime`
      - `cargo test -p canon-loop`

9. ENFORCE EVENTBUS DELIVERY AND HOOK IMMUTABILITY
    - Read `canon-utils/canon-runtime/src/bus.rs` and `canon-utils/canon-runtime/src/hooks.rs`.
    - Promote delivery gaps/lock failures to invariant failures for required control events.
    - Reject/block hook mutation or suppression of protected control events.
    - Test:
      - `cargo test -p canon-runtime`

10. PROVE FRESH RUNTIME PERSISTENCE
    - Audit active append path and segmented tlog freshness.
    - Add smoke verification that live cycles write fresh routing/control events.
    - Test:
      - `cargo test -p canon-runtime`

## LATER

11. CLOSE DETERMINISM, ASYNC, REPLAY, AND QUEUE-LOCAL FOLLOW-ON WORK
    - Add deterministic replay checks.
    - Add async propagation tracing.
    - Remove queue-local `Noop` control gates in `stage/plan.rs` / `stage/act.rs`.
    - Test:
      - `cargo test -p canon-runtime`
      - `cargo test -p canon-loop`

## NOTES

- RouteTick persistence is the primary root; do NOT proceed to downstream fixes until RouteTick is visible in logs.
- Passing tests are NOT proof — only emitted RouteSelected in runtime logs counts.
- Always validate the full pipeline, not isolated components.
