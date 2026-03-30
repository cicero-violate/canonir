# Bytecode Virtual Machine with Debugger and Coverage Discovery

This project implements a Rust-based stack-oriented bytecode virtual machine (VM) capable of executing a custom instruction set, along with an integrated debugger and a coverage discovery system that identifies untested instruction sequences, control flow paths, and edge-case execution states. It is interesting because virtual machines combine parsing, execution, control flow, memory management, and debugging, creating a rich space of execution paths ideal for systematic test coverage exploration.

## Target
- Project path: `/workspace/ai_sandbox/canon/test_projects/goalgen/bytecode-vm`

## Requirements

1. Implement a Rust binary crate organized into modules such as `instruction`, `opcode`, `bytecode`, `vm`, `stack`, `frame`, `memory`, `heap`, `callstack`, `loader`, `parser`, `assembler`, `disassembler`, `runtime`, `debugger`, `breakpoint`, `trace`, `coverage`, `analysis`, `generator`, `report`, `cli`, and `errors`.
2. Design a bytecode format and instruction set including arithmetic operations, stack manipulation, control flow (jumps, branches), function calls, and memory access.
3. Implement a stack-based execution engine that interprets bytecode instructions and manages call frames.
4. Support function definitions, local variables, and recursive calls with proper stack frame handling.
5. Build a memory model including a heap for dynamic allocation and a stack for execution state.
6. Implement a debugger supporting breakpoints, step execution, stack inspection, and variable inspection.
7. Provide an assembler that converts human-readable assembly into bytecode and a disassembler for reverse conversion.
8. Handle edge cases such as stack underflow/overflow, invalid opcodes, infinite loops, recursion limits, and memory errors.
9. Provide a CLI using `clap` to load programs, execute bytecode, run in debug mode, and inspect VM state.
10. Create a trace system that records instruction execution, stack changes, memory access, and control flow transitions.
11. Build a coverage tracking system that records which instructions, control flow paths, branching outcomes, and error conditions have been exercised.
12. Develop an analysis module that identifies untested scenarios such as rare opcode combinations, deep recursion paths, unusual control flow graphs, and error conditions, and implement a generator that produces synthetic bytecode programs targeting uncovered behaviors, including reporting features such as instruction counts, execution time, stack depth, coverage summaries, and uncovered scenarios, ensuring the implementation spans at least 800 lines of Rust code across modules and compiles successfully with `cargo check`.