use anyhow::{Context, Result};
use serde::Deserialize;
use std::path::{Path, PathBuf};

const CAPABILITY_POLICY_TOML: &str = "/workspace/ai_sandbox/canon/canon-agent-prompts/capability_policy.toml";

#[derive(Debug, Deserialize)]
struct RawPolicy {
    #[serde(default)]
    pub write_allowed_roots: Vec<String>,
    #[serde(default)]
    pub require_final_render: bool,
}

#[derive(Debug, Clone)]
pub struct CapabilityPolicy {
    pub write_allowed_roots: Vec<PathBuf>,
    pub require_final_render: bool,
}

impl CapabilityPolicy {
    pub fn load(workspace_root: &Path) -> Result<Self> {
        let raw_toml = std::fs::read_to_string(CAPABILITY_POLICY_TOML)
            .with_context(|| format!("cannot read {}", CAPABILITY_POLICY_TOML))?;
        let raw: RawPolicy = toml::from_str(&raw_toml).context("cannot parse capability_policy.toml")?;
        let roots = raw
            .write_allowed_roots
            .into_iter()
            .map(|p| {
                let path = Path::new(&p);
                if path.is_absolute() { path.to_path_buf() } else { workspace_root.join(path) }
            })
            .collect::<Vec<_>>();
        Ok(Self { write_allowed_roots: roots, require_final_render: raw.require_final_render })
    }
}
