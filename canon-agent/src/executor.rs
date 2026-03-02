//! Delta executor — applies CodeDeltas to disk then gates on `cargo check`.
//!
//! Flow:
//!   1. git stash          — clean rollback point
//!   2. apply each delta   — ApplyPatch via apply_patch(1), Bash via sh
//!   3. cargo check        — compile gate
//!   4. success → git stash drop   (keep changes)
//!      failure → git stash pop    (restore tree), return Err
use crate::ir::CodeDelta;
use std::io::Write;
use std::path::Path;
use std::process::{Command, Stdio};

#[derive(Debug)]
pub enum ExecutorError {
    /// git stash failed before we touched anything.
    StashFailed(String),
    /// apply_patch returned non-zero.
    PatchFailed { index: usize, stderr: String },
    /// A Bash delta returned non-zero.
    BashFailed { index: usize, command: String, stderr: String },
    /// cargo check failed after patches were applied — tree has been restored.
    CheckFailed(String),
    /// git stash pop failed — tree may be dirty.
    RollbackFailed(String),
    /// git stash drop failed (non-fatal, logged by caller).
    StashDropFailed(String),
}

impl std::fmt::Display for ExecutorError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            ExecutorError::StashFailed(e) => write!(f, "git stash failed: {e}"),
            ExecutorError::PatchFailed { index, stderr } => {
                write!(f, "apply_patch[{index}] failed: {stderr}")
            }
            ExecutorError::BashFailed { index, command, stderr } => {
                write!(f, "bash delta[{index}] `{command}` failed: {stderr}")
            }
            ExecutorError::CheckFailed(e) => write!(f, "cargo check failed: {e}"),
            ExecutorError::RollbackFailed(e) => write!(f, "git stash pop failed: {e}"),
            ExecutorError::StashDropFailed(e) => write!(f, "git stash drop failed: {e}"),
        }
    }
}

impl std::error::Error for ExecutorError {}

/// Apply `deltas` to disk inside `workspace`, gate on `cargo check`.
///
/// On any failure the working tree is restored via `git stash pop`.
/// On success the stash is dropped and the changes remain on disk.
pub fn execute_deltas(deltas: &[CodeDelta], workspace: &Path) -> Result<(), ExecutorError> {
    if deltas.is_empty() {
        return Ok(());
    }

    // 1. Stash the current working tree so we have a clean rollback point.
    git_stash(workspace)?;

    // 2. Apply each delta in order.
    for (i, delta) in deltas.iter().enumerate() {
        let result = match delta {
            CodeDelta::ApplyPatch { patch } => run_apply_patch(patch, workspace, i),
            CodeDelta::Bash { command } | CodeDelta::BashReadOnly { command } => run_bash(command, workspace, i),
        };
        if let Err(e) = result {
            // Something failed mid-way — restore the tree before returning.
            rollback(workspace)?;
            return Err(e);
        }
    }

    // 3. Compile gate.
    let check = Command::new("cargo").args(["check"]).current_dir(workspace).output().map_err(|e| ExecutorError::CheckFailed(e.to_string()))?;

    if !check.status.success() {
        let stderr = String::from_utf8_lossy(&check.stderr).into_owned();
        rollback(workspace)?;
        return Err(ExecutorError::CheckFailed(stderr));
    }

    // 4. Success — discard the stash, keep changes on disk.
    let drop = Command::new("git").args(["stash", "drop"]).current_dir(workspace).output().map_err(|e| ExecutorError::StashDropFailed(e.to_string()))?;

    if !drop.status.success() {
        let stderr = String::from_utf8_lossy(&drop.stderr).into_owned();
        return Err(ExecutorError::StashDropFailed(stderr));
    }

    Ok(())
}

// ── internals ────────────────────────────────────────────────────────────────

fn git_stash(workspace: &Path) -> Result<(), ExecutorError> {
    let out = Command::new("git").args(["stash"]).current_dir(workspace).output().map_err(|e| ExecutorError::StashFailed(e.to_string()))?;

    if !out.status.success() {
        let stderr = String::from_utf8_lossy(&out.stderr).into_owned();
        return Err(ExecutorError::StashFailed(stderr));
    }
    Ok(())
}

fn rollback(workspace: &Path) -> Result<(), ExecutorError> {
    let out = Command::new("git").args(["stash", "pop"]).current_dir(workspace).output().map_err(|e| ExecutorError::RollbackFailed(e.to_string()))?;

    if !out.status.success() {
        let stderr = String::from_utf8_lossy(&out.stderr).into_owned();
        return Err(ExecutorError::RollbackFailed(stderr));
    }
    Ok(())
}

fn run_apply_patch(patch: &str, workspace: &Path, index: usize) -> Result<(), ExecutorError> {
    let mut child = Command::new("apply_patch").stdin(Stdio::piped()).stderr(Stdio::piped()).current_dir(workspace).spawn().map_err(|e| ExecutorError::PatchFailed { index, stderr: e.to_string() })?;

    if let Some(stdin) = child.stdin.take() {
        let mut stdin = stdin;
        stdin.write_all(patch.as_bytes()).map_err(|e| ExecutorError::PatchFailed { index, stderr: e.to_string() })?;
    }

    let out = child.wait_with_output().map_err(|e| ExecutorError::PatchFailed { index, stderr: e.to_string() })?;

    if !out.status.success() {
        let stderr = String::from_utf8_lossy(&out.stderr).into_owned();
        return Err(ExecutorError::PatchFailed { index, stderr });
    }
    Ok(())
}

fn run_bash(command: &str, workspace: &Path, index: usize) -> Result<(), ExecutorError> {
    let out = Command::new("sh").args(["-c", command]).current_dir(workspace).output().map_err(|e| ExecutorError::BashFailed { index, command: command.to_string(), stderr: e.to_string() })?;

    if !out.status.success() {
        let stderr = String::from_utf8_lossy(&out.stderr).into_owned();
        return Err(ExecutorError::BashFailed { index, command: command.to_string(), stderr });
    }
    Ok(())
}
