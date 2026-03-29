use anyhow::{anyhow, bail, Context, Result};
use canon_event::{CapabilityFailed, CapabilityResult, EventEmitter, EventEmitterHandle, EventId, LlmCall, RuntimeEvent};
use canon_exec::{ExecutableEvent, ExecutionContext};
use canon_loop::{HarnessRepairTarget, LoopStageExecutor};
use canon_tools_patch::{apply_patch, parse_patch, Hunk};
use serde_json::Value;
use std::collections::hash_map::DefaultHasher;
use std::fs::OpenOptions;
use std::hash::{Hash, Hasher};
use std::io::Write;
use std::path::{Component, Path, PathBuf};
use std::process::Command;
use std::sync::mpsc::{Receiver, Sender};
use std::sync::{Arc, Mutex};
use std::time::Duration;
use uuid::Uuid;

fn hash_str(s: &str) -> u64 {
    let mut h = DefaultHasher::new();
    s.hash(&mut h);
    h.finish()
}

struct HarnessLogger {
    file: Mutex<std::fs::File>,
}

impl HarnessLogger {
    fn open(log_path: &Path) -> Result<Self> {
        if let Some(parent) = log_path.parent() {
            std::fs::create_dir_all(parent)?;
        }
        let file = OpenOptions::new().create(true).append(true).open(log_path)
            .with_context(|| format!("failed to open log file: {}", log_path.display()))?;
        Ok(Self { file: Mutex::new(file) })
    }

    fn log(&self, msg: &str) {
        let ts = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .map(|d| d.as_secs())
            .unwrap_or(0);
        let line = format!("[{ts}] {msg}\n");
        eprintln!("[canon-harness-repair] {msg}");
        if let Ok(mut f) = self.file.lock() {
            let _ = f.write_all(line.as_bytes());
        }
    }
}

const DEFAULT_WORKSPACE: &str = "/workspace/ai_sandbox/canon";
const DEFAULT_MAX_STEPS: usize = 5;
const LLM_TIMEOUT: Duration = Duration::from_secs(20);
const LLM_MAX_RETRIES: usize = 3;
const MAX_READ_BYTES: usize = 16 * 1024;
const MAX_TOOL_SNIPPET: usize = 4 * 1024;

const PLANNER_SYSTEM_INSTRUCTIONS: &str = r#"You are the canon harness repair agent.

Your sole responsibility is to make a single failing Rust test pass inside the canon workspace.
You will be given the failing test name, the crate it belongs to, the test failure output, and
optionally a human-authored guidance plan describing the exact fix required.

Each conversation turn you receive either:
  (a) the initial repair context — crate, test, failure output, guidance; or
  (b) the result of your last action — read output, patch success/failure, verify output.

You respond with exactly one action per turn. You work step-by-step:
  1. Read the function you need to change (never patch from memory or training knowledge).
  2. Apply the patch with correct anchors taken from the fresh read.
  3. Verify the fix compiled and the test passes.
  4. Declare done when the test is green.

You are operating inside a self-repair loop. The harness will call you repeatedly until the
test passes or the step budget is exhausted. Every action you emit is executed immediately and
its result is returned to you in the next turn.

Produce a plan as a JSON array of actions.

━━━ TOOLS ━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━

1. list_dir — list what files/dirs exist (use BEFORE assuming project state)
   {"action":"list_dir","path":"."}

2. read_file — read a file's current contents before editing it
   {"action":"read_file","path":"src/main.rs"}
   {"action":"read_file","path":"src/main.rs","line":120}  ← start from line 120
   ⚠ The file content is returned to you in the next turn. Do not mix reads with edits.
   ⚠ Always read the function you intend to patch BEFORE emitting apply_patch. Never patch from memory.
   ⚠ Use the `line` field to jump directly to the function — do not re-read the whole file if you know the line.

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
   - emit `read_file` with `line` set to the function start to get current content
   - then retry with anchors from the fresh read

4. run_command — run a shell command
   {"action":"run_command","cmd":"cargo check -p canon-route","cwd":"<TARGET_WORKSPACE>"}

   Use run_command with rg or sed to locate symbols before reading:
   Find a function:  {"action":"run_command","cmd":"rg -n 'fn deterministic_route_for_event' canon-utils/canon-route/src/policy.rs","cwd":"<TARGET_WORKSPACE>"}
   Slice lines:      {"action":"run_command","cmd":"sed -n '810,870p' canon-utils/canon-route/src/policy.rs","cwd":"<TARGET_WORKSPACE>"}
   Find callers:     {"action":"run_command","cmd":"rg -n 'workspace_state_drift_detected' canon-utils/canon-route/src/","cwd":"<TARGET_WORKSPACE>"}

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
- Never propose pseudo-code or textual edit instructions like "change X to Y".
- If you identify a code fix, read the target function first, then express it as an `apply_patch` action.
- If you are not ready to patch safely, emit `read_file` or `list_dir`; do not narrate the intended fix.

OUTPUT FORMAT:
- Return ONLY a JSON array of action objects.
 - No prose outside the JSON array.
 - Wrap the JSON array in a markdown code block (STRICT REQUIRED):
   ```json
   [ ... ]
   ```
"#;

static PLANNER_SYSTEM_PROMPT_ID: std::sync::LazyLock<u64> =
    std::sync::LazyLock::new(|| hash_str(PLANNER_SYSTEM_INSTRUCTIONS));

#[derive(Clone, Debug)]
struct PlannerAction {
    raw: Value,
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

    let log_path = workspace.join("state/harness_repair.log");
    let logger = Arc::new(HarnessLogger::open(&log_path)?);

    let target = HarnessRepairTarget::new(Some(crate_name.clone()), Some(test_name.clone()));
    let mut failure_output = if let Some(path) = stderr_file {
        std::fs::read_to_string(path)?
    } else {
        run_target_test_capture(&workspace, &crate_name, &test_name, always_dispatch)?
    };

    logger.log(&format!("start crate={crate_name} test={test_name} max_steps={max_steps}"));

    canon_exec::init_llm_worker();

    // FIX: create persistent emitter BEFORE any LLM calls
    let (tx, rx) = std::sync::mpsc::channel();
    let emitter: canon_event::EventEmitterHandle =
        std::sync::Arc::new(CapturingEmitter { tx: std::sync::Mutex::new(tx) });
    let result = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
        run_harness_loop(
            &workspace,
            &crate_name,
            &test_name,
            &target,
            &mut failure_output,
            max_steps,
            &logger,
            &emitter,
            &rx,
        )
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
            logger.log(&format!("panic: {panic_msg}"));
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
    logger: &HarnessLogger,
    emitter: &EventEmitterHandle,
    rx: &Receiver<RuntimeEvent>,
) -> Result<()> {
    let mut executor = LoopStageExecutor::new(workspace.to_path_buf(), workspace.join("state/event_log/event.tlog.d"));

    // use emitter from main (single channel)

    // Build tier-2 context base once: guidance plan + fn_index + initial failure.
    let plan_hint = find_implementation_plan(workspace, crate_name, test_name);
    let fn_index_text: Option<String> = extract_primary_file_line(failure_output)
        .and_then(|(cited_file, _)| {
            let cmd = format!("rg -n '^pub fn |^fn ' {cited_file}");
            run_shell_command(workspace, &cmd, &workspace.display().to_string())
                .ok()
                .filter(|r| !r.output.is_empty())
                .map(|r| format!("{cited_file} functions:\n{}", truncate(&r.output, MAX_TOOL_SNIPPET)))
        });
    let context_base = build_context_base(
        workspace, crate_name, test_name, failure_output,
        plan_hint.as_deref(),
        fn_index_text.as_deref(),
    );
    let context_base_id = hash_str(&context_base).to_string();
    let system_prompt_id = PLANNER_SYSTEM_PROMPT_ID.to_string();

    // Per-step tracking.
    let mut last_action_result: Option<String> = None;
    let mut last_request_id: Option<String> = None;

    for step in 0..max_steps {
        let directive = executor.evaluate_harness_repair_for_target(target, failure_output);
        let delta = build_turn_delta(
            crate_name, test_name, failure_output,
            &directive.decision.reason,
            last_action_result.as_deref(),
        );
        // Tier 1 + 2 sent only on the first call; stateful endpoints skip them on subsequent turns.
        let send_system = step == 0;
        let send_base = step == 0;
        let actions = match call_planner(
            workspace,
            &delta,
            send_system.then_some(PLANNER_SYSTEM_INSTRUCTIONS),
            &system_prompt_id,
            send_base.then_some(context_base.as_str()),
            &context_base_id,
            last_request_id.as_deref(),
            &emitter,
            &rx,
        ) {
            Ok((actions, req_id)) => {
                last_request_id = Some(req_id);
                actions
            }
            Err(err) => {
                let err_str = err.to_string();
                if err_str.starts_with(BAD_RESPONSE_PREFIX) {
                    // LLM responded with emoji/prose instead of JSON.
                    // Inject an explicit correction so the next turn demands valid JSON.
                    let correction = format!(
                        "INVALID RESPONSE — your last reply was not a JSON action array.\n\
                        {err_str}\n\n\
                        You MUST respond with ONLY a JSON array. No emoji. No prose. No markdown.\n\
                        Emit exactly one action now."
                    );
                    logger.log(&format!("step={} bad_response injecting_correction", step + 1));
                    last_action_result = Some(correction);
                } else {
                    let message = format!("planner call failed: {err_str}");
                    logger.log(&format!("step={} planner_failure reason={}", step + 1, message));
                    last_action_result = Some(message.clone());
                    *failure_output = message;
                }
                continue;
            }
        };
        if actions.len() != 1 {
            let message = format!(
                "planner returned {} actions; minimum harness requires exactly one",
                actions.len()
            );
            logger.log(&format!("step={} planner_failure reason={}", step + 1, message));
            last_action_result = Some(message.clone());
            *failure_output = message.clone();
            continue;
        }
        let action = &actions[0];
        let action_kind = match action.kind() {
            Ok(kind) => kind.to_string(),
            Err(err) => {
                let message = format!("planner action parse failed: {err}");
                logger.log(&format!("step={} malformed_action payload={} reason={}", step + 1, action.raw, message));
                last_action_result = Some(message.clone());
                *failure_output = message.clone();
                continue;
            }
        };
        logger.log(&format!("step={} action={} crate={crate_name} test={test_name}", step + 1, action_kind));
        // Returns Ok((done, action_result_for_next_turn)).
        let step_result = (|| -> Result<(bool, String)> {
            match action_kind.as_str() {
                "done" => {
                    let verify = verify_full_crate(workspace, crate_name, test_name)?;
                    print_verify_status(crate_name, test_name, &verify, logger);
                    if verify.success {
                        logger.log("harness repair complete: crate test suite passed");
                        return Ok((true, String::new()));
                    }
                    *failure_output = verify.output.clone();
                    return Ok((false, format!("verify failed:\n{}", truncate(&verify.output, 2000))));
                }
                "list_dir" => {
                    let path = action.path()?;
                    let output = run_list_dir(workspace, path)?;
                    return Ok((false, format!("list_dir {path}:\n{output}")));
                }
                "read_file" => {
                    let path = action.path()?;
                    let line_offset = action.line_offset();
                    let output = run_read_file(workspace, path, line_offset)?;
                    return Ok((false, format!("read_file {path}:\n{output}")));
                }
                "apply_patch" => {
                    let patch = action.patch()?;
                    if let Err(message) = validate_patch_attempt(workspace, patch) {
                        logger.log(&format!("step={} apply_patch rejected reason={}", step + 1, message));
                        *failure_output = message.clone();
                        return Ok((false, format!("apply_patch rejected: {message}")));
                    }
                    match apply_patch(patch, workspace) {
                        Ok(_) => {
                            logger.log(&format!("step={} apply_patch ok", step + 1));
                        }
                        Err(err) => {
                            let patch_dump = workspace.join("state/harness_last_failed.patch");
                            let _ = std::fs::create_dir_all(
                                patch_dump.parent().ok_or_else(|| anyhow!("invalid patch dump path"))?,
                            );
                            let _ = std::fs::write(&patch_dump, patch);
                            let mut delta_msg = format!("apply_patch failed: {err}");
                            logger.log(&format!("step={} apply_patch failed reason={}", step + 1, err));
                            *failure_output = format!(
                                "apply_patch failed: {err}\nfailed_patch_file={}",
                                patch_dump.display(),
                            );
                            // Auto-read the file whose anchors were not found so the
                            // next turn sees the actual content.
                            if let Some(anchor_path) = extract_anchor_fail_path(&err.to_string()) {
                                if let Ok(content) = run_read_file(workspace, &anchor_path, None) {
                                    logger.log(&format!("step={} auto_read anchor_fail_path={anchor_path}", step + 1));
                                    delta_msg = format!(
                                        "apply_patch failed: {err}\n\nCurrent content of {anchor_path}:\n{content}"
                                    );
                                }
                            }
                            return Ok((false, delta_msg));
                        }
                    }
                    let verify = verify_after_mutation(workspace, crate_name, test_name)?;
                    print_verify_status(crate_name, test_name, &verify, logger);
                    if verify.success {
                        logger.log("harness repair complete: crate test suite passed");
                        return Ok((true, String::new()));
                    }
                    *failure_output = verify.output.clone();
                    return Ok((false, format!("patch applied; verify failed:\n{}", truncate(&verify.output, 2000))));
                }
                "run_command" => {
                    let cmd = action.command()?;
                    let cwd = action.cwd()?;
                    logger.log(&format!("step={} run_command cmd={cmd}", step + 1));
                    let result = run_shell_command(workspace, cmd, cwd)?;
                    logger.log(&format!("step={} run_command success={}", step + 1, result.success));
                    if !result.success {
                        *failure_output = result.output.clone();
                        return Ok((false, format!("run_command failed:\n{}", truncate(&result.output, 2000))));
                    }
                    let verify = verify_after_mutation(workspace, crate_name, test_name)?;
                    print_verify_status(crate_name, test_name, &verify, logger);
                    if verify.success {
                        logger.log("harness repair complete: crate test suite passed");
                        return Ok((true, String::new()));
                    }
                    *failure_output = verify.output.clone();
                    return Ok((false, format!("run_command ok; verify failed:\n{}", truncate(&verify.output, 2000))));
                }
                other => {
                    let message = format!("unsupported planner action in minimum harness: {other}");
                    logger.log(&format!("step={} unsupported_action kind={other}", step + 1));
                    *failure_output = message.clone();
                    return Ok((false, message));
                }
            }
        })();
        match step_result {
            Ok((true, _)) => return Ok(()),
            Ok((false, result)) => {
                // ALWAYS update failure_output with latest result
                *failure_output = result.clone();

                // append guidance without removing signal
                if let Some(forced) = derive_next_action_hint(&result) {
                    last_action_result = Some(format!(
                        "{}\n\nNEXT ACTION HINT:\n{}",
                        result,
                        forced
                    ));
                } else {
                    last_action_result = Some(result);
                }
            }
            Err(err) => {
                let message = format!("action execution failed: {err}");
                logger.log(&format!("step={} action_error reason={err}", step + 1));
                last_action_result = Some(message.clone());
                *failure_output = message.clone();
            }
        }
    }

    logger.log(&format!("exhausted max_steps={max_steps} crate={crate_name} test={test_name}"));
    bail!("harness repair stopped after {max_steps} steps without passing the target test")
}

// deterministic result → action mapping
fn derive_next_action_hint(result: &str) -> Option<String> {
    // patch failure / anchor mismatch
    if result.contains("expected lines not found") || result.contains("apply_patch failed") {
        if let Some(path) = extract_anchor_fail_path(result) {
            return Some(format!(
                r#"{{"action":"read_file","path":"{}"}}"#,
                path
            ));
        }
    }

    // compiler or runtime error with file:line
    if let Some((file, line)) = extract_primary_file_line(result) {
        return Some(format!(
            r#"{{"action":"read_file","path":"{}","line":{}}}"#,
            file, line
        ));
    }

    // assertion failures
    if result.contains("assertion failed")
        || result.contains("panicked at")
        || result.contains("left != right")
    {
        if let Some((file, line)) = extract_primary_file_line(result) {
            return Some(format!(
                r#"{{"action":"read_file","path":"{}","line":{}}}"#,
                file, line
            ));
        }
    }

    None
}


/// Remove `... ok` passing test lines and collapse to a summary count.
fn strip_passing_tests(output: &str) -> String {
    let mut passed = 0usize;
    let mut kept: Vec<&str> = Vec::new();
    for line in output.lines() {
        if line.trim_start().starts_with("test ") && line.trim_end().ends_with("... ok") {
            passed += 1;
        } else {
            kept.push(line);
        }
    }
    let mut result = if passed > 0 {
        format!("[{passed} tests passed — output omitted]\n")
    } else {
        String::new()
    };
    result.push_str(&kept.join("\n"));
    result
}

/// Remove Rust backtrace frames — keep only lines that mention project source files.
fn strip_backtrace(output: &str) -> String {
    let mut in_backtrace = false;
    let mut kept: Vec<&str> = Vec::new();
    for line in output.lines() {
        let trimmed = line.trim();
        if trimmed == "stack backtrace:" {
            in_backtrace = true;
            continue;
        }
        if in_backtrace {
            // Keep lines that reference workspace source files
            if trimmed.contains(".rs:") && !trimmed.contains("/rustc/") && !trimmed.contains("/.rustup/") {
                // Extract just the "at path:line" part
                if let Some(at_idx) = trimmed.find("at ") {
                    kept.push(&line[line.find("at ").unwrap()..]);
                    let _ = at_idx;
                }
            }
            // Blank line ends backtrace
            if trimmed.is_empty() {
                in_backtrace = false;
            }
            continue;
        }
        kept.push(line);
    }
    kept.join("\n")
}

fn clean_failure_output(output: &str) -> String {
    let stripped = strip_passing_tests(output);
    strip_backtrace(&stripped)
}

/// Tier 2: slow-changing repair context. Sent once per session, cached by hash.
/// Contains everything the agent needs to understand the task — but not the per-step delta.
fn build_context_base(
    workspace: &Path,
    crate_name: &str,
    test_name: &str,
    initial_failure: &str,
    plan_hint: Option<&str>,
    fn_index: Option<&str>,
) -> String {
    let guidance_section = match plan_hint {
        Some(hint) => format!(
            "\nGuidance (human-authored implementation plan — apply this exactly):\n{hint}\n"
        ),
        None => String::new(),
    };
    let fn_index_section = match fn_index {
        Some(idx) if !idx.is_empty() => format!("\nFunction index:\n{idx}\n"),
        _ => String::new(),
    };
    let cleaned = clean_failure_output(initial_failure);
    let failure_focus = build_failure_focus(initial_failure);
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
{guidance_section}{fn_index_section}\n\
Context rules:\n\
- You decide what context you need. Use `read_file` and `list_dir` freely before patching.\n\
- Before patching any function, emit `read_file` for the file. Do not patch from memory.\n\
- If a patch failed with \"expected lines not found\", re-read the file and retry with anchors from the fresh content.\n\
- A file/line in the failure output identifies where the test asserts — the implementation to fix may be in a different function.\n\
- Your response must be a JSON action array only. No prose.\n\
\n\
Failure focus:\n{failure_focus}\n\
\n\
Initial failure output:\n{cleaned}\n\
\n\
Preferred verifier after mutation:\n\
- `cargo check -p {crate_name}`\n\
- then `cargo test -p {crate_name} {test_name} -- --nocapture`",
        workspace = workspace.display(),
        crate_name = crate_name,
        test_name = test_name,
        cleaned = truncate(&cleaned, 4000),
        guidance_section = guidance_section,
        fn_index_section = fn_index_section,
        failure_focus = failure_focus,
    )
}

/// Tier 3: per-step delta. First turn = repair directive. Subsequent turns = action result only.
fn build_turn_delta(
    crate_name: &str,
    test_name: &str,
    failure_output: &str,
    directive_reason: &str,
    last_action_result: Option<&str>,
) -> String {
    if let Some(result) = last_action_result {
        // FIX: suppress planner timeout noise from poisoning next prompt
        if result.contains("planner call failed") {
            return "Retry previous step. Emit exactly one action.".to_string();
        }
        let guidance = build_action_guidance(result);

        format!(
            "Action result:\n{result}\n\n\
Next action guidance:\n{guidance}\n\n\
Emit exactly one action.",
            result = truncate(result, 2000),
            guidance = guidance,
        )
    } else {
        let failure_focus = build_failure_focus(failure_output);
        format!(
            "Repair directive:\n\
- {directive_reason}\n\
- Emit exactly one action.\n\
\n\
Failure focus:\n{failure_focus}\n\
\n\
Preferred verifier after mutation:\n\
- `cargo check -p {crate_name}`\n\
- then `cargo test -p {crate_name} {test_name} -- --nocapture`",
            directive_reason = directive_reason,
            failure_focus = failure_focus,
            crate_name = crate_name,
            test_name = test_name,
        )
    }
}

fn build_failure_focus(failure_output: &str) -> String {
    let mut lines = Vec::new();

    if let Some((file, line)) = extract_primary_file_line(failure_output) {
        lines.push(format!("- assertion_site: {file}:{line} (this is the test assert line — read the implementation to find the function to fix)"));
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
    if let Some(rejection) = extract_patch_rejection(failure_output) {
        lines.push(format!("- patch_rejection: {rejection}"));
    }

    if lines.is_empty() {
        "none".to_string()
    } else {
        lines.join("\n")
    }
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

fn extract_patch_rejection(text: &str) -> Option<String> {
    for line in text.lines() {
        let trimmed = line.trim();
        if trimmed.starts_with("patch rejected:") {
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

fn call_planner(
    workspace: &Path,
    delta: &str,
    system: Option<&str>,
    system_prompt_id: &str,
    context_base: Option<&str>,
    context_base_id: &str,
    prev_prompt_id: Option<&str>,
    emitter: &EventEmitterHandle,
    rx: &Receiver<RuntimeEvent>,
) -> Result<(Vec<PlannerAction>, String)> {

    for attempt in 1..=LLM_MAX_RETRIES {
        let request_id = Uuid::new_v4().to_string();
        let event = RuntimeEvent::Llm(LlmCall {
            request_id: request_id.clone(),
            prompt: delta.to_string(),
            role: Some("planner".to_string()),
            agent_id: Some("planner_chatgpt_group".to_string()),
            dispatched: true,
            system: system.map(str::to_string),
            system_prompt_id: Some(system_prompt_id.to_string()),
            context_base: context_base.map(str::to_string),
            context_base_id: Some(context_base_id.to_string()),
            prompt_base_id: Some(system_prompt_id.to_string()),
            prev_prompt_id: prev_prompt_id.map(str::to_string),
        });
        let exec = ExecutableEvent::try_from(event).expect("llm event should be executable");
        exec.execute(ExecutionContext {
            workspace: workspace.to_path_buf(),
            emitter: emitter.clone(),
            trigger_id: EventId::new("min-harness-root"),
        })?;
        match wait_for_llm_response(&rx, &request_id) {
            Ok(actions) => return Ok((actions, request_id)),
            // Bad response (emoji/prose/empty): arrived fast but wasn't valid JSON.
            // Do NOT retry — return immediately so the outer loop sends a correction turn.
            Err(err) if err.to_string().starts_with(BAD_RESPONSE_PREFIX) => {
                return Err(err);
            }
            // True timeout: LLM API hung. Retry up to LLM_MAX_RETRIES times.
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

fn parse_planner_actions(value: &serde_json::Value) -> anyhow::Result<Vec<PlannerAction>> {
    let array = if let Some(array) = value.as_array() {
        array.clone()
    } else if value.is_object() {
        if value.get("action").is_some() || value.get("kind").is_some() {
            vec![value.clone()]
        } else if let Some(text) = value.get("text").and_then(|v| v.as_str()) {
            parse_planner_actions_from_text(text)?
        } else {
            let preview = value.to_string().chars().take(120).collect::<String>();
            anyhow::bail!("bad_response: response was not a JSON action payload. Got: {preview:?}");
        }
    } else if let Some(text) = value.as_str() {
        parse_planner_actions_from_text(text)?
    } else {
        let preview = value.to_string().chars().take(120).collect::<String>();
        anyhow::bail!("bad_response: response was not a JSON action payload. Got: {preview:?}");
    };

    if array.is_empty() {
        anyhow::bail!("bad_response: response was an empty JSON array");
    }

    Ok(array.into_iter().map(|raw| PlannerAction { raw }).collect())
}

fn parse_planner_actions_from_text(text: &str) -> anyhow::Result<Vec<serde_json::Value>> {
    let cleaned = text
        .trim()
        .trim_start_matches("```json")
        .trim_start_matches("```JSON")
        .trim_start_matches("```")
        .trim_end_matches("```")
        .trim();

    if let Ok(arr) = serde_json::from_str::<Vec<serde_json::Value>>(cleaned) {
        return Ok(arr);
    }

    if let Ok(obj) = serde_json::from_str::<serde_json::Value>(cleaned) {
        if obj.is_object() {
            return Ok(vec![obj]);
        }
    }

    let preview = text.chars().take(120).collect::<String>();
    anyhow::bail!("bad_response: response was not a JSON array. Got: {preview:?}");
}

/// Marker prefix on errors that mean "response arrived but was not a JSON action array".
const BAD_RESPONSE_PREFIX: &str = "bad_response:";

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

fn run_read_file(workspace: &Path, relative: &str, start_line: Option<usize>) -> Result<String> {
    let path = resolve_workspace_path(workspace, relative)?;
    let bytes = std::fs::read(&path).with_context(|| format!("read_file failed for {}", path.display()))?;
    let full = String::from_utf8_lossy(&bytes).into_owned();
    let text = if let Some(line) = start_line {
        full.lines()
            .enumerate()
            .skip(line.saturating_sub(1))
            .take(250)
            .map(|(i, l)| format!("{}: {}", line + (i + 1).saturating_sub(line.saturating_sub(1)), l))
            .collect::<Vec<_>>()
            .join("\n")
    } else {
        String::from_utf8_lossy(&bytes[..bytes.len().min(MAX_READ_BYTES)]).into_owned()
    };
    Ok(text)
}

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

/// Scan the workspace root for `implementation_plan_*.md` files and return the
/// content of the first one that mentions `crate_name` or `test_name`.
fn find_implementation_plan(workspace: &Path, crate_name: &str, test_name: &str) -> Option<String> {
    let entries = std::fs::read_dir(workspace).ok()?;
    for entry in entries.flatten() {
        let name = entry.file_name();
        let fname = name.to_string_lossy();
        if !fname.starts_with("implementation_plan_") || !fname.ends_with(".md") {
            continue;
        }
        if let Ok(content) = std::fs::read_to_string(entry.path()) {
            if content.contains(crate_name) || content.contains(test_name) {
                return Some(content);
            }
        }
    }
    None
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

fn print_verify_status(crate_name: &str, test_name: &str, verify: &CommandResult, logger: &HarnessLogger) {
    let status = if verify.success { "passed" } else { "failed" };
    logger.log(&format!("verify {status} crate={crate_name} test={test_name}"));
    if !verify.success {
        let snippet = truncate(&verify.output, 1200);
        logger.log(&format!("verify output:\n{snippet}"));
    }
}
fn build_action_guidance(result: &str) -> String {
    if result.contains("expected lines not found") || result.contains("apply_patch failed") {
        return "Patch failed → read the file again at the correct location before retrying.".to_string();
    }

    if result.contains(".rs:") && result.contains("error") {
        return "Compiler error → read the file at the reported line and fix the issue.".to_string();
    }

    if result.contains("assertion failed")
        || result.contains("panicked at")
        || result.contains("left != right")
    {
        return "Test failed → read the assertion location and trace back to the implementation.".to_string();
    }

    if result.contains("read_file") {
        return "File read complete → apply_patch using exact lines from the read.".to_string();
    }

    if result.contains("verify failed") {
        return "Patch applied but failed → inspect failure output and refine fix.".to_string();
    }

    "Use best judgment. Prefer read_file before patching.".to_string()
}

impl PlannerAction {
    fn kind(&self) -> Result<&str> {
        self.raw
            .get("action")
            .or_else(|| self.raw.get("kind"))
            .and_then(|v| v.as_str())
            .ok_or_else(|| anyhow!("planner action missing kind"))
    }

    fn path(&self) -> Result<&str> {
        self.raw
            .get("path")
            .and_then(|v| v.as_str())
            .ok_or_else(|| anyhow!("planner action missing path"))
    }

    fn patch(&self) -> Result<&str> {
        self.raw.get("patch").and_then(|v| v.as_str())
            .ok_or_else(|| anyhow!("planner action is missing `patch`: {}", self.raw))
    }

    fn command(&self) -> Result<&str> {
        self.raw.get("cmd").and_then(|v| v.as_str())
            .ok_or_else(|| anyhow!("planner action is missing `cmd`: {}", self.raw))
    }

    fn cwd(&self) -> Result<&str> {
        self.raw.get("cwd").and_then(|v| v.as_str())
            .ok_or_else(|| anyhow!("planner action is missing `cwd`: {}", self.raw))
    }

    fn line_offset(&self) -> Option<usize> {
        self.raw.get("line").and_then(|v| v.as_u64()).map(|v| v as usize)
    }
}
