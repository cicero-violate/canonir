# Canon Orchestration Execution Model

## Pipeline Stages

1. **IR Generation**
   - Canon source is analyzed and transformed into Canon IR.

2. **Capture Stage**
   - Structural elements (modules, symbols, functions, impl blocks) are captured from IR.

3. **Projection Stage**
   - Canon IR is projected into Rust constructs using the projection layer.

4. **Filesystem Emission**
   - Rust source files are emitted to the filesystem under:
     `test_projects/test_rust_projects/emit/<crate>/src`

## Emitted Crate Layout

Example: `repomap`

```
emit/repomap/
  Cargo.toml
  src/
    main.rs
    extractor.rs
    repomap.rs
    symbol.rs
```

## Module Relationships

- `main.rs` declares root modules:
  - `mod extractor;`
  - `mod repomap;`
  - `mod symbol;`

- `repomap` module orchestrates symbol extraction and rendering.
- `extractor` module uses tree-sitter to parse Rust source.
- `symbol` defines the `Symbol` enum and rendering helpers.

## Execution Flow

```
Canon Source
   ↓
Canon IR
   ↓
Capture Stage
   ↓
Projection Stage
   ↓
Emit Pipeline
   ↓
Rust crate under emit/<crate>/src
```

## Notes

The emitted sources show partial lowering artifacts (e.g., `canon missing assignment lowering` panics), indicating incomplete lowering during projection.
