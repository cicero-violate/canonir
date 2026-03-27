# SQL Query Planner and Optimizer with Execution Coverage Analysis

This project implements a Rust-based SQL query planner and optimizer that parses SQL queries, builds logical and physical plans, applies optimization rules, and simulates execution, while tracking coverage of planner decisions, optimization rules, and execution paths. It is interesting because query planners involve complex transformations, cost estimation, branching logic, and rule interactions, making them ideal for discovering untested combinations of query structures and optimization strategies.

## Target
- Project path: `/workspace/ai_sandbox/canon/test_projects/goalgen/sql-planner-coverage`

## Requirements

1. Implement a Rust binary crate organized into modules such as `lexer`, `parser`, `ast`, `expression`, `schema`, `catalog`, `logical_plan`, `physical_plan`, `operator`, `optimizer`, `rule`, `cost`, `statistics`, `planner`, `executor`, `row`, `value`, `engine`, `trace`, `coverage`, `analysis`, `generator`, `report`, `cli`, and `errors`.
2. Design a simplified SQL dialect supporting SELECT, FROM, WHERE, JOIN, GROUP BY, ORDER BY, and LIMIT clauses.
3. Build a lexer and parser that converts SQL queries into an abstract syntax tree (AST).
4. Implement a logical plan builder that transforms the AST into a tree of logical operators (scan, filter, projection, join, aggregation).
5. Develop a rule-based optimizer that applies transformations such as predicate pushdown, projection pruning, join reordering, and constant folding.
6. Implement a physical planner that converts logical plans into executable operator trees with strategies such as nested loop join and hash join.
7. Create a simple execution engine that simulates query execution over in-memory tables and produces result sets.
8. Implement a cost estimation system using basic statistics (row counts, selectivity) to guide optimization decisions.
9. Handle edge cases such as empty tables, NULL values, ambiguous columns, invalid queries, and complex nested expressions.
10. Create a trace system that records parsing steps, plan transformations, optimization rule applications, and execution steps.
11. Build a coverage tracking system that records which SQL constructs, optimization rules, plan shapes, and execution paths have been exercised.
12. Develop an analysis module that identifies untested scenarios such as rare join orders, complex nested queries, edge-case aggregations, and unusual predicate combinations, and implement a query generator that produces synthetic SQL queries targeting uncovered behaviors, with reporting features including coverage summaries, optimization statistics, and uncovered scenarios, optional JSON export, and a CLI using `clap` supporting commands like `parse`, `plan`, `optimize`, `execute`, `trace`, `coverage`, and `report`, ensuring the implementation spans at least 800 lines of real Rust code across modules and compiles successfully with `cargo check` without requiring external services.