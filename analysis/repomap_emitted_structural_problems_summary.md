# Structural Problems Detected in Emitted Rust (emit/repomap/src)

Inspection of emitted files under `test_projects/test_rust_projects/emit/repomap/src` reveals multiple deterministic structural failures introduced by the Canon emission pipeline.

---

## 1. Missing Assignment Lowering

Multiple locations emit explicit panics:

```
panic!("canon missing assignment lowering")
```

These appear in:

- `extractor.rs`
- `symbol.rs`

This indicates that Canon IR assignment expressions were never lowered into Rust expressions during projection.

Impact:

- Parser initialization fails
- Tree‑sitter parsing never occurs
- Symbol extraction cannot run

Example:

```
let mut _v12: () = panic!("canon missing assignment lowering");
```

---

## 2. Unreachable or Dead Control Flow

Several functions return immediately without executing the intended logic.

Example (`repomap.rs`):

```
let mut __ret = result;
return __ret;
return __ret;
```

The iterator traversal logic never runs.

Functions affected:

- `build_repomap`
- `render_repomap`

Result: these functions produce empty results regardless of input.

---

## 3. Invalid Closure Placeholders

Iterator chains contain invalid placeholder constructs:

```
ZeroSized: {closure@src/repomap.rs:16:64: 16:67}
```

These are MIR artifacts that were not properly lowered to Rust closures.

Impact:

- Generated Rust is syntactically invalid
- Iterator adapters cannot compile

---

## 4. Incorrect Pattern Projection

Generated match expressions assume specific enum variants and panic otherwise.

Example:

```
match self {
  Symbol::TypeAlias { ... } => ...,
  _ => panic!("canon downcast projection mismatch")
}
```

Multiple sequential matches overwrite `__ret`, producing meaningless control flow.

Impact:

- Invalid logic
- Potential runtime panics
- Broken enum handling

---

## 5. Uninitialized Formatting Pipeline

The `render()` implementation in `symbol.rs` contains incomplete formatting operations:

```
let mut _v25 = ::core::default::Default::default();
let mut _v30 = b"\t  struct ...";
```

These correspond to partially emitted formatting instructions.

Impact:

- Rendered output is always default values
- Symbol rendering cannot produce usable text

---

## 6. Undefined Variables in Generated Loops

Several helper functions reference variables never defined in the scope.

Example:

```
let mut _v21 = &child;
```

`child` is not declared in the surrounding code.

Affected functions:

- `collect_struct_fields`
- `collect_methods`
- `collect_enum_variants`

Impact:

- Compilation failure

---

## 7. Stubbed Iteration Logic

Many loops follow this pattern:

```
let mut iter = ...;
let mut _v12 = &mut iter;
let mut _v11 = _v12.next();
```

But no loop or match consumes the iterator.

Impact:

- Iteration logic never executes
- Collections remain empty

---

## 8. Redundant Return Statements

Multiple functions contain duplicated returns:

```
return __ret;
return __ret;
```

Impact:

- Dead code
- Indicates faulty control‑flow reconstruction

---

# Root Cause Categories

Observed structural issues originate from failures in these Canon pipeline stages:

1. **MIR → CanonIR lowering**
2. **Closure reconstruction**
3. **Assignment expression projection**
4. **Iterator lowering**
5. **Pattern match reconstruction**
6. **Formatting macro emission**

---

# Most Critical Pipeline Breakages

The following emission passes must be corrected:

- `canon-projection/src/emit/items.rs`
- `canon-projection/src/emit/impls.rs`
- `canon-projection/src/emit/file.rs`
- `canon-projection/src/emit/fmt.rs`

These components are responsible for producing the malformed Rust structures observed above.

---

# Summary

The emitted crate fails due to systematic projection failures rather than isolated syntax mistakes. The most severe problems are:

- unlowered assignments
- placeholder closures
- incomplete iterator lowering
- invalid enum pattern projection
- unfinished formatting emission

Until these projection stages are corrected, emitted Rust crates under `emit/<crate>/src` will remain structurally invalid and fail to compile.
