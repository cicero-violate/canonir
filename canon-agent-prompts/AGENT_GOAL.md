# Constraint Solver Engine with Backtracking and Coverage Analysis

This project implements a Rust-based constraint solver capable of solving constraint satisfaction problems (CSPs) using backtracking, constraint propagation, and heuristics, along with a coverage analysis system that tracks which constraint interactions, propagation paths, and edge cases are exercised. It is interesting because constraint solvers involve complex search spaces, pruning strategies, and subtle interactions between constraints, making them ideal for uncovering under-tested logic paths.

## Target
- Project path: `/workspace/ai_sandbox/canon/test_projects/goalgen/constraint-solver-coverage`

## Requirements

1. Implement a Rust binary crate organized into modules such as `variable`, `domain`, `constraint`, `expression`, `parser`, `lexer`, `ast`, `model`, `solver`, `search`, `backtrack`, `propagation`, `arc_consistency`, `heuristic`, `assignment`, `state`, `engine`, `runtime`, `trace`, `coverage`, `analysis`, `generator`, `report`, `cli`, and `errors`.
2. Design a constraint model supporting variables with finite domains and constraints such as equality, inequality, and arithmetic relations.
3. Implement a parser for a small constraint definition language that produces an AST representation of variables and constraints.
4. Build a backtracking search algorithm that explores possible assignments and detects conflicts.
5. Implement constraint propagation techniques such as forward checking and arc consistency (AC-3).
6. Support heuristics such as minimum remaining values (MRV) and degree heuristics for variable selection.
7. Implement value ordering strategies such as least constraining value.
8. Handle edge cases such as unsatisfiable constraint sets, redundant constraints, large domains, and deeply nested constraint dependencies.
9. Create a trace system that records variable assignments, constraint checks, propagation steps, and backtracking decisions.
10. Build a coverage tracking system that records which constraint combinations, propagation paths, heuristic choices, and edge cases have been exercised.
11. Develop an analysis module that identifies untested scenarios such as rare constraint interactions, deep backtracking paths, and propagation edge cases.
12. Implement a constraint and problem generator that produces synthetic CSP instances targeting uncovered behaviors, and provide reporting features including solve time, backtracking counts, coverage summaries, and uncovered scenarios, with optional JSON export, along with a CLI using `clap` supporting commands like `solve`, `generate`, `trace`, `coverage`, and `report`, ensuring the implementation spans at least 800 lines of real Rust code across modules and compiles successfully with `cargo check` without requiring external services.