# INTENT
## Objective
Prevent illegal Act transitions and empty-scheduler execution to restore a valid observe→plan→act→verify loop.
## Constraints
- no build break
- no test failure
## Targets
- canon-utils/canon-runtime/src/consumers/repair_control_consumer.rs
- canon-utils/canon-loop/src/stage/act.rs
- canon-utils/canon-runtime/src/bus.rs
## Success Criteria
- Act is never selected when scheduler_len == 0
- act_stall never transitions to classifying
- illegal Act → classifying transitions are blocked
- PlanningCompleted only occurs when actionable steps exist
- no repeated noop or PlanningCompleted loops without progress
- runtime logs show stable observe→plan→act→verify progression
