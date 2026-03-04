# System Readiness

This system currently supports:

- Continuous agent loop (planner + executor + scheduler).
- Policy-guided control of planner invocation and expansion.
- GPU-backed graph analytics (topo, SCC, reachability, depth, feature stats) when `--features cuda`.
- Adaptive scheduling with retry penalties, unblock bias, and completion-velocity bias.
- Auto-pruning of low-value or unlinked nodes (enabled by default).
- Online policy learning with dataset + weights stored under `agent_logs/`.
- Telemetry snapshots each iteration (metrics + policy signals).

Run continuously:

```bash
cargo run -p canon-agent --features cuda -- run-capability /workspace/ai_sandbox/canon
```

Pipeline flow:

```
Goal → Graph Features → Policy Decision → Planner (optional) → Scheduler → Executor → Telemetry → Loop
```

## STATUS (Verified)
1. ✓ **Template Auto-Selection**
   Policy must choose between:
   - load existing template
   - run planner

2. ✓ **Template Mutation Engine**
   Ability to evolve templates by mutating nodes/edges and keeping higher-reward variants.

3. ✓ **Graph Repair Operator**
   Local DAG rewrites when nodes fail (repair instead of full replanning).

4. ✓ **Failure Constraint Injection**
   Failure signatures must generate structural constraints for future planner iterations.

5. ✗ **Long-Horizon Credit Assignment**
   Planner decisions must receive reward based on downstream execution success.

6. ✗ **Capability Cost Model**
   Learned latency + reliability prediction used by scheduler and planner expansion.

7. ✗ **Goal Similarity Embedding**
   Better template retrieval using semantic goal similarity instead of only structural matching.

8. ✓ **Deterministic Resume State**
   Persist graph + policy + telemetry snapshot so runs can resume exactly.
