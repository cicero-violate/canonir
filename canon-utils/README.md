# canon-utils

Compiler instrumentation and program analysis tools for the Canon workspace.

## Overview

`canon-utils` is a `rustc_private` compiler plugin pipeline that instruments every
`cargo build` to extract a Unified Program Graph (UPG), augment it with compiler
diagnostics, and produce a ranked repair surface backed by SMT reachability proofs.

## Components

### `analysis` (compiler plugin library)

A `rustc_private` library invoked during compilation via `analysis_capture`.
Extracts the full program structure from MIR and HIR into a graph:

- **Nodes:** functions, methods, basic blocks, call sites, structs, enums,
  traits, impls, fields, params, variables, modules, errors
- **Edges:** control flow, calls, returns, unwinds, data assignment,
  propagation, trait bounds, error-to-function, error-to-block

Outputs written to `<project>/analysis/`:

| File                  | Description                                           |
|-----------------------+-------------------------------------------------------|
| `nodes.csv`           | All graph nodes with kind, symbol, file, line, column |
| `edges.csv`           | All graph edges with src, dst, kind                   |
| `csr_row_ptr.bin`     | CSR row pointer array (binary)                        |
| `csr_col_idx.bin`     | CSR column index array (binary)                       |
| `node_kinds.txt`      | Node kind legend                                      |
| `edge_kinds.txt`      | Edge kind legend                                      |
| `metadata.json`       | Project name, node/edge counts, generator             |
| `errors.json`         | Structured rustc diagnostics (JSON)                   |
| `repair_surface.json` | Ranked functions by error count                       |

### `analysis-engine` (standalone binary)

Consumes the graph outputs and runs multi-phase analysis:

| Phase          | Description                                           |
|----------------+-------------------------------------------------------|
| `reachability` | SMT-verified error reachability per function via Z3   |
| `invariants`   | Proves or refutes dataflow invariants                 |
| `duplicates`   | Semantic duplicate detection via embedding similarity |
| `anomalies`    | Statistical anomaly detection over graph structure    |
| `refactoring`  | Refactoring candidates with SMT equivalence proofs    |
| `all`          | All phases (default)                                  |

Additional outputs written to `<project>/analysis/`:

| File                          | Description                                      |
|-------------------------------+--------------------------------------------------|
| `repair_surface_smt.json`     | Repair surface with SMT reachability verdicts    |
| `semantic_duplicates.json`    | Duplicate function candidates                    |
| `invariants.json`             | Invariant proof results                          |
| `anomalies.json`              | Anomaly detection results                        |
| `refactoring_candidates.json` | Refactoring candidates with equivalence verdicts |
| `smt_cache.json`              | Persistent proof cache keyed by graph hash       |

## How It Works

```
cargo build
    └── analysis_capture (RUSTC_WRAPPER)
            ├── redirects rustc stderr → errors.jsonl
            ├── runs rustc with MirCaptureCallbacks
            │       └── after_analysis: extract_and_write(tcx)
            │               ├── extract UPG from MIR + HIR
            │               ├── write nodes.csv, edges.csv, CSR binaries
            │               └── augment_with_errors → repair_surface.json
            ├── parses errors.jsonl → errors.json
            ├── on rustc failure: augment_with_errors (error-only path)
            └── on success: spawns analysis-engine --phase all
                    ├── SMT reachability (Z3, scoped per function)
                    ├── GPU reachability (CUDA via algorithms crate)
                    ├── invariant proving
                    ├── duplicate detection
                    └── equivalence checking
```

## SMT Engine

The SMT engine uses Z3 (`z3 = "0.19.7"`, system `libz3.so`) with:

- A single `z3::Context` owned by `SmtSession`, created once with the
  configured timeout
- `EncodedGraph` built scoped per function at query time — only the basic
  blocks reachable from the queried function via `HasBlock` edges are encoded
- `solver.push()` / `solver.pop()` per error node query inside the reachability loop
- Persistent proof cache keyed by `SHA-256(fn_id, err_id, graph_hash)` —
  results are reused across builds when the function's subgraph is unchanged

## Usage

The plugin runs automatically via `RUSTC_WRAPPER`. To build the toolchain:

```bash
cd rustc_wrapper/analysis_capture
rustc build_wrapper.rs -o build_wrapper && ./build_wrapper
```

To run the analysis engine manually against an existing graph:

```bash
./target/debug/analysis-engine --dir <project>/analysis --phase all
./target/debug/analysis-engine --dir <project>/analysis --phase reachability
./target/debug/analysis-engine --dir <project>/analysis --phase duplicates --clear-cache
```

## Constraints

- Requires nightly rustc (`rustc_private`)
- Z3 must be installed system-wide (`libz3.so`) — do not change the z3 crate version
- `upg_analysis` must not be added to `analysis-engine/Cargo.toml` — it pulls
  in `rustc_private` which is incompatible with the standalone binary
- All graph JSON files are read with `serde_json` — no Python scripts in the pipeline
