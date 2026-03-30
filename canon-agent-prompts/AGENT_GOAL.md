# Workflow Automation Engine with State Machine Execution and Coverage Analysis

This project implements a Rust-based workflow automation engine that models workflows as state machines with tasks, transitions, conditions, and retries, executing them deterministically over simulated inputs, along with a coverage analysis system that tracks which workflow paths, transitions, and edge cases are exercised. It is interesting because workflow systems combine state machines, conditional branching, failure handling, and retries, producing complex execution paths ideal for uncovering untested behavior.

## Target
- Project path: `/workspace/ai_sandbox/canon/test_projects/goalgen/workflow-engine-coverage`

## Requirements

1. Implement a Rust binary crate organized into modules such as `workflow`, `state`, `transition`, `task`, `condition`, `action`, `context`, `instance`, `engine`, `executor`, `scheduler`, `retry`, `timer`, `event`, `runtime`, `trace`, `coverage`, `analysis`, `generator`, `report`, `cli`, and `errors`.
2. Design core data structures representing workflows as directed graphs of states and transitions, supporting start, intermediate, and terminal states.
3. Implement a DSL or JSON-based definition format for workflows including tasks, transitions, conditions, and retry policies.
4. Build a parser that converts workflow definitions into validated in-memory representations, detecting invalid structures such as unreachable states or cycles where not allowed.
5. Develop an execution engine that processes workflow instances, evaluates conditions, executes tasks, and transitions between states.
6. Implement task execution simulation with success, failure, and timeout outcomes, including configurable retry strategies (e.g., fixed retries, exponential backoff).
7. Support conditional branching based on context data, including boolean expressions and simple comparisons.
8. Handle edge cases such as infinite loops, failed retries, invalid transitions, missing context data, and concurrent workflow instances (simulated sequentially).
9. Provide a CLI interface for loading workflows, starting instances, stepping execution, and inspecting state.
10. Create a trace system that records state transitions, condition evaluations, task outcomes, and retry behavior.
11. Build a coverage tracking system that records which states, transitions, condition branches, retry paths, and failure scenarios have been exercised.
12. Develop an analysis module that identifies untested workflow paths such as rare branching combinations, retry exhaustion cases, and edge-case transitions, and implement a workflow/input generator that produces synthetic workflows and execution inputs targeting uncovered behaviors, with reporting features including execution counts, transition frequencies, coverage summaries, and uncovered scenarios, optional JSON export, and a CLI using `clap` supporting commands like `run`, `step`, `trace`, `coverage`, and `report`, ensuring the implementation spans at least 800 lines of real Rust code across modules and compiles successfully with `cargo check` without requiring external services.