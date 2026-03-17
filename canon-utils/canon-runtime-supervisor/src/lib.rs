pub mod config;
pub mod events;
pub mod executor;
pub mod process;
pub mod watcher;

pub use config::{load_config, write_default_config, ProcessConfig, RestartStrategy, SupervisorConfig, WatcherConfig};
pub use events::{wrap_event, SupervisorEvent};
pub use executor::{
    register_build_capabilities, BuildEvent, BuildRequest, BuildResult, CheckRequest,
    CheckResult, RunRequest, RunResult, CAP_BUILD_CARGO, CAP_CHECK_CARGO, CAP_RUN_CARGO,
};
pub use process::ProcessManager;
pub use watcher::{affected_crates, crate_for_path, start_watcher};
