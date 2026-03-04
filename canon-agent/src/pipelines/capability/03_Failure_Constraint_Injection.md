# Implementation Plan — 03 Failure Constraint Injection

## Variables

G = task graph  
σ(G) = graph signature  
F = failure store  
C = constraint set  
P = planner search space  

---

## Equations

Failure detection

σ(G) ∈ F

Constraint generation

C = f(σ(G))

Planner restriction

P' = P − C

---

## Objective

Prevent the planner from generating graphs that previously failed.

Convert stored failure signatures into **structural constraints** that the planner must obey.

---

# Architecture

Current

Planner → Graph → Execution → Failure → Stored in FailureStore

New

Planner → Candidate Graph  
  → Constraint Check  
  → Reject if known failure

Execution → Failure → Constraint Generation

---

# Constraint Types

## 1 Structural Graph Constraints

Prevent patterns such as:

- cyclic dependency
- unreachable nodes
- disconnected components

Constraint example

```

no_cycle(subgraph)

```

---

## 2 Capability Constraints

Avoid invalid capability combinations.

Example

```

Mutate + Verify in same node

```

Constraint

```

class_disjoint(node.capabilities)

```

---

## 3 Dependency Constraints

Prevent impossible ordering.

Example

```

ApplyPatch → Analyze

```

Correct order

```

Analyze → ApplyPatch

```

Constraint

```

must_precede(Analyze, ApplyPatch)

```

---

## 4 Failure Pattern Constraints

Repeated node failure pattern

Example

```

node(description="cargo build full workspace")

```

Rewrite constraint

```

replace_with("cargo check")

```

---

# Implementation Steps

## 1 Extend FailureStore

Add constraint extraction.

File

```

failure_store.rs

```

Function

```

fn constraints(&self) -> Vec<Constraint>

```

Constraint example

```

struct Constraint {
signature: String,
rule: ConstraintRule
}

```

---

## 2 Constraint Rule Types

```

enum ConstraintRule {
NoCycle,
NoUnreachable,
CapabilityConflict,
InvalidDependency,
PatternRewrite
}

```

---

## 3 Inject Constraints into Planner

Location

```

validate_planner_update()

```

Add step

```

check_constraints(candidate_graph)

```

Reject candidate graph if constraint violated.

---

## 4 Signature Matching

Use existing function

```

graph_signature(graph)

```

Compare with failure signatures.

```

if signature ∈ FailureStore
reject

```

---

## 5 Pattern Generalization

Convert repeated failures into rules.

Example

```

if failure_count > threshold
generate constraint

```

---

## 6 Planner Feedback

Return error to planner

```

"candidate graph violates constraint"

```

Planner generates new update.

---

# Telemetry

Add metrics

```

constraint_rejections
constraint_types
constraint_hit_rate

```

Helps track planner learning.

---

# Config

Add parameters

```

failure_constraint_threshold
max_constraints

```

Used to control constraint generation.

---

# Files Modified

failure_store.rs  
scheduler.rs  
planner_session.rs  
graph_algo.rs  
config.rs  

---

# Expected Impact

Planner avoids repeating failed graphs.

Search space reduces.

Execution stability increases.

---

# Result

Planner becomes **failure-aware**.

System evolves toward:

failure → constraint → improved planning
