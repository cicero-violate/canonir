pub mod context;
pub mod registry;
pub mod result;
pub mod r#trait;
pub mod executor;
pub mod routing;
pub mod supervisor;

// Re-export core capability types
pub use context::CapabilityContext;
pub use registry::CapabilityRegistry;
pub use result::CapabilityResult;
pub use r#trait::Capability;

// Re-export executor types
pub use executor::{
    register_build_capabilities, BuildEvent, BuildRequest, BuildResult, CheckRequest,
    CheckResult, RunRequest, RunResult, CAP_BUILD_CARGO, CAP_CHECK_CARGO, CAP_RUN_CARGO,
};

// Re-export supervisor types
pub use supervisor::{
    affected_crates, crate_for_path, load_config, start_watcher, wrap_event, write_default_config,
    ProcessConfig, ProcessManager, RestartStrategy, SupervisorConfig, SupervisorEvent, WatcherConfig,
};
