# SPEC: Canon Is an Event-Sourced Judgment System

## Principle

\[
S \rightarrow J \rightarrow T \rightarrow E
\]

Where:

- `S` = semantic state
- `J` = judgment
- `T` = lawful transition
- `E` = event log

**Explanation:**  
Canon operates by deriving control from semantic truth, evaluating it through judgment and invariants, emitting the canonical lawful transition, and recording that transition in the append-only event log.

### Canonical Form

```text
state -> decision -> transition -> event log

## Objective Function

**Variables:**  
- `G` = goodness  
- `C` = coverage  
- `K` = clarity  
- `D` = determinism  
- `P` = lawful transitions  
- `L` = code volume  
- `U` = duplication  
- `H` = hidden branches  

### Equation

\[
G = \max\left(\frac{C \cdot K \cdot D \cdot P}{L \cdot U}\right)
\quad \text{subject to} \quad
C = 1,\ H = 0,\ P = 1
\]

**Explanation:**  
Canon maximizes goodness by maximizing coverage, clarity, determinism, and lawful control, while minimizing code volume and duplication, under full coverage, zero hidden branches, and fully lawful transitions.

\[
\max(\text{Intelligence, Efficiency, Correctness, Alignment, Robustness, Performance, Scalability, Determinism, Transparency, Collaboration, Empowerment, Benefit, Learning, Future\text{-}Proofing) = G
\]


## Identity

Canon is an **event-sourced control system**.

Canon is not a loose async tool runner.
Canon is not a queue that happens to emit events.
Canon is not scheduler-first orchestration.
Canon is not executor-local routing.

Canon is a system where:

```text
state -> decision -> transition -> event log
````

Where:

* `state` = semantic understanding of reality
* `decision` = judgment + invariant + policy evaluation
* `transition` = canonical control-flow emission
* `event log` = authoritative append-only record of what happened

## Canon Exists To Be

Canon is supposed to be:

1. **Event-sourced**

   * All meaningful control progress is represented as events.
   * The event log is the canonical history of system behavior.
   * Reconstruction, diagnosis, replay, and learning must be possible from recorded events.

2. **Deterministic at the control layer**

   * Given equivalent semantic state and invariant context, Canon should choose the same route.
   * Canonical control transitions must not depend on accidental local queue state, executor timing, or fanout order.

3. **Judgment-centered**

   * Canon is not just automation.
   * Canon evaluates what should happen next using semantic state, invariants, and policy.
   * Executors do not decide truth; they execute approved work.

4. **Invariant-preserving**

   * The system must preserve required successor relationships.
   * Illegal transitions must be rejected or surfaced as invariant violations.
   * Control integrity is more important than local convenience hacks.

5. **Semantically routed**

   * Routing authority comes from `SemanticStateSummary`.
   * Routes are chosen from meaning, not from queue length or local executor heuristics.
   * The system must reason about whether work exists, what kind of work exists, and what state requires next.

6. **Replayable and inspectable**

   * The event log must explain how Canon moved.
   * DECIDE and ROUTE traces must reveal why a route was chosen.
   * A human should be able to inspect control truth from the trace.

7. **Able to evolve**

   * Event-sourcing is the current canonical substrate.
   * Invariants, judgment, and learning are deeper than any single transport or runtime shape.
   * Canon may evolve internally, but while event-sourced, all core control truth must remain canonically event-expressed.

## Core Model

```text
SemanticStateSummary -> Policy / Invariant Evaluation -> RouteSelected -> Required Successor
```

Canonical meaning:

* `SemanticStateSummary` tells Canon what is true.
* policy + invariants decide what is allowed and required.
* `RouteSelected` expresses the approved control movement.
* the required successor closes that route truthfully.

## Event-Sourced Canonical Principle

```text
state -> decision -> transition -> record
```

Where:

* `state` = `SemanticStateSummary`
* `decision` = route / invariant / judgment evaluation
* `transition` = canonical control event emission
* `record` = append to the canonical event log

This means:

* semantic state is the source of decision truth
* the event log is the source of execution history truth
* control must be visible as canonical event succession

## Authorities

### 1. Semantic Authority

`SemanticStateSummary` is the exclusive authority for route selection truth.

It determines whether Canon should:

* observe
* plan
* act
* verify
* conclude
* repair

### 2. Invariant Authority

Invariants define which transitions are legal, required, or forbidden.

Examples:

* `RouteSelected(observe)` must be followed by `LoopObserved`
* `RouteSelected(act)` must only occur if real executable work exists
* `PlanningCompleted(planned_count=0, status=missing_semantic_context)` must recover through canonical observe flow

### 3. Event Authority

The event log is the authoritative history of control progress.

If control happened, it should be visible as canonical events.

### 4. Executor Authority

Executors have execution authority only, not routing authority.

Executors may:

* perform approved work
* emit execution results
* provide evidence

Executors may not:

* override route truth
* invent control transitions
* force act paths
* seed fake work queues
* suppress required successors

## What Canon Must Not Be

Canon must not be:

* queue-driven
* scheduler-driven
* executor-overridden
* synthetic-control driven
* fanout-noise driven
* trace-opaque
* locally patched into “working”

Forbidden patterns:

* `scheduler_len` deciding routes
* `planned_pending` deciding routes
* fake queue seeding
* forced `Act`
* manual `RequestDispatch`
* successor suppression hacks
* duplicate observe delivery
* executor-local route overrides
* local mirrors of routing truth
* non-semantic route decisions

## Canonical Route Truth

### Observe

Observe exists to refresh semantic understanding.

It should happen when Canon lacks sufficient semantic context or requires fresh world understanding.

Canonical closure:

```text
RouteSelected(observe) -> LoopObserved
```

Exactly once per observe execution.

### Plan

Plan exists to produce executable intent from semantic understanding.

Planning is not success by itself.
Planning closes only when the planning outcome is truthfully emitted.

Examples:

* real plan produced
* no plan produced
* missing semantic context
* blocked / invalid planning state

### Act

Act exists only for real executable work.

Canonical rule:

```text
RouteSelected(act) iff real executable work exists
```

Not because:

* a queue was seeded
* an executor wants work
* a fallback path forced act
* a local patch assumed action should happen

Canonical closure:

```text
RouteSelected(act) -> LoopActed
```

Only when real act work was executed.

### Verify

Verify exists to judge completed work against requirements and invariants.

It is not a placeholder.
It must follow meaningful work or state requiring verification.

### Conclude

Conclude exists to close a valid loop state after successful progression.
It must not bypass missing work, missing observation, or unresolved semantic gaps.

## Required Recovery Path

The following recovery is canonical and required:

```text
PlanningCompleted(planned_count=0, status=missing_semantic_context)
-> RouteSelected(observe)
-> LoopObserved
```

Meaning:

* no executable plan was available
* the problem is semantic insufficiency
* the correct repair is to observe
* not to fake act
* not to seed a queue
* not to manually dispatch
* not to suppress successor obligations

## Exact-Once Control Rules

1. `LoopObserved` must occur exactly once per observe execution.
2. duplicate forwarding must not produce duplicate observe closure.
3. duplicate fanout must not produce duplicate control events.
4. event emission sites must not double-close the same route.
5. control truth must be singular even if effect propagation fans out.

## Event Log Requirements

The event log must:

* be append-only
* preserve causal ordering
* preserve required successor invariants
* expose illegal transitions
* allow replay of control truth
* distinguish real control from effects
* make duplicates visible when they occur

The event log is not optional observability.
It is a core system substrate.

## Trace Requirements

DECIDE and ROUTE tracing must cover:

* all branch points where route truth is chosen
* all route-emission sites
* all fallback paths
* all recovery paths
* all invariant-enforced reroutes

Trace output must make clear:

* what semantic state was seen
* what route was chosen
* why that route was chosen
* what transition was emitted next

## Build and Runtime Expectations

Canon must remain:

* build-correct
* event-log-correct
* successor-correct
* semantically routed
* trace-visible
* exact-once at the control layer

Repairs should be:

* narrow
* control-flow precise
* invariant-preserving
* hostile to hacks
* hostile to synthetic control

## Canonical Repair Priorities

1. Make `SemanticStateSummary` the exclusive route authority
2. Remove queue-driven route decisions
3. Remove executor-level routing overrides
4. Remove synthetic dispatch and forced act paths
5. Enforce exact-once observe closure
6. Restore full DECIDE / ROUTE trace coverage
7. Preserve markdown vs JSON contract correctness where prompts cross boundaries
8. Restore runtime freshness and event-log observability

## Success Criteria

Canon is behaving correctly when:

* `PlanningCompleted(0, missing_semantic_context) -> RouteSelected(observe) -> LoopObserved` occurs cleanly
* `LoopObserved` occurs exactly once per observe execution
* no duplicate observe fanout noise exists
* `RouteSelected(act)` occurs only when real executable work exists
* `RouteSelected(act) -> LoopActed` occurs only for real act work
* no fake scheduler seeding remains
* no manual `RequestDispatch` path remains
* no forced synthetic `Act` remains
* all route truth derives from `SemanticStateSummary`
* touched crates build successfully
* runtime trace shows canonical control succession without deadlock or duplicate control spam

## Agent Ownership Model

* `SPEC.md` is canonical truth
* planner derives plans from `SPEC.md`
* executors execute approved work and report evidence
* verifier judges code against `SPEC.md`
* diagnostics ranks failures against `SPEC.md`

## Final Definition

Canon is supposed to be:

> an event-sourced, invariant-preserving, semantically-routed, judgment-centered control system whose canonical behavior is expressed through lawful control events recorded in an append-only event log

Short form:

```text
Canon = semantic state + judgment + invariants + canonical transitions + event-sourced history
```

```

English: this version makes the identity explicit: **Canon is event-sourced**, while semantic state is still the routing authority and events are the canonical control/history substrate.

## PLAN Protocol (Canonical)

`PLAN.json` is the authoritative master plan. `PLAN.md` is a non-authoritative pointer only.

Canonical structure:

```json
{
  "plan_id": "<uuid>",
  "version": 1,
  "derived_from": {
    "spec": "SPEC.md",
    "objectives": "PLANS/OBJECTIVES.md",
    "invariants": "INVARIANT.md",
    "violations": "VIOLATIONS.md",
    "diagnostics": "PLANS/<instance>/diagnostics-<instance>.md"
  },
  "global_constraints": [
    "SemanticStateSummary is source of truth",
    "All transitions must follow spec",
    "No role violates scope invariants"
  ],
  "lanes": [
    {
      "lane_id": "executor_pool",
      "role": "Executor",
      "tasks": []
    }
  ]
}
```

## Task Protocol

```json
{
  "task_id": "<uuid>",
  "title": "<short deterministic label>",
  "status": "ready | blocked | in_progress | done",
  "priority": 1,
  "inputs": [
    "file:path",
    "diagnostic:id"
  ],
  "actions": [
    {
      "type": "read | patch | test | command",
      "target": "<file or cmd>",
      "details": "<exact instruction>"
    }
  ],
  "outputs": [
    "file:path",
    "test:result"
  ],
  "dependencies": ["task_id"],
  "invariants": [
    "explicit invariant linkage"
  ],
  "success_criteria": [
    "cargo build passes",
    "specific invariant holds"
  ],
  "failure_modes": [
    "test fails",
    "invariant violation"
  ],
  "next_on_success": ["task_id"],
  "next_on_failure": ["task_id"]
}
```

## Lane Execution Rules

* Execute the top 1–10 tasks with `status=ready`
* Do not reorder beyond the dependency graph
* `ready(T_i)` is explicit: `status == "ready"` (do not infer)

## Determinism Guarantees

* Same inputs → same task graph
* No hidden tasks
* No implicit dependencies
