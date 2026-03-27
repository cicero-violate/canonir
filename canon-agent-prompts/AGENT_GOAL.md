# Regex Engine with Execution Path Coverage Analyzer

This project implements a custom regular expression engine in Rust that compiles patterns into automata and executes them against input strings while tracking which matching paths and edge cases are exercised. It is interesting because it exposes the internal nondeterministic behavior of regex engines and highlights untested matching branches, backtracking paths, and pattern edge cases that typical tests overlook.

## Target
- Project path: `/workspace/ai_sandbox/canon/test_projects/goalgen/regex-coverage-engine`

## Requirements

1. Implement a Rust binary crate organized into modules such as `lexer`, `parser`, `ast`, `compiler`, `nfa`, `dfa`, `engine`, `state`, `backtrack`, `executor`, `trace`, `coverage`, `analysis`, `generator`, `report`, `cli`, and `errors`.
2. Build a regex parser that supports literals, concatenation, alternation (`|`), repetition (`*`, `+`, `?`), grouping, and character classes.
3. Define an abstract syntax tree (AST) representing parsed regex patterns.
4. Implement a compiler that converts the AST into a nondeterministic finite automaton (NFA).
5. Optionally implement NFA-to-DFA conversion for optimized execution paths.
6. Develop an execution engine that matches input strings against the automaton, supporting both NFA simulation and backtracking modes.
7. Design a state tracking system that records active states, transitions, and backtracking decisions during execution.
8. Implement a trace system that logs transitions, branching decisions, and match attempts for each input string.
9. Build a coverage tracking system that records which automaton states, transitions, and pattern constructs have been exercised.
10. Develop an analysis module that identifies untested regex branches, unused alternations, and rare backtracking paths.
11. Implement an input generator that produces strings aimed at exercising uncovered regex behaviors and edge cases.
12. Provide reporting features including state/transition coverage summaries, pattern-level insights, and suggested inputs, with optional JSON export, and implement a CLI using `clap` with commands like `compile`, `match`, `trace`, `coverage`, and `report`, ensuring the implementation spans at least 800 lines of real Rust code across modules and compiles successfully with `cargo check`.