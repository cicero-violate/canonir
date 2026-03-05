# Structural Analysis of Emitted Rust Sources (emit/repomap/src)

## Files Inspected
- extractor.rs
- main.rs
- repomap.rs
- symbol.rs

## Global Structural Problems

### 1. Placeholder Lowering Failures
Multiple locations contain:
```
panic!("canon missing assignment lowering")
```
These indicate the Canon lowering stage failed to emit valid Rust expressions for assignments, closures, or mapping operations.

Impact:
- Prevents compilation
- Breaks control flow
- Leaves placeholder panic expressions in executable code

Observed in:
- main.rs
- extractor.rs
- symbol.rs

---

### 2. Invalid Generic Placeholder Types
Example:
```
Option::<T>::unwrap_or_else(...)
```
`T` is undefined and cannot compile.

Likely intended form:
```
Option<String>
```

---

### 3. Invalid Closure Placeholders
Generated expressions such as:
```
ZeroSized: {closure@src/main.rs:10:57: 10:59}
```
are not valid Rust syntax.

These originate from IR closure captures that were not properly emitted.

---

### 4. Iterator Pipelines With Missing Bodies
Examples:
```
Iterator::filter_map(_v6, ZeroSized: {closure@...})
Iterator::filter(_v5, ZeroSized: {closure@...})
```

Closures are missing, leaving unusable iterator chains.

---

### 5. Invalid `match` Projections
Code patterns such as:
```
match self { Symbol::Struct { ... } => ..., _ => panic!(...) }
```
are duplicated sequentially, overwriting previous results.

The generated logic repeatedly assigns to the same variable rather than using a single exhaustive match.

---

### 6. Incorrect Temporary Variable Typing
Examples:
```
let mut _v2: () = panic!(...);
```
This forces a unit type while panic! returns `!`, producing type mismatches.

---

### 7. Invalid UTF8 Text Helper
Example in `node_text`:
```
let mut __ret = ();
__ret = _v3.unwrap_or("");
```
Return type mismatch (`()` vs `&str`).

---

### 8. Dead / Unreachable Control Flow
Example:
```
return __ret;
return __ret;
```

Duplicated returns indicate emitter incorrectly replicated terminal statements.

---

### 9. Missing Loop Bodies
Many iterator loops follow this pattern:

```
let mut iter = _v6;
let mut _v12 = &mut iter;
let mut _v11 = _v12.next();
```

But no loop exists, meaning iteration logic was never emitted.

---

### 10. Invalid Format Construction
Binary string templates like:
```
b"\t  struct ..."
```
combined with `Default::default()` arguments suggest a failed formatting lowering step.

---

## Root Cause Summary

The emitted Rust sources show consistent evidence of **incomplete Canon IR lowering**, specifically:

1. Assignment lowering not implemented
2. Closure emission missing
3. Iterator loop expansion missing
4. Match projection incorrectly duplicated
5. Formatting macro lowering incomplete

These issues originate in the **projection → emission stage** of the pipeline.

Likely affected components:

- canon-projection/src/emit/items.rs
- canon-projection/src/emit/impls.rs
- canon-projection/src/emit/file.rs
- canon-projection/src/emit/fmt.rs

---

## Priority Fixes

1. Implement assignment lowering
2. Implement closure emission
3. Implement iterator loop expansion
4. Replace projection-style matches with proper Rust matches
5. Replace binary format templates with `format!` emission

---

## Expected Outcome After Fix

Once emission is corrected, generated files should:

- compile with `cargo check`
- contain valid closures
- contain valid iterator loops
- avoid placeholder panic statements
