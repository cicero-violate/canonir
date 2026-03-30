# Discovery Report

## 1. File Tree

Workspace root: /workspace/ai_sandbox/canon

canon-utils crates (partial):
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
- canon-exec/src/exec/{llm.rs,bash.rs,file.rs}
- canon-loop/src/stage/{act.rs,plan.rs,observe.rs,verify.rs}
- canon-route/src/{executor.rs,policy.rs}

---

## 2. Module Structure

Each crate follows standard layout:
- Cargo.toml
- src/lib.rs
- optional submodules (exec/, reducers/, stage/, consumers/)

Notable:
- canon-loop → staged execution system
- canon-runtime → event + consumer system
- canon-route → routing + decision layer
- canon-exec → capability execution

---

## 3. Compiler State

Not fully verified in this run.
Previous context indicates cargo check succeeds.

Updated verification:
- cargo check --workspace completed successfully
- No compiler errors or warnings observed in final output
- Crates compiled:
  - canon-route
  - canon-runtime
  - canon-loop
  - canon-policy-matrix

Conclusion:
- Workspace is in a compilable state
- Invariant issues are runtime/logical, not compile-time

---

## 4. Test Surface

Tests exist across:
- canon-runtime (harness + repair binaries)
- canon-loop (harness_repair.rs)
- canon-exec (policy.rs tests)
- canon-judgment
- canon-llm-runtime

Tests are embedded in source files, not centralized.

---

## 5. Plan Status

Plan: Fix noop_spam invariant (loop_acted without action)

Status summary:
- Diagnosis: PARTIALLY VERIFIED (claims exceed evidence)
- loop_acted guards: PARTIAL (act.rs not fully covered)
- executor fixes: INCOMPLETE
- PlanningCompleted invariant: NOT VERIFIED (uses planned_pending)
- log validation: NOT VERIFIED (no artifacts)

---

## 6. Key Observations

### A. Core Failure Pattern
- Act executed with empty scheduler
- loop_acted emitted without ToolCall
- leads to noop_spam invariant

### B. Missing Enforcement
- scheduler_len not enforced at:
  - policy
  - executor
  - act stage

### C. Guard Fragmentation
- executor guard exists but weak
- act.rs has multiple emission paths
- no unified has_action check

### D. Logging Gaps
- no evidence of:
  - ACT_ENTRY logs
  - ROUTE logs
  - invariant correlation logs

### E. Runtime Signals
- observe_noop loops present
- llm call timeout errors
- diagnostics_triggered events

---

## 7. Risks

- Act can be entered without work
- loop_acted emitted incorrectly
- FSM transitions may regress
- invariants enforced only at bus (too late)

---

## Summary

System structure is modular and extensive, but invariant enforcement is inconsistent.

Primary issue remains unresolved:
Act lifecycle can execute without actionable work, producing invalid loop_acted events.

Fix requires:
- unified has_action invariant
- scheduler-based gating (not planned_pending)
- enforcement at policy, executor, and act stage
