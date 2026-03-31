# Discovery Report

## 1. File Tree

Workspace root: /workspace/ai_sandbox/canon

Key directories:
- canon-utils (core system)
- canon-ir
- canon-agent-prompts
- _old (legacy, inactive)

canon-utils crates (observed):
- canon-analyst
- canon-builder
- canon-check
- canon-decision
- canon-exec
- canon-goal
- canon-goodness
- canon-invariant
- canon-judgment
- canon-llm-runtime
- canon-loop
- canon-route
- canon-runtime

Representative files:
- canon-exec/src/exec/{llm.rs,bash.rs,file.rs,cargo.rs}
- canon-loop/src/stage/{act.rs,plan.rs,observe.rs,verify.rs}
- canon-route/src/{executor.rs,policy.rs}
- canon-runtime/src/{bus.rs,lib.rs,consumers/*}

---

## 2. Module Structure

Each crate follows standard Rust layout:
- Cargo.toml
- src/lib.rs
- optional submodules

Notable structure:

1. Routing Layer
   - canon-route
   - policy.rs + executor.rs

2. Loop Engine
   - canon-loop
   - staged pipeline: observe → plan → act → verify

3. Runtime/Event Bus
   - canon-runtime
   - bus.rs + event consumers

4. Execution Layer
   - canon-exec
   - exec/* (llm, bash, cargo, file)

5. Evaluation Layer
   - canon-goodness
   - reducers + metrics

Architecture is layered but loosely enforced.

---

## 3. Compiler State

Result:
- cargo check --workspace: SUCCESS
- No compile errors

Conclusion:
- System compiles cleanly
- Failures are runtime invariant violations

---

## 4. Test Surface

Test locations:
- canon-judgment/src/lib.rs
- canon-exec/src/policy.rs
- canon-runtime/src/bin/harness_repair.rs
- canon-runtime/src/bin/harness_suite.rs
- canon-runtime/src/bin/canon-eventlog-repair.rs
- canon-runtime/src/consumers/repair_control_consumer.rs

Observations:
- Tests are embedded, not centralized
- Heavy focus on harness / runtime testing
- Missing invariant-focused tests:
  - LoopActed requires tool_result_id
  - scheduler must be non-empty before Act

---

## 5. Plan Status

Plan: Fix noop_spam invariant

Status:
- Routing guard: implemented but ineffective
- PlanningCompleted guard: partially implemented
- Act guard: present but still violated
- Observe paths: still emit invalid signals
- Validation: NOT complete

Pending:
- Ensure no LoopActed without tool_result_id
- Ensure scheduler_len > 0 before Act
- Remove loop_acted from observe paths
- Validate logs show clean cycle

---

## 6. Runtime Evidence

Observed failures:
- NOOP_SPAM_TRACE triggered
- route_executor_idle_no_action
- loop_acted emitted during observe
- llm timeouts

Critical panic:
- canon-loop/src/stage/act.rs:1311
- "LoopActed emitted without tool_result_id"

Observed behavior:
- PlanningCompleted → Act even when no work
- Act emits LoopActed without tool_result
- Observe path also emits LoopActed

---

## 7. Core Failure

- LoopActed emitted without real execution
- Scheduler empty at Act stage
- Bus detects mismatch → noop_spam invariant
- System enters observe/noop loop

---

## 8. Root Causes

### A. Missing Scheduler Enforcement
- scheduler not checked consistently
- Act allowed with empty scheduler

### B. Fragmented Guards
- policy, executor, act all partially enforce
- no single authoritative invariant

### C. Incorrect Signals
- planned_pending used instead of scheduler
- mismatch between planning and execution readiness

### D. Invalid Observe Emissions
- observe paths emit loop_acted
- violates stage semantics

### E. Act Stage Leak
- emit_acted allows missing tool_result_id
- invariant enforced too late (panic instead of prevention)

### F. Bus is Reactive Only
- detects invariant violations
- does not prevent invalid events

---

## 9. Key Observations

- Multiple components can emit LoopActed
- No centralized invariant enforcement
- Logs show repeated observe_noop loops
- LLM failures amplify noop cycles
- System lacks strict phase boundaries

---

## 10. Conclusion

System is structurally sound but behaviorally unsafe:

- Planning → Act transition is not guarded by executable work
- Act stage does not strictly require tool results
- Observe stage leaks action signals

Primary invariant violation:
- LoopActed must ONLY occur when a ToolResult exists

Fix must enforce:
- scheduler_len > 0 before Act
- tool_result_id required for LoopActed
- Observe never emits LoopActed

