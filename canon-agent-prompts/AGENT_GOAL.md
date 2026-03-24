# Fallback Rust CLI Toolbox

A small but valid placeholder goal emitted locally when goal_gen LLM is unavailable. Builds a binary crate with a couple of modules and a CLI entrypoint so the planner can proceed.

## Target
- Project path: `/workspace/ai_sandbox/canon/test_projects/goalgen/fallback_toolbox`

## Requirements
1. Create a Rust binary crate with modules `cli`, `core`, and `utils`.
2. Implement a CLI using `clap` with a `run` command that prints a greeting.
3. Add a `core::add(a, b)` function with a unit test.
4. Add a `utils::slugify` helper with a unit test.
5. Wire `main` to call into `cli::run()`.
6. Ensure `cargo check` passes.
