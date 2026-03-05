# Structural Problems Detected in emit/repomap/src

## Overview
Inspection of the emitted Rust crate `emit/repomap/src` reveals multiple structural issues produced by the Canon emission pipeline. These issues prevent the generated crate from compiling and indicate incorrect lowering or incomplete projection during emission.

---

## 1. Unreachable / Duplicate Returns
Several functions contain duplicated `return` statements immediately following each other.

Example (`repomap.rs`):

```
return __ret;
return __ret;
```

This pattern appears repeatedly and indicates faulty control‑flow lowering during IR projection.

Impact:
- Invalid or unreachable code
- Indicates broken canonical lowering stage

---

## 2. Placeholder Lowering Panics
Generated code contains explicit placeholder panics such as:

```
panic!("canon missing assignment lowering")
```

These appear in multiple files (`extractor.rs`, `symbol.rs`).

Impact:
- Confirms missing lowering rules in the Canon projection pipeline
- Emitted code cannot execute successfully

---

## 3. Invalid Temporary Variable Patterns
Generated functions contain many compiler‑style temporaries (`_v1`, `_v2`, `_v31`, etc.) without meaningful semantics.

Example:

```
let mut _v32 = false;
let mut _v31 = false;
```

Many are unused.

Impact:
- Indicates raw SSA‑like intermediate lowering was emitted directly
- Final Rust emission stage failed to simplify temporaries

---

## 4. Incomplete Iterator Loops
Multiple loops are lowered only partially:

```
let mut iter = _v6;
let mut _v12 = &mut iter;
let mut _v11 = _v12.next();
```

No loop body or iteration structure follows.

Impact:
- Iterator traversal logic missing
- IR loop constructs were not properly expanded

---

## 5. Broken Pattern Matching Projections
Pattern matches are generated with panic branches for all mismatches.

Example:

```
match self {
  crate::symbol::Symbol::Struct { ... } => ...,
  _ => panic!("canon downcast projection mismatch")
}
```

Impact:
- Projection lowering assumes impossible states
- Pattern coverage incomplete

---

## 6. Invalid Byte‑Format Rendering Blocks
Formatting code in `symbol.rs` uses opaque byte sequences such as:

```
b"\t  struct \xc0\x08  (line \xc0\x01)\x00"
```

These appear to represent internal formatting templates that were never converted into valid Rust formatting calls.

Impact:
- Rendering logic cannot compile
- Indicates final formatting emission stage missing

---

## 7. Missing Parser Invocation
In `extractor.rs`, the parser invocation is broken:

```
let mut _v12: () = panic!("canon missing assignment lowering");
let mut _v10 = ::core::default::Default::default();
let mut tree = _v10.expect(_v13);
```

The actual `parser.parse()` call is missing.

Impact:
- AST construction cannot occur
- Extraction pipeline unusable

---

## 8. Structurally Valid Module Layout
Despite internal problems, the module structure itself is valid:

```
lib.rs
main.rs
extractor.rs
repomap.rs
symbol.rs
```

And module declarations are correct:

```
mod extractor;
mod repomap;
mod symbol;
```

Thus filesystem layout and module wiring are **not the primary issue**.

---

## Root Cause
The errors strongly indicate that the Canon projection pipeline emitted **pre‑lowered IR scaffolding instead of finalized Rust code**.

The missing stages likely include:

1. Assignment lowering
2. Loop reconstruction
3. Pattern projection completion
4. Temporary elimination
5. Format template expansion

---

## Conclusion
The emitted crate structure is correct, but the emitted code corresponds to an **incomplete lowering phase** of the Canon IR pipeline. Fixes must occur in the projection/emission stages responsible for:

- control flow lowering
- assignment lowering
- formatting emission
- iterator loop reconstruction

Relevant emission components likely responsible:

- `canon-projection/src/emit/items.rs`
- `canon-projection/src/emit/impls.rs`
- `canon-projection/src/emit/file.rs`
- `canon-projection/src/emit/fmt.rs`
