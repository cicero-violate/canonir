# Event-Driven State Machine Simulator with Transition Coverage Analysis

This project implements a Rust-based simulator for event-driven state machines, supporting hierarchical states, transitions, guards, and actions, along with a coverage analysis system that tracks which states, transitions, and event sequences are exercised. It is interesting because state machines encode complex behavioral logic with branching transitions and temporal sequencing, making them ideal for systematically discovering untested execution paths and edge cases.

## Target
- Project path: `/workspace/ai_sandbox/canon/test_projects/goalgen/state-machine-coverage`

## Requirements

1. Implement a Rust binary crate organized into modules such as `state`, `transition`, `event`, `machine`, `hierarchy`, `context`, `guard`, `action`, `executor`, `engine`, `scheduler`, `queue`, `timer`, `clock`, `history`, `serialization`, `trace`, `coverage`, `analysis`, `generator`, `report`, `cli`, and `errors`.
2. Design a state machine model supporting states, initial states, terminal states, and nested (hierarchical) state structures.
3. Implement transitions triggered by events, with optional guard conditions and associated actions.
4. Support event queues and asynchronous event processing with ordering guarantees.
5. Develop an execution engine that processes events, evaluates guards, executes actions, and updates the current state.
6. Implement hierarchical state behavior including entry/exit actions, parent-child transitions, and history states.
7. Handle edge cases such as invalid transitions, missing states, guard failures, re-entrant events, and infinite transition loops with safeguards.
8. Create a simulated clock and timer system to support time-based events and delayed transitions.
9. Implement a trace system that records event processing, state transitions, guard evaluations, and action executions.
10. Build a coverage tracking system that records which states, transitions, guard branches, and event sequences have been exercised.
11. Develop an analysis module that identifies untested transitions, unreachable states, rare event orderings, and edge-case guard conditions.
12. Implement a scenario generator that produces synthetic event sequences and state machine configurations to exercise uncovered behaviors, and provide reporting features including coverage summaries, transition graphs, and uncovered scenarios, with optional JSON export, along with a CLI using `clap` supporting commands like `run`, `simulate`, `trace`, `coverage`, and `report`, ensuring the implementation spans at least 800 lines of real Rust code across modules and compiles successfully with `cargo check` without requiring external services.