# Constraint Solver Engine with Backtracking, Propagation, and Coverage Analysis

This project implements a Rust-based constraint satisfaction problem (CSP) solver that models variables, domains, and constraints, supports backtracking search with constraint propagation (e.g., forward checking and arc consistency), and evaluates solutions, along with a coverage analysis system that tracks which solver paths, constraint interactions, and edge cases are exercised. It is interesting because constraint solving involves combinatorial search, pruning strategies, and complex interactions between constraints, making it a rich domain for uncovering untested behavior.

## Target
- Project path: `/workspace/ai_sandbox/canon/test_projects/goalgen/constraint-solver-coverage`

## Requirements

1. Implement a Rust binary crate organized into modules such as `variable`, `domain`, `constraint`, `relation`, `assignment`, `csp`, `parser`, `lexer`, `ast`, `solver`, `backtracking`, `heuristic`, `propagation`, `arc_consistency`, `forward_checking`, `search`, `runtime`, `trace`, `coverage`, `analysis`, `generator`, `report`, `cli`, and `errors`.
2. Design core data structures representing variables with finite domains and constraints (binary and n-ary relations).
3. Implement a parser for a simple CSP definition language allowing declaration of variables, domains, and constraints.
4. Build a backtracking search algorithm that explores assignments and finds solutions.
5. Add heuristics such as minimum remaining values (MRV) and degree heuristic for variable selection.
6. Implement constraint propagation techniques including forward checking and arc consistency (e.g., AC-3 algorithm).
7. Support constraint evaluation and pruning of inconsistent domain values during search.
8. Handle edge cases such as unsatisfiable problems, large domain sizes (simulated), cyclic constraints, and conflicting constraints.
9. Provide a CLI interface for loading CSP definitions, solving problems, and printing solutions or failure explanations.
10. Create a trace system that records assignment decisions, backtracking steps, propagation effects, and constraint evaluations.
11. Build a coverage tracking system that records which search paths, heuristics, constraint types, propagation branches, and failure cases have been exercised.
12. Develop an analysis module that identifies untested scenarios such as deep backtracking trees, rare constraint interactions, and edge-case domain reductions, and implement a CSP generator that produces synthetic problems targeting uncovered behaviors, with reporting features including node exploration counts, backtracking depth, coverage summaries, and uncovered scenarios, optional JSON export, and a CLI using `clap` supporting commands like `solve`, `trace`, `coverage`, and `report`, ensuring the implementation spans at least 800 lines of real Rust code across modules and compiles successfully with `cargo check` without requiring external services.