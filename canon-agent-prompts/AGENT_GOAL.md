# Graph Query Engine with Pattern Matching and Coverage Analysis

This project implements a Rust-based graph query engine capable of storing and querying property graphs using a pattern-matching query language similar to Cypher, along with a coverage analysis system that tracks which graph traversal paths, pattern matches, and edge cases are exercised. It is interesting because graph queries involve recursive traversal, pattern matching, and combinatorial path exploration, making it difficult to fully test without systematic coverage tracking.

## Target
- Project path: `/workspace/ai_sandbox/canon/test_projects/goalgen/graph-query-engine-coverage`

## Requirements

1. Implement a Rust binary crate organized into modules such as `node`, `edge`, `graph`, `property`, `store`, `index`, `query`, `lexer`, `parser`, `ast`, `planner`, `pattern`, `matcher`, `traversal`, `executor`, `runtime`, `state`, `binding`, `path`, `trace`, `coverage`, `analysis`, `generator`, `report`, `cli`, and `errors`.
2. Design a property graph model supporting nodes and edges with arbitrary key-value properties and labels.
3. Implement storage structures for nodes and edges with indexing support for efficient lookup by label and property.
4. Design a query language supporting pattern matching (e.g., `(a)-[r]->(b)`), filtering, projection, and simple aggregations.
5. Implement a lexer and parser that convert query strings into an AST with validation and error reporting.
6. Build a query planner that transforms ASTs into executable traversal plans with basic optimizations such as index usage and join ordering.
7. Implement a pattern matching engine that performs graph traversals and binds variables to nodes, edges, and paths.
8. Support traversal features such as variable-length paths, cycles, and constrained traversal depth.
9. Handle edge cases such as disconnected graphs, cyclic paths, duplicate matches, missing properties, and large graph traversal limits.
10. Create a trace system that records parsing steps, traversal decisions, pattern matches, and intermediate bindings.
11. Build a coverage tracking system that records which query constructs, traversal paths, pattern branches, and edge cases have been exercised.
12. Develop an analysis module and graph/query generator that produces synthetic graphs and queries targeting uncovered behaviors, and provide reporting features including traversal counts, execution time, coverage summaries, and uncovered scenarios, with optional JSON export, along with a CLI using `clap` supporting commands like `load`, `query`, `trace`, `coverage`, and `report`, ensuring the implementation spans at least 800 lines of real Rust code across modules and compiles successfully with `cargo check` without requiring external services.