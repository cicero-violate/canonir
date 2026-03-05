# Canon Orchestration Execution Model

## Overview
The orchestration pipeline converts Canon IR into emitted Rust crates under `emit/<crate>/src`. The pipeline proceeds through four deterministic stages: IR generation, capture, projection, and filesystem emission.

## Stage 1 — Canon IR Generation
Source programs are parsed and normalized into Canon IR. This stage resolves high‑level language constructs into a canonical intermediate representation suitable for structural analysis and lowering.

Responsibilities:
- Parse source files
- Normalize syntax and semantics
- Produce Canon IR graph

Outputs:
- `canon_ir_solved.json`

## Stage 2 — Capture Stage
The capture stage extracts structural program surfaces from Canon IR. This includes symbols, modules, and dependency relationships required for later projection.

Responsibilities:
- Symbol extraction
- Module boundary detection
- Structural surface generation

Outputs:
- `canon_structural_surface.json`
- symbol inventories

## Stage 3 — Projection Stage
The projection stage converts Canon IR structures into Rust constructs. Canon operations are lowered into Rust AST patterns that correspond to functions, enums, structs, traits, and impl blocks.

Responsibilities:
- Canon → Rust lowering
- Symbol projection
- Module generation

Outputs:
- Rust module definitions
- Intermediate representation for emission

## Stage 4 — Filesystem Emission
The emission stage materializes projected Rust modules into a crate layout.

Structure:

emit/<crate>/
  Cargo.toml
  src/
    lib.rs
    main.rs
    <module>.rs

Responsibilities:
- Write Rust source files
- Generate module declarations
- Produce build diagnostics reports

Outputs:
- Rust crate source tree
- `canon_build_report.json`

## Execution Flow
1. Generate Canon IR
2. Capture structural surface
3. Project IR to Rust modules
4. Emit crate filesystem
5. Run validation (cargo check)

## Result
The pipeline yields a compilable Rust crate representing the Canon IR program structure under `emit/<crate>/src`.
