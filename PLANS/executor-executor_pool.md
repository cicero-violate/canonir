# EXECUTOR PLAN (executor_pool)

## READY NOW (MAX 5)

1. PROVE THE LAUNCHED `canon-runtime` BINARY MATCHES THE REVIEWED SOURCE
   - trace the actual launched binary path for the failing runtime session
   - prove Cargo workspace resolution points to the intended `canon-runtime` crate
   - prove the launched binary contains the reviewed `EventBus` implementation
   - eliminate stale or duplicate build artifacts when binary identity is ambiguous

2. MOVE STARTUP AND REGISTRATION EVIDENCE INTO THE CANONICAL EVENT LOG
   - emit canonical startup evidence instead of relying only on stderr or ad-hoc debug files
   - record runtime bootstrap start, consumer registration count, and live bus identity in canonical logs
   - make current registration success observable from the same event log used for diagnostics

3. FAIL FAST ON MISSING REGISTRATION EVIDENCE OR ZERO-CONSUMER DISPATCH
   - add a hard runtime invariant requiring registration evidence before normal dispatch begins
   - add a hard runtime invariant requiring consumer count greater than zero before dispatch
   - halt runtime immediately when either invariant fails and record that failure canonically

4. PROVE REGISTRATION AND DISPATCH USE THE SAME LIVE EVENTBUS INSTANCE
   - trace the `EventBus` created in `EventRuntime::new` through later runtime dispatch
   - remove any path that swaps, shadows, reconstructs, or dispatches on a different bus instance
   - prove the bus that receives registration is the same bus that later dispatches runtime events

5. AFTER LIVE NON-ZERO CONSUMER REGISTRATION IS PROVEN, RESTORE RUNTIME AND DECISION ENTRY
   - emit `runtime_started` exactly once and make runtime actor identity visible in canonical logs
   - ensure recurring tick emission reaches persistence through the live runtime path
   - construct `SemanticStateSummary` at startup and on each tick
   - emit one canonical decision every tick before downstream execution
   - keep `scheduler_len`, `planned_pending`, and similar counters out of control truth

## BLOCKED / NOT READY YET

- route repair before non-zero live consumer registration is proven
- loop entry repair before route events exist
- downstream plan/act/verify/reward work before observe exists
- local queue-symptom patches that preserve scheduler-first routing
