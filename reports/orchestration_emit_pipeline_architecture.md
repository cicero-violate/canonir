# Orchestration Emit Pipeline Architecture

## Purpose
This document describes the corrected orchestration emit pipeline used by the Canon system. The architecture ensures deterministic code emission from CanonIR with strict structural invariants and reproducible build results.

The goal is that running:

```
cargo run --bin orchestration -- --all
```

produces emitted Rust source files that compile with zero build errors.

---

# Canon System Model

The Canon system treats **CanonIR as the single source of structural and semantic truth**.

Pipeline:

Capture → CanonIR → Analyze → Project → Emit → Build Verification

Key rule:

If CanonIR is valid, emission must compile.

If emission fails, CanonIR is incomplete or structurally invalid.

No subsystem compensates for another.

---

# Pipeline Stages

## 1. Capture Stage

Location:

```
canon-capture
```

Purpose:

Intercept Rust compiler invocations and construct CanonIR from compiler data.

Output:

```
canon_capture.json
```

Properties:

- Pure structural extraction
- No projection logic
- Deterministic mapping of compiler structures

---

## 2. CanonIR Analysis

Location:

```
canon-analyzer
```

Responsibilities:

- Resolve type references
- Validate structural invariants
- Normalize control flow
- Verify return type authority

Key invariant checks:

- All referenced types must resolve
- All function return locals must match FnSig
- All graph edges reference valid nodes

If unresolved types remain, emission may fail.

---

## 3. Projection Stage

Location:

```
canon-projection
```

Purpose:

Transform CanonIR into a projection plan describing emitted Rust source.

Projection responsibilities:

- Module layout reconstruction
- Function body reconstruction
- Control flow lowering
- Pattern matching lowering

Projection must be deterministic and purely derived from CanonIR.

---

# Deterministic Emit Ordering

The emitter uses a deterministic ordering strategy to ensure reproducible builds.

Ordering rules:

1. Modules sorted lexicographically
2. Structs emitted before implementations
3. Traits emitted before trait implementations
4. Functions emitted in dependency order

This ensures stable output across runs.

---

# Dependency Graph Model

The emit pipeline uses a dependency graph derived from CanonIR.

Graph nodes represent:

- modules
- types
- traits
- implementations
- functions

Edges represent:

- type usage
- function calls
- trait implementation dependencies

Graph properties:

- Directed
- Acyclic
- Deterministic traversal

Topological sorting determines emit order.

---

# IR Structural Invariants

The following invariants must hold before emission.

## Type Resolution

All types referenced in nodes must resolve to:

- primitive types
- user defined types
- external crate types

Unresolved types indicate capture or analysis defects.

---

## Return Type Authority

Each function must contain a return local:

```
__ret
```

Invariant:

```
__ret.ty == FnSig.ret
```

Mismatch indicates capture inconsistency.

---

## Control Flow Validity

All basic blocks must satisfy:

- valid predecessor edges
- valid successor edges

No orphan blocks may exist.

---

## Pattern Matching Correctness

Match expressions must lower into valid Rust match syntax.

All arms must be exhaustive or include wildcard fallback.

---

# Emit Stage

Location:

```
orchestration
```

The emitter performs:

1. Load CanonIR
2. Run analyzer
3. Generate projection plan
4. Emit Rust source

Output directory structure:

```
emit/<fixture>/
  Cargo.toml
  src/
    lib.rs
    modules...
```

---

# Structural Surface Verification

After emission the pipeline scans the source tree.

Metrics collected:

- suppressed bindings
- unresolved gaps
- unreachable blocks
- match lowering gaps

These metrics indicate projection correctness.

---

# Build Verification

The final step runs:

```
cargo build
```

on emitted sources.

Telemetry extracted:

- error counts
- warning counts
- error categories
- errors by file

Build success indicates:

- CanonIR is structurally complete
- Projection logic is correct
- Emit ordering is deterministic

---

# Failure Interpretation

If build errors appear in emitted code, the cause must be one of:

1. Capture error
2. CanonIR invariant violation
3. Projection logic defect

The system **never performs heuristic repair in the emitter**.

Instead the root structural defect must be corrected.

---

# Determinism Guarantees

The pipeline guarantees:

- deterministic IR
- deterministic projection
- deterministic emission

Given the same CanonIR input, emitted code must be identical.

---

# Summary

The corrected orchestration emit pipeline ensures that:

- CanonIR fully determines emitted source
- structural invariants guarantee correctness
- deterministic ordering ensures reproducible builds

The result is a reliable pipeline where emitted Rust sources compile successfully whenever CanonIR is valid.
