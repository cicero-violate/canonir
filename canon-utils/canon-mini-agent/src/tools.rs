use anyhow::{anyhow, bail, Context, Result};
use canon_llm::config::LlmEndpoint;
use canon_tools_patch::apply_patch;
use serde_json::Value;
use std::io::Write;
use std::collections::BTreeSet;
use std::path::{Component, Path, PathBuf};
use std::process::{Command, Stdio};
use std::fs;

use crate::constants::{
    diagnostics_file, MASTER_PLAN_FILE, MAX_FULL_READ_LINES, MAX_SNIPPET, SPEC_FILE,
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
    if !path.starts_with("PLANS/") {
        return false;
    }
    let is_json = path.ends_with(".json");
    let is_md = path.ends_with(".md");
    if !is_json && !is_md {
        return false;
    }
    // Allow both legacy and instance-scoped lane plans:
    // - PLANS/executor-<id>.json
    // - PLANS/<instance>/executor-<id>.json
    // - legacy .md variants
    path.starts_with("PLANS/executor-") || path.contains("/executor-")
}

fn default_graph_out_dir(workspace: &Path, crate_name: &str) -> PathBuf {
    workspace
        .join("state")
        .join("reports_out")
        .join("crates")
        .join(crate_name)
}

fn read_first_lines(path: &Path, max_lines: usize, max_bytes: usize) -> Result<String> {
    let content = fs::read_to_string(path).with_context(|| format!("failed to read {}", path.display()))?;
    let mut out = String::new();
    for (idx, line) in content.lines().enumerate() {
        if idx >= max_lines || out.len() >= max_bytes {
            break;
        }
        out.push_str(line);
        out.push('\n');
    }
    Ok(out)
}

fn read_json_report(path: &Path, max_bytes: usize) -> Result<String> {
    let content = fs::read_to_string(path).with_context(|| format!("failed to read {}", path.display()))?;
    let trimmed = truncate(&content, max_bytes);
    Ok(trimmed.to_string())
}

fn load_graph_symbols(graph_json: &Path) -> Result<std::collections::HashMap<u32, (String, String)>> {
    let content = fs::read_to_string(graph_json).with_context(|| format!("failed to read {}", graph_json.display()))?;
    let value: Value = serde_json::from_str(&content)?;
    let mut out = std::collections::HashMap::new();
    let nodes = value.get("nodes").and_then(|v| v.as_array()).cloned().unwrap_or_default();
    for node in nodes {
        let id = node.get("id").and_then(|v| v.as_u64()).unwrap_or(0) as u32;
        if id == 0 {
            continue;
        }
        let kind = node.get("kind").and_then(|v| v.as_str()).unwrap_or("").to_string();
        let symbol = node.get("symbol").and_then(|v| v.as_str()).unwrap_or("").to_string();
        out.insert(id, (kind, symbol));
    }
    Ok(out)
}

fn symbol_label(map: &std::collections::HashMap<u32, (String, String)>, raw: &str) -> String {
    let id = raw.parse::<u32>().ok();
    if let Some(id) = id {
        if let Some((kind, symbol)) = map.get(&id) {
            if symbol.is_empty() {
                return format!("{id} {kind}").trim().to_string();
            }
            return format!("{id} {kind} {symbol}").trim().to_string();
        }
        return id.to_string();
    }
    raw.to_string()
}

fn patch_scope_error(role: &str, patch: &str) -> Option<String> {
    let targets = patch_targets(patch);
    if targets.is_empty() {
        return None;
    }

    let diagnostics_file = diagnostics_file();
    let legacy_diagnostics_file = "DIAGNOSTICS.md";
    let touches_spec = targets.iter().any(|path| *path == SPEC_FILE);
    let touches_lane = targets.iter().any(|path| is_lane_plan(path));
    let touches_master_plan = targets.iter().any(|path| *path == MASTER_PLAN_FILE);
    let touches_violations = targets.iter().any(|path| *path == VIOLATIONS_FILE);
    let touches_diagnostics = targets.iter().any(|path| *path == diagnostics_file || *path == legacy_diagnostics_file);
    let touches_other = targets
        .iter()
        .any(|path| *path != SPEC_FILE
            && *path != MASTER_PLAN_FILE
            && !is_lane_plan(path)
            && *path != VIOLATIONS_FILE
            && *path != diagnostics_file
            && *path != legacy_diagnostics_file);

    match role {
        role if role.starts_with("executor") => {
            if touches_spec || touches_master_plan || touches_lane || touches_violations || touches_diagnostics {
                Some(
                    "Executor may not patch spec, plan files, violations, or diagnostics. Execute code/tests only and report evidence in `message.payload`."
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
                    "Planner may only patch `PLAN.json` and lane plans under `PLANS/<instance>/executor-<id>.json` (or legacy `PLANS/executor-<id>.md`) because planner derives plans from the spec and diagnostics."
                        .to_string(),
                )
            } else if touches_master_plan || touches_lane {
                None
            } else {
                Some(
                    "Planner must update `PLAN.json` and lane plans; no other patches are allowed."
                        .to_string(),
                )
            }
        }
        "diagnostics" => {
            if touches_spec || touches_master_plan || touches_lane || touches_violations || touches_other {
                Some(
                    format!(
                        "Diagnostics may only patch {} or {} because diagnostics owns ranked failure reporting.",
                        diagnostics_file,
                        legacy_diagnostics_file
                    ),
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
        let diagnostics_file = diagnostics_file();
        let legacy_diagnostics_file = "DIAGNOSTICS.md";
        if file == diagnostics_file || file == legacy_diagnostics_file || file.ends_with(".md") {
            hints.push("This is a prose/markdown file: prefer rewriting the whole section or the whole file instead of a tiny surgical hunk.".to_string());
            hints.push(format!(
                "For {}, one full-file rewrite is usually more reliable than repeated partial patches.",
                diagnostics_file
            ));
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
        Some(n) => (n.saturating_sub(1).min(total), 300),
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
        "message" => {
            let status = action.get("status").and_then(|v| v.as_str()).unwrap_or("");
            let payload = action.get("payload").cloned().unwrap_or_else(|| Value::Null);
            let summary = payload
                .get("summary")
                .and_then(|v| v.as_str())
                .unwrap_or("message accepted");
            if status == "complete" && check_on_done {
                eprintln!("[{role}] step={} message complete — running cargo build --workspace", step);
                let (build_ok, build_out) = exec_run_command(workspace, "cargo build --workspace", WORKSPACE)
                    .unwrap_or_else(|e| (false, e.to_string()));
                if !build_ok {
                    eprintln!("[{role}] step={} cargo build failed — rejecting message", step);
                    return Ok((
                        false,
                        format!(
                            "message rejected: cargo build --workspace failed.\n\n{}",
                            truncate(&build_out, MAX_SNIPPET)
                        ),
                    ));
                }
                eprintln!("[{role}] step={} cargo build ok — running cargo test --workspace", step);
                let (test_ok, test_out) = exec_run_command(workspace, "cargo test --workspace", WORKSPACE)
                    .unwrap_or_else(|e| (false, e.to_string()));
                if test_ok {
                    eprintln!("[{role}] step={} cargo test ok — accepting message", step);
                    Ok((true, summary.to_string()))
                } else {
                    eprintln!("[{role}] step={} cargo test failed — rejecting message", step);
                    Ok((
                        false,
                        format!(
                            "message rejected: cargo test --workspace failed.\n\n{}",
                            truncate(&test_out, MAX_SNIPPET)
                        ),
                    ))
                }
            } else {
                Ok((true, summary.to_string()))
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
        k @ ("rustc_hir" | "rustc_mir") => {
            let action_kind = k;
            let crate_name = action
                .get("crate")
                .and_then(|v| v.as_str())
                .ok_or_else(|| anyhow!("{kind} missing 'crate'", kind = action_kind))?;
            let mode = action
                .get("mode")
                .and_then(|v| v.as_str())
                .unwrap_or(if action_kind == "rustc_hir" { "hir-tree" } else { "mir" });
            let extra = action
                .get("extra")
                .and_then(|v| v.as_str())
                .unwrap_or("");
            let cmd = if extra.trim().is_empty() {
                format!("cargo rustc -p {crate_name} -- -Zunpretty={mode}")
            } else {
                format!("cargo rustc -p {crate_name} -- -Zunpretty={mode} {extra}")
            };
            eprintln!("[{role}] step={} {action_kind} cmd={cmd}", step);
            let (success, out) = exec_run_command(workspace, &cmd, WORKSPACE)?;
            let label = if success {
                format!("{action_kind} ok")
            } else {
                format!("{action_kind} failed")
            };
            eprintln!("[{role}] step={} {label} output_bytes={}", step, out.len());
            Ok((false, format!("{label}:\n{}", truncate(&out, MAX_SNIPPET))))
        }
        "graph_probe" => {
            let crate_name = action.get("crate").and_then(|v| v.as_str());
            let entry = action.get("entry").and_then(|v| v.as_str());
            let tlog = action.get("tlog").and_then(|v| v.as_str());
            let symbol_limit = action.get("symbol_limit").and_then(|v| v.as_u64()).unwrap_or(50);
            let unreachable_limit = action.get("unreachable_limit").and_then(|v| v.as_u64()).unwrap_or(20);
            let cfg_limit = action.get("cfg_limit").and_then(|v| v.as_u64()).unwrap_or(20);
            let mut cmd = format!(
                "cargo run -p canon-tools-analysis --bin graph_probe -- --workspace {} --symbol-limit {} --unreachable-limit {} --cfg-limit {}",
                WORKSPACE, symbol_limit, unreachable_limit, cfg_limit
            );
            if let Some(name) = crate_name {
                cmd.push_str(&format!(" --crate {name}"));
            }
            if let Some(val) = entry {
                cmd.push_str(&format!(" --entry {val}"));
            }
            if let Some(path) = tlog {
                cmd.push_str(&format!(" --tlog {path}"));
            }
            eprintln!("[{role}] step={} graph_probe cmd={cmd}", step);
            let (success, out) = exec_run_command(workspace, &cmd, WORKSPACE)?;
            let label = if success { "graph_probe ok" } else { "graph_probe failed" };
            eprintln!("[{role}] step={} {label} output_bytes={}", step, out.len());
            Ok((false, format!("{label}:\n{}", truncate(&out, MAX_SNIPPET))))
        }
        k @ ("graph_call" | "graph_cfg") => {
            let action_kind = k;
            let crate_name = action
                .get("crate")
                .and_then(|v| v.as_str())
                .ok_or_else(|| anyhow!("{kind} missing 'crate'", kind = action_kind))?;
            let out_dir = action
                .get("out_dir")
                .and_then(|v| v.as_str())
                .map(PathBuf::from)
                .unwrap_or_else(|| default_graph_out_dir(workspace, crate_name));
            let out_dir_str = out_dir.to_string_lossy();
            let cmd = format!(
                "cargo run -p canon-tools-analysis --bin graph_bin -- --workspace {} --crate {} --out {}",
                WORKSPACE, crate_name, out_dir_str
            );
            eprintln!("[{role}] step={} {action_kind} cmd={cmd}", step);
            let (success, out) = exec_run_command(workspace, &cmd, WORKSPACE)?;
            let label = if success {
                format!("{action_kind} ok")
            } else {
                format!("{action_kind} failed")
            };
            let target_path = if action_kind == "graph_call" {
                out_dir.join("graphs").join("callgraph.csv")
            } else {
                out_dir.join("graphs").join("cfg.csv")
            };
            let preview = if target_path.exists() { read_first_lines(&target_path, 50, MAX_SNIPPET)? } else { String::new() };
            let mut symbol_preview = String::new();
            let mut symbol_path = None;
            if target_path.exists() {
                let mut out_lines = Vec::new();
                let content = fs::read_to_string(&target_path)?;
                let mut lines = content.lines();
                let header = lines.next().unwrap_or("");
                let header_cols: Vec<&str> = header.split(',').collect();
                let has_symbol_cols = header_cols.iter().any(|c| *c == "caller_symbol" || *c == "callee_symbol");
                let map = if !has_symbol_cols {
                    let graph_json = out_dir.join("graph").join("graph.json");
                    if graph_json.exists() {
                        Some(load_graph_symbols(&graph_json)?)
                    } else {
                        None
                    }
                } else {
                    None
                };
                let mut count = 0usize;
                for line in lines {
                    if count >= 200 {
                        break;
                    }
                    let cols: Vec<&str> = line.split(',').collect();
                    if has_symbol_cols {
                        let caller_idx = header_cols.iter().position(|c| *c == "caller_symbol");
                        let callee_idx = header_cols.iter().position(|c| *c == "callee_symbol");
                        let caller = caller_idx.and_then(|i| cols.get(i)).map(|s| s.trim()).unwrap_or("");
                        let callee = callee_idx.and_then(|i| cols.get(i)).map(|s| s.trim()).unwrap_or("");
                        if !caller.is_empty() || !callee.is_empty() {
                            out_lines.push(format!("{caller} -> {callee}"));
                            count += 1;
                            continue;
                        }
                    }
                    if cols.len() < 2 {
                        continue;
                    }
                    let src = cols[0].trim();
                    let dst = cols[1].trim();
                    if let Some(map) = map.as_ref() {
                        out_lines.push(format!("{} -> {}", symbol_label(map, src), symbol_label(map, dst)));
                    } else {
                        out_lines.push(format!("{src} -> {dst}"));
                    }
                    count += 1;
                }
                if !out_lines.is_empty() {
                    symbol_preview = out_lines.join("\n");
                    let fname = if action_kind == "graph_call" { "callgraph.symbol.txt" } else { "cfg.symbol.txt" };
                    let out_path = out_dir.join("graphs").join(fname);
                    if let Some(parent) = out_path.parent() {
                        fs::create_dir_all(parent)?;
                    }
                    fs::write(&out_path, format!("{}\n", symbol_preview))?;
                    symbol_path = Some(out_path);
                }
            }
            let mut summary = format!(
                "{label}\noutput_dir: {}\n{}",
                out_dir_str,
                target_path.display()
            );
            if !preview.is_empty() {
                summary.push_str("\npreview:\n");
                summary.push_str(&preview);
            }
            if let Some(path) = symbol_path {
                summary.push_str(&format!("\nsymbol_edges: {}", path.display()));
                if !symbol_preview.is_empty() {
                    summary.push_str("\nsymbol_preview:\n");
                    summary.push_str(&symbol_preview);
                }
            }
            Ok((false, format!("{summary}\n\nfull_output:\n{}", truncate(&out, MAX_SNIPPET))))
        }
        k @ ("graph_dataflow" | "graph_reachability") => {
            let action_kind = k;
            let crate_name = action
                .get("crate")
                .and_then(|v| v.as_str())
                .ok_or_else(|| anyhow!("{kind} missing 'crate'", kind = action_kind))?;
            let tlog = action.get("tlog").and_then(|v| v.as_str());
            let out_dir = action
                .get("out_dir")
                .and_then(|v| v.as_str())
                .map(PathBuf::from)
                .unwrap_or_else(|| default_graph_out_dir(workspace, crate_name));
            let out_dir_str = out_dir.to_string_lossy();
            let mut cmd = format!(
                "cargo run -p canon-tools-analysis --bin graph_reports -- --workspace {} --crate {} --out {}",
                WORKSPACE, crate_name, out_dir_str
            );
            if let Some(path) = tlog {
                cmd.push_str(&format!(" --tlog {path}"));
            }
            eprintln!("[{role}] step={} {action_kind} cmd={cmd}", step);
            let (success, out) = exec_run_command(workspace, &cmd, WORKSPACE)?;
            let label = if success {
                format!("{action_kind} ok")
            } else {
                format!("{action_kind} failed")
            };
            let (report_path, report_label) = if action_kind == "graph_dataflow" {
                (out_dir.join("metrics").join("dataflow_fanout_report.json"), "dataflow_fanout_report.json")
            } else {
                let runtime_path = out_dir.join("analysis").join("runtime_reachability_report.json");
                if runtime_path.exists() {
                    (runtime_path, "runtime_reachability_report.json")
                } else {
                    (out_dir.join("metrics").join("reachability_report.json"), "reachability_report.json")
                }
            };
            let report_preview = if report_path.exists() {
                read_json_report(&report_path, MAX_SNIPPET)?
            } else {
                String::new()
            };
            let mut summary = format!(
                "{label}\noutput_dir: {}\nreport: {}",
                out_dir_str,
                report_path.display()
            );
            if !report_preview.is_empty() {
                summary.push_str("\nreport_preview:\n");
                summary.push_str(&report_preview);
            } else {
                summary.push_str(&format!("\nreport_note: {} not found", report_label));
            }
            Ok((false, format!("{summary}\n\nfull_output:\n{}", truncate(&out, MAX_SNIPPET))))
        }
        "cargo_test" => {
            let crate_name = action
                .get("crate")
                .and_then(|v| v.as_str())
                .ok_or_else(|| anyhow!("cargo_test missing 'crate'"))?;
            let test_name = action
                .get("test")
                .and_then(|v| v.as_str())
                .ok_or_else(|| anyhow!("cargo_test missing 'test'"))?;
            let cmd = format!(
                "cargo test -p {} {} -- --exact --nocapture",
                crate_name, test_name
            );
            eprintln!("[{role}] step={} cargo_test cmd={}", step, cmd);
            let (success, out) = exec_run_command(workspace, &cmd, WORKSPACE)?;
            let label = if success { "cargo_test ok" } else { "cargo_test failed" };
            eprintln!("[{role}] step={} {label} output_bytes={}", step, out.len());
            let mut locations = BTreeSet::new();
            let mut failing_tests = BTreeSet::new();
            let mut rerun_hint = None;
            let mut failure_block = Vec::new();
            let mut in_failure_block = false;
            for line in out.lines() {
                let trimmed = line.trim();
                if let Some(rest) = trimmed.strip_prefix("test ") {
                    if let Some(name) = rest.strip_suffix(" ... FAILED") {
                        failing_tests.insert(name.to_string());
                    }
                }
                if let Some(rest) = trimmed.strip_prefix("error: test failed, to rerun pass ") {
                    rerun_hint = Some(rest.trim().trim_matches('`').to_string());
                }
                if trimmed.starts_with("---- ") && trimmed.ends_with(" ----") {
                    in_failure_block = true;
                } else if in_failure_block && trimmed == "failures:" {
                    in_failure_block = false;
                }
                if in_failure_block {
                    failure_block.push(line);
                }
                // capture file:line:col (keep only workspace-relative or absolute paths)
                if let Some(idx) = trimmed.find(".rs:") {
                    let path = &trimmed[..idx + 3];
                    let rest = &trimmed[idx + 3..];
                    let mut it = rest.splitn(3, ':');
                    let line_no = it.next().unwrap_or("");
                    let col_no = it.next().unwrap_or("");
                    if !line_no.is_empty() && !col_no.is_empty() {
                        locations.insert(format!("{}:{}:{}", path, line_no, col_no));
                    }
                }
            }
            let mut summary = format!("{label}");
            if !failing_tests.is_empty() {
                summary.push_str("\nfailed_tests:");
                for name in &failing_tests {
                    summary.push_str(&format!("\n- {}", name));
                }
            }
            if !locations.is_empty() {
                summary.push_str("\nerror_locations:");
                for loc in &locations {
                    summary.push_str(&format!("\n- {}", loc));
                }
            }
            if let Some(hint) = rerun_hint {
                summary.push_str(&format!("\nrerun_hint: {}", hint));
            }
            if !failure_block.is_empty() {
                summary.push_str("\nfailure_block:");
                for line in &failure_block {
                    summary.push_str("\n");
                    summary.push_str(line);
                }
            }
            Ok((false, format!("{summary}\n\nfull_output:\n{}", truncate(&out, MAX_SNIPPET))))
        }
        other => Ok((
            false,
            format!(
                "unsupported action '{other}' — use list_dir, read_file, apply_patch, run_command, python, cargo_test, rustc_hir, rustc_mir, graph_probe, graph_call, graph_cfg, graph_dataflow, graph_reachability, or message"
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
