use std::path::Path;
use std::process::Command;

pub(crate) fn run_cmd(project: &Path, cmd: &str, args: &[&str]) -> bool {
    Command::new(cmd)
        .args(args)
        .current_dir(project)
        .status()
        .map(|s| s.success())
        .unwrap_or(false)
}

pub(crate) fn run_capture(
    project: &Path,
    cmd: &str,
    args: &[&str],
) -> Result<String, Box<dyn std::error::Error>> {
    let output = Command::new(cmd).args(args).current_dir(project).output()?;
    Ok(String::from_utf8_lossy(&output.stdout).to_string())
}
