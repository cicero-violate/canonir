# EXECUTOR PLAN (exec_chatgpt_d)

## READY NOW (MAX 5)

1. MAKE LOOPSTAGEEXECUTOR THE ONLY RUNTIME ENTRY (START HERE)
   - locate ALL RuntimeEvent consumers
   - ensure ONLY LoopStageExecutor is registered as RuntimeEvent handler
   - remove RouteExecutor from runtime entrypoints
   - enforce SPEC invariant: no hidden branches (H = 0)
   - prove only one control path exists from RuntimeEvent
   - VERIFY: EventBus has exactly one sync consumer (LoopStageExecutor)
   - FAIL if any additional consumer (e.g. RouteExecutor) is registered

2. COLLAPSE CONTROL-FLOW INTO LOOPSTAGEEXECUTOR
   - move full pipeline into LoopStageExecutor: state → decision → route → dispatch → observe
   - prohibit execution of any stage outside LoopStageExecutor
   - ensure transition emission is the ONLY control output
   - ensure all effects are recorded via event log (no side paths)

3. DEMOTE ROUTEEXECUTOR TO PURE FUNCTION
   - remove ALL event handling from RouteExecutor
   - ensure it only computes RouteSelected (no execution)
   - ensure LoopStageExecutor calls RouteExecutor internally

4. ELIMINATE PARALLEL CONTROL PATHS
   - remove EventBus routing/filtering behavior
   - eliminate fanout or alternate execution paths
   - enforce single path: RuntimeEvent → LoopStageExecutor
   - remove ALL fanout in EventBus (no multiple consumers per event)
   - ensure EventBus does not branch or duplicate RuntimeEvent delivery

5. FIX EVENT LOG INITIALIZATION (BLOCKING)
   - locate tlog initialization and lifecycle
   - ensure tlog is initialized BEFORE any event emission
   - remove INIT GUARD drop behavior for canonical events
   - enforce: LoopObserved MUST NOT be dropped under any condition
   - fail-fast if event emission occurs with uninitialized tlog
   - add invariant: event log append MUST succeed for LoopObserved
   - verify end-to-end: LoopObserved emitted → appended → visible in diagnostics
   - ensure no intermediate layer (EventBus/runtime) drops or filters events
   - fail if LoopObserved is missing from persisted event log

5. VERIFY FUNCTIONAL LOOP (HARD GATE)
   - LoopStageExecutor invoked for EVERY RuntimeEvent
   - observe > 0 AND LoopObserved > 0
   - no execution path exists outside LoopStageExecutor
   - verify determinism: identical state → identical decision/transition
   - verify no hidden branches or alternate execution paths exist
   - verify LoopObserved is persisted in event log (not just emitted)
   - detect INIT GUARD drops in runtime logs
   - fail if any canonical event is dropped before persistence

6. FIX EVENT LOG INITIALIZATION (BLOCKING)
   - ensure tlog is initialized before ANY event emission
   - remove or fail-fast INIT GUARD that drops events
   - enforce: no event emission allowed before tlog ready
   - MUST confirm via diagnostics: observe > 0 (not inferred)
   - MUST confirm RouteExecutor is absent from runtime execution traces
   - MUST confirm LoopObserved appears in persisted event log (not dropped)

2. FIX EVENT HANDOFF
   - ensure RouteExecutor forwards EVERY RuntimeEvent to LoopStageExecutor
   - remove ANY early return or branch before forwarding
   - enforce unconditional forwarding (no exceptions)
   - locate ALL branches in RouteExecutor and prove forwarding occurs on EVERY path
   - remove any secondary handler or side-effect path that bypasses forwarding
   - add fail-fast: if LoopStageExecutor not invoked, raise error immediately
   - prohibit RouteExecutor from spawning independent control-flow
   - enforce single linear flow: RuntimeEvent → RouteExecutor → LoopStageExecutor
   - enforce: forwarding MUST occur before ANY other logic
   - prohibit dispatch, observe, or terminal actions inside RouteExecutor

3. VERIFY LOOP ENTRY
   - confirm LoopStageExecutor.on_event is actually invoked at runtime
   - confirm canonical loop executes inside this handler
   - prove RouteExecutor never completes control-flow independently
   - confirm observe stage is actually invoked within LoopStageExecutor
   - confirm no parallel handler executes without LoopStageExecutor
   - instrument and log invocation to prove LoopStageExecutor is reached for each RuntimeEvent

4. RESTORE OBSERVE EXECUTION
   - ensure observe is explicitly called after dispatch inside LoopStageExecutor
   - remove any condition or branch that skips observe
   - enforce: observe MUST execute every cycle
   - add runtime log at observe entry (proof of execution)
   - add fail-fast: if observe not called in a cycle, raise error
   - emit LoopObserved exactly once per cycle

5. VERIFY CANONICAL LOOP (HARD GATE)
   - observe > 0 AND LoopObserved > 0
   - decision → route → dispatch → observe all occur
   - runtime executes BOTH RouteExecutor and LoopStageExecutor
   - require runtime logs proving observe executed for each cycle
     * invariant_errors == 0
   - BOTH structural AND runtime proof required

## NOTE
- NO other tasks are allowed until scheduler_len is fully removed
- Grep-only claims are invalid without structural diff proof
- Both diff + zero-match proof are mandatory before proceeding
- File-level proof (struct + usage removal) is mandatory for verification
- TYPE-SYSTEM proof is mandatory: scheduler_len must not exist in any compiled type
- File path + snippet proof is mandatory (verifier requires direct code evidence)
- Snippets must be verbatim file contents; summaries or descriptions are invalid

## BLOCKED

- ALL WORK except scheduler_len removal is blocked
