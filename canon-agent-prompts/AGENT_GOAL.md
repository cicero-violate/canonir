# Queryable Log Processing Engine with Pattern Matching and Coverage Insights

This project implements a Rust CLI tool that ingests structured and semi-structured logs, builds an indexed query engine, and analyzes which log patterns are covered by existing queries versus which remain undiscovered. It is interesting because it combines parsing, indexing, and query execution with a novel “query coverage” model that reveals blind spots in observability and monitoring logic.

## Target
- Project path: `/workspace/ai_sandbox/canon/test_projects/goalgen/log-query-engine`

## Requirements

1. Implement a Rust binary crate organized into modules such as `ingest`, `parser`, `schema`, `record`, `index`, `query`, `engine`, `pattern`, `coverage`, `analysis`, `report`, `storage`, `cli`, and `errors`.
2. Build a log ingestion system that reads files line-by-line and supports multiple formats (e.g., JSON logs, key-value logs, and plain text) with pluggable parsers.
3. Implement a schema inference module that detects fields, types, and structure from ingested logs and normalizes them into a common record representation.
4. Design an indexing system (e.g., inverted index or hashmap-based) that enables efficient querying over fields and values.
5. Implement a query language supporting filtering, boolean expressions, field matching, and simple aggregations.
6. Build a query execution engine that evaluates queries against indexed logs and returns matching records.
7. Create a pattern extraction system that identifies common log patterns (e.g., templates or repeated structures) from ingested data.
8. Implement a query coverage tracker that maps executed queries to the log patterns they match and identifies patterns not covered by any query.
9. Develop an analysis module that highlights uncovered patterns, rare events, and fields never queried.
10. Implement a ranking system that prioritizes uncovered patterns based on frequency, uniqueness, or anomaly likelihood.
11. Provide reporting features including summaries of query coverage, uncovered log patterns, and suggested queries, with optional JSON export.
12. Implement a CLI using `clap` with commands like `ingest`, `index`, `query`, `coverage`, and `report`, integrate structured logging with `tracing`, and ensure the implementation spans at least 800 lines of real Rust code across modules and compiles successfully with `cargo check` without requiring external services.