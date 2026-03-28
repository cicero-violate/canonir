use anyhow::{anyhow, bail, Context, Result};
use canon_event::{CapabilityFailed, CapabilityResult, EventEmitter, EventEmitterHandle, EventId, LlmCall, RuntimeEvent};
use canon_exec::{ExecutableEvent, ExecutionContext};
use canon_loop::{HarnessRepairTarget, LoopStageExecutor};
use canon_tools_patch::{apply_patch, parse_patch, Hunk};
use serde_json::Value;
use std::path::{Component, Path, PathBuf};
use std::process::Command;
use std::sync::mpsc::{self, Receiver, Sender};
use std::sync::{Arc, Mutex};
use std::time::Duration;
use uuid::Uuid;

const DEFAULT_WORKSPACE: &str = "/workspace/ai_sandbox/canon";
const DEFAULT_MAX_STEPS: usize = 5;
const LLM_TIMEOUT: Duration = Duration::from_secs(60);
const LLM_MAX_RETRIES: usize = 3;
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

   Correct example:
   {"action":"apply_patch","patch":"*** Begin Patch\n*** Update File: canon-utils/canon-route/src/policy.rs\n@@\n fn apply_rule(decision: &mut RouteDecision, rule: RoutePolicyRule) {\n     match rule {\n         RoutePolicyRule::ForcePlanOnMissingTarget => {\n+            decision.suggested_route = RouteKind::Plan;\n         }\n         _ => {}\n     }\n }\n*** End Patch"}

   Wrong example (do NOT do this):
   {"action":"apply_patch","patch":"*** Begin Patch\n*** Update File: canon-utils/canon-route/src/policy.rs\n@@\n fn some_function_that_was_not_read(...) {\n+    // guessed edit\n*** End Patch"}

   If a patch fails because expected lines were not found:
   - use the failure feedback to correct the patch anchors
   - emit `read_file` only if you need more file context

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
- Never describe a fix in prose. Emit only a valid JSON action array.
- Never propose pseudo-code or textual edit instructions like “change X to Y”.
- If you identify a code fix, express it as an `apply_patch` action with a complete valid patch.
- If you are not ready to patch safely, emit `read_file` for the cited file or `list_dir`; do not narrate the intended fix.

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
    let result = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
        run_harness_loop(&workspace, &crate_name, &test_name, &target, &mut failure_output, max_steps)
    }));
    canon_exec::shutdown_llm_worker();
    match result {
        Ok(inner) => inner,
        Err(payload) => {
            let panic_msg = if let Some(msg) = payload.downcast_ref::<&str>() {
                (*msg).to_string()
            } else if let Some(msg) = payload.downcast_ref::<String>() {
                msg.clone()
            } else {
                "unknown panic payload".to_string()
            };
            Err(anyhow!("harness repair panicked: {panic_msg}"))
        }
    }
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
    if let Some((cited_file, _)) = extract_primary_file_line(failure_output) {
        if let Ok(output) = run_read_file(workspace, &cited_file) {
            recent_steps.push(StepRecord {
                kind: "read_file".to_string(),
                success: true,
                summary: format!("{cited_file}\n{}", truncate(&output, MAX_TOOL_SNIPPET)),
            });
        }
    }

    for step in 0..max_steps {
        let directive = executor.evaluate_harness_repair_for_target(target, failure_output);
        let prompt = build_planner_prompt(workspace, crate_name, test_name, failure_output, &directive.decision.reason, &recent_steps);
        let actions = match call_planner(workspace, &prompt) {
            Ok(actions) => actions,
            Err(err) => {
                let message = format!("planner call failed: {err}");
                eprintln!(
                    "[canon-harness-repair] planner failure for {}::{}",
                    crate_name, test_name
                );
                eprintln!("[canon-harness-repair] planner failure reason:\n{}", message);
                *failure_output = message.clone();
                recent_steps.push(StepRecord {
                    kind: "planner_failure".to_string(),
                    success: false,
                    summary: truncate(&message, MAX_TOOL_SNIPPET),
                });
                continue;
            }
        };
        if actions.len() != 1 {
            let message = format!(
                "planner returned {} actions; minimum harness requires exactly one",
                actions.len()
            );
            eprintln!("[canon-harness-repair] {}", message);
            *failure_output = message.clone();
            recent_steps.push(StepRecord {
                kind: "planner_failure".to_string(),
                success: false,
                summary: truncate(&message, MAX_TOOL_SNIPPET),
            });
            continue;
        }
        let action = &actions[0];
        let action_kind = match action.kind() {
            Ok(kind) => kind.to_string(),
            Err(err) => {
                let message = format!("planner action parse failed: {err}");
                eprintln!(
                    "[canon-harness-repair] malformed planner action for {}::{}",
                    crate_name, test_name
                );
                eprintln!("[canon-harness-repair] malformed action payload:\n{}", action.raw);
                *failure_output = message.clone();
                recent_steps.push(StepRecord {
                    kind: "planner_failure".to_string(),
                    success: false,
                    summary: truncate(&message, MAX_TOOL_SNIPPET),
                });
                continue;
            }
        };
        eprintln!("[canon-harness-repair] step {} action={}", step + 1, action_kind);
        let step_result = (|| -> Result<bool> {
            match action_kind.as_str() {
                "done" => {
                    let verify = verify_full_crate(workspace, crate_name, test_name)?;
                    print_verify_status(crate_name, test_name, &verify);
                    if verify.success {
                        println!("harness repair complete: crate test suite passed");
                        return Ok(true);
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
                        summary: format!("{path}\n{}", truncate(&output, MAX_TOOL_SNIPPET)),
                    });
                }
                "apply_patch" => {
                    let patch = action.patch()?;
                    if let Err(message) = validate_patch_attempt(workspace, patch) {
                        eprintln!(
                            "[canon-harness-repair] patch rejected for {}::{}",
                            crate_name, test_name
                        );
                        eprintln!("[canon-harness-repair] patch rejection reason:\n{}", message);
                        *failure_output = message.clone();
                        recent_steps.push(StepRecord {
                            kind: "apply_patch".to_string(),
                            success: false,
                            summary: truncate(&message, MAX_TOOL_SNIPPET),
                        });
                        return Ok(false);
                    }
                    match apply_patch(patch, workspace) {
                        Ok(_) => {
                            eprintln!(
                                "[canon-harness-repair] patch applied successfully for {}::{}",
                                crate_name, test_name
                            );
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
                            eprintln!(
                                "[canon-harness-repair] patch apply failed for {}::{}",
                                crate_name, test_name
                            );
                            eprintln!("[canon-harness-repair] patch failure reason:\n{}", message);
                            *failure_output = message.clone();
                            recent_steps.push(StepRecord {
                                kind: "apply_patch".to_string(),
                                success: false,
                                summary: truncate(&message, MAX_TOOL_SNIPPET),
                            });
                            return Ok(false);
                        }
                    }
                    let verify = verify_after_mutation(workspace, crate_name, test_name)?;
                    print_verify_status(crate_name, test_name, &verify);
                    if verify.success {
                        println!("harness repair complete: crate test suite passed");
                        return Ok(true);
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
                        eprintln!(
                            "[canon-harness-repair] command failed for {}::{}",
                            crate_name, test_name
                        );
                        *failure_output = result.output;
                        return Ok(false);
                    }
                    let verify = verify_after_mutation(workspace, crate_name, test_name)?;
                    print_verify_status(crate_name, test_name, &verify);
                    if verify.success {
                        println!("harness repair complete: crate test suite passed");
                        return Ok(true);
                    }
                    *failure_output = verify.output.clone();
                    recent_steps.push(StepRecord {
                        kind: "verify".to_string(),
                        success: false,
                        summary: truncate(&verify.output, MAX_TOOL_SNIPPET),
                    });
                }
                other => {
                    let message = format!("unsupported planner action in minimum harness: {other}");
                    *failure_output = message.clone();
                    recent_steps.push(StepRecord {
                        kind: "planner_failure".to_string(),
                        success: false,
                        summary: truncate(&message, MAX_TOOL_SNIPPET),
                    });
                    eprintln!("[canon-harness-repair] {}", message);
                    return Ok(false);
                }
            }
            Ok(false)
        })();
        match step_result {
            Ok(true) => return Ok(()),
            Ok(false) => {}
            Err(err) => {
                let message = format!("action execution failed: {err}");
                eprintln!(
                    "[canon-harness-repair] action failure for {}::{}",
                    crate_name, test_name
                );
                eprintln!("[canon-harness-repair] action failure reason:\n{}", message);
                *failure_output = message.clone();
                recent_steps.push(StepRecord {
                    kind: "action_failure".to_string(),
                    success: false,
                    summary: truncate(&message, MAX_TOOL_SNIPPET),
                });
            }
        }
    }

    eprintln!(
        "[canon-harness-repair] failed after {} steps for {}::{}",
        max_steps, crate_name, test_name
    );
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
    let failure_focus = build_failure_focus(failure_output);
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
Localization rules:\n\
- Prefer editing the file and function named in the failure output before proposing architectural rewrites.\n\
- If a panic or assertion gives a file/line, treat that as the primary repair site.\n\
- If a previous patch failed, use the failed patch diagnostics to correct the next patch.\n\
- Do not generalize to other routes/modules unless the cited failure location proves it is necessary.\n\
- Do not answer with prose, explanation, or “single repair action” text.\n\
- Your response must be a JSON action array only.\n\
- If you want to modify code, emit a concrete `apply_patch` action; do not describe the patch in English.\n\
- If the failure is localized but the exact edit is still uncertain, emit `read_file` for the cited source file first.\n\
- If a previous patch failed with “expected lines not found”, correct the patch anchors; use `read_file` only when the failure output is not enough.\n\
\n\
Failure focus:\n{failure_focus}\n\
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
        failure_focus = failure_focus,
        failure_output = truncate(failure_output, 12000),
        recent = recent,
    )
}

fn build_failure_focus(failure_output: &str) -> String {
    let mut lines = Vec::new();

    if let Some((file, line)) = extract_primary_file_line(failure_output) {
        lines.push(format!("- primary_file: {file}:{line}"));
        if let Some(snippet) = read_failure_focus_snippet(&file, line.parse::<usize>().ok()) {
            lines.push("- source_excerpt:".to_string());
            lines.push(snippet);
        }
    }
    if let Some(test_name) = extract_first_failed_test_name(failure_output) {
        lines.push(format!("- failing_test_from_output: {test_name}"));
    }
    if let Some(assertion) = extract_assertion_summary(failure_output) {
        lines.push(format!("- assertion: {assertion}"));
    }
    if let Some(patch_file) = extract_tagged_line_value(failure_output, "failed_patch_file=") {
        lines.push(format!("- previous_failed_patch: {patch_file}"));
    }
    if let Some(reason) = extract_apply_patch_error(failure_output) {
        lines.push(format!("- previous_patch_error: {reason}"));
    }

    if lines.is_empty() {
        "none".to_string()
    } else {
        lines.join("\n")
    }
}

fn read_failure_focus_snippet(file: &str, line: Option<usize>) -> Option<String> {
    let root = Path::new(DEFAULT_WORKSPACE);
    let path = root.join(file);
    let content = std::fs::read_to_string(path).ok()?;
    let lines = content.lines().collect::<Vec<_>>();
    let center = line.unwrap_or(1).saturating_sub(1);
    let start = center.saturating_sub(6);
    let end = usize::min(lines.len(), center.saturating_add(7));
    let excerpt = lines[start..end]
        .iter()
        .enumerate()
        .map(|(idx, text)| format!("  {:>4}: {}", start + idx + 1, text))
        .collect::<Vec<_>>()
        .join("\n");
    Some(excerpt)
}

fn extract_primary_file_line(text: &str) -> Option<(String, String)> {
    for line in text.lines() {
        let trimmed = line.trim();
        if let Some(idx) = trimmed.find(".rs:") {
            let suffix = &trimmed[idx + 4..];
            let line_no: String = suffix.chars().take_while(|c| c.is_ascii_digit()).collect();
            if !line_no.is_empty() {
                let start = trimmed[..idx].rfind(' ').map(|v| v + 1).unwrap_or(0);
                let file = trimmed[start..idx + 3].to_string();
                return Some((file, line_no));
            }
        }
    }
    None
}

fn extract_first_failed_test_name(text: &str) -> Option<String> {
    for line in text.lines() {
        let trimmed = line.trim();
        if trimmed.starts_with("test ") && trimmed.ends_with(" ... FAILED") {
            return Some(
                trimmed
                    .trim_start_matches("test ")
                    .trim_end_matches(" ... FAILED")
                    .trim()
                    .to_string(),
            );
        }
    }
    None
}

fn extract_assertion_summary(text: &str) -> Option<String> {
    for line in text.lines() {
        let trimmed = line.trim();
        if trimmed.contains("assertion `left == right` failed") {
            return Some(trimmed.to_string());
        }
    }
    None
}

fn extract_tagged_line_value<'a>(text: &'a str, prefix: &str) -> Option<String> {
    for line in text.lines() {
        let trimmed = line.trim();
        if let Some(value) = trimmed.strip_prefix(prefix) {
            return Some(value.trim().to_string());
        }
    }
    None
}

fn extract_apply_patch_error(text: &str) -> Option<String> {
    for line in text.lines() {
        let trimmed = line.trim();
        if trimmed.starts_with("apply_patch failed:") {
            return Some(trimmed.to_string());
        }
    }
    None
}

fn validate_patch_attempt(workspace: &Path, patch: &str) -> Result<(), String> {
    let parsed = parse_patch(patch).map_err(|err| format!("patch rejected before apply: {err}"))?;
    let patch_paths = parsed
        .hunks
        .iter()
        .map(hunk_path_string)
        .collect::<Result<Vec<_>, _>>()?;

    for path in &patch_paths {
        if Path::new(path).is_absolute() || path.starts_with("/workspace/") {
            return Err(format!("patch rejected: patch path must be workspace-relative: {path}"));
        }
        if workspace.ends_with("canon") && !path.starts_with("canon-utils/") && !path.starts_with("scripts/") {
            return Err(format!("patch rejected: unexpected patch path for repo-root harness: {path}"));
        }
    }

    Ok(())
}

fn hunk_path_string(hunk: &Hunk) -> Result<String, String> {
    match hunk {
        Hunk::AddFile { path, .. } | Hunk::DeleteFile { path } | Hunk::UpdateFile { path, .. } => {
            path.to_str()
                .map(|s| s.to_string())
                .ok_or_else(|| "patch rejected: non-utf8 path".to_string())
        }
    }
}

fn call_planner(workspace: &Path, prompt: &str) -> Result<Vec<PlannerAction>> {
    let (tx, rx) = mpsc::channel();
    let emitter: EventEmitterHandle = Arc::new(CapturingEmitter { tx: Mutex::new(tx) });

    for attempt in 1..=LLM_MAX_RETRIES {
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
            emitter: emitter.clone(),
            trigger_id: EventId::new("min-harness-root"),
        })?;
        match wait_for_llm_response(&rx, &request_id) {
            Ok(actions) => return Ok(actions),
            Err(err) if err.to_string().contains("timed out waiting for planner result") && attempt < LLM_MAX_RETRIES => {
                eprintln!(
                    "[canon-harness-repair] planner timeout after {}s, retrying ({}/{})",
                    LLM_TIMEOUT.as_secs(),
                    attempt,
                    LLM_MAX_RETRIES
                );
                continue;
            }
            Err(err) => return Err(err),
        }
    }

    bail!("planner retries exhausted without a response")
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
            .or_else(|| self.raw.get("kind"))
            .and_then(|v| v.as_str())
            .ok_or_else(|| anyhow!("planner action is missing `action`/`kind`: {}", self.raw))
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
    eprintln!(
        "[canon-harness-repair] initial target test {} for {}::{}",
        if result.success { "passed" } else { "failed" },
        crate_name,
        test_name
    );
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

fn print_verify_status(crate_name: &str, test_name: &str, verify: &CommandResult) {
    eprintln!(
        "[canon-harness-repair] verifier {} for {}::{}",
        if verify.success { "passed" } else { "failed" },
        crate_name,
        test_name
    );
    if !verify.success {
        let snippet = truncate(&verify.output, 1200);
        eprintln!("[canon-harness-repair] verifier output:\n{}", snippet);
    }
}
