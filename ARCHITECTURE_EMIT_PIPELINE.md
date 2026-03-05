# Canon Orchestration Emit Pipeline Architecture

## Purpose

This document describes the corrected architecture of the Canon orchestration pipeline responsible for producing compilable Rust source code from CanonIR. The design enforces strict determinism, clear subsystem boundaries, and structural invariants so that emitted code builds successfully when executing:

```
cargo run --bin orchestration -- --all
```

The pipeline follows the canonical system invariant:

```
Capture → CanonIR → Graph → Solve → Emit
```

If CanonIR is structurally and semantically valid, the emitter must produce compilable Rust without performing repairs.

---

# System Layers

## 1. Capture Layer (`canon-capture`)

### Responsibility
Deterministically extract compiler truth and construct CanonIR.

### Guarantees

- DefId → NodeId mapping is stable.
- All compiler-visible items are represented.
- Structural edges reflect compiler relationships.
- No semantic inference occurs.

### Output

A fully populated **CanonIR** representing structural program truth.

---

## 2. CanonIR Core (`canon`)

CanonIR is the single shared truth boundary between subsystems.

### Structure

CanonIR contains:

- Arena storage for nodes
- Intern tables
- NodeId identity space
- EdgeKind definitions
- 8 deterministic CSR graphs

### Core Invariants

1. Node identity is immutable.
2. Graph edges represent structural relationships only.
3. No semantic repair occurs at this layer.
4. Graph storage is deterministic.

### Graph Representation

All graphs are stored using **CSR (Compressed Sparse Row)** format:

```
row_offsets
col_indices
```

This guarantees:

- Deterministic iteration
- Cache-friendly traversal
- Stable ordering

---

# Dependency Graph Model

The Canon analyzer derives dependency graphs from CanonIR.

### Graph Types

Examples include:

- Name graph
- Dependency graph
- Type graph
- Control flow graph

Each graph satisfies:

```
Edge set = deterministic function of CanonIR
```

No graph may introduce nodes not present in CanonIR.

---

# Solver Phase (`canon-analyzer`)

### Purpose

Transform CanonIR into a semantically closed state.

### Responsibilities

- Resolve impl targets
- Build derived graphs
- Normalize structural relationships
- Validate semantic invariants

### Solver Chain

Solvers run in deterministic order until reaching a fixpoint.

```
solve(ir)
    → derive graphs
    → run solver chain
    → verify invariants
```

### Fixpoint Requirement

Running the solver twice must produce identical CanonIR.

---

# Deterministic Emit Layer (`canon-projection`)

The emitter converts CanonIR into Rust source code.

### Hard Rule

The emitter is a **pure projection**.

It must never:

- infer missing semantics
- modify CanonIR
- inspect emitted text
- perform repairs

### Emission Inputs

The emitter reads only:

- CanonIR nodes
- CanonIR graphs
- CanonIR type information

### Emission Outputs

- Rust source files
- Cargo.toml

---

# File Planning

File layout is determined before emission begins.

Algorithm:

```
build_plan(ir)

for each module node:
    assign file

for each item:
    assign deterministic order
```

Ordering key:

```
(module_depth, node_id)
```

This ensures deterministic output across runs.

---

# Deterministic Emit Ordering

Items inside each file are emitted in stable order.

Ordering rule:

```
1. Modules
2. Types
3. Traits
4. Impl blocks
5. Functions
6. Constants
```

Tie-breaking uses NodeId ordering.

### Property

Identical CanonIR always produces identical emitted source.

---

# Rust Construct Emission Rules

Correct emission requires mapping CanonIR structures to valid Rust syntax.

### Closure Emission

Closures must emit as valid Rust lambda syntax:

```
|arg| expr
```

Invalid debug representations (e.g. `closure@file.rs`) must never appear in emitted code.

---

### Iterator Adaptors

Iterator chains must emit in method form:

```
iter.map(|x| ...)
```

Not:

```
Iterator::map(iter, ...)
```

unless required by canonical form.

---

### Option Combinators

Correct form:

```
opt.map(|x| expr)
```

The second argument must always be a closure.

---

### Return Value Emission

Functions return the value of their final expression.

Intermediate variables must respect function signature types.

Invariant:

```
FnSig.ret == emitted return type
```

---

# Structural Emission Invariants

For emitted code to compile, the following must hold:

1. All referenced variables exist in scope.
2. Iterator closures have correct parameter types.
3. Option combinators receive closures.
4. Return values match function signatures.
5. Emitted tokens correspond to valid Rust syntax.

If any invariant fails, CanonIR is incomplete or projection logic is incorrect.

---

# Orchestration Layer

The orchestration binary runs the full pipeline.

Pipeline execution order:

```
run_capture()
run_analyzer()
run_emit()
```

The orchestration layer must not:

- mutate CanonIR
- interpret emitted text
- perform repair logic

Its role is strictly execution coordination.

---

# Determinism Guarantees

The system guarantees deterministic builds when:

- CanonIR structure is identical
- solver chain order is identical
- emission ordering is stable

Sources of nondeterminism eliminated:

- hash iteration
- unstable graph traversal
- thread scheduling effects

All iteration uses stable ordering.

---

# Validation Strategy

A successful orchestration run must satisfy:

```
cargo run --bin orchestration -- --all
```

Result:

```
zero build errors
```

If the build fails:

- CanonIR is incomplete
OR
- projection logic violates emission invariants.

---

# Summary

The corrected architecture enforces:

- CanonIR as the only truth boundary
- deterministic graph derivation
- solver-based semantic closure
- pure projection emission
- stable file and item ordering

These guarantees ensure that a valid CanonIR deterministically produces compilable Rust source code.