# Corrected Orchestration Emit Pipeline Architecture

## Overview

This document describes the corrected architecture for the orchestration emit pipeline responsible for producing Rust source files from Canon IR. The objective is to ensure that running:

```
cargo run --bin orchestration -- --all
```

produces emitted sources that compile with **zero build errors**.

The architecture enforces deterministic projection, structural invariants, and dependency-consistent emit ordering.

---

# Pipeline Stages

The corrected pipeline consists of deterministic stages:

1. Capture → Canon IR
2. IR Validation
3. Dependency Graph Construction
4. Emit Ordering
5. Module Tree Normalization
6. Projection Emit
7. Filesystem Output

Each stage is pure and deterministic except for the final filesystem write.

---

# Canon IR

Canon IR represents the structural model of the codebase.

Core entities:

- Modules
- Items (structs, enums, functions, traits)
- Use statements
- Visibility

IR must satisfy invariants before projection occurs.

---

# IR Structural Invariants

The following invariants must hold before emit:

1. **Unique Item Identifiers**

No duplicate item identifiers exist within a module.

2. **Valid Module Hierarchy**

Modules form a valid tree.

3. **Resolved Symbol Paths**

All `use` paths resolve to valid items.

4. **Valid Visibility Rules**

Visibility does not expose private parents.

5. **Dependency Consistency**

Items referencing other items must appear in the dependency graph.

---

# Dependency Graph

A dependency graph is constructed from IR items.

Nodes:

- IR items

Edges represent:

- Type references
- Module membership
- Use statements

The graph must be **acyclic**.

---

# Emit Ordering

A deterministic topological ordering is computed:

```
ordered_items = topo_sort(dependency_graph)
```

Ordering guarantees:

- referenced items appear before dependents
- modules appear before contained items

---

# Module Tree Normalization

Before emit, module layout is normalized.

Rules:

- filesystem path matches module path
- mod declarations align with directory structure
- root module declared in lib.rs or mod.rs

Example:

```
crate
 └── repomap
     └── src
         ├── lib.rs
         └── modules...
```

---

# Projection Emit

The projection stage converts IR items into Rust AST text.

Projection responsibilities:

- generate struct/enum/function definitions
- emit use statements
- enforce visibility

Projection must not introduce new symbols.

---

# Rust Structural Validation

Before writing files the system runs:

```
validate_emitted_rust_structure(ir_items)
```

This detects:

- unresolved imports
- duplicate definitions
- invalid module references
- missing module declarations

---

# Filesystem Emit

The final stage writes Rust source files.

Rules:

- deterministic file paths
- idempotent generation
- no duplicate files

Only this stage performs side effects.

---

# Determinism Guarantees

The pipeline guarantees:

- stable emit ordering
- deterministic module layout
- invariant-validated IR

Therefore emitted Rust always compiles provided the IR satisfies invariants.

---

# Failure Handling

If invariants fail:

1. Emit is aborted
2. Validation report is produced
3. No files are written

---

# Summary

The corrected architecture ensures that emitted Rust code is structurally valid, dependency-consistent, and deterministically ordered. By enforcing IR invariants and separating pure analysis stages from filesystem emission, the orchestration pipeline reliably produces compilable Rust source files.
