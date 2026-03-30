# Incremental SQL Query Engine with Cost-Based Optimizer and Coverage Discovery

This project implements a Rust-based SQL-like query engine capable of parsing, planning, and executing queries over in-memory tables with an incremental execution model and a cost-based optimizer, along with a coverage discovery system that identifies untested query plans, operator combinations, and edge-case data distributions. It is interesting because query engines combine parsing, logical/physical planning, optimization, and execution, producing a vast space of execution paths that are difficult to fully test without systematic exploration.

## Target
- Project path: `/workspace/ai_sandbox/canon/test_projects/goalgen/sql-engine-coverage`

## Requirements

1. Implement a Rust binary crate organized into modules such as `table`, `row`, `schema`, `value`, `catalog`, `lexer`, `parser`, `ast`, `logical_plan`, `physical_plan`, `optimizer`, `cost`, `statistics`, `operator`, `scan`, `filter`, `project`, `join`, `aggregate`, `sort`, `executor`, `runtime`, `trace`, `coverage`, `analysis`, `generator`, `report`, `cli`, and `errors`.
2. Design a relational data model supporting tables, schemas, rows, and typed values (integers, floats, strings, booleans, nulls).
3. Implement a SQL-like parser supporting SELECT queries with projections, filters (WHERE), joins (inner only), aggregations (COUNT, SUM, AVG), GROUP BY, and ORDER BY.
4. Build a logical plan representation from parsed queries and implement transformation rules such as predicate pushdown and projection pruning.
5. Develop a cost-based optimizer that selects physical execution strategies (e.g., nested loop join vs hash join) based on simple statistics.
6. Implement physical operators including table scan, filter, projection, hash join, aggregation, and sort, with iterator-based or pull-based execution.
7. Support incremental execution where intermediate results can be reused or recomputed partially when input data changes.
8. Handle edge cases such as null handling, empty tables, join key mismatches, aggregation over empty inputs, type coercion errors, and large intermediate results (simulated limits).
9. Provide a CLI using `clap` to load tables (CSV/JSON), run queries, inspect execution plans, and simulate data updates.
10. Create a trace system that records parsing steps, optimization decisions, chosen plans, operator execution, and intermediate row counts.
11. Build a coverage tracking system that records which query constructs, plan shapes, operator combinations, and data distributions have been exercised.
12. Develop an analysis module that identifies untested scenarios such as rare join patterns, skewed data distributions, deep operator pipelines, and edge-case aggregations, and implement a query/data generator that produces synthetic tables and queries targeting uncovered behaviors, with reporting features including execution metrics, plan frequencies, coverage summaries, and uncovered scenarios, ensuring the implementation spans at least 800 lines of Rust code across modules and compiles successfully with `cargo check`.