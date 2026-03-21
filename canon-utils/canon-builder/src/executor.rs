// Executor module - consolidates capability implementations
mod build_events;
mod build_runtime;
pub use build_events::BuildEvent;
pub use build_runtime::{run_cargo_build, run_cargo_check, run_cargo_run, BuildRequest, BuildResult, CheckRequest, CheckResult, RunRequest, RunResult};
