use anyhow::Result;
use std::process::Command;

#[derive(Debug, Clone)]
pub struct BuildRequest {
    pub crate_name: String,
}

#[derive(Debug, Clone)]
pub struct BuildResult {
    pub crate_name: String,
    pub success: bool,
    pub duration_ms: u128,
}

pub fn run_cargo_build(req: &BuildRequest) -> Result<BuildResult> {
    let start = std::time::Instant::now();
    let status = Command::new("cargo")
        .args(["build", "-p", &req.crate_name])
        .status()?;
    Ok(BuildResult {
        crate_name: req.crate_name.clone(),
        success: status.success(),
        duration_ms: start.elapsed().as_millis(),
    })
}
