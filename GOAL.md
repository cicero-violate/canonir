# GOAL.md

# Canon System Objective

CanonIR is the single source of structural and semantic truth.

All subsystems are pure functions across defined boundaries.

Pipeline:

Capture → CanonIR → Graph → Solve → Emit

If CanonIR is valid, emission compiles.
If emission fails, CanonIR is incomplete.

No subsystem compensates for another.
No hidden repair logic.

---
# Subsystem Definitions
---

## canon (Core IR Layer)

### Role
Own the data model and structural invariants.

### Responsibilities
- Arena storage
- Intern tables
- Node/Edge definitions
- 8 CSR graphs
- Canon-owned IDs
- Structural validation

### Hard Invariants
- No semantic interpretation
- No graph derivation
- No solver logic
- No emission logic
- No compiler access
- No text generation

### Must Not Do
- Read rustc
- Fix semantic errors
- Decide layout
- Perform liveness pruning
- Infer missing edges

### Success Criteria
- CanonIR validates deterministically
- No duplicate structural edges
- Graph CSR structures are consistent
- Intern tables stable and deterministic
- CanonIR is serializable and reloadable without mutation

---

## canon-capture

### Role
Extract compiler truth and assemble CanonIR deterministically.

### Responsibilities
- rustc frontend interaction
- DefId → NodeId indexing
- Project items/bodies/relations
- Assemble Partial → CanonIR

### Hard Invariants
- Deterministic assembly
- No semantic repair
- No liveness pruning
- No normalization beyond structural lowering
- No graph derivation
- No emission logic

### Must Not Do
- Solve dependencies
- Resolve missing impl targets
- Fix module cycles
- Reorder for layout
- Mutate after assembly

### Success Criteria
- Same input crate → identical CanonIR
- All compiler-visible items represented
- Structural edges reflect compiler truth
- No missing NodeIds
- Assembly stable across runs

---

## canon-analyzer

### Role
Derive graphs and enforce semantic law.

### Responsibilities
- Build derived graphs
- Implement solver chain
- Enforce invariants
- Perform normalization
- Prune dead code
- Validate semantic consistency

### Hard Invariants
- May mutate IR only via invariant-preserving transforms
- All mutations must be deterministic
- No emission logic
- No compiler calls
- No text inspection

### Must Not Do
- Generate source text
- Read emitted files
- Introduce nodes not derivable from CanonIR
- Hide solver failure

### Success Criteria
- CanonIR becomes semantically closed
- All required edges derived
- Impl targets resolved or reported
- No invariant violations post-solve
- Re-running solver yields identical IR (fixpoint)

---

## canon-projection

### Role
Pure deterministic projection of CanonIR into source + Cargo.

### Responsibilities
- File planning
- Item ordering
- Text emission
- Cargo.toml generation

### Hard Invariants
- Never mutates IR
- Never performs semantic decisions
- Never inspects emitted text
- Never repairs missing semantics

### Must Not Do
- Resolve impl targets
- Infer imports
- Prune dead items
- Normalize paths
- Solve dependencies

### Success Criteria
- Same CanonIR → identical emitted source
- Emitted project compiles if CanonIR valid
- Emission stable across runs
- No hidden logic beyond projection

---

## canon-mutation

### Role
Structured IR transformation and verification.

### Responsibilities
- Define mutation ops
- Apply deterministic transforms
- Generate diffs
- Re-verify invariants

### Hard Invariants
- All mutations explicit
- No hidden side effects
- Must re-validate IR post-mutation
- No emission logic

### Must Not Do
- Direct text edits
- Bypass solver validation
- Introduce invalid states

### Success Criteria
- Mutations produce valid CanonIR
- Diff reflects exact structural delta
- Verification passes post-mutation
- Deterministic mutation behavior

---

## orchestration

### Role
Pipeline executor only.

### Responsibilities
- Run Capture
- Run Analyzer
- Run Emit
- Provide entrypoint

### Hard Invariants
- No business logic
- No mutation logic
- No semantic decisions
- No repair behavior

### Must Not Do
- Inspect IR internals
- Modify CanonIR
- Branch based on emitted output

### Success Criteria
- Pipeline stages execute in strict order
- Deterministic end-to-end execution
- Failure surfaces at correct stage
- No hidden state between stages

---

# Global System Invariants

1. CanonIR is the only shared truth boundary.
2. Emitter is a pure function of CanonIR.
3. Solvers encode semantic law.
4. Capture reflects compiler truth only.
5. Graph derivation is deterministic.
6. System reaches semantic fixpoint before emission.
7. No stage compensates for another.

---

# Final Pipeline

rustc
↓
Capture
↓
CanonIR (Structural Truth)
↓
Graph Derivation
↓
Solver (Semantic Law)
↓
Emit (Pure Projection)
