use anyhow::{anyhow, bail, Context, Result};
use canon_event::{CapabilityFailed, CapabilityResult, EventEmitter, EventEmitterHandle, EventId, LlmCall, RuntimeEvent};
use canon_exec::{ExecutableEvent, ExecutionContext};
use canon_loop::{HarnessRepairTarget, LoopStageExecutor};
use canon_tools_patch::apply_patch;
use serde_json::Value;
use std::path::{Component, Path, PathBuf};
use std::process::Command;
use std::sync::mpsc::{self, Receiver, Sender};
use std::sync::{Arc, Mutex};
use std::time::Duration;
use uuid::Uuid;

const DEFAULT_WORKSPACE: &str = "/workspace/ai_sandbox/canon";
const DEFAULT_MAX_STEPS: usize = 5;
const LLM_TIMEOUT: Duration = Duration::from_secs(120);
const MAX_READ_BYTES: usize = 16 * 1024;
const MAX_TOOL_SNIPPET: usize = 4 * 1024;

const PLANNER_SYSTEM_INSTRUCTIONS: &str = r#"You are a code-editing agent. Produce a plan as a JSON array of actions.

━━━ TOOLS ━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━

1. list_dir — list what files/dirs exist (use BEFORE assuming project state)
   {"action":"list_dir","path":"."}

2. read_file — read a file's current contents when you do not already have enough context to edit safely
   {"action":"read_file","path":"src/main.rs"}
   ⚠ Results appear in "Recent actions" on your NEXT call. Do not mix with edits.

3. apply_patch — create, update, or delete files
   {"action":"apply_patch","patch":"*** Begin Patch\n...\n*** End Patch"}

   Patch rules:
   - use `*** Add File:` for new files
   - use `*** Update File:` for edits to existing files
   - every changed line in an update hunk must start with ` `, `+`, or `-`
   - do not emit prose inside the patch
   - do not omit unchanged context around edits

4. run_command — run a shell command
   {"action":"run_command","cmd":"cargo check -p canon-route","cwd":"<TARGET_WORKSPACE>"}

5. done — declare goal complete
   {"action":"done","reason":"..."}

━━━ WORKFLOW ━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━

- Emit exactly one action per step.
- Use discovery (`list_dir`, `read_file`) before assuming repo state.
- Use `apply_patch` for code edits.
- Use `run_command` for cargo or shell operations.
- The `done` action must be the only action in the batch.

SAFETY RULES:
- Never operate outside TARGET WORKSPACE.
- Never touch `/workspace/ai_sandbox/canon/test_projects/goalgen`.
- Never emit destructive commands (`rm -rf`, `git reset --hard`, `git clean -f`, `dd if=`, `mkfs`, `shred`).
- Prefer the smallest step that advances the failing test.

OUTPUT FORMAT:
- Return ONLY a JSON array of action objects.
- No prose outside the JSON array."#;

#[derive(Clone, Debug)]
struct PlannerAction {
    raw: Value,
}

#[derive(Clone, Debug)]
struct StepRecord {
    kind: String,
    success: bool,
    summary: String,
}

#[derive(Clone, Debug)]
struct CommandResult {
    success: bool,
    output: String,
}

struct CapturingEmitter {
    tx: Mutex<Sender<RuntimeEvent>>,
}

impl EventEmitter for CapturingEmitter {
    fn emit_with_parents(&self, event: RuntimeEvent, _parents: Vec<EventId>, _file: &'static str, _line: u32) {
        let _ = self.tx.lock().unwrap().send(event);
    }
}

fn main() -> Result<()> {
    let mut args = std::env::args().skip(1);
    let crate_name = args.next().ok_or_else(|| anyhow!("missing <crate>"))?;
    let test_name = args.next().ok_or_else(|| anyhow!("missing <test-name>"))?;

    let mut always_dispatch = false;
    let mut stderr_file: Option<PathBuf> = None;
    let mut workspace = PathBuf::from(DEFAULT_WORKSPACE);
    let mut max_steps = DEFAULT_MAX_STEPS;

    while let Some(arg) = args.next() {
        match arg.as_str() {
            "--always-dispatch" => always_dispatch = true,
            "--stderr-file" => {
                let value = args.next().ok_or_else(|| anyhow!("missing value for --stderr-file"))?;
                stderr_file = Some(PathBuf::from(value));
            }
            "--workspace" => {
                let value = args.next().ok_or_else(|| anyhow!("missing value for --workspace"))?;
                workspace = PathBuf::from(value);
            }
            "--max-steps" => {
                let value = args.next().ok_or_else(|| anyhow!("missing value for --max-steps"))?;
                max_steps = value.parse().context("--max-steps must be an integer")?;
            }
            other => return Err(anyhow!("unknown argument: {other}")),
        }
    }

    let target = HarnessRepairTarget::new(Some(crate_name.clone()), Some(test_name.clone()));
    let mut failure_output = if let Some(path) = stderr_file {
        std::fs::read_to_string(path)?
    } else {
        run_target_test_capture(&workspace, &crate_name, &test_name, always_dispatch)?
    };

    canon_exec::init_llm_worker();
    let result = run_harness_loop(&workspace, &crate_name, &test_name, &target, &mut failure_output, max_steps);
    canon_exec::shutdown_llm_worker();
    result
}

fn run_harness_loop(
    workspace: &Path,
    crate_name: &str,
    test_name: &str,
    target: &HarnessRepairTarget,
    failure_output: &mut String,
    max_steps: usize,
) -> Result<()> {
    let mut recent_steps = Vec::new();
    let mut executor = LoopStageExecutor::new(workspace.to_path_buf(), workspace.join("state/event_log/event.tlog.d"));

    for step in 0..max_steps {
        let directive = executor.evaluate_harness_repair_for_target(target, failure_output);
        let prompt = build_planner_prompt(workspace, crate_name, test_name, failure_output, &directive.decision.reason, &recent_steps);
        let actions = call_planner(workspace, &prompt)?;
        if actions.len() != 1 {
            bail!("planner returned {} actions; minimum harness requires exactly one", actions.len());
        }
        let action = &actions[0];
        let action_kind = action.kind()?.to_string();
        eprintln!("[canon-harness-repair] step {} action={}", step + 1, action_kind);

        match action_kind.as_str() {
            "done" => {
                let verify = verify_full_crate(workspace, crate_name, test_name)?;
                if verify.success {
                    println!("harness repair complete: crate test suite passed");
                    return Ok(());
                }
                *failure_output = verify.output.clone();
                recent_steps.push(StepRecord {
                    kind: "done_verify".to_string(),
                    success: false,
                    summary: truncate(&verify.output, MAX_TOOL_SNIPPET),
                });
            }
            "list_dir" => {
                let path = action.path()?;
                let output = run_list_dir(workspace, path)?;
                recent_steps.push(StepRecord {
                    kind: "list_dir".to_string(),
                    success: true,
                    summary: truncate(&output, MAX_TOOL_SNIPPET),
                });
            }
            "read_file" => {
                let path = action.path()?;
                let output = run_read_file(workspace, path)?;
                recent_steps.push(StepRecord {
                    kind: "read_file".to_string(),
                    success: true,
                    summary: truncate(&output, MAX_TOOL_SNIPPET),
                });
            }
            "apply_patch" => {
                let patch = action.patch()?;
                match apply_patch(patch, workspace) {
                    Ok(_) => {
                        recent_steps.push(StepRecord {
                            kind: "apply_patch".to_string(),
                            success: true,
                            summary: "patch applied".to_string(),
                        });
                    }
                    Err(err) => {
                        let patch_dump = workspace.join("state/harness_last_failed.patch");
                        let _ = std::fs::create_dir_all(
                            patch_dump.parent().ok_or_else(|| anyhow!("invalid patch dump path"))?,
                        );
                        let _ = std::fs::write(&patch_dump, patch);
                        let message = format!(
                            "apply_patch failed: {err}\nfailed_patch_file={}\npatch_preview=\n{}",
                            patch_dump.display(),
                            truncate(patch, MAX_TOOL_SNIPPET),
                        );
                        *failure_output = message.clone();
                        recent_steps.push(StepRecord {
                            kind: "apply_patch".to_string(),
                            success: false,
                            summary: truncate(&message, MAX_TOOL_SNIPPET),
                        });
                        continue;
                    }
                }
                let verify = verify_after_mutation(workspace, crate_name, test_name)?;
                if verify.success {
                    println!("harness repair complete: crate test suite passed");
                    return Ok(());
                }
                *failure_output = verify.output.clone();
                recent_steps.push(StepRecord {
                    kind: "verify".to_string(),
                    success: false,
                    summary: truncate(&verify.output, MAX_TOOL_SNIPPET),
                });
            }
            "run_command" => {
                let cmd = action.command()?;
                let cwd = action.cwd()?;
                let result = run_shell_command(workspace, cmd, cwd)?;
                recent_steps.push(StepRecord {
                    kind: "run_command".to_string(),
                    success: result.success,
                    summary: truncate(&result.output, MAX_TOOL_SNIPPET),
                });
                if !result.success {
                    *failure_output = result.output;
                    continue;
                }
                let verify = verify_after_mutation(workspace, crate_name, test_name)?;
                if verify.success {
                    println!("harness repair complete: crate test suite passed");
                    return Ok(());
                }
                *failure_output = verify.output.clone();
                recent_steps.push(StepRecord {
                    kind: "verify".to_string(),
                    success: false,
                    summary: truncate(&verify.output, MAX_TOOL_SNIPPET),
                });
            }
            other => bail!("unsupported planner action in minimum harness: {other}"),
        }
    }

    bail!("harness repair stopped after {max_steps} steps without passing the target test")
}

fn build_planner_prompt(
    workspace: &Path,
    crate_name: &str,
    test_name: &str,
    failure_output: &str,
    directive_reason: &str,
    recent_steps: &[StepRecord],
) -> String {
    let recent = if recent_steps.is_empty() {
        "none".to_string()
    } else {
        recent_steps
            .iter()
            .map(|step| format!("- kind={} success={} summary={}", step.kind, step.success, step.summary))
            .collect::<Vec<_>>()
            .join("\n")
    };
    format!(
        "TARGET WORKSPACE: {workspace}\n\
All relative paths resolve against TARGET WORKSPACE.\n\
\n\
Harness target:\n\
- crate: {crate_name}\n\
- failing test: {test_name}\n\
\n\
Hard scope rules:\n\
- Only modify files under TARGET WORKSPACE.\n\
- Only work on the canon harness / crate under repair.\n\
- Do not operate on `/workspace/ai_sandbox/canon/test_projects/goalgen`.\n\
- Fix the failing harness/test path, not unrelated generated projects.\n\
\n\
Repair directive:\n\
- {directive_reason}\n\
- Emit exactly one action.\n\
\n\
Current failure output:\n{failure_output}\n\
\n\
Recent actions:\n{recent}\n\
\n\
Preferred verifier after mutation:\n- `cargo check -p {crate_name}`\n\
- then `cargo test -p {crate_name} {test_name} -- --nocapture`\n",
        workspace = workspace.display(),
        crate_name = crate_name,
        test_name = test_name,
        directive_reason = directive_reason,
        failure_output = truncate(failure_output, 12000),
        recent = recent,
    )
}

fn call_planner(workspace: &Path, prompt: &str) -> Result<Vec<PlannerAction>> {
    let (tx, rx) = mpsc::channel();
    let emitter: EventEmitterHandle = Arc::new(CapturingEmitter { tx: Mutex::new(tx) });
    let request_id = Uuid::new_v4().to_string();
    let event = RuntimeEvent::Llm(LlmCall {
        request_id: request_id.clone(),
        prompt: prompt.to_string(),
        role: Some("planner".to_string()),
        agent_id: Some("planner_chatgpt_group".to_string()),
        dispatched: true,
        system: Some(PLANNER_SYSTEM_INSTRUCTIONS.to_string()),
        system_prompt_id: None,
        context_base: None,
        context_base_id: None,
        prompt_base_id: None,
        prev_prompt_id: None,
    });
    let exec = ExecutableEvent::try_from(event).expect("llm event should be executable");
    exec.execute(ExecutionContext {
        workspace: workspace.to_path_buf(),
        emitter,
        trigger_id: EventId::new("min-harness-root"),
    })?;
    wait_for_llm_response(&rx, &request_id)
}

fn wait_for_llm_response(rx: &Receiver<RuntimeEvent>, request_id: &str) -> Result<Vec<PlannerAction>> {
    loop {
        let event = rx.recv_timeout(LLM_TIMEOUT).context("timed out waiting for planner result")?;
        match event {
            RuntimeEvent::CapabilityCompleted(done)
                if done.request_id == request_id && done.capability == "llm.call" =>
            {
                let CapabilityResult::Llm(result) = done.result else {
                    bail!("planner returned non-LLM capability result")
                };
                return parse_planner_actions(&result.response);
            }
            RuntimeEvent::CapabilityFailed(CapabilityFailed { request_id: failed_request_id, capability, error, .. })
                if failed_request_id == request_id && capability == "llm.call" =>
            {
                bail!("planner call failed: {error}");
            }
            _ => {}
        }
    }
}

fn parse_planner_actions(value: &Value) -> Result<Vec<PlannerAction>> {
    let array = if let Some(array) = value.as_array() {
        array.clone()
    } else if let Some(text) = value.get("text").and_then(|v| v.as_str()) {
        serde_json::from_str::<Vec<Value>>(text).context("planner text payload was not a JSON action array")?
    } else {
        bail!("planner payload was not a JSON action array: {value}");
    };
    if array.is_empty() {
        bail!("planner returned an empty action array");
    }
    Ok(array.into_iter().map(|raw| PlannerAction { raw }).collect())
}

impl PlannerAction {
    fn kind(&self) -> Result<&str> {
        self.raw
            .get("action")
            .and_then(|v| v.as_str())
            .ok_or_else(|| anyhow!("planner action is missing `action`: {}", self.raw))
    }

    fn path(&self) -> Result<&str> {
        self.raw
            .get("path")
            .and_then(|v| v.as_str())
            .ok_or_else(|| anyhow!("planner action is missing `path`: {}", self.raw))
    }

    fn patch(&self) -> Result<&str> {
        self.raw
            .get("patch")
            .and_then(|v| v.as_str())
            .ok_or_else(|| anyhow!("planner action is missing `patch`: {}", self.raw))
    }

    fn command(&self) -> Result<&str> {
        self.raw
            .get("cmd")
            .and_then(|v| v.as_str())
            .ok_or_else(|| anyhow!("planner action is missing `cmd`: {}", self.raw))
    }

    fn cwd(&self) -> Result<&str> {
        self.raw
            .get("cwd")
            .and_then(|v| v.as_str())
            .ok_or_else(|| anyhow!("planner action is missing `cwd`: {}", self.raw))
    }
}

fn run_list_dir(workspace: &Path, relative: &str) -> Result<String> {
    let path = resolve_workspace_path(workspace, relative)?;
    let mut entries = std::fs::read_dir(&path)
        .with_context(|| format!("list_dir failed for {}", path.display()))?
        .flatten()
        .map(|entry| entry.file_name().to_string_lossy().into_owned())
        .collect::<Vec<_>>();
    entries.sort();
    Ok(entries.join("\n"))
}

fn run_read_file(workspace: &Path, relative: &str) -> Result<String> {
    let path = resolve_workspace_path(workspace, relative)?;
    let bytes = std::fs::read(&path).with_context(|| format!("read_file failed for {}", path.display()))?;
    let text = String::from_utf8_lossy(&bytes[..bytes.len().min(MAX_READ_BYTES)]).into_owned();
    Ok(text)
}

fn run_shell_command(workspace: &Path, cmd: &str, cwd: &str) -> Result<CommandResult> {
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
        .with_context(|| format!("failed to run command: {cmd}"))?;
    Ok(CommandResult {
        success: output.status.success(),
        output: combine_output(&output.stdout, &output.stderr),
    })
}

fn verify_after_mutation(workspace: &Path, crate_name: &str, test_name: &str) -> Result<CommandResult> {
    let check = run_shell_command(workspace, &format!("cargo check -p {crate_name}"), &workspace.display().to_string())?;
    if !check.success {
        return Ok(check);
    }
    verify_full_crate(workspace, crate_name, test_name)
}

fn verify_full_crate(workspace: &Path, crate_name: &str, test_name: &str) -> Result<CommandResult> {
    let target = run_test_command(workspace, crate_name, test_name)?;
    if !target.success {
        return Ok(target);
    }
    run_crate_test_command(workspace, crate_name)
}

fn run_test_command(workspace: &Path, crate_name: &str, test_name: &str) -> Result<CommandResult> {
    let output = Command::new("cargo")
        .arg("test")
        .arg("-p")
        .arg(crate_name)
        .arg(test_name)
        .arg("--")
        .arg("--nocapture")
        .current_dir(workspace)
        .output()
        .with_context(|| format!("failed to run target test {crate_name}::{test_name}"))?;
    Ok(CommandResult {
        success: output.status.success(),
        output: combine_output(&output.stdout, &output.stderr),
    })
}

fn run_crate_test_command(workspace: &Path, crate_name: &str) -> Result<CommandResult> {
    let output = Command::new("cargo")
        .arg("test")
        .arg("-p")
        .arg(crate_name)
        .current_dir(workspace)
        .output()
        .with_context(|| format!("failed to run crate test suite for {crate_name}"))?;
    Ok(CommandResult {
        success: output.status.success(),
        output: combine_output(&output.stdout, &output.stderr),
    })
}

fn run_target_test_capture(workspace: &Path, crate_name: &str, test_name: &str, always_dispatch: bool) -> Result<String> {
    let result = run_test_command(workspace, crate_name, test_name)?;
    if result.success && !always_dispatch {
        bail!("target test passed; no harness repair requested");
    }
    if result.success {
        return Ok(format!(
            "target test passed\ncrate: {crate_name}\ntest: {test_name}\nmode: always-dispatch\ninstruction: inspect harness state and propose one minimal repair or verification step"
        ));
    }
    Ok(result.output)
}

fn resolve_workspace_path(workspace: &Path, relative: &str) -> Result<PathBuf> {
    let path = Path::new(relative);
    if path.is_absolute() {
        bail!("absolute paths are not allowed: {relative}");
    }
    if path.components().any(|c| matches!(c, Component::ParentDir)) {
        bail!("parent directory traversal is not allowed: {relative}");
    }
    Ok(workspace.join(path))
}

fn ensure_safe_command(cmd: &str) -> Result<()> {
    let blocked = ["rm -rf", "git reset --hard", "git clean -f", "dd if=", "mkfs", "shred"];
    if blocked.iter().any(|needle| cmd.contains(needle)) {
        bail!("blocked command in minimum harness: {cmd}");
    }
    if cmd.contains("/test_projects/goalgen") {
        bail!("goalgen workspace is out of scope for minimum harness: {cmd}");
    }
    Ok(())
}

fn combine_output(stdout: &[u8], stderr: &[u8]) -> String {
    let mut text = String::from_utf8_lossy(stdout).into_owned();
    if !stderr.is_empty() {
        if !text.is_empty() {
            text.push('\n');
        }
        text.push_str(&String::from_utf8_lossy(stderr));
    }
    text
}

fn truncate(text: &str, limit: usize) -> String {
    if text.len() <= limit {
        text.to_string()
    } else {
        format!("{}...", &text[..limit])
    }
}
