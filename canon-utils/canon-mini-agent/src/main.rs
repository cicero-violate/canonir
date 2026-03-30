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
const WS_ADDR: &str = "127.0.0.1:9100";
const AGENT_ROLE: &str = "mini_agent";
const MAX_STEPS: usize = 2000;
const MAX_READ_BYTES: usize = 16 * 1024;
const MAX_SNIPPET: usize = 3000;

const SYSTEM_INSTRUCTIONS: &str = r#"You are the canon mini-agent.

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

2. read_file — read a file before editing
   {"action":"read_file","path":"canon-utils/some-crate/src/lib.rs"}
   {"action":"read_file","path":"canon-utils/some-crate/src/lib.rs","line":120}
   ⚠ Always read a file before patching it. Never patch from memory.

3. apply_patch — create or update files
   {"action":"apply_patch","patch":"*** Begin Patch\n*** Update File: path/to/file.rs\n@@\n context\n+added line\n context\n*** End Patch"}
   {"action":"apply_patch","patch":"*** Begin Patch\n*** Add File: path/to/new.rs\n+line one\n+line two\n*** End Patch"}

4. run_command — run shell commands for discovery or verification
   {"action":"run_command","cmd":"cargo check -p some-crate","cwd":"/workspace/ai_sandbox/canon"}
   {"action":"run_command","cmd":"rg -n 'fn foo' canon-utils/some-crate/src/","cwd":"/workspace/ai_sandbox/canon"}

5. done — declare the objective complete
   {"action":"done","reason":"brief description of what was accomplished"}

━━━ RULES ━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━

- Emit exactly one action per turn.
- Always read a file before patching it.
- Use list_dir and read_file freely before assuming project state.
- Use run_command for cargo builds, tests, and shell discovery.
- Never operate outside /workspace/ai_sandbox/canon.
- Never emit destructive commands (rm -rf, git reset --hard, git clean -f, etc.).
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
    let bytes =
        std::fs::read(&path).with_context(|| format!("read_file: {}", path.display()))?;
    let full = String::from_utf8_lossy(&bytes).into_owned();
    if let Some(line) = start_line {
        let lines: Vec<&str> = full.lines().collect();
        let from = line.saturating_sub(1).min(lines.len());
        let text = lines[from..]
            .iter()
            .take(250)
            .enumerate()
            .map(|(i, l)| format!("{}: {}", from + i + 1, l))
            .collect::<Vec<_>>()
            .join("\n");
        Ok(text)
    } else {
        Ok(String::from_utf8_lossy(&bytes[..bytes.len().min(MAX_READ_BYTES)]).into_owned())
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
        .arg("-lc")
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
    let workspace = PathBuf::from(WORKSPACE);

    let plan_path = workspace.join(PLAN_FILE);
    let objective = std::fs::read_to_string(&plan_path)
        .with_context(|| format!("failed to read plan file: {}", plan_path.display()))?;

    if objective.trim().is_empty() {
        bail!("plan file is empty — write an objective into {PLAN_FILE} before running");
    }

    eprintln!("[canon-mini-agent] objective loaded ({} bytes)", objective.len());

    // Load capability config to get endpoint details.
    let config = CapabilityConfig::snapshot_store_load()
        .context("failed to load capability_config.toml")?;

    let endpoint = config
        .llm_endpoints
        .iter()
        .find(|e| e.role.as_deref() == Some(AGENT_ROLE))
        .ok_or_else(|| anyhow!("no endpoint with role '{AGENT_ROLE}' in capability_config.toml"))?
        .clone();

    eprintln!(
        "[canon-mini-agent] endpoint id={} url={} stateful={} max_tabs={}",
        endpoint.id, endpoint.url, endpoint.stateful, endpoint.max_tabs
    );

    // Start the WebSocket bridge — Chrome extension connects here.
    let ws_addr: std::net::SocketAddr = WS_ADDR.parse()?;
    let bridge = ws_server::spawn(ws_addr, config.response_timeout_secs, Arc::new(OnceLock::new()));

    eprintln!("[canon-mini-agent] waiting for Chrome extension on ws://{WS_ADDR}");
    bridge.wait_for_connection().await;
    eprintln!("[canon-mini-agent] Chrome extension connected");

    let tabs = llm_worker_new_tabs();

    let initial_prompt = format!(
        "WORKSPACE: {WORKSPACE}\n\
         All relative paths resolve against WORKSPACE.\n\
         \n\
         Objective (from {PLAN_FILE}):\n\
         {objective}\n\
         \n\
         Emit exactly one action to begin.",
    );

    let mut step = 0usize;
    let mut last_result: Option<String> = None;

    loop {
        if step >= MAX_STEPS {
            break;
        }

        let (role_schema, prompt) = if step == 0 {
            (SYSTEM_INSTRUCTIONS.to_string(), initial_prompt.clone())
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
            "mini_agent",
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
                Ok((true, reason.to_string()))
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
                apply_patch(patch, &workspace)
                    .map_err(|e| anyhow!("apply_patch failed: {e}"))?;
                eprintln!("[canon-mini-agent] step={} apply_patch ok", step + 1);
                Ok((false, "apply_patch ok".to_string()))
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
