# EXECUTOR A PLAN (CONTROL-FLOW ROOT FIXES)

## READY NOW (MAX 5)
1. Fix LoopObserved emission (PRIMARY ROOT)
   1. Identify ALL emission sites in observe stage
   2. Remove noop / fallback / bypass emission paths
   3. Enforce exactly one LoopObserved per observe execution

2. Eliminate observe bypass paths
   1. Remove executor shortcuts skipping observe
   2. Ensure every loop produces LoopObserved

3. Guarantee observe → decision continuity
   1. Ensure LoopObserved always triggers decision()
   2. Remove fallback / NoOp handling

4. Prevent duplicate propagation
   1. Ensure single propagation path to EventBus
   2. Fail-fast on duplication

5. Validate LoopObserved → decision entry
   1. Ensure single delivery into decision()
   2. Fail-fast on re-entry

## BLOCKED
  - Decision and routing correctness depends on semantic authority fixes (executor-b)
