# multi-dag pipeline

This directory contains the multi-dag pipeline implementation.

It replaces the prior linear phase loop with a DAG-driven controller:
Goal → Decompose → Plan → Execute → Verify → DAG update.

Key components:
- `dag.rs`: task graph IR and status transitions
- `scheduler.rs`: ready-node resolution
- `decompose.rs`: goal decomposition agent
- `planner.rs`: DAG planning agent
- `execute.rs`: executor agent (deltas applied)
- `verify.rs`: verifier agent (status updates only)
- `llm.rs`: shared LLM call utilities
