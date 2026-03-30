# Discovery Report

## 1. File Tree

Workspace root: `/workspace/ai_sandbox/canon`

Crates under `canon-utils/`:
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
- canon-route (confirmed present via test scan)

Each crate follows standard Rust layout:
- Cargo.toml
- src/*.rs

---

## 2. Module Structure

### canon-analyst
- agent.rs
- python.rs
- lib.rs

### canon-builder
- executor/build_events.rs
- executor/build_runtime.rs
- config.rs
- events.rs
- process.rs
- watcher.rs

### canon-exec
- exec/{analysis, bash, cargo, edit, file, llm}.rs
- policy.rs
- lib.rs

### canon-goodness
- reducers/* (alignment, correctness, performance, etc.)
- aggregator.rs
- reducer.rs
- metrics.rs
- storage.rs

### canon-invariant
- control_harness.rs
- request_lifecycle_harness.rs
- constraint_harness.rs
- cross_product_harness.rs
- lib.rs

### canon-llm-runtime
- llm.rs
- relay.rs
- parsers.rs
- response_router.rs
- repair_server.rs
- endpoint_worker.rs
- tab_management.rs

---

## 3. Compiler State

Command: `cargo check --workspace`

Result:
- No compilation errors observed (workspace builds clean)

Conclusion:
- Workspace builds successfully

---

## 4. Test Surface

Search:
- `#[cfg(test)]`
- `#[test]`

Result:
- Extensive tests in canon-route, canon-judgment, canon-semantic-state
- High density of routing/policy tests (canon-route dominant)
- Coverage remains fragmented (no unified invariant suite)

---

## 5. Plan Status (Cross-Product Invariant Suite)

### Already Present
- Control harness exists
- Constraint types exist
- Invariant pipeline exists
- Partial cross-product + constraint harness files exist

### Missing / Incomplete (Critical Gaps)

#### T_S — Seed Coverage
- ❌ No verified cross-product seed completeness

#### T_SE — Transition Closure
- ⚠ Partial (reachability added for control only)
- ❌ Joint closure correctness not validated

#### T_C — Constraint Precedence
- ⚠ Implementation added
- ❌ Not fully validated in pipeline behavior

#### T_I — Lifecycle
- ❌ No verified lifecycle module (promotion-only still dominant)

 #### T_P — Persistence
 - ❌ No round-trip validation
 - ❌ No idempotency checks
 - ❌ persistence module missing or unverified
 
 #### T_R — Bisimulation
 - ⚠ Hook present in pipeline
 - ❌ bisim_check currently called with empty inputs (NOOP)
 - ❌ No real trace validation occurring

---

## 6. Critical Wiring Gaps (Updated)

### GAP A — bisim_check ineffective
- Called with empty slices
- Always returns ok=true
- Does not validate actual system behavior

### GAP B — joint_reachability_table unused
- Not written to disk
- No coverage.json produced
- Transition closure not externally observable

### GAP C — Missing tests for new modules
- constraint_harness
- cross_product_harness
- invariant_lifecycle
- persistence
- bisimulation
- constraint_precedence

---

## 7. Overall Status (Revised)

State: PARTIALLY IMPLEMENTED / FUNCTIONALLY INCOMPLETE

Primary blockers:
1. Lifecycle system missing (no demotion path)
2. Persistence system missing (no round-trip / idempotency)
3. Bisimulation ineffective (no real inputs)
4. Cross-product correctness unverified
5. No test coverage for new invariant subsystems

#### T_R — Bisimulation
- ⚠ Hook added in pipeline
- ❌ No verified correctness of projections or traces

---

## 6. Key Observations

- canon-route crate exists and is heavily tested (earlier assumption incorrect)
- invariant expansion modules now exist but several are stub/partial
- constraint_harness uses placeholder logic (identity-like transitions)
- cross_product_harness depends on incomplete derive_route mapping
- pipeline wiring (bisim + conflicts) recently added but not validated
- lifecycle + persistence subsystems remain missing or incomplete
- system is mid-transition from structural-only invariants to full cross-product system
