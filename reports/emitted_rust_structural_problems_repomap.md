# Structural Problems Detected in Emitted Rust Sources (repomap)

Location analyzed:
`test_projects/test_rust_projects/emit/repomap/src`

Files:
- main.rs
- extractor.rs
- repomap.rs
- symbol.rs

## Critical Structural Issues

### 1. Placeholder Panics Emitted Instead of Valid Rust
Multiple locations contain generated code such as:

```
panic!("canon missing assignment lowering")
```

These appear in:
- main.rs
- extractor.rs
- symbol.rs

This indicates the Canon projection pipeline failed to lower assignment or expression nodes into valid Rust code.

Impact:
- Code compiles but panics immediately
- Indicates incomplete IR lowering

---

### 2. Invalid Generic Type Usage
Example in `main.rs`:

```
let mut target = std::option::Option::<T>::unwrap_or_else(...)
```

`T` is undefined and no generic context exists.

Impact:
- Rust compiler error: unknown type `T`

Root cause:
Type parameters not substituted during IR → Rust emission.

---

### 3. Invalid Temporary Variable Graph
Large sequences of temporaries:

```
let mut _v4 = ...
let mut _v3 = ...
let mut _v2 = ...
```

Problems:
- values often unused
- some references created before assignment
- many expressions lowered into meaningless temporaries

Impact:
- dead code
- invalid ownership flows
- borrow checker errors

---

### 4. Invalid `ZeroSized` Closure Artifacts
Example:

```
ZeroSized: {closure@src/main.rs:10:57: 10:59}
```

These are MIR-style artifacts leaking into emitted Rust source.

Impact:
- Not valid Rust syntax

Root cause:
Lowering step emitting MIR debug representation instead of Rust AST.

---

### 5. Broken Iterator Lowering
Example pattern:

```
let mut _v10 = &mut iter;
let mut _v9 = _v10.next();
```

Generated loops terminate immediately and do not process iterator contents.

Impact:
- semantic logic missing
- loops never executed

Root cause:
Iterator lowering incomplete.

---

### 6. Invalid Match Projections
Example:

```
match self {
  Symbol::TypeAlias { name: __canon_f0, line: __canon_f1 } => *__canon_f1,
  _ => panic!("canon downcast projection mismatch")
}
```

Problems:
- repeated matches overriding return values
- unnecessary panics

Impact:
- incorrect logic
- unreachable code patterns

---

### 7. Multiple Redundant Returns
Example:

```
return __ret;
return __ret;
```

Appears across multiple functions.

Impact:
- indicates broken control flow lowering

---

## Summary of Root Causes
The emitted code strongly indicates failures in the **Canon projection lowering stage**, specifically:

1. Assignment lowering
2. Closure lowering
3. Iterator lowering
4. Pattern match projection lowering
5. Type parameter substitution

The projection pipeline is emitting **intermediate IR artifacts** rather than valid Rust syntax.

## Recommended Fix Points
Primary files to repair:

- canon-projection/src/emit_pipeline.rs
- canon-projection/src/emit/items.rs
- canon-projection/src/emit/impls.rs
- canon-projection/src/helpers/module_normalization.rs

Key tasks:

1. Replace MIR-like temporary graph emission with structured Rust AST emission
2. Implement assignment lowering pass
3. Remove `ZeroSized` closure placeholders
4. Implement proper iterator loop lowering
5. Ensure type parameters are resolved before emission
