//! Act phase — execute CodeDelta list against the working directories.
//! Returns accumulated stdout from all BashReadOnly commands.

use crate::ir::CodeDelta;
use anyhow::{Context, Result};
use std::io::Write as _;
use std::path::PathBuf;
use std::process::Command;
use std::process::Stdio;

/// Commands allowed under BashReadOnly.
const READONLY_WHITELIST: &[&str] = &[
    "rg", "cat", "ls", "tree", "sed", "awk", "perl",
    "find", "head", "tail", "wc", "diff", "stat", "echo", "pwd",
    "cargo",
];

pub fn act(deltas: &[CodeDelta], capture_dirs: &[PathBuf]) -> Result<String> {
    let capture_dir = &capture_dirs[0];
    let mut bash_output = String::new();

    for delta in deltas {
        match delta {
            CodeDelta::Bash { command } => {
                let status = Command::new("bash")
                    .arg("-c")
                    .arg(command)
                    .current_dir(capture_dir)
                    .stdout(Stdio::null())
                    .stderr(Stdio::null())
                    .status()
                    .context("bash command failed to spawn")?;
                anyhow::ensure!(status.success(), "bash command exited with {}", status);
            }

            CodeDelta::BashReadOnly { command } => {
                let trimmed = command.trim();
                let is_allowed = READONLY_WHITELIST.iter().any(|a| trimmed.starts_with(a));
                if !is_allowed {
                    anyhow::bail!("BashReadOnly rejected non-whitelisted command: {}", command);
                }
                // cargo commands must run from the workspace root, not the
                // capture_dir, so they can resolve -p <crate> correctly.
                let run_dir = if trimmed.starts_with("cargo") {
                    std::path::Path::new("/workspace/ai_sandbox/canon")
                } else {
                    capture_dir.as_path()
                };
                let output = Command::new("bash")
                    .arg("-c")
                    .arg(trimmed)
                    .current_dir(run_dir)
                    .output()
                    .context("readonly bash failed to spawn")?;
                // exit code 1 = rg no match (warn only), 2+ = real error
                if output.status.code().unwrap_or(2) >= 2 {
                    let stderr = String::from_utf8_lossy(&output.stderr);
                    anyhow::bail!("readonly bash exited with {}: {}", output.status, stderr);
                }
                bash_output.push_str(&format!("$ {}\n", trimmed));
                bash_output.push_str(&String::from_utf8_lossy(&output.stdout));
                bash_output.push('\n');
            }

            CodeDelta::ApplyPatch { patch } => {
                let expanded = patch.replace("\\n", "\n").replace("\\t", "\t");
                let mut last_err = String::new();
                let mut applied = false;

                for dir in capture_dirs {
                    let mut child = Command::new("apply_patch")
                        .stdin(Stdio::piped())
                        .current_dir(dir)
                        .spawn()
                        .context("apply_patch failed to spawn")?;

                    if let Some(mut stdin) = child.stdin.take() {
                        stdin
                            .write_all(expanded.as_bytes())
                            .context("apply_patch: failed to write patch to stdin")?;
                    }

                    let out = child.wait_with_output().context("apply_patch: wait failed")?;
               if out.status.success() {
                        applied = true;
                        let stdout = String::from_utf8_lossy(&out.stdout);
                        if !stdout.trim().is_empty() {
                            bash_output.push_str(&stdout);
                            bash_output.push('\n');
                        }
                        break;
               }
                    last_err = format!("apply_patch failed in {:?}: {}", dir, out.status);
                }

                if !applied {
                    anyhow::bail!("{}", last_err);
                }
            }
        }
    }

    Ok(bash_output)
}
