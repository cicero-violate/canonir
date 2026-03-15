// Executor module - consolidates capability implementations
mod build_events;
mod build_runtime;
mod capabilities;

pub use build_events::BuildEvent;
pub use build_runtime::{
    run_cargo_build, run_cargo_check, run_cargo_run, BuildRequest, BuildResult, CheckRequest,
    CheckResult, RunRequest, RunResult,
};
pub use capabilities::{
    register_build_capabilities, CAP_BUILD_CARGO, CAP_CHECK_CARGO, CAP_RUN_CARGO,
};
