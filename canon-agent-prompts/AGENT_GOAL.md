# Markdown Document Processor with AST Transformations and Coverage Analysis

This project implements a Rust-based Markdown document processor that parses Markdown into an abstract syntax tree (AST), applies transformations (formatting, rewriting, linting), and renders output formats such as HTML or plain text, along with a coverage analysis system that tracks which parsing branches, AST transformations, and edge cases are exercised. It is interesting because Markdown parsing involves complex grammar rules, nested structures, and ambiguous constructs, making it a rich domain for discovering untested behavior.

## Target
- Project path: `/workspace/ai_sandbox/canon/test_projects/goalgen/markdown-processor-coverage`

## Requirements

1. Implement a Rust binary crate organized into modules such as `lexer`, `parser`, `token`, `ast`, `node`, `block`, `inline`, `renderer`, `html`, `text`, `transform`, `formatter`, `linter`, `rule`, `visitor`, `walker`, `runtime`, `trace`, `coverage`, `analysis`, `generator`, `report`, `cli`, and `errors`.
2. Design a Markdown AST supporting elements such as headings, paragraphs, lists, code blocks, blockquotes, links, emphasis, strong text, and inline code.
3. Implement a lexer that tokenizes Markdown input, handling edge cases like escaped characters and nested delimiters.
4. Build a parser that converts tokens into an AST, correctly handling nested structures and precedence rules.
5. Develop a rendering system that converts AST nodes into HTML and plain text output formats.
6. Implement transformation passes such as formatting normalization (e.g., consistent indentation), rewriting (e.g., link rewriting), and linting rules (e.g., heading order validation).
7. Use a visitor or walker pattern to traverse and manipulate the AST for transformations.
8. Handle edge cases such as malformed Markdown, deeply nested elements, mixed inline/block constructs, and large documents.
9. Provide a CLI interface for parsing, transforming, and rendering Markdown documents from files or stdin.
10. Create a trace system that records tokenization steps, parsing decisions, AST construction, and transformation actions.
11. Build a coverage tracking system that records which parsing rules, AST node types, transformation paths, and error conditions have been exercised.
12. Develop an analysis module that identifies untested scenarios such as rare nesting patterns, unusual delimiter combinations, and complex inline/block interactions, and implement a document generator that produces synthetic Markdown inputs targeting uncovered behaviors, with reporting features including node counts, transformation statistics, coverage summaries, and uncovered scenarios, optional JSON export, and a CLI using `clap` supporting commands like `parse`, `render`, `transform`, `trace`, `coverage`, and `report`, ensuring the implementation spans at least 800 lines of real Rust code across modules and compiles successfully with `cargo check` without requiring external services.