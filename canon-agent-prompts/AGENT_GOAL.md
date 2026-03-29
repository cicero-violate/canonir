# Interactive CLI Spreadsheet Engine with Formula Evaluation and Coverage Analysis

This project implements a Rust-based interactive command-line spreadsheet engine that supports cell formulas, dependency tracking, recalculation, and tabular data manipulation, along with a coverage analysis system that tracks which formula paths, dependency updates, and edge cases are exercised. It is interesting because spreadsheet computation involves expression parsing, dependency graphs, incremental recomputation, and subtle edge cases like circular references and type coercion.

## Target
- Project path: `/workspace/ai_sandbox/canon/test_projects/goalgen/cli-spreadsheet-coverage`

## Requirements

1. Implement a Rust binary crate organized into modules such as `cell`, `value`, `sheet`, `grid`, `formula`, `lexer`, `parser`, `ast`, `evaluator`, `function`, `dependency`, `graph`, `recalc`, `runtime`, `input`, `display`, `trace`, `coverage`, `analysis`, `generator`, `report`, `cli`, and `errors`.
2. Design a spreadsheet model with rows and columns, supporting cell addressing (e.g., A1, B2) and multiple sheets.
3. Implement a formula language supporting arithmetic operations, references to other cells, and built-in functions (e.g., SUM, AVG, MIN, MAX).
4. Build a lexer and parser that convert formula strings into an AST with proper error handling.
5. Implement an evaluator that computes cell values based on formulas and raw inputs, including type handling (numbers, strings, booleans).
6. Maintain a dependency graph between cells and implement incremental recalculation when values change.
7. Detect and handle circular dependencies, invalid references, and evaluation errors.
8. Provide a CLI interface for interacting with the spreadsheet (editing cells, printing ranges, loading/saving data).
9. Handle edge cases such as empty cells, large grids, chained dependencies, and mixed-type operations.
10. Create a trace system that records parsing steps, evaluation order, dependency updates, and recalculation events.
11. Build a coverage tracking system that records which formula branches, function paths, dependency graph scenarios, and error conditions have been exercised.
12. Develop an analysis module that identifies untested scenarios such as deep dependency chains, complex formula nesting, and edge-case inputs, and implement a spreadsheet generator that produces synthetic sheets and formulas targeting uncovered behaviors, with reporting features including recalculation time, dependency depth, coverage summaries, and uncovered scenarios, optional JSON export, and a CLI using `clap` supporting commands like `edit`, `print`, `load`, `save`, `trace`, `coverage`, and `report`, ensuring the implementation spans at least 800 lines of real Rust code across modules and compiles successfully with `cargo check` without requiring external services.