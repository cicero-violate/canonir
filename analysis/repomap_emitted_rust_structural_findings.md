# Structural Findings: emit/repomap/src

## Overview
Inspection of emitted Rust sources reveals systematic lowering failures and malformed constructs originating from the Canon projection / emit pipeline. The generated Rust code is structurally invalid in multiple locations and cannot compile.

## Major Structural Problems

### 1. Missing Assignment Lowering
Numerous placeholders remain:

panic!("canon missing assignment lowering")

These appear where expressions should exist (e.g., closure bodies, map functions, parser invocations). The emit stage failed to translate IR assignments into Rust expressions.

Affected files:
- extractor.rs
- symbol.rs

### 2. Invalid Match Downcast Projections
Generated code repeatedly emits patterns like:

match self { ... => *__canon_f0 }

with fallback arms:

panic!("canon downcast projection mismatch")

These are artifacts of incomplete enum destructuring lowering.

### 3. Invalid Option Mapping
Examples:

_v4.map(_v6)

Where `_v6` is defined as:

let mut _v6: () = panic!("canon missing assignment lowering");

This results in invalid closures and type mismatches.

### 4. Dead Iterator Lowering
Loops such as:

let mut _v11 = _v12.next();

are emitted without control flow (no loop body), meaning the iteration logic was never projected.

### 5. Placeholder Formatting System
Binary formatting tables appear:

b"\x03fn \xc0\xc0\xc0\x00"

but argument expansion is missing, indicating the format emission phase was partially executed but argument lowering failed.

## Root Cause
The Canon projection pipeline is emitting IR fragments without completing the following lowering passes:

1. Assignment lowering
2. Pattern projection for enum destructuring
3. Closure lowering
4. Iterator loop lowering
5. Formatting macro argument binding

These passes appear to be implemented but not executed or not wired correctly in the emit pipeline.

## Required Fix Areas
The following components likely require repair:

- canon-projection/src/emit/items.rs
- canon-projection/src/emit/impls.rs
- canon-projection/src/emit/file.rs
- canon-projection/src/emit/fmt.rs

## Impact
Current emitted crates under:

```
test_projects/test_rust_projects/emit/repomap/src
```

are structurally invalid and cannot pass `cargo check` until the lowering stages are repaired.
