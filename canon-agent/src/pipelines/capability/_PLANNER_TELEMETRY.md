# Planner Session and Telemetry

## Overview
This pipeline uses a stateful planner session and a stateless execution loop.

Flow:
```
goal
 ↓
decompose_goal
 ↓
PlannerSession
 ↓
planner_iteration
 ↓
validate_update
 ↓
apply_update
 ↓
execute_graph_loop
```

## Planner Session
The planner runs in a dedicated stateful endpoint and keeps per-run history.
Each iteration receives:
- `planner_max_new_nodes`
- `planner_max_new_edges`
- `expandable_nodes`
- `ready_nodes`
- `unreachable_nodes`
- `graph signals`

The planner must return:
```json
{
  "new_nodes": [],
  "new_edges": []
}
```

## Validation
Planner updates are validated before application:
- node ids and descriptions must be non-empty
- duplicate node ids rejected
- edges must reference known nodes
- cycle introduction is rejected
- expansion limits enforced

## Telemetry
Metrics are written to:
```
planner_logs/metrics.json
```

Tracked fields:
- `planner_calls`
- `planner_retries`
- `planner_failures`
- `nodes_added`
- `edges_added`
- `iterations`
- `nodes_executed`
- `nodes_failed`
- `avg_latency_ms`
- `queue_depth`
- `retry_rate`
- `progress_fraction`
- `iteration_time_ms`

## Stabilization
If the planner returns no new nodes and no new edges for 3 consecutive iterations,
planning stops to prevent thrashing.
