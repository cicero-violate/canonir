# Structural Analysis of Emitted Rust Sources (emit/repomap/src)

## Files Inspected
- main.rs
- repomap.rs
- extractor.rs
- symbol.rs

## Critical Structural Problems Identified

### 1. Missing Assignment Lowering
Multiple locations contain placeholders:
```
panic!("canon missing assignment lowering")
```
These appear where IR assignments or Option handling should have been lowered. This prevents valid Rust compilation and indicates incomplete lowering in the projection pipeline.

Observed in:
- main.rs
- extractor.rs
- symbol.rs

Impact:
- Immediate runtime panics
- Broken control flow
- Invalid temporary expressions

---

### 2. Invalid Generic Types
Example from `main.rs`:
```
Option::<T>::unwrap_or_else(...)
```
`T` is undefined and indicates the emitter failed to substitute the concrete type.

Impact:
- Compiler type resolution failure

---

### 3. Invalid Format String Lowering
Binary formatting fragments appear such as:
```
b"\xc0\x0b files  |  \xc0\x0e symbols"
```
These represent partially lowered formatting templates and should instead produce proper `format!` or `println!` calls.

Impact:
- Invalid formatting implementation
- Nonsensical byte string literals

---

### 4. Unreachable / Duplicate Returns
Several functions contain duplicated return paths:

Example:
```
return __ret;
return __ret;
```

Impact:
- Dead code
- Evidence of IR control‑flow reconstruction errors

---

### 5. Incomplete Iterator Lowering
Loops are emitted as iterator initialization followed by unused `next()` calls without loop constructs.

Example pattern:
```
let mut iter = ...;
let mut _v11 = iter.next();
```

Impact:
- Loop bodies never execute
- Missing `while let` / `for` constructs

---

### 6. Invalid Pattern Matching Expansion
Example from `symbol.rs`:

```
match self {
  Symbol::Struct { ... } => ...
  _ => panic!("canon downcast projection mismatch")
}
```

This pattern is repeatedly applied sequentially instead of a single exhaustive match, producing invalid control flow and unreachable branches.

Impact:
- Logic corruption
- Redundant destructuring

---

### 7. Temporary Variable Explosion
Generated temporaries (`_v1`, `_v2`, `_v3`, … `_v178`) dominate the code without meaningful scoping.

Impact:
- unreadable IR lowering
- increased probability of incorrect ordering

---

### 8. Module Structure
Module declarations are structurally correct:

```
mod extractor;
mod repomap;
mod symbol;
```

Corresponding files exist, so module resolution itself is valid.

However the internal code of modules is malformed due to the issues above.

---

## Root Cause Summary
The projection pipeline is failing in several IR‑to‑Rust lowering stages:

1. Assignment lowering
2. Pattern‑match reconstruction
3. Loop reconstruction
4. Formatting macro lowering
5. Generic type substitution

These failures lead to structurally invalid Rust source even though the filesystem module layout is correct.

---

## Recommended Pipeline Fixes

1. Implement **assignment lowering kernel** replacing placeholder panics.
2. Convert iterator‑next sequences into `while let` or `for` loops.
3. Replace byte‑encoded format templates with `format!` / `println!` macros.
4. Consolidate sequential pattern matches into a single `match` block.
5. Perform concrete type substitution during IR emission.
6. Run structural validation before filesystem emission.

---

## Conclusion
The emitted crate structure is syntactically organized but semantically invalid. The majority of issues originate from incomplete IR lowering rather than module resolution errors.
