# EXECUTOR PLAN (executor_pool)

## READY NOW (MAX 5)

1. RESTORE RUNTIME BOOTSTRAP INTO THE EVENT SYSTEM
   - audit the active runtime entrypoint and loop initialization path
   - ensure runtime actor registration happens in the event system
   - ensure `runtime_started` is emitted exactly once
   - restore recurring tick emission from the live runtime loop
   - add fail-fast if process boots and only `rustc` actor activity appears

2. REPAIR RUNTIME EMITTER WIRING END-TO-END
   - trace runtime event emission from bootstrap/tick sites into the event bus
   - verify runtime events reach the tlog append path
   - remove or expose any early-return, missing-emitter, or disconnected write path
   - add fail-fast if startup emits no `runtime_started` or no tick events

3. MAKE SEMANTIC STATE PRODUCE DECISIONS EVERY TICK
   - construct `SemanticStateSummary` at startup and on each tick
   - invoke decision evaluation from semantic state every cycle
   - emit canonical decision/control output each tick
   - add fail-fast if a tick completes without a decision event
   - keep queue counters out of decision truth

4. RESTORE DECISION -> ROUTE UNDER SEMANTIC-STATE AUTHORITY
   - ensure decision output emits lawful `RouteSelected`
   - verify route derivation depends on `SemanticStateSummary` plus policy/invariants
   - demote `scheduler_len`, `planned_pending`, and similar counters from route authority
   - add fail-fast if decision does not produce a lawful transition

5. VERIFY ROUTE -> OBSERVE HANDOFF ONLY AFTER ROUTE EVENTS EXIST
   - confirm route consumer/subscription path receives `RouteSelected`
   - restore lawful observe entry and `LoopObserved`
   - keep plan/act/verify repairs blocked until route and observe are present

## BLOCKED / NOT READY YET

- downstream plan/act/verify/reward repair before observe exists
- planner timeout tuning before canonical loop entry exists
- queue-local symptom patches that preserve scheduler-first routing
