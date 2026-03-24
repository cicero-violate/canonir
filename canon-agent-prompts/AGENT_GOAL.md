# Incremental Build System with Dependency Graph, Change Detection, and Parallel Execution

This project implements an incremental build system in Rust similar to tools like Make or Bazel (simplified). It tracks file dependencies, detects changes, and rebuilds only the necessary parts of a project using a dependency graph. The system supports rule definitions, caching of build artifacts, and parallel execution of independent tasks. This project is interesting because it combines graph algorithms, file system monitoring, hashing, and scheduling into a practical developer productivity tool.

## Target
- Project path: `/workspace/ai_sandbox/canon/test_projects/goalgen/incremental_build_system`

## Requirements

1. Implement a Rust binary crate structured into modules such as `graph`, `node`, `edge`, `rule`, `parser`, `executor`, `scheduler`, `cache`, `hasher`, `filesystem`, `cli`, and `errors`.
2. Design a build rule format (e.g., JSON/TOML using `serde`) that defines targets, inputs, outputs, and commands.
3. Build a dependency graph where nodes represent targets and edges represent dependencies between them.
4. Implement change detection using file metadata (timestamps) and content hashing (e.g., SHA-256) to determine if a target is stale.
5. Develop a caching mechanism that stores build outputs and metadata to avoid redundant work.
6. Implement a topological sort and cycle detection to ensure valid build order and detect dependency loops.
7. Build a scheduler that executes independent build tasks in parallel using threads or async execution.
8. Implement an execution engine that runs shell commands or simulated tasks and captures output and errors.
9. Support incremental rebuilds where only affected nodes in the graph are re-executed.
10. Provide detailed build logs, including timing, cache hits/misses, and executed commands.
11. Provide a CLI using `clap` with commands like `build <target>`, `clean`, `graph`, and `status`.
12. Integrate structured logging with `tracing` to trace dependency resolution, scheduling decisions, cache usage, and execution results, and ensure the implementation spans at least 800 lines of real Rust code across modules and compiles successfully with `cargo check` without requiring external services.