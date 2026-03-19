use serde::Deserialize;
use std::path::Path;

#[derive(Debug, Clone, Deserialize)]
pub struct SupervisorConfig {
    #[serde(default)]
    pub watcher: WatcherConfig,
    #[serde(default)]
    pub process: Vec<ProcessConfig>,
}

#[derive(Debug, Clone, Deserialize)]
pub struct WatcherConfig {
    #[serde(default = "default_debounce_ms")]
    pub debounce_ms: u64,
    #[serde(default = "default_watch_dirs")]
    pub watch_dirs: Vec<String>,
}

impl Default for WatcherConfig {
    fn default() -> Self {
        Self {
            debounce_ms: default_debounce_ms(),
            watch_dirs: default_watch_dirs(),
        }
    }
}

#[derive(Debug, Clone, Deserialize)]
pub struct ProcessConfig {
    pub name: String,
    pub bin: String,
    #[serde(default)]
    pub args: Vec<String>,
    #[serde(default)]
    pub env: std::collections::HashMap<String, String>,
    #[serde(default)]
    pub depends_on: Vec<String>,
    #[serde(default = "default_drain_timeout_ms")]
    pub drain_timeout_ms: u64,
    #[serde(default)]
    pub crate_name: Option<String>,
}

#[derive(Debug, Clone, Copy, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum RestartStrategy {
    Kill,
    Drain,
}

impl Default for RestartStrategy {
    fn default() -> Self {
        RestartStrategy::Kill
    }
}

pub fn load_config(path: &Path) -> anyhow::Result<SupervisorConfig> {
    let text = std::fs::read_to_string(path)?;
    let config: SupervisorConfig = toml::from_str(&text)?;
    Ok(config)
}

pub fn write_default_config(path: &Path) -> anyhow::Result<()> {
    if let Some(parent) = path.parent() {
        std::fs::create_dir_all(parent)?;
    }
    let default = r#"[watcher]
debounce_ms = 300
watch_dirs = ["canon-utils"]

[[process]]
name = "analysis-engine"
bin  = "target/debug/analysis-engine"
args = []
restart = "kill"
drain_timeout_ms = 3000
"#;
    std::fs::write(path, default)?;
    Ok(())
}

fn default_debounce_ms() -> u64 {
    300
}

fn default_watch_dirs() -> Vec<String> {
    vec!["canon-utils".to_string()]
}

fn default_drain_timeout_ms() -> u64 {
    3000
}
