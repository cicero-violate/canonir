# Parser Combinator Framework with Grammar DSL and Coverage Discovery

This project implements a Rust-based parser combinator framework that allows users to define grammars compositionally, parse structured text into typed syntax trees, and inspect parser behavior through tracing and coverage analysis. It is interesting because parser combinators combine higher-order composition, backtracking, error recovery, and ambiguity handling, creating a rich execution surface with many subtle branches that are ideal for discovering untested behavior.

## Target
- Project path: `/workspace/ai_sandbox/canon/test_projects/goalgen/parser-combinator-coverage`

## Requirements

1. Implement a Rust binary crate organized into multiple modules such as `input`, `span`, `token`, `parser`, `combinator`, `primitive`, `sequence`, `choice`, `repeat`, `lookahead`, `error`, `diagnostic`, `ast`, `grammar`, `dsl`, `runtime`, `trace`, `coverage`, `analysis`, `generator`, `report`, `cli`, and `errors`.
2. Design a core parser trait or abstraction that operates over string input with spans, supports success and failure results, and carries structured error information including expected tokens and failure position.
3. Implement primitive parsers for literals, character classes, identifiers, integers, whitespace, end-of-input, and custom predicate-based matching, with utilities for optional consumption of insignificant whitespace.
4. Build parser combinators including sequence, ordered choice, repetition (`many`, `many1`), optional, separated lists, lookahead, negative lookahead, mapping, labeling, and cut/commit behavior to control backtracking.
5. Develop a small grammar DSL or builder API that lets users define grammars for mini languages and produce typed AST nodes through parser composition rather than hand-written recursive descent code.
6. Implement parser tracing and diagnostics that record entry/exit of parsers, backtracking events, commit points, and the deepest failure encountered, with human-readable error rendering that includes line/column context.
7. Support left-factored expression parsing with precedence and associativity helpers for arithmetic-style grammars, plus handling of nested delimiters such as parentheses and brackets.
8. Handle edge cases such as empty input, ambiguous alternatives, partial parses, deeply nested constructs, repetition over zero-width parsers, runaway recursion, and malformed grammars that could otherwise loop indefinitely.
9. Provide a CLI using `clap` with commands such as `parse`, `trace`, `grammar-check`, `coverage`, and `report`, allowing users to load sample grammars or built-in examples and parse input from files or stdin.
10. Build a coverage tracking subsystem that records which primitive parsers, combinators, grammar rules, backtracking branches, commit paths, and error-rendering cases have been exercised during parsing runs.
11. Develop an analysis module that identifies untested grammar scenarios such as overlapping prefixes, deep nesting, separator edge cases, precedence ambiguities, and error-recovery branches, and pair it with an input generator that produces synthetic strings targeting uncovered parser behavior.
12. Ensure the project contains at least 800 lines of real Rust implementation spread across multiple modules, uses only crates.io dependencies, remains self-contained as a binary crate, and compiles successfully with `cargo check` as the sole success criterion.