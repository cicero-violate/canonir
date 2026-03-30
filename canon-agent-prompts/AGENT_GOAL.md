# SQL-like In-Memory Query Engine with Optimization and Coverage Analysis

This project implements a Rust-based in-memory relational query engine that supports a subset of SQL, including SELECT queries with filtering, joins, grouping, and aggregation, along with a coverage analysis system that tracks which query plans, operator combinations, and edge cases are exercised. It is interesting because query engines combine parsing, logical planning, physical execution, and data transformations, resulting in complex branching behavior and optimization paths ideal for uncovering untested scenarios.

## Target
- Project path: `/workspace/ai_sandbox/canon/test_projects/goalgen/sql-engine-coverage`

## Requirements

1. Implement a Rust binary crate organized into modules such as `table`, `row`, `schema`, `value`, `parser`, `lexer`, `ast`, `planner`, `logical_plan`, `physical_plan`, `optimizer`, `executor`, `operator`, `scan`, `filter`, `projection`, `join`, `aggregation`, `group`, `expression`, `runtime`, `trace`, `coverage`, `analysis`, `generator`, `report`, `cli`, and `errors`.
2. Design an in-memory table representation supporting schemas, typed values, and row storage with efficient iteration.
3. Implement a SQL-like parser supporting SELECT, FROM, WHERE, JOIN (inner), GROUP BY, and basic aggregation functions (COUNT, SUM, AVG).
4. Build an AST representation and semantic validation layer that resolves table and column references.
5. Develop a logical query planner that transforms AST into a logical plan tree with operators such as scan, filter, join, and aggregation.
6. Implement a basic optimizer that applies rule-based transformations such as predicate pushdown and projection pruning.
7. Create a physical execution engine that evaluates plans using iterator-based or pipeline-based execution.
8. Support expression evaluation including arithmetic, comparison, boolean logic, and column references.
9. Handle edge cases such as empty tables, null-like values (optional), invalid queries, join mismatches, large datasets, and ambiguous column names.
10. Provide a CLI interface for loading tables from CSV/JSON, executing queries, and printing results in tabular and JSON formats.
11. Create a trace system that records parsing steps, planning decisions, optimization transformations, and execution operator flows.
12. Build a coverage tracking system that records which query constructs, operator combinations, optimization rules, and error conditions have been exercised, and develop an analysis module that identifies untested scenarios such as complex join patterns, nested expressions, and rare aggregation cases, along with a query/data generator that produces synthetic tables and queries targeting uncovered behaviors, with reporting features including execution metrics, row counts, coverage summaries, and uncovered scenarios, optional JSON export, and a CLI using `clap` supporting commands like `load`, `query`, `trace`, `coverage`, and `report`, ensuring the implementation spans at least 800 lines of real Rust code across modules and compiles successfully with `cargo check` without requiring external services.