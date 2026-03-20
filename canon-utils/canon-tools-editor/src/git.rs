use std::path::Path;
use std::process::Command;

pub fn restore_project_src(project: &Path) -> Result<bool, Box<dyn std::error::Error>> {
    let status = Command::new("git").args(["restore", "--source=HEAD", "--worktree", "--staged", "src"]).current_dir(project).status()?;
    Ok(status.success())
}
