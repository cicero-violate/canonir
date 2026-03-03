# DAG-Control Migration Plan (Case A — 4 Agents) — multi-dag

## Summary
Replace the linear Observe/Plan/Act/Verify loop with a DAG-driven controller:
Goal → D_g → Tasks → P → TG → X → S → V → TG.

Use four distinct agents:
- D_g: https://chatgpt.com/gg/69a5aa249554819e9ac25e2df27102f1
- P:   https://chatgpt.com/gg/69a32d7d1a008199948ad06498df2f4f
- X:   https://chatgpt.com/gg/699c50e06bc881a3aa5ac1866bf15679
- V:   https://chatgpt.com/gg/6992c359272881a19d30c226925f575d

## Files to Replace
Remove the phase pipeline:
- observe.rs
- plan.rs
- act.rs
- score.rs

Replace with DAG modules:
- goal.rs        (goal intake + normalization)
- decompose.rs   (D_g agent)
- dag.rs         (DAG IR + status)
- planner.rs     (P agent)
- execute.rs     (X agent)
- verify.rs      (V agent or rule engine)
- scheduler.rs   (ready-node resolution)

## DAG IR
Status enum:
- Pending
- Ready
- Running
- Completed
- Failed
- Blocked

Progress metric: count of Completed nodes.

## Loop Contract
1. Goal → D_g → tasks
2. tasks → P → TaskGraph
3. TaskGraph → X → apply deltas
4. Workspace state + execution output → V → TaskGraph update
5. Repeat until all nodes Completed

Verifier must be independent from Executor: V ≠ X.

## Logging
Add logs for:
- goal_spec.json
- decompose_output.json
- planner_output.json
- task_graph_before.json
- task_graph_after.json
- execute_output.json
- verify_output.json

## Implementation Phases
1. **IR + Scheduler**
   - Implement `dag.rs` and `scheduler.rs`.
   - Add validation helpers and status transitions.
2. **Agents**
   - Implement D_g/P/X/V calls using agent cards.
   - Enforce V ≠ X in routing.
3. **Execution**
   - Wire `execute.rs` to delta application.
   - Record per-node execution results.
4. **Verification**
   - Implement verifier updates from external signals.
   - Only verifier updates node status.
5. **Integration**
   - Replace `run_tick` loop with DAG loop.
   - Remove scoring and phase logic.

## Success Criteria
- DAG loop can complete a simple goal with multiple tasks.
- Failures isolate to individual nodes without stalling the loop.
- Verifier exclusively controls status transitions.
