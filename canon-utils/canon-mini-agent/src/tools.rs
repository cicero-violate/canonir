use anyhow::{anyhow, bail, Context, Result};
use canon_llm::config::LlmEndpoint;
use canon_tools_patch::apply_patch;
use serde_json::Value;
use std::io::Write;
use std::path::{Component, Path, PathBuf};
use std::process::{Command, Stdio};

use crate::{
    DIAGNOSTICS_FILE, MASTER_PLAN_FILE, MAX_FULL_READ_LINES, MAX_SNIPPET, SPEC_FILE,
    VIOLATIONS_FILE, WORKSPACE,
};
use crate::logging::{append_action_log, append_action_result_log};
use crate::prompts::truncate;

/// Extract the first file path touched by the patch (*** Update File: / *** Add File:).
fn patch_first_file(patch: &str) -> Option<&str> {
    for line in patch.lines() {
        if let Some(rest) = line.strip_prefix("*** Update File:").or_else(|| line.strip_prefix("*** Add File:")) {
            let path = rest.trim();
            if !path.is_empty() {
                return Some(path);
            }
        }
    }
    None
}

fn patch_targets<'a>(patch: &'a str) -> Vec<&'a str> {
    patch
        .lines()
        .filter_map(|line| line.strip_prefix("*** Update File:").or_else(|| line.strip_prefix("*** Add File:")))
        .map(str::trim)
        .filter(|path| !path.is_empty())
        .collect()
}

fn is_lane_plan(path: &str) -> bool {
    path.starts_with("PLANS/executor-") && path.ends_with(".md")
}

fn patch_scope_error(role: &str, patch: &str) -> Option<String> {
    let targets = patch_targets(patch);
    if targets.is_empty() {
        return None;
    }

    let touches_spec = targets.iter().any(|path| *path == SPEC_FILE);
    let touches_lane = targets.iter().any(|path| is_lane_plan(path));
    let touches_master_plan = targets.iter().any(|path| *path == MASTER_PLAN_FILE);
    let touches_violations = targets.iter().any(|path| *path == VIOLATIONS_FILE);
    let touches_diagnostics = targets.iter().any(|path| *path == DIAGNOSTICS_FILE);
    let touches_other = targets
        .iter()
        .any(|path| *path != SPEC_FILE
            && *path != MASTER_PLAN_FILE
            && !is_lane_plan(path)
            && *path != VIOLATIONS_FILE
            && *path != DIAGNOSTICS_FILE);

    match role {
        role if role.starts_with("executor") => {
            if touches_spec || touches_master_plan || touches_lane || touches_violations || touches_diagnostics {
                Some(
                    "Executor may not patch spec, plan files, violations, or diagnostics. Execute code/tests only and report evidence in `done.reason`."
                        .to_string(),
                )
            } else {
                None
            }
        }
        "verifier" | "verifier_a" | "verifier_b" => {
            if touches_spec || touches_master_plan || touches_lane || touches_diagnostics || touches_other {
                Some(
                    "Verifier may only patch `VIOLATIONS.md`. Do not modify spec, plans, diagnostics, or source files."
                        .to_string(),
                )
            } else if touches_violations {
                None
            } else {
                Some(
                    "Verifier should write `VIOLATIONS.md` when violations are found; no other patches are allowed."
                        .to_string(),
                )
            }
        }
        "planner" | "mini_planner" => {
            if touches_spec || touches_violations || touches_diagnostics || touches_other {
                Some(
                    "Planner may only patch `PLAN.md` and lane plans under `PLANS/executor-<id>.md` because planner derives plans from the spec and diagnostics."
                        .to_string(),
                )
            } else if touches_master_plan || touches_lane {
                None
            } else {
                Some(
                    "Planner must update `PLAN.md` and lane plans; no other patches are allowed."
                        .to_string(),
                )
            }
        }
        "diagnostics" => {
            if touches_spec || touches_master_plan || touches_lane || touches_violations || touches_other {
                Some(
                    "Diagnostics may only patch DIAGNOSTICS.md because diagnostics owns ranked failure reporting."
                        .to_string(),
                )
            } else {
                None
            }
        }
        _ => None,
    }
}

/// Walk up from `file_path` (workspace-relative) to find the nearest Cargo.toml.
/// Returns the package name from that manifest, or None if not found.
fn infer_crate_for_patch(workspace: &Path, file_path: &str) -> Option<String> {
    let mut dir = workspace.join(file_path);
    dir.pop(); // start from parent of the file
    loop {
        let manifest = dir.join("Cargo.toml");
        if manifest.exists() {
            let text = std::fs::read_to_string(&manifest).ok()?;
            for line in text.lines() {
                if let Some(rest) = line.strip_prefix("name") {
                    let name = rest.trim().trim_start_matches('=').trim().trim_matches('"');
                    if !name.is_empty() {
                        return Some(name.to_string());
                    }
                }
            }
        }
        if dir == workspace {
            break;
        }
        if !dir.pop() {
            break;
        }
    }
    None
}

// ── Patch-anchor auto-read (mirrors harness_repair logic) ─────────────────────

const AUTO_READ_CONTEXT_BEFORE: usize = 20;
const AUTO_READ_CONTEXT_AFTER: usize = 40;

/// Extract the file path from an apply_patch anchor-miss error.
/// Matches: "Failed to find expected lines in PATH:\n..."
fn extract_anchor_fail_path(err_msg: &str) -> Option<String> {
    let prefix = "Failed to find expected lines in ";
    for line in err_msg.lines() {
        if let Some(rest) = line.strip_prefix(prefix) {
            let path = rest.trim_end_matches(':').trim();
            if !path.is_empty() {
                return Some(path.to_string());
            }
        }
    }
    None
}

/// Parse the indented anchor lines out of the patch error message.
fn extract_expected_anchor_lines(err_msg: &str) -> Vec<String> {
    let mut lines = Vec::new();
    let mut capture = false;
    for line in err_msg.lines() {
        if line.starts_with("Failed to find expected lines in ") {
            capture = true;
            continue;
        }
        if !capture {
            continue;
        }
        if line.trim().is_empty() {
            if !lines.is_empty() {
                break;
            }
            continue;
        }
        if line.starts_with("    ") || line.starts_with('\t') {
            lines.push(line.trim().to_string());
            continue;
        }
        if !lines.is_empty() {
            break;
        }
    }
    lines
}

fn patch_failure_guidance(path: Option<&str>, err_msg: &str) -> String {
    let mut hints = Vec::new();
    hints.push("Patch anchor miss: deleted/context lines must match the current file EXACTLY.".to_string());
    hints.push("Do not abbreviate deleted lines like `-1. Centralize d`; copy exact text from read_file output.".to_string());
    hints.push("Next step: emit `read_file` for the target file, then build a new patch with at least 3 unchanged context lines.".to_string());

    if let Some(file) = path {
        if file == DIAGNOSTICS_FILE || file.ends_with(".md") {
            hints.push("This is a prose/markdown file: prefer rewriting the whole section or the whole file instead of a tiny surgical hunk.".to_string());
            hints.push("For DIAGNOSTICS.md, one full-file rewrite is usually more reliable than repeated partial patches.".to_string());
        }
    }

    let anchors = extract_expected_anchor_lines(err_msg);
    if !anchors.is_empty() {
        hints.push(format!("Failed anchor lines: {}", anchors.join(" | ")));
    }

    hints.join("\n")
}

/// Find the file region closest to the failed anchor and return a numbered excerpt.
fn extract_anchor_context_excerpt(full: &str, err_msg: &str) -> Option<(usize, usize, String)> {
    let anchor_lines = extract_expected_anchor_lines(err_msg);
    if anchor_lines.is_empty() {
        return None;
    }
    let file_lines: Vec<&str> = full.lines().collect();
    let mut best_idx: Option<usize> = None;
    for anchor in anchor_lines.iter().rev() {
        let needle = anchor.trim();
        if needle.len() < 8 {
            continue;
        }
        if let Some(idx) = file_lines.iter().position(|l| l.contains(needle)) {
            best_idx = Some(idx);
            break;
        }
    }
    let idx = best_idx?;
    let start_idx = idx.saturating_sub(AUTO_READ_CONTEXT_BEFORE);
    let end_idx = (idx + AUTO_READ_CONTEXT_AFTER + 1).min(file_lines.len());
    let start_line = start_idx + 1;
    let excerpt = file_lines[start_idx..end_idx].iter().enumerate().map(|(i, l)| format!("{}: {}", start_line + i, l)).collect::<Vec<_>>().join("\n");
    Some((start_line, end_idx, excerpt))
}

/// Auto-read the region near the failed anchor, falling back to the full file.
fn auto_read_for_patch_anchor(workspace: &Path, relative: &str, err_msg: &str) -> Result<String> {
    let path = safe_join(workspace, relative)?;
    let full = std::fs::read_to_string(&path).with_context(|| format!("auto-read failed: {}", path.display()))?;
    if let Some((start, end, excerpt)) = extract_anchor_context_excerpt(&full, err_msg) {
        return Ok(format!("Current content near likely match of failed anchor in {relative} (lines {start}-{end}):\n{excerpt}"));
    }
    // Fallback: first MAX_FULL_READ_LINES lines of the file.
    let text = full.lines().take(MAX_FULL_READ_LINES).enumerate().map(|(i, l)| format!("{}: {}", i + 1, l)).collect::<Vec<_>>().join("\n");
    Ok(format!("Current content of {relative}:\n{text}"))
}

// ── Action executors ───────────────────────────────────────────────────────────

fn exec_list_dir(workspace: &Path, relative: &str) -> Result<String> {
    let path = safe_join(workspace, relative)?;
    let mut entries = std::fs::read_dir(&path).with_context(|| format!("list_dir: {}", path.display()))?.flatten().map(|e| e.file_name().to_string_lossy().into_owned()).collect::<Vec<_>>();
    entries.sort();
    Ok(entries.join("\n"))
}

fn exec_read_file(workspace: &Path, relative: &str, start_line: Option<usize>) -> Result<String> {
    let path = safe_join(workspace, relative)?;
    let full = std::fs::read_to_string(&path).with_context(|| format!("read_file: {}", path.display()))?;
    let lines: Vec<&str> = full.lines().collect();
    let total = lines.len();
    let (from, max_lines) = match start_line {
        Some(n) => (n.saturating_sub(1).min(total), 250),
        None => (0, MAX_FULL_READ_LINES),
    };
    let text = lines[from..].iter().take(max_lines).enumerate().map(|(i, l)| format!("{}: {}", from + i + 1, l)).collect::<Vec<_>>().join("\n");
    let shown = max_lines.min(total.saturating_sub(from));
    if total > from + shown {
        Ok(format!("{text}\n(file has {total} lines total; use \"line\":{} to read more)", from + shown + 1))
    } else {
        Ok(text)
    }
}

fn shell_tokens(cmd: &str) -> Vec<&str> {
    cmd.split(|c: char| c.is_whitespace() || matches!(c, '|' | '&' | ';' | '(' | ')' | '<' | '>'))
        .filter(|part| !part.is_empty())
        .collect()
}

fn contains_token_pair(cmd: &str, first: &str, second: &str) -> bool {
    let tokens = shell_tokens(cmd);
    tokens.windows(2).any(|window| window[0] == first && window[1] == second)
}

fn starts_direct_debug_binary(cmd: &str) -> bool {
    let first = shell_tokens(cmd).into_iter().next().unwrap_or("");
    first.starts_with("./target/debug/") || first.contains("/target/debug/")
}

fn looks_like_long_running_command(cmd: &str) -> bool {
    contains_token_pair(cmd, "cargo", "run")
        || contains_token_pair(cmd, "cargo", "watch")
        || starts_direct_debug_binary(cmd)
        || cmd.contains(" --tlog ")
        || cmd.contains("| tee")
}

fn exec_run_command(workspace: &Path, cmd: &str, cwd: &str) -> Result<(bool, String)> {
    let cwd_path = PathBuf::from(cwd);
    if !cwd_path.is_absolute() {
        bail!("run_command cwd must be absolute: {cwd}");
    }
    if !cwd_path.starts_with(workspace) {
        bail!("run_command cwd escapes workspace: {cwd}");
    }
    ensure_safe_command(cmd)?;
    // Hybrid execution model:
    // - long-running commands → spawn (non-blocking)
    // - short commands → capture output (blocking)

    let is_long_running = looks_like_long_running_command(cmd);

    if is_long_running {
        let child = Command::new("/bin/bash")
            .arg("-c")
            .arg(cmd)
            .current_dir(&cwd_path)
            .stdin(Stdio::null())
            .stdout(Stdio::inherit())
            .stderr(Stdio::inherit())
            .spawn()
            .with_context(|| format!("failed to spawn: {cmd}"))?;

        Ok((true, format!("spawned pid={}", child.id())))
    } else {
        let output = Command::new("/bin/bash").arg("-c").arg(cmd).current_dir(&cwd_path).output().with_context(|| format!("failed to spawn: {cmd}"))?;

        let mut combined = String::from_utf8_lossy(&output.stdout).into_owned();
        if !output.stderr.is_empty() {
            if !combined.is_empty() {
                combined.push('\n');
            }
            combined.push_str(&String::from_utf8_lossy(&output.stderr));
        }
        if combined.trim().is_empty() && !output.status.success() {
            if cmd.contains("rg ") || cmd.contains("grep ") {
                combined = format!("no matches (exit={})", output.status.code().unwrap_or(-1));
                if cmd.contains("/tmp/runtime.trace") {
                    combined.push_str("\ntrace probe returned no matches; file may be stale, missing, or the pattern may not be present yet");
                }
            }
        }
        if cmd.contains("/tmp/runtime.trace") && (cmd.contains("rg ") || cmd.contains("grep ")) {
            let trace = PathBuf::from("/tmp/runtime.trace");
            match std::fs::metadata(&trace) {
                Ok(meta) => {
                    combined.push_str(&format!("\ntrace_path=/tmp/runtime.trace trace_size={}B", meta.len()));
                }
                Err(_) => {
                    combined.push_str("\ntrace_path=/tmp/runtime.trace trace_missing=true");
                }
            }
        }

        Ok((output.status.success(), combined))
    }
}

fn exec_python(workspace: &Path, code: &str, cwd: &str) -> Result<(bool, String)> {
    let cwd_path = PathBuf::from(cwd);
    if !cwd_path.is_absolute() {
        bail!("python cwd must be absolute: {cwd}");
    }
    if !cwd_path.starts_with(workspace) {
        bail!("python cwd escapes workspace: {cwd}");
    }
    let mut child = Command::new("python3")
        .arg("-")
        .current_dir(&cwd_path)
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()
        .with_context(|| format!("failed to spawn python3 in {}", cwd_path.display()))?;
    if let Some(stdin) = child.stdin.as_mut() {
        stdin.write_all(code.as_bytes()).context("failed writing python stdin")?;
    }
    let output = child.wait_with_output().context("failed waiting for python3")?;
    let mut combined = String::from_utf8_lossy(&output.stdout).into_owned();
    if !output.stderr.is_empty() {
        if !combined.is_empty() {
            combined.push('\n');
        }
        combined.push_str(&String::from_utf8_lossy(&output.stderr));
    }
    Ok((output.status.success(), combined))
}

fn ensure_safe_command(cmd: &str) -> Result<()> {
    const BLOCKED: &[&str] = &["rm -rf", "git reset --hard", "git clean -f", "dd if=", "mkfs", "shred"];
    for needle in BLOCKED {
        if cmd.contains(needle) {
            bail!("blocked command: {cmd}");
        }
    }
    Ok(())
}

fn safe_join(workspace: &Path, relative: &str) -> Result<PathBuf> {
    let p = Path::new(relative);
    if p.is_absolute() {
        bail!("absolute paths not allowed: {relative}");
    }
    if p.components().any(|c| matches!(c, Component::ParentDir)) {
        bail!("path traversal not allowed: {relative}");
    }
    Ok(workspace.join(p))
}

fn execute_action(
    role: &str,
    step: usize,
    action: &Value,
    workspace: &Path,
    check_on_done: bool,
) -> Result<(bool, String)> {
    let kind = action
        .get("action")
        .and_then(|v| v.as_str())
        .unwrap_or("unknown")
        .to_string();
    tokio::task::block_in_place(|| match kind.as_str() {
        "done" => {
            let reason = action
                .get("reason")
                .and_then(|v| v.as_str())
                .unwrap_or("complete");
            if !check_on_done {
                return Ok((true, reason.to_string()));
            }
            eprintln!("[{role}] step={} done — running cargo build --workspace", step);
            let (build_ok, build_out) = exec_run_command(workspace, "cargo build --workspace", WORKSPACE)
                .unwrap_or_else(|e| (false, e.to_string()));
            if !build_ok {
                eprintln!("[{role}] step={} cargo build failed — rejecting done", step);
                return Ok((
                    false,
                    format!(
                        "done rejected: cargo build --workspace failed.\n\n{}",
                        truncate(&build_out, MAX_SNIPPET)
                    ),
                ));
            }
            eprintln!("[{role}] step={} cargo build ok — running cargo test --workspace", step);
            let (test_ok, test_out) = exec_run_command(workspace, "cargo test --workspace", WORKSPACE)
                .unwrap_or_else(|e| (false, e.to_string()));
            if test_ok {
                eprintln!("[{role}] step={} cargo test ok — accepting done", step);
                Ok((true, reason.to_string()))
            } else {
                eprintln!("[{role}] step={} cargo test failed — rejecting done", step);
                Ok((
                    false,
                    format!(
                        "done rejected: cargo test --workspace failed.\n\n{}",
                        truncate(&test_out, MAX_SNIPPET)
                    ),
                ))
            }
        }
        "list_dir" => {
            let path = action
                .get("path")
                .and_then(|v| v.as_str())
                .ok_or_else(|| anyhow!("list_dir missing 'path'"))?;
            let out = exec_list_dir(workspace, path)?;
            Ok((false, format!("list_dir {path}:\n{out}")))
        }
        "read_file" => {
            let path = action
                .get("path")
                .and_then(|v| v.as_str())
                .ok_or_else(|| anyhow!("read_file missing 'path'"))?;
            let line = action
                .get("line")
                .and_then(|v| v.as_u64())
                .map(|n| n as usize);
            let out = exec_read_file(workspace, path, line)?;
            eprintln!("[{role}] step={} read_file path={path} bytes={}", step, out.len());
            Ok((false, format!("read_file {path}:\n{out}")))
        }
        "apply_patch" => {
            let patch = action
                .get("patch")
                .and_then(|v| v.as_str())
                .ok_or_else(|| anyhow!("apply_patch missing 'patch'"))?;
            if let Some(msg) = patch_scope_error(role, patch) {
                Ok((false, msg))
            } else {
                match apply_patch(patch, workspace) {
                    Ok(_) => {
                        eprintln!("[{role}] step={} apply_patch ok", step);
                        let check_result = patch_first_file(patch)
                            .and_then(|f| infer_crate_for_patch(workspace, f))
                            .map(|krate| {
                                eprintln!("[{role}] step={} cargo check -p {krate}", step);
                                exec_run_command(
                                    workspace,
                                    &format!("cargo check -p {krate}"),
                                    WORKSPACE,
                                )
                                .unwrap_or_else(|e| (false, e.to_string()))
                            });
                        match check_result {
                            Some((ok, out)) => {
                                let label = if ok {
                                    "cargo check ok"
                                } else {
                                    "cargo check failed"
                                };
                                eprintln!("[{role}] step={} {label}", step);
                                Ok((
                                    false,
                                    format!(
                                        "apply_patch ok\n\n{label}:\n{}",
                                        truncate(&out, MAX_SNIPPET)
                                    ),
                                ))
                            }
                            None => Ok((false, "apply_patch ok".to_string())),
                        }
                    }
                    Err(e) => {
                        let err_str = e.to_string();
                        eprintln!("[{role}] step={} apply_patch failed: {err_str}", step);
                        let read_path = extract_anchor_fail_path(&err_str)
                            .or_else(|| patch_first_file(patch).map(|s| s.to_string()));
                        let guidance = patch_failure_guidance(read_path.as_deref(), &err_str);
                        let mut msg = format!("apply_patch failed: {err_str}\n\n{guidance}");
                        if let Some(fp) = read_path {
                            if let Ok(content) = auto_read_for_patch_anchor(workspace, &fp, &err_str) {
                                eprintln!("[{role}] step={} auto_read path={fp}", step);
                                msg =
                                    format!("apply_patch failed: {err_str}\n\n{guidance}\n\n{content}");
                            }
                        }
                        Ok((false, msg))
                    }
                }
            }
        }
        "run_command" => {
            let cmd = action
                .get("cmd")
                .and_then(|v| v.as_str())
                .ok_or_else(|| anyhow!("run_command missing 'cmd'"))?;
            let cwd = action
                .get("cwd")
                .and_then(|v| v.as_str())
                .unwrap_or(WORKSPACE);
            eprintln!("[{role}] step={} run_command cmd={cmd}", step);
            let (success, out) = exec_run_command(workspace, cmd, cwd)?;
            let label = if success {
                "run_command ok"
            } else {
                "run_command failed"
            };
            eprintln!("[{role}] step={} {label} output_bytes={}", step, out.len());
            Ok((false, format!("{label}:\n{}", truncate(&out, MAX_SNIPPET))))
        }
        "python" => {
            let code = action
                .get("code")
                .and_then(|v| v.as_str())
                .ok_or_else(|| anyhow!("python missing 'code'"))?;
            let cwd = action
                .get("cwd")
                .and_then(|v| v.as_str())
                .unwrap_or(WORKSPACE);
            eprintln!("[{role}] step={} python bytes={}", step, code.len());
            let (success, out) = exec_python(workspace, code, cwd)?;
            let label = if success { "python ok" } else { "python failed" };
            eprintln!("[{role}] step={} {label} output_bytes={}", step, out.len());
            Ok((false, format!("{label}:\n{}", truncate(&out, MAX_SNIPPET))))
        }
        other => Ok((
            false,
            format!(
                "unsupported action '{other}' — use list_dir, read_file, apply_patch, run_command, python, or done"
            ),
        )),
    })
}

pub(crate) fn execute_logged_action(
    role: &str,
    prompt_kind: &str,
    endpoint: &LlmEndpoint,
    workspace: &Path,
    step: usize,
    command_id: &str,
    action: &Value,
    check_on_done: bool,
) -> Result<(bool, String)> {
    if let Err(e) = append_action_log(role, endpoint, prompt_kind, step, command_id, action) {
        eprintln!("[{role}] step={} action_log_error: {e}", step);
    }
    match execute_action(role, step, action, workspace, check_on_done) {
        Ok((done, out)) => {
            if let Err(e) = append_action_result_log(
                role,
                endpoint,
                prompt_kind,
                step,
                command_id,
                action,
                true,
                &out,
            ) {
                eprintln!("[{role}] step={} action_result_log_error: {e}", step);
            }
            Ok((done, out))
        }
        Err(e) => {
            let err_text = format!("Error executing action: {e}");
            if let Err(log_err) = append_action_result_log(
                role,
                endpoint,
                prompt_kind,
                step,
                command_id,
                action,
                false,
                &err_text,
            ) {
                eprintln!("[{role}] step={} action_result_log_error: {log_err}", step);
            }
            eprintln!("[{role}] step={} error: {e}", step);
            Ok((false, err_text))
        }
    }
}
