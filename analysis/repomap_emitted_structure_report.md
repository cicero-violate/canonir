# Emitted Rust Structural Issues (repomap)

## Observed Files
- src/main.rs
- src/repomap.rs
- src/extractor.rs
- src/symbol.rs

No `lib.rs` exists, but `main.rs` declares modules:
```
mod extractor;
mod repomap;
mod symbol;
```
This is valid for a binary crate, assuming the crate root is `main.rs`.

---

# Critical Structural Problems

## 1. Unlowered Canon Placeholders
Multiple emitted expressions contain placeholders such as:

- `panic!("canon missing assignment lowering")`
- `panic!("canon unreachable")`

These appear in:
- main.rs
- extractor.rs
- symbol.rs

This indicates the Canon projection stage failed to lower assignment expressions and control‑flow joins.

---

## 2. Invalid Type Placeholder
Example in `main.rs`:
```
Option::<T>::unwrap_or_else
```
`T` is not defined anywhere in the emitted crate. The generic parameter was not resolved during projection.

---

## 3. ZeroSized Closure Placeholders
Examples:
```
ZeroSized: {closure@src/main.rs:10:57: 10:59}
```

and

```
Iterator::map(..., ZeroSized: {closure@...})
```

These are MIR‑like artifacts and are not valid Rust syntax.
Closures must be emitted as `|args| expr` or `|args| { block }`.

---

## 4. Duplicate Return Statements
Several functions contain repeated returns:

```
return __ret;
return __ret;
```

Appears in:
- repomap.rs
- extractor.rs

While syntactically valid, this is a strong indicator that control‑flow lowering is incomplete.

---

## 5. Invalid Variable Introduction
Generated code repeatedly emits temporary variables like:

```
let mut _v2
let mut _v3
let mut _v4
```

These variables frequently:
- are unused
- hold invalid placeholder expressions
- are part of incomplete lowering passes

This suggests the Canon IR still reflects MIR‑style SSA temporaries that were not reconstructed into idiomatic Rust.

---

## 6. Invalid Pattern Downcasts
Example:

```
match self {
  crate::symbol::Symbol::TypeAlias { name: __canon_f0, line: __canon_f1 } => *__canon_f1,
  _ => panic!("canon downcast projection mismatch")
}
```

These patterns are repeated multiple times in a single function to simulate sequential matches. In Rust this should be a single match expression.

---

## 7. Invalid Byte‑String Formatting Artifacts
Example:

```
b"\x03fn \xc0\xc0\xc0\x00"
```

These appear to be placeholders for formatting templates that were not expanded.

---

# Root Cause Categories

### A. Assignment Lowering Missing
Evidence:
```
panic!("canon missing assignment lowering")
```

Required stage missing in projection.

---

### B. Closure Reconstruction Missing
Evidence:
```
ZeroSized: {closure@...}
```

Closures were captured but not re‑emitted as Rust closures.

---

### C. Generic Type Resolution Missing
Evidence:
```
Option::<T>
```

Type parameter not resolved in CanonIR analysis.

---

### D. Control‑Flow Reconstruction Missing
Evidence:

- duplicated `return`
- sequential match rewrites

The emitter is serializing MIR control flow instead of reconstructing Rust blocks.

---

# Summary
The emitted crate structure (modules, imports) is mostly correct. However the **item bodies are not valid Rust** due to incomplete Canon projection stages:

Missing passes:

1. Assignment lowering
2. Closure reconstruction
3. Generic resolution
4. Control‑flow reconstruction
5. Format string emission

These failures originate in `canon_projection::project()` rather than the orchestration pipeline.
