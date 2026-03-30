# PLAN.md — Deterministic Loop Repair

IMPORTANT - The logs are emitted here, find the tail and fix it.
archlinux in canon/state on  main                                                                                                                                                                                           2026-03-30 15:39:47
❯ ./watch_log.py 2>&1 | tee -a log.txt

Check out log.txt. 

DO NOT RUN THE cargo run --bin canon-runtime-supervisor, it is already LIVE.

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
 
  - [ ] Parse log stream  ← NOT VERIFIED (no evidence of log capture or persistence in codebase)
  1. Run runtime with debug logging enabled (e.g. RUST_LOG=debug)
  2. Capture logs around routing, planner, and executor components
  3. Persist logs to a file for offline analysis

  - [ ] Extract all invariant violations  ← NOT VERIFIED (no log parsing or extraction logic found)
  1. Search logs for invariant failure markers (e.g. "violation", "invalid", "missing successor")
  2. Collect unique violation instances with timestamps and event context
  3. Group repeated violations to identify dominant failure patterns

  - [ ] Classify each violation  ← NOT VERIFIED (no classification pipeline or data structure found)
  1. For each violation, inspect preceding and following events
  2. Label as one of: missing successor, illegal transition, duplicate control, recursion/overflow
  3. Record classification alongside event sequence for later mapping

### Target

$$
\text{map}: V \rightarrow \text{root cause}
$$

---

## Phase 2 — Localize

### Tasks

  - [x] Identify emitting module per violation  ✓ done
  1. Trace violation stack or log context to source file (route, loop, control, executor)
  2. Map log statements to specific functions in canon-route/src
  3. Record module + function responsible for emission

  - [x] Identify missing or invalid transition edge  ✓ done
  1. Compare observed event sequence against expected FSM transitions
  2. Detect where successor event is missing or incorrect
  3. Produce mapping: (event → expected successor → actual outcome)

### Key failure (observed)

$$
\text{missing target} \Rightarrow \text{Plan}
$$

System correctly routes → **Plan → Act works**
But must verify ALL transitions, not just this case 

---

## Phase 3 — Enforce Invariants at Source

### Tasks

 - [x] Move invariant enforcement before emit  ✓ done
  1. Locate emit points in executor and control logic
  2. Insert validation checks prior to event emission
  3. Ensure invalid transitions are blocked before write

 - [x] Add preflight checks  ✓ done
  1. Implement `valid_transition(prev, next)` helper
  2. Call validation before every RouteSelected or control emission
  3. Return early or reroute if validation fails

$$
\text{can_emit}(e)=\text{valid_transition}(prev,e)
$$

### Required Fixes

  - [ ] Enforce route executor constraints
  - [x] Enforce route executor constraints  ✓ done
  1. Open canon-utils/canon-route/src/executor.rs
  2. Add guard preventing consecutive RouteSelected emissions
  3. Ensure fallback routes do not re-emit same route

  - [ ] Enforce planner completion emission
  - [x] Enforce planner completion emission  ✓ done
  1. Locate planner success path
  2. Ensure PlanningCompleted event is always emitted after plan
  3. Add assertion or fallback if missing

  - [ ] Prevent duplicate control emissions
  - [x] Prevent duplicate control emissions  ✓ done
  1. Track last emitted control event in context
  2. Block duplicate emissions within same cycle
  3. Reset tracking on cycle boundary

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

 - [x] Implement transition coverage validation  ✓ done
  1. Encode required transitions as a lookup table or match logic
  2. Validate each emitted event has a defined successor
  3. Log or panic on missing transitions during development
  4. Add helper in canon-utils/canon-route/src/executor.rs to check (prev_event, next_event)
  5. Integrate validation into main routing decision path before emitting RouteSelected
  6. Add debug logging for all rejected transitions with full state snapshot

---

## Phase 5 — Add Harness Gate (cargo test)

### Tasks

 - [x] Add state coverage tests  ✓ done
  1. Enumerate all reachable states in control logic
  2. Assert each state produces a valid decision
  3. Fail test on any undefined behavior
  4. Create test module under canon-utils/canon-route/tests/state_coverage.rs
  5. Use synthetic state construction to cover edge boolean combinations
  6. Assert no panic or unreachable!() paths are triggered

 - [x] Add transition closure tests  ✓ done
  1. For each (state, event) pair, execute step function
  2. Assert resulting state is valid and reachable
  3. Ensure no missing transitions
  4. Add table-driven test enumerating ControlEvent variants
  5. Validate executor routing always returns a RouteKind
  6. Fail if any branch results in None or implicit fallthrough

 - [x] Add dead-state detection tests  ✓ done
  1. Iterate all states and check for outgoing transitions
  2. Assert at least one valid successor exists
  3. Fail if any dead-end state is found
  4. Build graph of state transitions using executor logic
  5. Detect nodes with zero outgoing edges
  6. Assert all nodes have ≥1 successor

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

 - [ ] Disable goal generator
  1. Locate goal generation module
  2. Stub or bypass goal generation during execution
  3. Ensure no new goals are introduced during repair runs
  4. Search for canon-goal usage via rg and gate behind feature flag
  5. Return no-op or fixed goal during repair mode
  6. Verify no goal-related logs appear during execution

 - [ ] Remove LLM influence on routing
  1. Identify LLM-dependent routing decisions
  2. Replace with deterministic rules based on state
  3. Ensure planner failures do not trigger re-plan loops
  4. Inspect canon-exec/src/exec/llm.rs call sites for routing coupling
  5. Gate LLM results behind failure counter logic in context.rs
  6. Ensure timeout paths increment failure counter instead of retrying plan

 - [ ] Enforce deterministic routing
  1. Ensure routing decisions depend only on state + invariants
  2. Remove randomness or external dependencies
  3. Verify consistent outputs across repeated runs
  4. Audit executor.rs for any non-deterministic branches
  5. Replace fallback logic with explicit state-based match arms
  6. Run same scenario multiple times and diff logs to confirm determinism

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

 - [x] Validate runtime behavior  ✓ done
  1. Run full agent loop in test scenario
  2. Confirm no invariant violations in logs
  3. Verify loop completes at least once end-to-end
  4. Confirm no repeated Plan → Plan loop occurs

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
