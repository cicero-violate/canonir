# Orchestration Pipeline Execution Model

This document summarizes the execution model used by the orchestration pipeline that converts Canon IR into emitted Rust crates.

## Pipeline Overview

The orchestration system converts Canon IR into a Rust crate under:

```
test_projects/test_rust_projects/emit/<crate>/src
```

The pipeline is deterministic and proceeds through four major stages:

1. IR Generation
2. Capture Stage
3. Projection Stage
4. Filesystem Emission

Each stage produces artifacts used by the next stage.

---

# Stage 1 — IR Generation

The orchestration process begins by constructing Canon Intermediate Representation (IR) items.

Characteristics:

- Canon IR represents Rust program structure.
- IR items include modules, structs, functions, enums, and impl blocks.
- Structural invariants are validated before projection.

Validation occurs through:

```
validate_emitted_rust_structure(&items)
```

If validation fails the pipeline aborts.

---

# Stage 2 — Capture Stage

The capture stage gathers structural information about the program representation.

Responsibilities:

- Build an internal representation of modules
- Normalize module hierarchy
- Ensure deterministic ordering of IR items

Normalization is performed using:

```
normalize_module_tree(&items)
```

This step determines which Rust modules must be emitted.

---

# Stage 3 — Projection Stage

The projection stage converts Canon IR items into emission blocks that correspond to Rust source code.

Emission planning occurs through:

```
compute_emit_plan(&items)
```

The result is a deterministic mapping:

```
module -> Vec<String>
```

Where each string block represents Rust code to write into the module file.

The projection stage guarantees:

- Deterministic module ordering
- Deterministic item ordering
- Stable emission plan

---

# Stage 4 — Filesystem Emission

The emission stage materializes Rust files on disk.

Module paths are converted into filesystem paths using:

```
module_to_file(crate_name, module)
```

Mapping example:

```
a::b::c  -> emit/<crate>/src/a/b/c.rs
root     -> emit/<crate>/src/lib.rs
```

Files are written using:

```
write_module_file(path, blocks)
```

Emission guarantees:

- Parent directories are created
- Blocks are written deterministically
- Each block is newline separated

The final emitted crate structure becomes:

```
emit/<crate>/
  src/
    lib.rs
    <module>.rs
```

---

# Example Emitted Layout (repomap)

Observed filesystem layout:

```
test_projects/test_rust_projects/emit/repomap/src
├── extractor.rs
├── lib.rs
├── main.rs
├── repomap.rs
└── symbol.rs
```

The module tree inferred from `lib.rs`:

```
crate
 ├─ extractor
 ├─ repomap
 └─ symbol
```

`main.rs` acts as the binary entrypoint.

---

# Deterministic Emission Guarantees

The pipeline enforces deterministic output via:

- IR validation
- module tree normalization
- ordered emission plan
- deterministic filesystem writes

These guarantees ensure reproducible builds.

---

# Summary

The orchestration pipeline converts Canon IR into Rust crates through four deterministic stages:

1. IR validation
2. Module normalization
3. Emission planning
4. Filesystem materialization

The resulting crate structure is emitted under:

```
emit/<crate>/src
```

and represents the canonical Rust projection of the Canon IR program.
