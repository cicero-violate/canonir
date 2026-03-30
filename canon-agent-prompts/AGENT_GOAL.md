# Constraint Solver Engine with Backtracking and Coverage Analysis

This project implements a Rust-based constraint satisfaction problem (CSP) solver that supports variables, domains, and constraints with backtracking search, heuristics, and propagation techniques, along with a coverage analysis system that tracks which constraint combinations, solver paths, and edge cases are exercised. It is interesting because constraint solving involves recursive search, pruning strategies, and combinatorial explosion, creating a rich execution surface ideal for uncovering untested behavior.

## Target
- Project path: `/workspace/ai_sandbox/canon/test_projects/goalgen/csp-solver-coverage`

## Requirements

1. Implement a Rust binary crate organized into modules such as `variable`, `domain`, `constraint`, `assignment`, `problem`, `parser`, `dsl`, `ast`, `solver`, `backtracking`, `heuristic`, `propagation`, `arc_consistency`, `search`, `state`, `runtime`, `trace`, `coverage`, `analysis`, `generator`, `report`, `cli`, and `errors`.
2. Design core data structures representing variables with domains and constraints between variables (binary and n-ary).
3. Implement a DSL for defining constraint problems including variables, domains, and constraint expressions.
4. Build a parser that converts DSL input into an AST and validates problem definitions.
5. Develop a backtracking search engine that explores assignments and detects constraint violations.
6. Implement heuristics such as minimum remaining values (MRV) and degree heuristic for variable selection.
7. Add constraint propagation techniques such as forward checking and arc consistency (AC-3).
8. Handle edge cases such as unsatisfiable problems, large domains, cyclic constraints, and redundant constraints.
9. Provide a CLI interface for loading problems, running the solver, and displaying solutions or failure explanations.
10. Create a trace system that records search decisions, backtracking steps, constraint checks, and propagation events.
11. Build a coverage tracking system that records which constraint types, propagation paths, heuristics, and failure cases have been exercised.
12. Develop an analysis module that identifies untested scenarios such as deep backtracking trees, rare constraint interactions, and edge-case domain reductions, and implement a problem generator that produces synthetic CSP instances targeting uncovered behaviors, with reporting features including search depth, node expansions, coverage summaries, and uncovered scenarios, optional JSON export, and a CLI using `clap` supporting commands like `solve`, `trace`, `coverage`, and `report`, ensuring the implementation spans at least 800 lines of real Rust code across modules and compiles successfully with `cargo check` without requiring external services.