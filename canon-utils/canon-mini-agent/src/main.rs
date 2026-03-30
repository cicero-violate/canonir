use anyhow::{anyhow, bail, Context, Result};
use canon_llm::{
    config::CapabilityConfig,
    endpoint_worker::{llm_worker_new_tabs, llm_worker_send_request},
    ws_server,
};
use canon_tools_patch::apply_patch;
use serde_json::Value;
use std::path::{Component, Path, PathBuf};
use std::process::Command;
use std::sync::{Arc, OnceLock};

const WORKSPACE: &str = "/workspace/ai_sandbox/canon";
const PLAN_FILE: &str = "PLANS/mini-agent-plan.md";
const WS_PORT_DEFAULT: u16 = 9100;
const MAX_STEPS: usize = 2000;
const MAX_FULL_READ_LINES: usize = 500;
const MAX_SNIPPET: usize = 3000;

const SYSTEM_INSTRUCTIONS_EXECUTOR: &str = r#"You are the canon mini-agent.

Your job is to complete the objective described in the plan provided to you. Read the plan carefully and execute it step by step.

You work inside the canon workspace at /workspace/ai_sandbox/canon. All relative file paths resolve against this workspace root.

Each turn you receive either:
  (a) the initial objective and workspace context; or
  (b) the result of your last action.

You respond with exactly one action per turn, wrapped in a `json` code block:

```json
[ { "action": "..." } ]
```

━━━ TOOLS ━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━

1. list_dir — list directory contents
   {"action":"list_dir","path":"canon-utils"}

2. read_file — read a file before editing; output is always line-numbered ("42: code here")
   {"action":"read_file","path":"canon-utils/some-crate/src/lib.rs"}
   {"action":"read_file","path":"canon-utils/some-crate/src/lib.rs","line":120}
   With "line":N the output starts at line N and shows up to 250 lines.
   ⚠ Always read a file before patching it. Never patch from memory.
   ⚠ When applying a patch, copy context lines exactly as shown — the numbers are for reference only.

3. apply_patch — create or update files
   {"action":"apply_patch","patch":"*** Begin Patch\n*** Update File: path/to/file.rs\n@@\n context\n+added line\n context\n*** End Patch"}
   {"action":"apply_patch","patch":"*** Begin Patch\n*** Add File: path/to/new.rs\n+line one\n+line two\n*** End Patch"}

4. run_command — run shell commands for discovery or verification
   {"action":"run_command","cmd":"cargo check -p some-crate","cwd":"/workspace/ai_sandbox/canon"}
   {"action":"run_command","cmd":"rg -n 'fn foo' canon-utils/some-crate/src/","cwd":"/workspace/ai_sandbox/canon"}

5. done — declare the objective complete (triggers cargo build --workspace then cargo test --workspace)
   {"action":"done","reason":"brief description of what was accomplished"}
   ⚠ done is REJECTED if the build or any test fails — fix all errors first.

━━━ PROGRESS TRACKING ━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━

After completing each task or sub-task from the plan, immediately update PLANS/mini-agent-plan.md
to mark it as done. Use apply_patch to replace the task line with a checked version:

  - [ ] task description   →   - [x] task description  ✓ done
  - task description       →   - [x] task description  ✓ done

Read the plan file first if you need to see its current state before patching.
Keep the rest of the plan file intact — only change the line(s) you just completed.

━━━ RULES ━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━

- Emit exactly one action per turn.
- Always read a file before patching it.
- Use list_dir and read_file freely before assuming project state.
- Use run_command for cargo builds, tests, and shell discovery.
- Never operate outside /workspace/ai_sandbox/canon.
- Never emit destructive commands (rm -rf, git reset --hard, git clean -f, etc.).
- Output format: exactly one JSON array in a ```json code block. No prose outside it.
"#;

const SYSTEM_INSTRUCTIONS_VERIFIER: &str = r#"You are the canon verifier agent.

Your job is to critically review PLANS/mini-agent-plan.md and verify that every task marked as complete (`- [x]`) was actually completed correctly in the codebase. Be skeptical — do not trust the status marks at face value.

You work inside the canon workspace at /workspace/ai_sandbox/canon.

Each turn you receive either:
  (a) the initial plan and instructions; or
  (b) the result of your last action.

You respond with exactly one action per turn, wrapped in a `json` code block:

```json
[ { "action": "..." } ]
```

━━━ TOOLS ━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━

1. list_dir — explore directory contents
   {"action":"list_dir","path":"canon-utils"}

2. read_file — read a source file; output is line-numbered ("42: code here")
   {"action":"read_file","path":"canon-utils/some-crate/src/lib.rs"}
   {"action":"read_file","path":"canon-utils/some-crate/src/lib.rs","line":120}
   With "line":N the output starts at line N and shows up to 250 lines.

3. apply_patch — correct the plan file if a status mark is wrong
   {"action":"apply_patch","patch":"*** Begin Patch\n*** Update File: PLANS/mini-agent-plan.md\n@@\n- [x] task ✓ done\n+- [ ] task  ← NOT VERIFIED\n*** End Patch"}

4. run_command — run build/test commands to verify correctness
   {"action":"run_command","cmd":"cargo check -p some-crate","cwd":"/workspace/ai_sandbox/canon"}
   {"action":"run_command","cmd":"cargo test --workspace","cwd":"/workspace/ai_sandbox/canon"}
   {"action":"run_command","cmd":"rg -n 'fn foo'","cwd":"/workspace/ai_sandbox/canon"}

5. done — declare verification complete
   {"action":"done","reason":"summary of findings: N tasks verified, M incorrect or missing"}
   ⚠ done triggers cargo build --workspace then cargo test --workspace — fix any failures first.

━━━ VERIFICATION PROCESS ━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━

For each task marked `- [x]` in the plan:
1. Read the relevant source files to confirm the described change exists.
2. Run cargo check or cargo test if the task involves code correctness.
3. If the task is NOT actually done: use apply_patch to revert its status to `- [ ]` and add a note.
4. If the task IS done correctly: leave it as-is.

━━━ RULES ━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━

- Be critical and thorough — verify evidence, not just the claim.
- Do not mark anything verified unless you have read the actual code or seen passing tests.
- Only modify PLANS/mini-agent-plan.md — never edit source files.
- Emit exactly one action per turn.
- Output format: exactly one JSON array in a ```json code block. No prose outside it.
"#;

// ── Action parsing ─────────────────────────────────────────────────────────────

fn parse_actions(raw: &str) -> Result<Vec<Value>> {
    if let Some(json_text) = extract_json_fence(raw) {
        return parse_json_array(json_text)
            .with_context(|| "fenced json block was not a valid action array");
    }
    parse_json_array(raw.trim()).with_context(|| {
        format!(
            "response was not a JSON action array: {:?}",
            &raw.chars().take(200).collect::<String>()
        )
    })
}

fn extract_json_fence(text: &str) -> Option<&str> {
    let start = text.find("```json").or_else(|| text.find("```JSON"))?;
    let after_newline = start + text[start..].find('\n')?;
    let rest = &text[after_newline + 1..];
    let end = rest.find("```")?;
    Some(rest[..end].trim())
}

fn parse_json_array(text: &str) -> Result<Vec<Value>> {
    if let Ok(arr) = serde_json::from_str::<Vec<Value>>(text) {
        if arr.is_empty() {
            bail!("empty action array");
        }
        return Ok(arr);
    }
    if let Ok(obj) = serde_json::from_str::<Value>(text) {
        if obj.is_object() && obj.get("action").is_some() {
            return Ok(vec![obj]);
        }
    }
    bail!("not a JSON array: {:?}", &text.chars().take(120).collect::<String>())
}

// ── Patch crate inference ──────────────────────────────────────────────────────

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
    let excerpt = file_lines[start_idx..end_idx]
        .iter()
        .enumerate()
        .map(|(i, l)| format!("{}: {}", start_line + i, l))
        .collect::<Vec<_>>()
        .join("\n");
    Some((start_line, end_idx, excerpt))
}

/// Auto-read the region near the failed anchor, falling back to the full file.
fn auto_read_for_patch_anchor(workspace: &Path, relative: &str, err_msg: &str) -> Result<String> {
    let path = safe_join(workspace, relative)?;
    let full = std::fs::read_to_string(&path)
        .with_context(|| format!("auto-read failed: {}", path.display()))?;
    if let Some((start, end, excerpt)) = extract_anchor_context_excerpt(&full, err_msg) {
        return Ok(format!(
            "Current content near likely match of failed anchor in {relative} (lines {start}-{end}):\n{excerpt}"
        ));
    }
    // Fallback: first MAX_FULL_READ_LINES lines of the file.
    let text = full.lines()
        .take(MAX_FULL_READ_LINES)
        .enumerate()
        .map(|(i, l)| format!("{}: {}", i + 1, l))
        .collect::<Vec<_>>()
        .join("\n");
    Ok(format!("Current content of {relative}:\n{text}"))
}

// ── Action executors ───────────────────────────────────────────────────────────

fn exec_list_dir(workspace: &Path, relative: &str) -> Result<String> {
    let path = safe_join(workspace, relative)?;
    let mut entries = std::fs::read_dir(&path)
        .with_context(|| format!("list_dir: {}", path.display()))?
        .flatten()
        .map(|e| e.file_name().to_string_lossy().into_owned())
        .collect::<Vec<_>>();
    entries.sort();
    Ok(entries.join("\n"))
}

fn exec_read_file(workspace: &Path, relative: &str, start_line: Option<usize>) -> Result<String> {
    let path = safe_join(workspace, relative)?;
    let full = std::fs::read_to_string(&path)
        .with_context(|| format!("read_file: {}", path.display()))?;
    let lines: Vec<&str> = full.lines().collect();
    let total = lines.len();
    let (from, max_lines) = match start_line {
        Some(n) => (n.saturating_sub(1).min(total), 250),
        None => (0, MAX_FULL_READ_LINES),
    };
    let text = lines[from..]
        .iter()
        .take(max_lines)
        .enumerate()
        .map(|(i, l)| format!("{}: {}", from + i + 1, l))
        .collect::<Vec<_>>()
        .join("\n");
    let shown = max_lines.min(total.saturating_sub(from));
    if total > from + shown {
        Ok(format!("{text}\n(file has {total} lines total; use \"line\":{} to read more)", from + shown + 1))
    } else {
        Ok(text)
    }
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
    let output = Command::new("/bin/bash")
        .arg("-c")
        .arg(cmd)
        .current_dir(&cwd_path)
        .output()
        .with_context(|| format!("failed to spawn: {cmd}"))?;
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
    const BLOCKED: &[&str] =
        &["rm -rf", "git reset --hard", "git clean -f", "dd if=", "mkfs", "shred"];
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

fn truncate(s: &str, max: usize) -> &str {
    let end = s.char_indices().nth(max).map(|(i, _)| i).unwrap_or(s.len());
    &s[..end]
}

// ── Main ───────────────────────────────────────────────────────────────────────

#[tokio::main]
async fn main() -> Result<()> {
    // Parse --verifier / --executor (default: executor) and optional --port N
    let args: Vec<String> = std::env::args().collect();
    let is_verifier = args.iter().any(|a| a == "--verifier");
    let agent_role = if is_verifier { "verifier" } else { "mini_agent" };
    let system_instructions = if is_verifier { SYSTEM_INSTRUCTIONS_VERIFIER } else { SYSTEM_INSTRUCTIONS_EXECUTOR };
    let ws_port: u16 = args.windows(2)
        .find(|w| w[0] == "--port")
        .and_then(|w| w[1].parse().ok())
        .unwrap_or(WS_PORT_DEFAULT);

    let workspace = PathBuf::from(WORKSPACE);

    let plan_path = workspace.join(PLAN_FILE);
    let objective = std::fs::read_to_string(&plan_path)
        .with_context(|| format!("failed to read plan file: {}", plan_path.display()))?;

    if objective.trim().is_empty() {
        bail!("plan file is empty — write an objective into {PLAN_FILE} before running");
    }

    eprintln!("[canon-mini-agent] role={agent_role} objective loaded ({} bytes)", objective.len());

    // Load capability config to get endpoint details.
    let config = CapabilityConfig::snapshot_store_load()
        .context("failed to load capability_config.toml")?;

    let endpoint = config
        .llm_endpoints
        .iter()
        .find(|e| e.role.as_deref() == Some(agent_role))
        .ok_or_else(|| anyhow!("no endpoint with role '{agent_role}' in capability_config.toml"))?
        .clone();

    eprintln!(
        "[canon-mini-agent] endpoint id={} url={} stateful={} max_tabs={}",
        endpoint.id, endpoint.url, endpoint.stateful, endpoint.max_tabs
    );

    // Start the WebSocket bridge — Chrome extension connects here.
    let ws_addr: std::net::SocketAddr = format!("127.0.0.1:{ws_port}").parse()?;
    let bridge = ws_server::spawn(ws_addr, config.response_timeout_secs, Arc::new(OnceLock::new()));

    eprintln!("[canon-mini-agent] waiting for Chrome extension on ws://127.0.0.1:{ws_port}");
    bridge.wait_for_connection().await;
    eprintln!("[canon-mini-agent] Chrome extension connected");

    let tabs = llm_worker_new_tabs();

    let initial_prompt = if is_verifier {
        format!(
            "WORKSPACE: {WORKSPACE}\n\
             All relative paths resolve against WORKSPACE.\n\
             \n\
             Plan to verify (from {PLAN_FILE}):\n\
             {objective}\n\
             \n\
             Begin by reading the plan and identifying all tasks marked `- [x]`. \
             Verify each one. Emit exactly one action to begin.",
        )
    } else {
        format!(
            "WORKSPACE: {WORKSPACE}\n\
             All relative paths resolve against WORKSPACE.\n\
             \n\
             Objective (from {PLAN_FILE}):\n\
             {objective}\n\
             \n\
             Emit exactly one action to begin.",
        )
    };

    let mut step = 0usize;
    let mut last_result: Option<String> = None;

    loop {
        if step >= MAX_STEPS {
            break;
        }

        let (role_schema, prompt) = if step == 0 {
            (system_instructions.to_string(), initial_prompt.clone())
        } else {
            let result = last_result.as_deref().unwrap_or("");
            (
                String::new(),
                format!(
                    "Action result:\n{}\n\nEmit exactly one action.",
                    truncate(result, MAX_SNIPPET)
                ),
            )
        };

        eprintln!(
            "[canon-mini-agent] step={} prompt_bytes={}",
            step + 1,
            prompt.len()
        );

        let raw = match llm_worker_send_request(
            &bridge,
            &endpoint.id,
            &endpoint.url,
            endpoint.stateful,
            &prompt,
            &role_schema,
            None,
            None,
            false,
            true,
            agent_role,
            &tabs,
            endpoint.max_tabs,
            config.tab_cooldown_ms,
        )
        .await
        {
            Ok(r) => r,
            Err(e) => {
                eprintln!("[canon-mini-agent] step={} llm_error: {e}", step + 1);
                last_result = Some(format!(
                    "LLM error: {e}\nReturn exactly one action in a ```json code block."
                ));
                step += 1;
                continue;
            }
        };

        eprintln!(
            "[canon-mini-agent] step={} response_bytes={}",
            step + 1,
            raw.len()
        );

        let actions = match parse_actions(&raw) {
            Ok(a) => a,
            Err(e) => {
                eprintln!("[canon-mini-agent] step={} parse_error: {e}", step + 1);
                last_result = Some(format!(
                    "Parse error: {e}\n\
                     Return exactly one action in a ```json code block. No prose outside it."
                ));
                step += 1;
                continue;
            }
        };

        if actions.len() != 1 {
            let msg = format!(
                "Got {} actions — emit exactly one action per turn.",
                actions.len()
            );
            eprintln!("[canon-mini-agent] step={} {msg}", step + 1);
            last_result = Some(msg);
            step += 1;
            continue;
        }

        let action = &actions[0];
        let kind = action.get("action").and_then(|v| v.as_str()).unwrap_or("unknown");
        eprintln!("[canon-mini-agent] step={} action={kind}", step + 1);

        let step_result: Result<(bool, String)> = (|| match kind {
            "done" => {
                let reason = action
                    .get("reason")
                    .and_then(|v| v.as_str())
                    .unwrap_or("objective complete");
                // Step 1: cargo build --workspace
                eprintln!("[canon-mini-agent] step={} done requested — running cargo build --workspace", step + 1);
                let (build_ok, build_out) =
                    exec_run_command(&workspace, "cargo build --workspace", WORKSPACE)
                        .unwrap_or_else(|e| (false, e.to_string()));
                if !build_ok {
                    eprintln!("[canon-mini-agent] step={} cargo build failed — rejecting done", step + 1);
                    Ok((false, format!(
                        "done rejected: cargo build --workspace failed. Fix the build errors before declaring done.\n\n{}",
                        truncate(&build_out, MAX_SNIPPET)
                    )))
                } else {
                    eprintln!("[canon-mini-agent] step={} cargo build ok — running cargo test --workspace", step + 1);
                    // Step 2: cargo test --workspace
                    let (test_ok, test_out) =
                        exec_run_command(&workspace, "cargo test --workspace", WORKSPACE)
                            .unwrap_or_else(|e| (false, e.to_string()));
                    if test_ok {
                        eprintln!("[canon-mini-agent] step={} cargo test ok — accepting done", step + 1);
                        Ok((true, reason.to_string()))
                    } else {
                        eprintln!("[canon-mini-agent] step={} cargo test failed — rejecting done", step + 1);
                        Ok((false, format!(
                            "done rejected: cargo test --workspace failed. Fix the failures before declaring done.\n\n{}",
                            truncate(&test_out, MAX_SNIPPET)
                        )))
                    }
                }
            }
            "list_dir" => {
                let path = action
                    .get("path")
                    .and_then(|v| v.as_str())
                    .ok_or_else(|| anyhow!("list_dir missing 'path'"))?;
                let out = exec_list_dir(&workspace, path)?;
                Ok((false, format!("list_dir {path}:\n{out}")))
            }
            "read_file" => {
                let path = action
                    .get("path")
                    .and_then(|v| v.as_str())
                    .ok_or_else(|| anyhow!("read_file missing 'path'"))?;
                let line =
                    action.get("line").and_then(|v| v.as_u64()).map(|n| n as usize);
                let out = exec_read_file(&workspace, path, line)?;
                eprintln!(
                    "[canon-mini-agent] step={} read_file path={path} bytes={}",
                    step + 1,
                    out.len()
                );
                Ok((false, format!("read_file {path}:\n{out}")))
            }
            "apply_patch" => {
                let patch = action
                    .get("patch")
                    .and_then(|v| v.as_str())
                    .ok_or_else(|| anyhow!("apply_patch missing 'patch'"))?;
                match apply_patch(patch, &workspace) {
                    Ok(_) => {
                        eprintln!("[canon-mini-agent] step={} apply_patch ok", step + 1);
                        // Infer the affected crate so we check only it, not the whole
                        // workspace (which may have pre-existing errors in other crates).
                        let check_result = patch_first_file(patch)
                            .and_then(|f| infer_crate_for_patch(&workspace, f))
                            .map(|krate| {
                                eprintln!(
                                    "[canon-mini-agent] step={} cargo check -p {krate}",
                                    step + 1
                                );
                                exec_run_command(
                                    &workspace,
                                    &format!("cargo check -p {krate}"),
                                    WORKSPACE,
                                )
                                .unwrap_or_else(|e| (false, e.to_string()))
                            });
                        match check_result {
                            Some((check_ok, check_out)) => {
                                let label = if check_ok {
                                    "cargo check ok"
                                } else {
                                    "cargo check failed"
                                };
                                eprintln!("[canon-mini-agent] step={} {label}", step + 1);
                                Ok((false, format!(
                                    "apply_patch ok\n\n{label}:\n{}",
                                    truncate(&check_out, MAX_SNIPPET)
                                )))
                            }
                            None => Ok((false, "apply_patch ok".to_string())),
                        }
                    }
                    Err(e) => {
                        let err_str = e.to_string();
                        eprintln!(
                            "[canon-mini-agent] step={} apply_patch failed: {err_str}",
                            step + 1
                        );
                        // Auto-read the affected file so the LLM can retry with
                        // correct anchors. Two cases:
                        //   1. Anchor-miss: path is in the error message.
                        //   2. Malformed hunk / other: extract path from the patch text.
                        let mut msg = format!("apply_patch failed: {err_str}");
                        let read_path = extract_anchor_fail_path(&err_str)
                            .or_else(|| patch_first_file(patch).map(|s| s.to_string()));
                        if let Some(file_path) = read_path {
                            if let Ok(content) =
                                auto_read_for_patch_anchor(&workspace, &file_path, &err_str)
                            {
                                eprintln!(
                                    "[canon-mini-agent] step={} auto_read path={file_path}",
                                    step + 1
                                );
                                msg = format!("apply_patch failed: {err_str}\n\n{content}");
                            }
                        }
                        Ok((false, msg))
                    }
                }
            }
            "run_command" => {
                let cmd = action
                    .get("cmd")
                    .and_then(|v| v.as_str())
                    .ok_or_else(|| anyhow!("run_command missing 'cmd'"))?;
                let cwd =
                    action.get("cwd").and_then(|v| v.as_str()).unwrap_or(WORKSPACE);
                eprintln!("[canon-mini-agent] step={} run_command cmd={cmd}", step + 1);
                let (success, out) = exec_run_command(&workspace, cmd, cwd)?;
                let label =
                    if success { "run_command ok" } else { "run_command failed" };
                eprintln!(
                    "[canon-mini-agent] step={} {label} output_bytes={}",
                    step + 1,
                    out.len()
                );
                Ok((false, format!("{label}:\n{}", truncate(&out, MAX_SNIPPET))))
            }
            other => Ok((
                false,
                format!(
                    "unsupported action '{other}' — use list_dir, read_file, apply_patch, run_command, or done"
                ),
            )),
        })();

        match step_result {
            Ok((true, reason)) => {
                eprintln!("[canon-mini-agent] complete: {reason}");
                println!("done: {reason}");
                return Ok(());
            }
            Ok((false, out)) => {
                last_result = Some(out);
            }
            Err(e) => {
                eprintln!("[canon-mini-agent] step={} error: {e}", step + 1);
                last_result = Some(format!("Error executing action: {e}"));
            }
        }

        step += 1;
    }

    bail!("mini-agent exhausted {MAX_STEPS} steps without completing the objective")
}
