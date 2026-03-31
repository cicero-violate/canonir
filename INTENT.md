# INTENT
## Objective
Prevent Act-stage execution with an empty scheduler and block illegal Act → classifying transitions to restore a valid observe→plan→act→verify loop.
## Constraints
- no build break
- no test failure
## Targets
- canon-utils/canon-runtime/src/consumers/repair_control_consumer.rs
- canon-utils/canon-loop/src/stage/act.rs
- canon-utils/canon-runtime/src/bus.rs
## Success Criteria
- Act is only selected when scheduler_len > 0
- act_stall never transitions to classifying
- Act → classifying transitions are explicitly guarded and blocked
- PlanningCompleted is emitted only when executable actions exist
- no repeated noop or PlanningCompleted loops without progress
- runtime logs confirm Observe → Plan → Act → ToolCall → ToolResult → Verify sequence without stalls
