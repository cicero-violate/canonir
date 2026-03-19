use anyhow::Result;
use std::process::Command;

#[derive(Debug, Clone)]
pub struct BuildRequest {
    pub crate_name: String,
}

#[derive(Debug, Clone)]
pub struct RunRequest {
    pub crate_name: String,
    pub bin: Option<String>,
    pub args: Vec<String>,
}

#[derive(Debug, Clone)]
pub struct CheckRequest {
    pub crate_name: String,
}

#[derive(Debug, Clone)]
pub struct BuildResult {
    pub crate_name: String,
    pub status: i32,
    pub success: bool,
    pub stdout: String,
    pub stderr: String,
    pub duration_ms: u128,
}

#[derive(Debug, Clone)]
pub struct RunResult {
    pub crate_name: String,
    pub status: i32,
    pub success: bool,
    pub stdout: String,
    pub stderr: String,
    pub duration_ms: u128,
}

#[derive(Debug, Clone)]
pub struct CheckResult {
    pub crate_name: String,
    pub status: i32,
    pub success: bool,
    pub stdout: String,
    pub stderr: String,
    pub duration_ms: u128,
}

pub fn run_cargo_build(req: &BuildRequest) -> Result<BuildResult> {
    let start = std::time::Instant::now();
    let output = Command::new("cargo")
        .args(["build", "-p", &req.crate_name])
        .output()?;
    Ok(BuildResult {
        crate_name: req.crate_name.clone(),
        status: output.status.code().unwrap_or(-1),
        success: output.status.success(),
        stdout: String::from_utf8_lossy(&output.stdout).to_string(),
        stderr: String::from_utf8_lossy(&output.stderr).to_string(),
        duration_ms: start.elapsed().as_millis(),
    })
}

pub fn run_cargo_run(req: &RunRequest) -> Result<RunResult> {
    let start = std::time::Instant::now();
    let mut cmd = Command::new("cargo");
    cmd.args(["run", "-p", &req.crate_name]);
    if let Some(bin) = req.bin.as_deref() {
        cmd.args(["--bin", bin]);
    }
    if !req.args.is_empty() {
        cmd.arg("--");
        cmd.args(&req.args);
    }
    let output = cmd.output()?;
    Ok(RunResult {
        crate_name: req.crate_name.clone(),
        status: output.status.code().unwrap_or(-1),
        success: output.status.success(),
        stdout: String::from_utf8_lossy(&output.stdout).to_string(),
        stderr: String::from_utf8_lossy(&output.stderr).to_string(),
        duration_ms: start.elapsed().as_millis(),
    })
}

pub fn run_cargo_check(req: &CheckRequest) -> Result<CheckResult> {
    let start = std::time::Instant::now();
    let output = Command::new("cargo")
        .args(["check", "-p", &req.crate_name])
        .output()?;
    Ok(CheckResult {
        crate_name: req.crate_name.clone(),
        status: output.status.code().unwrap_or(-1),
        success: output.status.success(),
        stdout: String::from_utf8_lossy(&output.stdout).to_string(),
        stderr: String::from_utf8_lossy(&output.stderr).to_string(),
        duration_ms: start.elapsed().as_millis(),
    })
}
