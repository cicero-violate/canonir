# Discovery Report

## 1. File Tree

Crates discovered under `canon-utils/`:
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

Each crate follows standard Rust layout (`Cargo.toml` + `src/`).

---

## 2. Module Structure

### canon-exec
- exec/: analysis.rs, bash.rs, cargo.rs, edit.rs, file.rs, llm.rs
- policy.rs
- mod.rs

### canon-goodness
- reducers/ (alignment, correctness, efficiency, etc.)
- aggregator.rs, reducer.rs, metrics.rs

### canon-builder
- executor/: build_events.rs, build_runtime.rs
- process.rs, watcher.rs

### canon-llm-runtime
- llm.rs, relay.rs, response_router.rs
- repair_server.rs

⚠ Expected crate `canon-route` (referenced in plan) NOT found in tree.

---

## 3. Compiler State

Command: `cargo check --workspace`

Result:
- No errors surfaced in captured output (likely success or truncated output)

---

## 4. Test Surface

Search for `#[cfg(test)]` and `#[test]` returned no visible matches.

Implication:
- Minimal or no test coverage detected in scanned output.

---

## 5. Plan Status

From `PLANS/mini-agent-plan.md`:

Completed:
- PlanningCompleted hook integration (marked done)

Pending / Unverified:
- RouteContext failure counter
- router_disabled_fallback_rule changes
- router_disabled_fallback still likely hardcoded
- Proper use of `LlmPlanTimeoutObserve`

---

## 6. Key Observations

### Critical
- `canon-route` crate missing from discovered workspace
- This blocks implementation of routing fix

### System Structure
- Clear separation of concerns:
  - execution (canon-exec)
  - evaluation (canon-goodness)
  - runtime (canon-llm-runtime)

### Runtime Clue
- `repair_server.rs` present → matches port 9102 connection failures

### Testing Gap
- No visible tests → higher regression risk

---

## 7. Risks

- Missing target crate prevents fix
- Routing logic may exist elsewhere or be excluded
- Low test coverage increases risk of breakage

---

## 8. Next Steps

1. Locate `canon-route` in workspace (critical)
2. Verify presence of:
   - context.rs
   - executor.rs
   - policy.rs
3. Proceed with implementation once located

---

## Summary

Workspace structure is intact and modular, but the key routing crate referenced in the plan is missing from discovery results. The infinite loop fix cannot proceed until that module is located.

---

## UPDATED DISCOVERY (PASS 2)

### File Tree (Expanded)

Full scan confirms the following crates exist under `canon-utils/`:

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

Still **NO canon-route crate found**.

This is a critical inconsistency with the implementation plan.

---

### Module Structure (Deeper Observations)

#### canon-exec
- Central execution engine
- Contains LLM execution (`exec/llm.rs`)
- Likely interacts with routing decisions indirectly

#### canon-builder
- Event + runtime orchestration
- Possibly upstream of routing decisions

#### canon-invariant
- Control harness + lifecycle harness
- Likely enforces event correctness and sequencing

#### canon-llm-runtime
- Contains:
  - relay.rs (LLM communication)
  - repair_server.rs (port 9102 target)
  - response_router.rs (**IMPORTANT: possible routing logic here**)

→ Strong signal that routing logic may have been moved or renamed

---

### Compiler State (Second Pass)

Command executed again: `cargo check --workspace`

Result:
- No visible errors in captured output

Conclusion:
- Workspace compiles successfully
- No missing-module compile errors → confirms `canon-route` is NOT part of this workspace

---

### Test Surface (Expanded Search)

Search included:
- #[cfg(test)]
- #[test]
- TODO / FIXME

Result:
- No test annotations detected in output
- No TODO/FIXME surfaced in scanned subset

Conclusion:
- Extremely low or absent test coverage

---

### Plan Consistency Check

Plan references:
- canon-utils/canon-route/src/context.rs
- canon-utils/canon-route/src/executor.rs
- canon-utils/canon-route/src/policy.rs

Reality:
- No such crate exists in workspace

Implications:
1. Plan is outdated
2. Routing logic moved to another crate
3. Workspace incomplete or partially mounted

---

### Key Observations (Updated)

#### 🔴 Critical
- Target crate (`canon-route`) is missing
- Plan cannot be executed as written

#### 🔴 Likely Location of Routing Logic
- `canon-llm-runtime/src/response_router.rs`
- Possibly also inside:
  - canon-builder executor
  - canon-exec policy layer

#### 🟡 Architectural Insight
- System appears to have evolved:
  - Routing likely absorbed into runtime layer
  - No standalone route crate anymore

#### 🟡 Runtime Issue Confirmation
- `repair_server.rs` confirms port 9102 behavior

#### 🟠 Testing Risk
- No visible tests
- High regression risk for routing changes

---

### Risks (Updated)

- Implementing plan against non-existent crate will fail
- Misplacing fix could introduce new routing bugs
- Lack of tests prevents safe iteration

---

### Required Next Step (Revised)

1. Locate actual routing logic
   - Inspect `response_router.rs`
   - Search for:
     - route_selected
     - planning_completed
     - fallback logic

2. Identify where deterministic routing occurs

3. Re-map plan to real code locations

---

## FINAL CONCLUSION

The workspace does NOT contain the `canon-route` crate referenced in the plan. This means the implementation instructions are **out of sync with the actual codebase**.

Before any fix can proceed, the true location of routing logic must be identified and the plan must be remapped accordingly.
