# SQL-like Query Engine with Optimization and Coverage Analysis

This project implements a Rust-based SQL-like query engine that parses queries, builds execution plans, optimizes them, and executes against in-memory relational data, along with a coverage analysis system that tracks which query patterns, execution paths, and edge cases are exercised. It is interesting because query engines involve parsing, planning, optimization, and execution phases with many branching decisions, making them ideal for discovering untested behavior.

## Target
- Project path: `/workspace/ai_sandbox/canon/test_projects/goalgen/sql-engine-coverage`

## Requirements

1. Implement a Rust binary crate organized into modules such as `table`, `row`, `schema`, `storage`, `value`, `lexer`, `parser`, `ast`, `planner`, `optimizer`, `logical_plan`, `physical_plan`, `executor`, `operator`, `expression`, `join`, `aggregate`, `filter`, `projection`, `runtime`, `trace`, `coverage`, `analysis`, `generator`, `report`, `cli`, and `errors`.
2. Design in-memory relational data structures supporting tables, schemas, rows, and typed values (integer, float, string, boolean, null).
3. Implement a SQL-like parser supporting SELECT, FROM, WHERE, JOIN, GROUP BY, and basic aggregation functions.
4. Build an AST representation and validate query correctness including column resolution and type checking.
5. Develop a logical query planner that converts AST into a logical plan tree.
6. Implement an optimizer that performs transformations such as predicate pushdown, projection pruning, and simple join reordering.
7. Create a physical execution engine with operators such as scan, filter, projection, nested-loop join, and aggregation.
8. Handle edge cases such as empty tables, null values, invalid queries, ambiguous columns, and large result sets (simulated).
9. Provide a CLI interface for loading sample data, executing queries, and printing results.
10. Create a trace system that records parsing steps, plan generation, optimization decisions, and execution flow.
11. Build a coverage tracking system that records which query constructs, operator paths, optimization rules, and error conditions have been exercised.
12. Develop an analysis module that identifies untested scenarios such as complex join combinations, nested queries, and rare optimization paths, and implement a query/data generator that produces synthetic tables and queries targeting uncovered behaviors, with reporting features including execution statistics, plan structures, coverage summaries, and uncovered scenarios, optional JSON export, and a CLI using `clap` supporting commands like `query`, `trace`, `coverage`, and `report`, ensuring the implementation spans at least 800 lines of real Rust code across modules and compiles successfully with `cargo check` without requiring external services.