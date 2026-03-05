# Emitted Rust Structural Problems — repomap

Location: test_projects/test_rust_projects/emit/repomap/src

## Files
- main.rs
- extractor.rs
- repomap.rs
- symbol.rs

## Detected Structural Issues

### 1. Invalid Generic Usage
`Option::<T>::unwrap_or_else` appears in main.rs but `T` is undefined.

### 2. Canon Placeholder Panics
Multiple occurrences of:
- `panic!("canon missing assignment lowering")`
- `panic!("canon unreachable")`
These indicate incomplete lowering in the emit pipeline.

### 3. Invalid Closure Placeholders
Constructs such as:
`ZeroSized: {closure@src/main.rs:...}`
are emitted instead of valid Rust closures.

### 4. Iterator Skeletons Without Bodies
Patterns like:
```
let mut _v11 = _v12.next();
```
appear without loops or match handling.

### 5. Unresolved Temporary Variables
Temporary SSA-like variables (`_v1`, `_v2`, etc.) remain in final code, indicating IR was emitted without cleanup.

### 6. Incomplete Match Projections
Generated code such as:
```
match self { ... => *__canon_f0 }
```
is emitted with strict pattern projections and panics on other variants, producing invalid or unsafe logic.

### 7. Formatting Macro Artifacts
Byte-string formatting templates such as:
```
b"\t  struct \xc0\x08  (line \xc0\x01)\x00"
```
appear without corresponding formatting expansion.

### 8. Uninitialized Variables
Variables like `child` are referenced before definition in extractor.rs helper functions.

### 9. Incomplete Tree-sitter Parse Invocation
Parser call result is not assigned before `.expect()` usage, causing invalid code generation.

### 10. Missing Control Flow Reconstruction
Large portions of code show flattened IR without reconstructed Rust constructs (loops, conditionals, matches).

## Summary
The emitted crate structure is correct at the module level, but the code generation stage failed to reconstruct high-level Rust constructs from Canon IR. The primary issues originate from incomplete lowering passes and formatting projection stages.
