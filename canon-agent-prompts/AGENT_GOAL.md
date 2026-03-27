# SQL-Like Query Engine with Logical Optimization and Coverage Analyzer

This project implements a Rust-based SQL-like query engine capable of parsing, planning, optimizing, and executing queries over in-memory tabular data, along with a coverage analysis system that tracks which query constructs, optimization paths, and execution strategies are exercised. It is interesting because query engines involve parsing, relational algebra, cost-based decisions, and multiple execution strategies, leading to a large and nuanced surface for discovering untested logic.

## Target
- Project path: `/workspace/ai_sandbox/canon/test_projects/goalgen/sql-engine-coverage`

## Requirements

1. Implement a Rust binary crate organized into modules such as `table`, `row`, `column`, `schema`, `value`, `storage`, `catalog`, `parser`, `lexer`, `ast`, `planner`, `logical_plan`, `physical_plan`, `optimizer`, `rules`, `executor`, `operators`, `join`, `filter`, `projection`, `aggregation`, `engine`, `trace`, `coverage`, `analysis`, `generator`, `report`, `cli`, and `errors`.
2. Design an in-memory tabular storage system supporting multiple tables, schemas, and typed columns (integers, floats, strings, booleans).
3. Implement a SQL-like parser supporting SELECT, WHERE, JOIN, GROUP BY, ORDER BY, and LIMIT clauses.
4. Build an abstract syntax tree (AST) representation for parsed queries and validate semantic correctness.
5. Develop a logical query planner that converts ASTs into relational algebra-style logical plans.
6. Implement an optimizer that applies rule-based transformations such as predicate pushdown, projection pruning, and join reordering.
7. Create a physical execution engine with operators for scanning, filtering, joining (nested loop and hash join), aggregation, and sorting.
8. Support multiple execution strategies and switch between them based on simple heuristics or cost estimation.
9. Handle edge cases such as NULL values, empty tables, invalid queries, type mismatches, and division by zero in expressions.
10. Create a trace system that records parsing steps, plan transformations, operator execution, and intermediate results.
11. Build a coverage tracking system that records which query constructs, optimization rules, execution operators, and edge cases have been exercised.
12. Develop a query and data generator that produces synthetic tables and queries designed to exercise uncovered logical and physical plan paths, and provide reporting features including coverage summaries, execution statistics, and uncovered scenarios, with optional JSON export, along with a CLI using `clap` supporting commands like `load`, `query`, `explain`, `trace`, `coverage`, and `report`, ensuring the implementation spans at least 800 lines of real Rust code across modules and compiles successfully with `cargo check` without requiring external services.