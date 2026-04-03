# EXECUTOR PLAN (executor_pool)

## READY NOW (MAX 5)

1. RESTORE RUNTIME EVENT EMISSION (PRIMARY BLOCKER)
   - locate RuntimeEvent emission sites in runtime
   - verify emission is invoked during execution
   - ensure ≥1 RuntimeEvent per tick
   - fail-fast if no RuntimeEvent observed
   - verify runtime bootstrap emits initial RuntimeEvent

2. FIX EVENT BUS DISPATCH
   - ensure RuntimeEvent enters event bus
   - verify RouteExecutor receives RuntimeEvent
   - remove filtering / early termination
   - fail-fast if RuntimeEvent not delivered
   - ensure all consumers receive RuntimeEvent unless explicitly filtered

3. RESTORE LOOP ENTRY
   - ensure RouteExecutor forwards to LoopStageExecutor
   - verify loop execution begins (observed > 0)
   - fail-fast if loop not entered

4. RESTORE DECISION STAGE
   - ensure observe → decision always executes
   - verify RouteSelected events emitted
   - ensure decisions derive from SemanticStateSummary
   - fail-fast if decision missing

5. ENFORCE CANONICAL FLOW
   - enforce state → decision → transition chain
   - remove all bypass paths
   - ensure all transitions originate from decision output
