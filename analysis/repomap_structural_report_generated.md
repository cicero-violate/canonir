# Structural Problems Detected in Emitted Rust (emit/repomap/src)

## Overview
Inspection of the emitted Rust files reveals systematic structural corruption produced by the Canon projection / lowering pipeline. The problems are not typical Rust authoring mistakes but artifacts of incomplete IR lowering and formatter substitution.

Files inspected:
- lib.rs
- repomap.rs
- extractor.rs
- symbol.rs

## 1. Unlowered IR placeholders
Multiple locations contain placeholder constructs that are not valid Rust:

Examples:
- `panic!("canon missing assignment lowering")`
- `ZeroSized: {closure@src/...}`
- raw byte format templates such as:
  `b"\x03fn \xc0\xc0\xc0\x00"`

These indicate that IR lowering stages (assignment lowering, closure lowering, formatting lowering) did not run or failed.

## 2. Invalid iterator lowering
Example from `build_repomap`:

```
let mut _v5 = std::iter::Iterator::filter_map(_v6, ZeroSized: {closure@src/...});
```

Problems:
- Closure body not emitted
- `ZeroSized` placeholder type inserted
- Closure syntax missing

Correct Rust should resemble:

```
let iter = WalkDir::new(root_dir)
    .into_iter()
    .filter_map(|e| e.ok())
    .filter(|e| e.file_type().is_file());
```

## 3. Dead variables and unused temporaries
The emitter generates large numbers of temporary variables (`_v1`, `_v2`, etc.) that are never used.

Example:

```
let mut _v3 = (4_usize == 0_usize);
```

These appear to originate from SSA-style IR temporaries that were not eliminated during projection.

## 4. Duplicate return statements
Many functions contain repeated returns:

```
return __ret;
return __ret;
```

This indicates a failure in control‑flow simplification after lowering.

## 5. Match arm projection corruption
Example from `symbol.rs`:

```
match self { crate::symbol::Symbol::Struct { ... } => *__canon_f2, _ => panic!(...) }
```

Issues:
- Variant destructuring repeated sequentially
- Temporary reassignment of `__ret`
- Panic fallback inserted for every match arm

This is likely the result of incorrect enum projection lowering.

## 6. Formatting system corruption
Multiple formatting templates appear as raw byte arrays:

```
b"\t  struct \xc0\x08  (line \xc0\x01)\x00"
```

These are unresolved formatting IR tokens that should have been converted into `format!` invocations.

## 7. Parser construction failure
In `extract_symbols`:

```
let mut _v12: () = panic!("canon missing assignment lowering");
```

The actual parser invocation was not emitted. This prevents AST parsing entirely.

## Root Cause
The emitted code shows evidence that the following lowering stages did not execute or partially executed:

- Assignment lowering
- Closure lowering
- Formatting / template lowering
- Control‑flow simplification
- Temporary elimination

These failures originate upstream in the Canon projection pipeline rather than the Rust emitter itself.

## Required Fix Area
The issue most likely resides in:

```
canon-projection/src/emit/
```

particularly:

- emit/file.rs
- emit/fmt.rs
- emit/items.rs
- emit_pipeline.rs

The pipeline is emitting IR artifacts directly instead of fully lowered Rust syntax.
