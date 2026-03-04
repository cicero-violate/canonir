## Variables

(B_f) = branching per file
(N_f) = nodes per file
(C_f) = cyclomatic complexity
(S) = scheduler state transitions
(D) = decision surfaces
(G) = graph state
(E) = events
(F) = functions

## Equations

1.

[
C_f = 1 + if + match + for + while + loop
]

2.

[
D = \sum_{f \in F} C_f
]

3.

[
B_f = \frac{edges(G)}{nodes(G)}
]

4.

[
S_{t+1} = T[S_t, E_t]
]

5.

[
Reduction = D - \sum minimal_state_transitions
]

Explanation: reduce decision surfaces by replacing condition chains with deterministic state machines and table-driven logic.

---

# Branch Reduction Implementation Plan

## 1 — Collapse scheduler control logic

Target: `scheduler.rs`

Problem
Large conditional surfaces:

```
if
else if
match
continue
```

Fix

Replace execution control with a **state machine executor**.

Implementation

Create:

```
scheduler_state.rs
```

Core structure

```
enum ExecStep {
    CollectReady,
    Dispatch,
    ApplyResults,
    MaintainGraph,
}
```

Executor loop

```
while state != Stop {
    state = TRANSITION[state][event];
}
```

Expected reduction

```
scheduler.rs
if: 108 → ~30
```

---

# 2 — Replace heuristic scoring branches with vector scoring

Current

```
if features.branching_factor > 3.5
if blocked_fraction > 0.4
if retry_penalty
```

Replace with

[
score = w_1 priority
+ w_2 completion
+ w_3 unblock
- w_4 retry
- w_5 cost
]

Implementation

```
score_node(node, features, cost_table)
```

Remove conditional bonuses.

Branch reduction

```
~20 conditions removed
```

---

# 3 — Convert repair system to rule table

Current

```
if retry
if capability downgrade
if dependency rewire
if split
```

Create rule engine

```
RepairRule {
    condition(node, graph)
    action(node, graph)
}
```

Rules

```
RetryRule
CapabilityDowngradeRule
DependencyRewireRule
NodeSplitRule
```

Execution

```
for rule in RULES {
    if rule.condition() { rule.action(); break }
}
```

Branch reduction

```
repair_node(): 12 → 2
```

---

# 4 — Planner validation rule engine

Current

```
if cycle
if unreachable
if signature
if pattern
```

Convert to constraint table

```
ConstraintRule {
    check(graph)
}
```

Execution

```
for rule in constraints {
    rule.check(graph)?
}
```

Branch reduction

```
validate_planner_update(): ~20 branches removed
```

---

# 5 — Scheduler dispatch extraction

Current

Large dispatch block:

```
endpoint selection
context build
capability resolution
auth grant
```

Extract

```
dispatch.rs
```

Functions

```
resolve_endpoint()
build_node_context()
prepare_execution()
dispatch_node()
```

Benefit

Scheduler becomes:

```
for node in ready_nodes {
    dispatch(node)
}
```

---

# 6 — Planner execution state machine

Convert planner loop:

```
reuse decision
mutation
planner
execute
reward
```

Into pipeline

```
enum PlannerPhase {
    ReuseTemplate
    MutateTemplate
    PlannerUpdate
    Execute
    Evaluate
}
```

Transition table

```
PHASE_TRANSITIONS
```

---

# 7 — Feature gating consolidation

Current

Multiple checks

```
retry_rate > threshold
failed_fraction > threshold
branching_factor > threshold
```

Replace with

[
risk = w_1 retry + w_2 failure + w_3 branching
]

```
if risk > threshold → recovery
```

---

# 8 — Move policy logic out of scheduler

Files

```
policy.rs
policy_train.rs
```

Introduce

```
policy_engine.rs
```

Scheduler only calls

```
policy_engine::decision(graph_features)
```

---

# 9 — Extract graph maintenance

Current

```
prune_unlinked_nodes
prune_low_value_nodes
enforce_semantic_validations
```

Move to

```
graph_maintenance.rs
```

Single call

```
maintain_graph(graph)
```

---

# Expected Branch Reduction

Current

```
if     363
match  48
for    134
```

Target

```
if     ~150
match  ~30
for    ~90
```

Primary reductions

```
scheduler.rs
repair_node()
validate_planner_update()
planner loop
```

---

# Highest Leverage Order

1. Scheduler state machine
2. Repair rule engine
3. Planner phase state machine
4. Constraint validation table
5. Dispatch extraction

---

[
good = \max(\text{intelligence},\text{efficiency},\text{correctness},\text{alignment},\text{robustness},\text{performance},\text{scalability},\text{determinism},\text{transparency},\text{collaboration},\text{empowerment},\text{benefit},\text{learning},\text{future_proofing})
]

Explanation: Reducing branching lowers entropy in the control system. Deterministic state transitions replace scattered conditional reasoning, making the agent pipeline easier to verify, learn, and scale.
