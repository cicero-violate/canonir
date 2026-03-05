# Structural Problems Detected in Emitted Rust (emit/repomap/src)

Inspection of emitted Rust files under:

`test_projects/test_rust_projects/emit/repomap/src`

reveals several deterministic structural problems introduced by the Canon emission pipeline.

---

## 1. Incomplete Iterator Lowering

Example from `repomap.rs`:

```
let mut _v7 = walkdir::WalkDir::new(root_dir);
let mut _v6 = _v7.into_iter();
let mut _v5 = std::iter::Iterator::filter_map(_v6, ZeroSized: {closure@src/repomap.rs:16:64: 16:67});
```

Problems:

- Rust syntax `ZeroSized: {closure@...}` is invalid
- closure lowering is incomplete
- iterator chain never executes

Result:

The function immediately returns an empty result.

---

## 2. Dead Iterators

Several loops are emitted as:

```
let mut iter = _v3;
let mut _v10 = &mut iter;
let mut _v9 = _v10.next();
```

But no loop body is generated.

Effect:

- iteration never processes elements
- emitted functions do nothing

---

## 3. Missing Assignment Lowering

Many locations contain explicit panic placeholders:

```
panic!("canon missing assignment lowering")
```

These appear in:

- extractor.rs
- symbol.rs

Meaning the Canon lowering stage never generated real assignment logic.

---

## 4. Invalid Pattern Matching

Example in `symbol.rs`:

```
match self { ... } => *__canon_f0
```

Problems:

- projections rely on temporary destructuring variables
- branches panic on mismatch
- enum handling logic is duplicated incorrectly

Result:

Generated methods like `line()` and `render()` are structurally incorrect.

---

## 5. Invalid Formatting Bytecode

Generated formatting sequences contain raw bytecode-like strings:

```
b"\t  struct \xc0\x08  (line \xc0\x01)\x00"
```

These appear to be partially lowered formatting templates that were never converted into valid Rust `format!` calls.

---

## 6. Unreachable Code

Many functions contain duplicated returns:

```
return __ret;
return __ret;
```

Everything after the first return is unreachable.

---

## 7. Missing Tree‑sitter Parse Invocation

In `extractor.rs`:

```
let mut _v12: () = panic!("canon missing assignment lowering");
let mut _v10 = ::core::default::Default::default();
let mut tree = _v10.expect("parse failed");
```

The actual call to `parser.parse(...)` is missing.

Therefore the parser never runs.

---

# Root Cause

These issues indicate that **projection lowering from Canon IR to Rust AST is incomplete**, specifically in:

- assignment lowering
- closure lowering
- iterator lowering
- match/enum projection
- formatting projection

Likely responsible modules:

- `canon-projection/src/emit/items.rs`
- `canon-projection/src/emit/impls.rs`
- `canon-projection/src/emit/file.rs`
- `canon-projection/src/emit/fmt.rs`

---

# Consequence

Because of these deterministic projection failures:

- emitted Rust crates under `emit/<crate>/src` are **structurally invalid**
- `cargo build` will fail
- emitted logic is non-functional even when syntactically valid

---

# Required Repairs

Projection pipeline must implement:

1. Proper closure lowering
2. Iterator loop emission
3. Assignment lowering
4. Enum match lowering
5. Formatting conversion to `format!`
6. Removal of placeholder panics

Until these are repaired, the Canon emission pipeline cannot produce valid Rust crates.