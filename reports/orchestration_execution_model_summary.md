# Canon Orchestration Pipeline Execution Model

## Pipeline Overview
The Canon orchestration pipeline converts Canon IR into fully emitted Rust crates under:

```
test_projects/test_rust_projects/emit/<crate>/src
```

The execution model proceeds through four deterministic stages:

1. Canon IR Generation
2. Capture Stage
3. Projection Stage
4. Filesystem Emission

---

## 1. Canon IR Generation

Input Rust projects are parsed and transformed into Canon IR. This IR represents a normalized semantic structure describing:

- modules
- structs
- enums
- traits
- functions
- impl blocks
- type aliases

The IR graph becomes the canonical representation used by the projection pipeline.

---

## 2. Capture Stage

The capture stage collects and normalizes IR nodes produced from parsing.

Responsibilities include:

- module path normalization
- symbol extraction
- dependency relationships
- canonical item ownership

The output of this stage is a complete Canon IR graph describing the crate structure.

---

## 3. Projection Stage

The projection stage converts Canon IR nodes into Rust syntax structures.

Primary projection modules:

- `canon-projection/src/emit/items.rs`
- `canon-projection/src/emit/impls.rs`
- `canon-projection/src/emit/file.rs`
- `canon-projection/src/emit/fmt.rs`

Responsibilities:

- translate IR items into Rust syntax
- assemble module contents
- render structs, enums, traits, impl blocks
- format Rust code

Projection produces an in-memory representation of Rust source files mapped to module paths.

---

## 4. Filesystem Emission

The emission stage writes projected modules to disk.

Key implementation files:

- `canon-projection/src/emit_pipeline.rs`
- `canon-projection/src/emit/fs_layout.rs`

Module paths are mapped deterministically to filesystem paths.

Example:

```
module path: a::b::c
filesystem: emit/<crate>/src/a/b/c.rs
```

The crate layout produced:

```
emit/<crate>/
  Cargo.toml
  src/
    lib.rs or main.rs
    <module>.rs
```

Example emitted crate:

```
test_projects/test_rust_projects/emit/repomap/src
  main.rs
  extractor.rs
  repomap.rs
  symbol.rs
```

---

## Validation

After emission, the orchestration pipeline validates the generated crate using Cargo:

```
cd test_projects/test_rust_projects/emit/<crate>
CARGO_NET_OFFLINE=true cargo build
```

Compilation errors indicate issues in projection logic, IR lowering, or emission structure.
