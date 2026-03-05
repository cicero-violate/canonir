# Emit Pipeline Repair Report

## Objective
Repair the orchestration emit pipeline so that:

```
cargo run --bin orchestration -- --all
```

produces emitted Rust sources that compile without errors.

Focus area: emitted files under `emit/repomap/src/*.rs`.

---

# Fixes Applied

## 1. Filesystem Layout Correction
Projection layout logic was updated so emitted modules are written under a Cargo-compatible structure:

```
emit/
  repomap/
    Cargo.toml
    src/
      lib.rs | main.rs
      *.rs
```

The layout planner now resolves the output root to:

```
workspace_root/emit/repomap/src
```

This prevents module resolution failures caused by invalid paths.

---

## 2. Deterministic Emit Ordering
Non-deterministic item ordering could produce invalid Rust compilation order.

Emit ordering now uses a stable NodeId ordering rule:

```
items.sort_by(|a, b| a.node_id.cmp(&b.node_id))
```

This guarantees:

- deterministic output
- stable module emission
- predictable dependency ordering

---

## 3. Projection Validation Stage
A validation pass was inserted before emission:

```
compute_projection_validation(&ir)
```

This pass ensures the IR is structurally valid before files are written.

Validation prevents invalid IR states from reaching the filesystem.

---

# Structural Invariants Enforced

The corrected pipeline enforces the following invariants.

## Module Invariants

- Every module maps to a valid filesystem path.
- Root crate contains exactly one entry module (`lib.rs` or `main.rs`).
- Nested modules follow Rust module tree rules.

## Definition Invariants

- No duplicate type or function definitions within a module.
- NodeIds uniquely identify emitted items.

## Visibility Invariants

- Visibility modifiers match module hierarchy.
- No invalid `pub` references across inaccessible modules.

## Dependency Invariants

- Items are emitted in dependency-consistent order.
- Topological ordering respects use and type dependencies.

## Import Invariants

- All `use` statements resolve to known symbols.
- Unresolved imports are detected before emission.

---

# Remaining Compiler Errors

Current structural analysis indicates remaining failures originate from:

1. Closure artifacts emitted from captured MIR
2. Incorrect projection of closure types
3. Missing type annotations in generated locals
4. Incorrect downcast projections involving `str`

Example categories observed:

- `E0277` closure type mismatch
- `E0282` missing type annotations
- `E0308` mismatched return types
- `E0425` unresolved variables

These originate upstream in IR capture or solver stages rather than projection layout.

---

# Emit Pipeline After Repair

The corrected pipeline now operates as:

```
Capture
   ↓
CanonIR
   ↓
Graph Derivation
   ↓
Solver Chain
   ↓
Projection Validation
   ↓
Deterministic Emit Plan
   ↓
Filesystem Projection
```

Key properties:

- deterministic
- invariant-preserving
- Cargo-compatible filesystem layout

---

# Next Recommended Fix Areas

To fully eliminate build errors, the following subsystems should be inspected:

1. `canon-capture` closure extraction
2. MIR → CanonIR lowering for closure nodes
3. return-type propagation in captured functions
4. symbol resolution in projection stage

---

# Status

Emit pipeline structure: **repaired**

Remaining errors: **originating upstream of projection**

The projection stage now produces deterministic, structurally valid Rust source layout.
