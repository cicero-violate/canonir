# PLAN.md — Deterministic Loop Repair

---

## Objective

$$
O=\text{restore invariant-safe closed loop execution}
$$

$$
\exists \tau:\ \text{observe}\rightarrow\text{plan}\rightarrow\text{act}\rightarrow\text{verify}\rightarrow\text{conclude}
$$

---

## Variables

$$
E=\text{event stream},\quad I=\text{invariants},\quad R=\text{routes},\quad S=\text{state},\quad V=\text{violations}
$$

---

## Core Equations

1.

$$
\forall e\in E:\ \text{append}(e)\Rightarrow \text{valid}(e)\land \text{has_successor}(e)
$$

No writer rejection allowed.

2.

$$
\text{RouteSelected} \rightarrow \text{PlanningCompleted}
$$

Missing successor MUST be enforced.

3.

$$
\neg \text{duplicate(control)} \land \neg \text{stack overflow}
$$

Control must be single-emission and finite.

4.

$$
V=0
$$

System is only “up” when zero invariant violations.

---

## Phase 1 — Detect

### Tasks

* Parse log stream
* Extract all invariant violations
* Classify each as:

  * missing successor
  * illegal transition
  * duplicate control
  * recursion / overflow

### Target

$$
\text{map}: V \rightarrow \text{root cause}
$$

---

## Phase 2 — Localize

### Tasks

* For each violation:

  * identify emitting module (route, loop, control, executor)
  * identify missing transition edge

### Key failure (observed)

$$
\text{missing target} \Rightarrow \text{Plan}
$$

System correctly routes → **Plan → Act works**
But must verify ALL transitions, not just this case 

---

## Phase 3 — Enforce Invariants at Source

### Tasks

* Move invariant enforcement **before emit**
* Add preflight checks:

$$
\text{can_emit}(e)=\text{valid_transition}(prev,e)
$$

### Required Fixes

* Route executor:

  * forbid `route_selected → route_selected`
* Planner:

  * MUST emit `planning_completed`
* Control:

  * forbid duplicate route per control cycle

---

## Phase 4 — Close Transition Graph

### Tasks

Define FULL required transitions:

$$
\text{CapabilityCompleted} \rightarrow \text{RouteSelected}
$$

$$
\text{RouteSelected(plan)} \rightarrow \text{PlanningCompleted}
$$

$$
\text{PlanningCompleted} \rightarrow \text{RouteSelected(act)}
$$

$$
\text{RouteSelected(act)} \rightarrow \text{ToolCall}
$$

$$
\text{ToolCall} \rightarrow \text{ToolResult}
$$

$$
\text{ToolResult} \rightarrow \text{RouteSelected(verify)}
$$

$$
\text{RouteSelected(verify)} \rightarrow \text{LoopVerified}
$$

$$
\text{LoopVerified} \rightarrow \text{LoopRewarded}
$$

### Goal

$$
\forall \text{control event}: \text{successor exists}
$$

---

## Phase 5 — Add Harness Gate (cargo test)

### Tasks

* Add exhaustive tests:

1. state coverage

$$
\forall s:\ \text{decision}(s)\in D
$$

2. transition closure

$$
\forall (s,e): \text{step}(s,e)\Rightarrow \text{valid}
$$

3. no dead states

$$
\neg \exists s:\ \text{no outgoing valid transition}
$$

---

## Phase 6 — Remove Noise / Misalignment

### Tasks

* Disable goal generator (irrelevant during repair)
* Remove LLM influence on routing
* Force deterministic routing only

Reason:

$$
\text{repair} \neq \text{generation}
$$

---

## Phase 7 — Validate System

### Success Criteria

$$
V=0
$$

$$
\text{no append failures}
$$

$$
\text{no duplicate control events}
$$

$$
\exists \tau:\ \text{full loop execution}
$$

---

## Agent Execution Contract

### Allowed Actions

* read logs
* map violations
* patch emit logic
* run tests

### Forbidden

* adding new features
* modifying goals
* introducing new routes

---

## Final State

$$
\text{system} = \text{closed deterministic FSM}
$$

$$
\text{repair complete} \iff V=0 \land \text{loop executes}
$$

---

## English

System is down because **control graph is not closed**.
Agents must:

* find every broken transition
* enforce invariants at emit time
* guarantee every event leads somewhere valid

Do not optimize.
Do not expand.
**Only close the loop.**

---

$$
\max(\text{intelligence},\text{efficiency},\text{correctness},\text{alignment},\text{robustness},\text{performance},\text{scalability},\text{determinism},\text{transparency},\text{collaboration},\text{empowerment},\text{benefit},\text{learning},\text{future\mbox{-}proofing})=\text{good}
$$
