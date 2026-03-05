# Canon Orchestration Pipeline Execution Model

## Overview
The orchestration pipeline converts Canon IR into emitted Rust crates located under:

```
test_projects/test_rust_projects/emit/<crate>/src
```

The pipeline has four primary stages:

1. Canon IR Generation
2. Capture Stage
3. Projection Stage
4. Filesystem Emission

---

## 1. Canon IR Generation

Source Rust projects are parsed and transformed into Canon IR. The IR represents a normalized, language-agnostic intermediate structure describing:

- modules
- items (structs, enums, traits, functions, impls)
- symbol relationships

This IR becomes the authoritative representation used by later pipeline stages.

---

## 2. Capture Stage

The capture stage records the Canon IR graph and prepares it for projection. Responsibilities include:

- normalizing module paths
- resolving symbol ownership
- collecting item definitions
- building dependency graphs

The output of capture is a complete Canon IR graph describing the crate structure.

---

## 3. Projection Stage

Projection converts Canon IR nodes into Rust code structures.

Key components:

- `canon-projection/src/emit/items.rs`
- `canon-projection/src/emit/impls.rs`
- `canon-projection/src/emit/file.rs`
- `canon-projection/src/emit/fmt.rs`

Responsibilities:

- translate IR items into Rust syntax
- assemble module contents
- render structs, enums, traits, impl blocks
- normalize module paths

Projection produces in-memory representations of Rust source files.

---

## 4. Filesystem Emission

The emission stage writes projected Rust modules to disk.

Implemented primarily in:

```
canon-projection/src/emit_pipeline.rs
canon-projection/src/emit/fs_layout.rs
```

Responsibilities:

- map module paths (e.g. `a::b::c`) to filesystem paths
- generate `src/lib.rs` or crate root files
- write module files under `src/`

Example mapping:

```
module path: a::b::c
filesystem: emit/<crate>/src/a/b/c.rs
```

---

## Resulting Structure

After emission, each crate is written to:

```
emit/<crate>/
  Cargo.toml
  src/
    lib.rs or main.rs
    module files
```

Example:

```
test_projects/test_rust_projects/emit/repomap/src
  main.rs
  extractor.rs
  repomap.rs
  symbol.rs
```

These emitted crates are then compiled with Cargo to verify correctness.

---

## Validation Step

The orchestration pipeline concludes by building the emitted crate:

```
cd emit/<crate>
CARGO_NET_OFFLINE=true cargo build
```

Any compile failures indicate issues in projection or emission logic.
