# Canon Orchestration Execution Model (Generated Summary)

## Overview

The orchestration pipeline converts Canon IR into emitted Rust crates located under:

```
test_projects/test_rust_projects/emit/<crate>/src
```

The process is deterministic and proceeds through four primary stages:

1. Canon IR Generation
2. Capture Stage
3. Projection Stage
4. Filesystem Emission

Each stage produces artifacts consumed by the next stage to ensure stable Rust source emission.

---

# Stage 1 — Canon IR Generation

The pipeline begins by producing Canon Intermediate Representation (IR) items representing Rust program structure.

Typical IR elements include:

- modules
- structs
- enums
- functions
- impl blocks

Before emission, structural validation occurs:

```
validate_emitted_rust_structure(&items)
```

If structural invariants fail, emission is aborted.

---

# Stage 2 — Capture / Module Normalization

The capture stage derives the module hierarchy from IR items and normalizes it into a canonical module tree.

Normalization is performed via:

```
normalize_module_tree(&items)
```

Responsibilities of this stage:

- Determine crate module structure
- Ensure deterministic ordering
- Guarantee valid Rust module topology

---

# Stage 3 — Projection / Emission Planning

The projection stage transforms IR items into emission blocks representing Rust source fragments.

This stage computes a deterministic emission plan:

```
compute_emit_plan(&items)
```

Result:

```
module -> Vec<String>
```

Each module receives ordered Rust code blocks that will later be written to disk.

Properties:

- deterministic item ordering
- deterministic module ordering
- stable reproducible output

---

# Stage 4 — Filesystem Emission

The emission stage converts module paths into filesystem paths and writes Rust files.

Module-to-file mapping:

```
a::b::c -> emit/<crate>/src/a/b/c.rs
root    -> emit/<crate>/src/lib.rs
```

Implementation:

```
module_to_file(crate_name, module)
write_module_file(path, blocks)
```

Emission guarantees:

- directories are created automatically
- blocks are written deterministically
- files are newline separated

---

# Resulting Crate Layout

Example emitted crate:

```
emit/<crate>/
  src/
    lib.rs
    module.rs
    submodule.rs
```

Example observed for `repomap`:

```
emit/repomap/src
├── extractor.rs
├── lib.rs
├── main.rs
├── repomap.rs
└── symbol.rs
```

---

# Determinism Guarantees

The orchestration pipeline ensures reproducible emission through:

- IR structural validation
- normalized module hierarchy
- deterministic emission planning
- ordered filesystem writes

These guarantees allow Canon IR to produce stable Rust crate projections suitable for compilation and further analysis.
