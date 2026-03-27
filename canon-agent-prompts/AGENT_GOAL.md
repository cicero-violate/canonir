# Interactive SQL Query Planner and Cost-Based Optimizer Simulator

This project implements a simplified SQL query planner and cost-based optimizer in Rust that parses SQL queries, generates logical and physical execution plans, and selects optimal strategies based on cost estimation. It simulates core database internals such as join ordering, index selection, and predicate pushdown. The system allows experimentation with different optimization strategies and provides explain plans for educational and debugging purposes. This project is interesting because it combines parsing, relational algebra, cost modeling, and optimization algorithms into a system resembling the core of modern relational databases.

## Target
- Project path: `/workspace/ai_sandbox/canon/test_projects/goalgen/sql_optimizer_sim`

## Requirements

1. Implement a Rust binary crate structured into modules such as `lexer`, `parser`, `token`, `ast`, `logical_plan`, `physical_plan`, `expression`, `schema`, `catalog`, `statistics`, `cost`, `optimizer`, `rules`, `join`, `scan`, `filter`, `projection`, `engine`, `cli`, and `errors`.
2. Design a SQL parser (using `nom` or `pest`) supporting a subset of SQL including SELECT, FROM, WHERE, JOIN, and GROUP BY.
3. Convert parsed queries into a logical plan using relational algebra operators (scan, filter, projection, join, aggregation).
4. Implement transformation rules such as predicate pushdown, projection pruning, and join reordering.
5. Build a cost model that estimates execution cost based on table statistics (row count, selectivity, cardinality).
6. Implement a cost-based optimizer that explores multiple execution plans and selects the lowest-cost plan.
7. Design physical operators such as nested loop join, hash join, and index scan.
8. Support simple table statistics and catalog metadata stored in-memory or serialized via `serde`.
9. Implement an execution engine that can simulate running the physical plan on in-memory data.
10. Provide an EXPLAIN feature that outputs logical and physical plans with cost estimates.
11. Provide a CLI using `clap` with commands like `query`, `explain`, `load-data`, and `stats`.
12. Integrate structured logging with `tracing` to trace parsing, plan generation, optimization decisions, cost evaluation, and execution steps, and ensure the implementation spans at least 800 lines of real Rust code across modules and compiles successfully with `cargo check` without requiring external services.