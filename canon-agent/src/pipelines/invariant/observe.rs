//! Verify phase helper — runs exit-check command.

use anyhow::{Context, Result};
use std::path::Path;
use std::process::Command;

pub struct ObserveResult {
    pub exit_code: i32,
    pub stdout: String,
}

/// Run exit-check command in `cwd`.
/// Uses `bash -c` because the command may contain pipes or redirects.
pub fn run_exit_check(command: &str, cwd: &Path) -> Result<ObserveResult> {
    let output = Command::new("bash").arg("-c").arg(command).current_dir(cwd).output().with_context(|| format!("failed to spawn exit-check: {}", command))?;

    let mut combined = String::from_utf8_lossy(&output.stdout).into_owned();
    let stderr = String::from_utf8_lossy(&output.stderr);
    if !stderr.trim().is_empty() {
        if !combined.is_empty() {
            combined.push('\n');
        }
        combined.push_str("--- stderr ---\n");
        combined.push_str(&stderr);
    }
    let exit_code = output.status.code().unwrap_or(-1);

    Ok(ObserveResult { exit_code, stdout: combined })
}
