# Time-Travel Debugger for Deterministic Program Execution with Event Logging and Replay

This project implements a time-travel debugging engine in Rust that records program execution as a sequence of deterministic events and allows replaying, stepping backward, and inspecting past states. It simulates execution of a simple interpreted language or instruction set while capturing state transitions, enabling reverse debugging and inspection of historical program states. This project is interesting because it combines execution tracing, deterministic replay, state snapshots, and debugging abstractions into a powerful tool for understanding program behavior.

## Target
- Project path: `/workspace/ai_sandbox/canon/test_projects/goalgen/time_travel_debugger`

## Requirements

1. Implement a Rust binary crate structured into modules such as `instruction`, `program`, `vm`, `state`, `memory`, `stack`, `event`, `log`, `recorder`, `replay`, `snapshot`, `checkpoint`, `debugger`, `breakpoint`, `engine`, `cli`, and `errors`.
2. Design a simple instruction set (e.g., arithmetic, memory load/store, jumps, function calls) for a deterministic virtual machine.
3. Implement a virtual machine that executes instructions step-by-step while producing execution events.
4. Build an event logging system that records all state transitions (register changes, memory writes, control flow).
5. Implement deterministic replay that reconstructs execution from the event log without re-running original logic.
6. Design snapshot/checkpoint mechanisms to store periodic full state for faster rewind operations.
7. Implement reverse execution (step backward) by replaying from nearest checkpoint and reconstructing prior states.
8. Support breakpoints and watchpoints that trigger during forward or reverse execution.
9. Provide state inspection tools for registers, memory, and stack at any execution point.
10. Implement persistence using file-based serialization with `serde` for event logs and snapshots.
11. Provide a CLI using `clap` with commands like `run`, `record`, `replay`, `step`, `back`, `break`, and `inspect`.
12. Integrate structured logging with `tracing` to trace instruction execution, event recording, replay steps, checkpointing, and debugging operations, and ensure the implementation spans at least 800 lines of real Rust code across modules and compiles successfully with `cargo check` without requiring external services.