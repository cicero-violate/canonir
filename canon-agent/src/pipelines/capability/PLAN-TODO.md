1. Keep types pure: ensure no types/graph modules depend on planner/policy/scheduler/engine/IO.
2. Keep graph pure: ensure graph_* only depend on types and graph utilities.
3. Keep planner pure: planner/templates depend only on types/graph (no policy/scheduler/engine/IO).
4. Keep policy pure: policy_* depend only on types/graph/planner (no scheduler/engine/IO).
5. Keep scheduler/execution orchestration-only: no core logic from lower layers; all IO via engine.
6. Keep engine the only caller of IO, and IO modules do not import higher layers.
7. After each change, re-scan imports and update TARGET_MODULE_DEPENDENCY.md notes if needed.
