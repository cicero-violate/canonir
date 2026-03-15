// Supervisor module - process orchestration and lifecycle management
mod config;
mod events;
mod process;
mod watcher;

pub use config::{load_config, write_default_config, ProcessConfig, RestartStrategy, SupervisorConfig, WatcherConfig};
pub use events::{wrap_event, SupervisorEvent};
pub use process::ProcessManager;
pub use watcher::{affected_crates, crate_for_path, start_watcher};
