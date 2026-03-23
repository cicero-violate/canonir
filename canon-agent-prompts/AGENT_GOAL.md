# Agent Goal

Implement a self-contained CLI tool in Rust at `/workspace/ai_sandbox/canon/canon-cli`.

## Target
- Project path: `/workspace/ai_sandbox/canon/canon-cli`
- Type: binary crate (`cargo new --bin canon-cli`)
- Minimum 1500 lines of real implementation code (no padding, no placeholder comments)

## Requirements

### Core: task runner with dependency graph
Build a task runner that reads a `tasks.toml` config file and executes tasks with dependency ordering.

1. **`tasks.toml` format** — each task has:
   - `name`: string identifier
   - `cmd`: shell command to run
   - `depends_on`: list of task names that must complete first (default: empty)
   - `env`: optional key/value pairs injected into the task environment
   - `timeout_secs`: optional per-task timeout (default: 60)

2. **CLI interface** (`canon-cli run [--file tasks.toml] [task_name...]`):
   - `run`: execute all tasks (or named subset) respecting dependency order
   - `list`: print all task names and their deps
   - `validate`: parse tasks.toml and report any cycles or missing deps, exit 1 if invalid
   - `--file`: path to tasks.toml (default: `./tasks.toml`)
   - `--parallel`: run independent tasks concurrently (default: serial)

3. **Dependency resolution**:
   - Topological sort (Kahn's algorithm or DFS)
   - Detect and report cycles with the cycle path shown
   - If a task fails, skip all downstream dependents and report them as skipped

4. **Execution**:
   - Stream stdout/stderr of each task to the terminal with a `[task_name]` prefix per line
   - Write a structured JSON run log to `.canon-cli/run_TIMESTAMP.json`
   - Each log entry: `{ "task": "...", "status": "ok|failed|skipped", "exit_code": N, "duration_ms": N, "stdout": "...", "stderr": "..." }`
   - On failure, print a summary table of all task statuses at the end

5. **Code structure** — split across these modules:
   - `main.rs` — CLI parsing (use `clap` derive)
   - `config.rs` — `tasks.toml` parsing and validation (use `serde` + `toml`)
   - `graph.rs` — dependency graph, topological sort, cycle detection
   - `runner.rs` — task execution engine, streaming output, timeout handling
   - `log.rs` — structured JSON run log writer

### Build requirement
`cargo build` must succeed with zero errors and zero warnings.

### Example `tasks.toml` to include in the repo
```toml
[[task]]
name = "fmt"
cmd = "cargo fmt --check"

[[task]]
name = "lint"
cmd = "cargo clippy -- -D warnings"
depends_on = ["fmt"]

[[task]]
name = "test"
cmd = "cargo test"
depends_on = ["lint"]

[[task]]
name = "build"
cmd = "cargo build --release"
depends_on = ["test"]
```

## Verification criteria
The agent must declare `done` only after ALL of the following are confirmed true:
- `cargo build` exits 0 in `/workspace/ai_sandbox/canon/canon-cli`
- `cargo clippy -- -D warnings` exits 0
- All five source files (`main.rs`, `config.rs`, `graph.rs`, `runner.rs`, `log.rs`) exist and are non-empty
- `tasks.toml` exists at the project root
- Total non-blank source lines across all `.rs` files ≥ 1500
