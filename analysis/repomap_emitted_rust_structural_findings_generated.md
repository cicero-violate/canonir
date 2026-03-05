# Structural Findings for emit/repomap/src

## Module Layout
Observed modules:
- lib.rs
- main.rs
- repomap.rs
- extractor.rs
- symbol.rs

`lib.rs` declares:
```
mod extractor;
mod repomap;
mod symbol;
```

These correspond to files present in the directory, so the module tree itself is structurally valid.

---

# Critical Structural Problems

## 1. Invalid Closure Artifacts in repomap.rs
Example:
```
Iterator::filter_map(_v6, ZeroSized: {closure@src/repomap.rs:16:64: 16:67});
```
This is not valid Rust syntax. The generator emitted compiler MIR-style placeholder text instead of a closure expression.

Expected shape:
```
.filter_map(|entry| ... )
```

Impact:
- Immediate compilation failure

---

## 2. Unreachable Code / Duplicate Returns
Multiple functions contain duplicate returns:
```
return __ret;
return __ret;
```

Impact:
- Rust will emit unreachable-code warnings or errors depending on context

---

## 3. Panic Placeholders for Missing Lowering
Numerous locations contain:
```
panic!("canon missing assignment lowering")
```

These appear in:
- extractor.rs
- symbol.rs

Impact:
- Any runtime execution will immediately panic
- Indicates incomplete IR lowering

---

## 4. Invalid Variable Usage
Example in extractor.rs:
```
let mut _v12: () = panic!(...);
let mut _v10 = Default::default();
let mut tree = _v10.expect("parse failed");
```

This sequence is logically invalid because `_v10` is not a `Result` type.

Impact:
- Type mismatch compile errors

---

## 5. Undefined Variables
Example:
```
let mut _v21 = &child;
```

`child` is never defined in the function scope.

Impact:
- Hard compilation failure

---

## 6. Invalid Pattern Downcasts
Example:
```
match self {
  Symbol::TypeAlias { name: __canon_f0, line: __canon_f1 } => *__canon_f1,
  _ => panic!("canon downcast projection mismatch")
}
```

Functions repeatedly attempt mutually exclusive matches on the same value.

Impact:
- Many branches unreachable
- Indicates incorrect enum lowering

---

## 7. Invalid Format String Generation
Example:
```
let mut _v15 = b"\t  struct \xc0\x08  (line \xc0\x01)\x00";
```

Binary format templates are emitted without any formatter usage.

Impact:
- No valid string construction occurs

---

# Summary
The emitted crate is structurally unsound due to generator artifacts:

Major categories of failure:

1. MIR‑like placeholders (`ZeroSized: {closure@...}`)
2. Missing assignment lowering (`panic!("canon missing assignment lowering")`)
3. Undefined variables
4. Invalid enum projection logic
5. Non‑Rust formatting templates

The module tree itself is correct, but the function bodies are not valid Rust and cannot compile.

---

# Likely Root Cause
The Canon → Rust projection stage appears to emit partially lowered IR where:

- closures
- pattern matches
- formatting operations
- variable bindings

have not been fully converted into Rust AST constructs.

The failure is therefore located in the **projection emit stage**, particularly:

- `canon-projection/src/emit/items.rs`
- `canon-projection/src/emit/impls.rs`
- `canon-projection/src/emit/file.rs`
