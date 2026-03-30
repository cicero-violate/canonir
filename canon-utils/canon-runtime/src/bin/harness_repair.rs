use anyhow::{anyhow, bail, Context, Result};
use canon_event::{EventConsumer, EventEmitter, EventEmitterHandle, EventId, RuntimeEvent};
use canon_invariant::control_harness::{
    evaluate_control_state, step_control_state, synthetic_control_events,
    synthetic_control_metrics, synthetic_control_seed_states, synthetic_control_trace_metrics,
    ControlDecision,
};
use canon_llm::relay::{relay_client_call, RelayRequest, RELAY_ADDR};
use canon_loop::{evaluate_harness_repair_loop, HarnessRepairTarget, HarnessRepairPhase, LoopStageExecutor};
use canon_runtime::consumers::repair_control_consumer::RepairControlConsumer;
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

fn log_control_harness_summary(logger: &HarnessLogger) {
    let seed_metrics = synthetic_control_metrics();
    logger.log(&format!(
        "control_harness seeds={} suppressed={} replay={} fresh={} emit={} invariant={}",
        seed_metrics.states_explored,
        seed_metrics.suppressed,
        seed_metrics.replayed_cached_route,
        seed_metrics.requested_fresh_route,
        seed_metrics.emitted_route,
        seed_metrics.invariant_violations,
    ));

    let depth_one = synthetic_control_trace_metrics(1);
    logger.log(&format!(
        "control_harness trace_depth=1 start_states={} traces={} suppressed={} replay={} fresh={} emit={} invariant={}",
        depth_one.start_states,
        depth_one.traces_explored,
        depth_one.suppressed_terminal,
        depth_one.replay_terminal,
        depth_one.fresh_route_terminal,
        depth_one.emit_terminal,
        depth_one.invariant_terminal,
    ));

    let depth_two = synthetic_control_trace_metrics(2);
    logger.log(&format!(
        "control_harness trace_depth=2 start_states={} traces={} suppressed={} replay={} fresh={} emit={} invariant={}",
        depth_two.start_states,
        depth_two.traces_explored,
        depth_two.suppressed_terminal,
        depth_two.replay_terminal,
        depth_two.fresh_route_terminal,
        depth_two.emit_terminal,
        depth_two.invariant_terminal,
    ));
}

fn log_repair_harness_summary(logger: &HarnessLogger) -> Result<()> {
    let metrics = canon_loop::harness_repair::synthetic_harness_repair_metrics();
    let classified =
        metrics.observe + metrics.decide + metrics.repair + metrics.verify + metrics.update + metrics.stop;

    if metrics.total_states == 0 {
        bail!("repair_harness state mapping explored zero states");
    }
    if metrics.total_states != classified {
        bail!(
            "repair_harness state mapping mismatch: total={} classified={}",
            metrics.total_states,
            classified
        );
    }
    if metrics.observe == 0
        || metrics.decide == 0
        || metrics.repair == 0
        || metrics.verify == 0
        || metrics.update == 0
    {
        bail!(
            "repair_harness missing required phase coverage: observe={} decide={} repair={} verify={} update={} stop={}",
            metrics.observe,
            metrics.decide,
            metrics.repair,
            metrics.verify,
            metrics.update,
            metrics.stop
        );
    }

    logger.log(&format!(
        "repair_harness states={} observe={} decide={} repair={} verify={} update={} stop={}",
        metrics.total_states,
        metrics.observe,
        metrics.decide,
        metrics.repair,
        metrics.verify,
        metrics.update,
        metrics.stop,
    ));

    Ok(())
}

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
struct SyntheticRuntimeProductMetrics {
    control_seed_states: usize,
    repair_seed_states: usize,
    total_pairs: usize,
    total_transitions: usize,
    suppressed_pairs: usize,
    replay_pairs: usize,
    fresh_pairs: usize,
    emit_pairs: usize,
    invariant_pairs: usize,
    observe_pairs: usize,
    decide_pairs: usize,
    repair_pairs: usize,
    verify_pairs: usize,
    update_pairs: usize,
    stop_pairs: usize,
    blocked_pairs: usize,
    productive_pairs: usize,
}

fn synthetic_runtime_product_metrics() -> SyntheticRuntimeProductMetrics {
    let control_states = synthetic_control_seed_states();
    let repair_states = canon_loop::harness_repair::synthetic_harness_repair_states();
    let control_events = synthetic_control_events();
    let mut metrics = SyntheticRuntimeProductMetrics {
        control_seed_states: control_states.len(),
        repair_seed_states: repair_states.len(),
        ..SyntheticRuntimeProductMetrics::default()
    };

    for control in &control_states {
        for repair in &repair_states {
            metrics.total_pairs += 1;

            match evaluate_control_state(*control) {
                ControlDecision::Suppress(_) => metrics.suppressed_pairs += 1,
                ControlDecision::ReplayCachedRoute => metrics.replay_pairs += 1,
                ControlDecision::RequestFreshRoute => metrics.fresh_pairs += 1,
                ControlDecision::EmitRoute => metrics.emit_pairs += 1,
                ControlDecision::InvariantViolation(_) => metrics.invariant_pairs += 1,
            }

            match evaluate_harness_repair_loop(repair).phase {
                HarnessRepairPhase::Observe => metrics.observe_pairs += 1,
                HarnessRepairPhase::Decide => metrics.decide_pairs += 1,
                HarnessRepairPhase::Repair => metrics.repair_pairs += 1,
                HarnessRepairPhase::Verify => metrics.verify_pairs += 1,
                HarnessRepairPhase::Update => metrics.update_pairs += 1,
                HarnessRepairPhase::Stop => metrics.stop_pairs += 1,
            }

            let mut pair_blocked = true;
            for event in control_events {
                metrics.total_transitions += 1;
                let next_control = step_control_state(*control, *event);
                let next_decision = evaluate_control_state(next_control);
                if !matches!(next_decision, ControlDecision::Suppress(_) | ControlDecision::InvariantViolation(_)) {
                    pair_blocked = false;
                }
            }

            if pair_blocked {
                metrics.blocked_pairs += 1;
            } else {
                metrics.productive_pairs += 1;
            }
        }
    }

    metrics
}

fn log_runtime_product_summary(logger: &HarnessLogger) -> Result<()> {
    let metrics = synthetic_runtime_product_metrics();
    let control_classified = metrics.suppressed_pairs
        + metrics.replay_pairs
        + metrics.fresh_pairs
        + metrics.emit_pairs
        + metrics.invariant_pairs;
    let repair_classified = metrics.observe_pairs
        + metrics.decide_pairs
        + metrics.repair_pairs
        + metrics.verify_pairs
        + metrics.update_pairs
        + metrics.stop_pairs;

    if metrics.total_pairs == 0 || metrics.total_transitions == 0 {
        bail!("runtime_product explored zero composed states or transitions");
    }
    if control_classified != metrics.total_pairs {
        bail!(
            "runtime_product control classification mismatch: total_pairs={} classified={}",
            metrics.total_pairs,
            control_classified
        );
    }
    if repair_classified != metrics.total_pairs {
        bail!(
            "runtime_product repair classification mismatch: total_pairs={} classified={}",
            metrics.total_pairs,
            repair_classified
        );
    }
    if metrics.blocked_pairs == metrics.total_pairs {
        bail!("runtime_product all composed states are blocked");
    }
    if metrics.replay_pairs == 0
        || metrics.fresh_pairs == 0
        || metrics.emit_pairs == 0
        || metrics.invariant_pairs == 0
        || metrics.observe_pairs == 0
        || metrics.decide_pairs == 0
        || metrics.repair_pairs == 0
        || metrics.verify_pairs == 0
        || metrics.update_pairs == 0
    {
        bail!(
            "runtime_product missing required coverage: replay={} fresh={} emit={} invariant={} observe={} decide={} repair={} verify={} update={} stop={}",
            metrics.replay_pairs,
            metrics.fresh_pairs,
            metrics.emit_pairs,
            metrics.invariant_pairs,
            metrics.observe_pairs,
            metrics.decide_pairs,
            metrics.repair_pairs,
            metrics.verify_pairs,
            metrics.update_pairs,
            metrics.stop_pairs,
        );
    }

    logger.log(&format!(
        "runtime_product control_states={} repair_states={} pairs={} transitions={} productive={} blocked={} suppress={} replay={} fresh={} emit={} invariant={} observe={} decide={} repair={} verify={} update={} stop={}",
        metrics.control_seed_states,
        metrics.repair_seed_states,
        metrics.total_pairs,
        metrics.total_transitions,
        metrics.productive_pairs,
        metrics.blocked_pairs,
        metrics.suppressed_pairs,
        metrics.replay_pairs,
        metrics.fresh_pairs,
        metrics.emit_pairs,
        metrics.invariant_pairs,
        metrics.observe_pairs,
        metrics.decide_pairs,
        metrics.repair_pairs,
        metrics.verify_pairs,
        metrics.update_pairs,
        metrics.stop_pairs,
    ));

    Ok(())
}

const DEFAULT_WORKSPACE: &str = "/workspace/ai_sandbox/canon";
const DEFAULT_MAX_STEPS: usize = 30;
const MAX_READ_BYTES: usize = 16 * 1024;
const MAX_TOOL_SNIPPET: usize = 4 * 1024;
const AUTO_READ_CONTEXT_BEFORE: usize = 20;
const AUTO_READ_CONTEXT_AFTER: usize = 40;
const MAX_CONSECUTIVE_PLANNER_TRANSPORT_FAILURES: usize = 3;
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

If the prompt contains a section named `FORCED PLANNER CONSTRAINT`, you must obey it exactly.
When a forced constraint is present, do not choose a different action class.

Return exactly one action in a JSON array wrapped in a `json` code block.

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
 - Wrap the JSON array in a markdown code block with language `json` (STRICT REQUIRED):
   ```json
   [ ... ]
   ```
"#;

static PLANNER_SYSTEM_PROMPT_ID: std::sync::LazyLock<u64> =
    std::sync::LazyLock::new(|| hash_str(PLANNER_SYSTEM_INSTRUCTIONS));

fn harness_should_use_relay() -> bool {
    std::env::var("CANON_HARNESS_USE_RELAY")
        .map(|value| {
            matches!(
                value.trim().to_ascii_lowercase().as_str(),
                "1" | "true" | "yes" | "on"
            )
        })
        .unwrap_or(false)
}

fn local_planner_fallback(
    delta: &str,
    prompt: &str,
    context_base: Option<&str>,
) -> Result<(Vec<PlannerAction>, String)> {
    let post_read_action = derive_post_read_action(delta, prompt);
    eprintln!(
        "[canon-harness-repair] local_planner_post_read_debug matched={} delta_preview={:?}",
        post_read_action.is_some(),
        delta.lines().take(6).collect::<Vec<_>>()
    );
    if let Some(next) = post_read_action {
        eprintln!("[canon-harness-repair] local_planner_post_read action={}", next);
        let next_json = format!("[{}]", next);
        let actions = parse_planner_actions(&serde_json::Value::String(next_json))?;
        return Ok((actions, "local-fallback-post-read".to_string()));
    }

    let delta_hint = derive_next_action_hint(delta);
    let prompt_primary = extract_primary_file_line(prompt)
        .map(|(path, line)| format!("{path}:{line}"));

    eprintln!(
        "[canon-harness-repair] local_planner_debug delta_bytes={} prompt_bytes={} context_bytes={} delta_hint={} prompt_primary={}",
        delta.len(),
        prompt.len(),
        context_base.map(|s| s.len()).unwrap_or(0),
        delta_hint.as_deref().unwrap_or("<none>"),
        prompt_primary.as_deref().unwrap_or("<none>"),
    );

    let fallback = delta_hint
        .or_else(|| {
            extract_primary_file_line(prompt).map(|(path, line)| {
                format!(
                    "{{\"action\":\"read_file\",\"path\":\"{}\",\"line\":{}}}",
                    path, line
                )
            })
        })
        .unwrap_or_else(|| "{\"action\":\"list_dir\",\"path\":\"canon-utils\"}".to_string());

    eprintln!(
        "[canon-harness-repair] local_planner_dispatch action={}",
        fallback
    );
    let fallback_json = format!("[{}]", fallback);
    let actions = parse_planner_actions(&serde_json::Value::String(fallback_json))?;
    Ok((actions, "local-planner".to_string()))
}

fn derive_post_run_command_action(result: &str) -> Option<String> {
    let mut lines = result.lines();
    if lines.next()? != "run_command ok:" {
        return None;
    }
    let path = lines.next()?.strip_prefix("path=")?.trim();

    for line in lines {
        let trimmed = line.trim_start();
        let (line_no, rest) = trimmed.split_once(':')?;
        let code = rest.trim_start();
        if code.starts_with("fn ") || code.starts_with("pub fn ") {
            let line_no = line_no.trim().parse::<usize>().ok()?;
            return Some(format!(
                "{{\"action\":\"read_file\",\"path\":\"{}\",\"line\":{}}}",
                path, line_no
            ));
        }
    }
    None
}

fn derive_post_read_action(delta: &str, prompt: &str) -> Option<String> {
    let lines: Vec<&str> = delta.lines().collect();
    let action_result_match = lines.windows(2).find_map(|pair| {
        if pair[0].trim() == "Action result:" {
            pair[1]
                .strip_prefix("read_file ")
                .or_else(|| pair[1].strip_prefix("run_command "))
                .map(str::to_string)
        } else {
            None
        }
    });
    let read_file_match = lines.iter().find_map(|line| {
        line.strip_prefix("read_file ")
            .or_else(|| line.strip_prefix("run_command "))
            .map(str::to_string)
    });
    eprintln!(
        "[canon-harness-repair] derive_post_read_action_trace lines0={:?} lines1={:?} action_result_match={:?} read_file_match={:?}",
        lines.get(0),
        lines.get(1),
        action_result_match,
        read_file_match
    );
    let path = action_result_match.or(read_file_match)?;
    let path = path.split(':').next()?.trim();
    if path.is_empty() || !path.ends_with(".rs") {
        return None;
    }
    let crate_name = path
        .split('/')
        .nth(1)
        .filter(|s| !s.is_empty())
        .or_else(|| {
            prompt
                .lines()
                .find_map(|line| line.trim().strip_prefix("- crate: "))
        })?;
    let test_name = delta
        .lines()
        .chain(prompt.lines())
        .find_map(|line| {
            let s = line.trim();
            s.strip_prefix("- failing_test_from_output: ")
                .or_else(|| s.strip_prefix("- failing test: "))
        })?;

    eprintln!(
        "[canon-harness-repair] derive_post_read_action_debug path={} crate={} test={} matched_action_result={} matched_read_file={} delta_preview={:?}",
        path,
        crate_name,
        test_name,
        lines.windows(2).any(|pair| {
            pair[0].trim() == "Action result:"
                && (pair[1].starts_with("read_file ") || pair[1].starts_with("run_command "))
        }),
        delta.lines().any(|line| line.starts_with("read_file ") || line.starts_with("run_command ")),
        delta.lines().take(10).collect::<Vec<_>>()
    );

    Some(format!(
        "{{\"action\":\"run_command\",\"cmd\":\"cargo test -p {} {} -- --exact --nocapture\",\"cwd\":\"/workspace/ai_sandbox/canon\"}}",
        crate_name,
        test_name
    ))
}

fn derive_next_action_hint(result: &str) -> Option<String> {
    if result.contains("expected lines not found") || result.contains("apply_patch failed") {
        if let Some(path) = extract_anchor_fail_path(result) {
            eprintln!(
                "[canon-harness-repair] derive_next_action_hint_debug branch=anchor_failure path={}",
                path
            );
            return Some(format!(r#"{{"action":"read_file","path":"{}"}}"#, path));
        }
    }

    if result.contains("assertion failed") || result.contains("panicked at") || result.contains("left != right") {
        return read1(result, "assertion");
    }

    if let Some(next) = derive_post_run_command_action(result) {
        eprintln!(
            "[canon-harness-repair] derive_next_action_hint_debug branch=post_run_command action={}",
            next
        );
        return Some(next);
    }

    if should_force_control_path_read(result, None) {
        return read1(result, "forced_control_path_read");
    }

    eprintln!(
        "[canon-harness-repair] derive_next_action_hint_debug branch=none result_preview={:?}",
        result.lines().take(8).collect::<Vec<_>>()
    );
    None
}

fn read1(result: &str, branch: &str) -> Option<String> {
    let (file, line) = extract_primary_file_line(result)?;
    eprintln!(
        "[canon-harness-repair] derive_next_action_hint_debug branch={} file={} line={}",
        branch,
        file,
        line
    );
    Some(format!(
        r#"{{"action":"read_file","path":"{}","line":{}}}"#,
        file, line
    ))
}

fn extract_rs_path_from_command(cmd: &str) -> Option<String> {
    cmd.split_whitespace()
        .find(|token| token.ends_with(".rs") || token.contains(".rs"))
        .map(|token| token.trim_matches(|c| c == '"' || c == '\'').to_string())
}

fn is_planner_transport_failure(message: &str) -> bool {
    message.contains("relay call to ")
        || message.contains("connection refused")
        || message.contains("tcp connect error")
        || message.contains("error trying to connect")
}

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
    repair_control: Mutex<RepairControlConsumer>,
}

impl EventEmitter for CapturingEmitter {
    fn emit_with_parents(&self, event: RuntimeEvent, _parents: Vec<EventId>, _file: &'static str, _line: u32) {
        let _ = self.tx.lock().unwrap().send(event.clone());
        let trigger_id = EventId::new("harness-repair-local");
        let _ = self
            .repair_control
            .lock()
            .unwrap()
            .on_event(&event, trigger_id);
    }
}

fn main() -> Result<()> {
    let mut args = std::env::args().skip(1);
    let crate_name = args.next().ok_or_else(|| anyhow!("missing <crate>"))?;
    let test_name = args.next().ok_or_else(|| anyhow!("missing <test-name>"))?;

    let mut always_dispatch = false;
    let mut stderr_file: Option<PathBuf> = None;
    let mut incident_file: Option<PathBuf> = None;
    let mut workspace = PathBuf::from(DEFAULT_WORKSPACE);
    let mut max_steps = DEFAULT_MAX_STEPS;

    while let Some(arg) = args.next() {
        match arg.as_str() {
            "--always-dispatch" => always_dispatch = true,
            "--stderr-file" => {
                let value = args.next().ok_or_else(|| anyhow!("missing value for --stderr-file"))?;
                stderr_file = Some(PathBuf::from(value));
            }
            "--incident-file" => {
                let value =
                    args.next().ok_or_else(|| anyhow!("missing value for --incident-file"))?;
                incident_file = Some(PathBuf::from(value));
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
    log_control_harness_summary(&logger);
    log_repair_harness_summary(&logger)?;
    log_runtime_product_summary(&logger)?;

    let target = HarnessRepairTarget::new(Some(crate_name.clone()), Some(test_name.clone()));
    let mut failure_output = if let Some(path) = stderr_file {
        std::fs::read_to_string(path)?
    } else {
        run_target_test_capture(&workspace, &crate_name, &test_name, always_dispatch)?
    };
    let incident_context = incident_file
        .as_ref()
        .map(|path| std::fs::read_to_string(path))
        .transpose()
        .with_context(|| "failed to read incident context file")?;

    logger.log(&format!("start crate={crate_name} test={test_name} max_steps={max_steps}"));

    canon_exec::init_llm_worker();

    // FIX: create persistent emitter BEFORE any LLM calls
    let (tx, rx) = std::sync::mpsc::channel();
    let emitter: canon_event::EventEmitterHandle =
        std::sync::Arc::new(CapturingEmitter {
            tx: std::sync::Mutex::new(tx),
            repair_control: std::sync::Mutex::new(RepairControlConsumer::new()),
        });
    let result = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
        run_harness_loop(
            &workspace,
            &crate_name,
            &test_name,
            &target,
            &mut failure_output,
            incident_context.as_deref(),
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
    incident_context: Option<&str>,
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
        incident_context,
    );
    let context_base_id = hash_str(&context_base).to_string();
    let system_prompt_id = PLANNER_SYSTEM_PROMPT_ID.to_string();

    // Per-step tracking.
    let mut last_action_result: Option<String> = None;
    let mut last_request_id: Option<String> = None;
    let mut last_local_fallback_action: Option<String> = None;
    let mut repeated_local_fallbacks: usize = 0;
    let mut consecutive_planner_transport_failures: usize = 0;

    for step in 0..max_steps {
        let directive = executor.evaluate_harness_repair_for_target(target, failure_output);
        let delta = build_turn_delta(
            crate_name, test_name, failure_output,
            &directive.decision.reason,
            last_action_result.as_deref(),
            incident_context,
        );
        logger.log(&format!(
            "step={} planner_inputs directive_reason={} failure_bytes={} failure_primary={} last_action_bytes={} last_action_hint={} delta_bytes={} delta_hint={}",
            step + 1,
            truncate(&directive.decision.reason, 160),
            failure_output.len(),
            extract_primary_file_line(failure_output)
                .map(|(path, line)| format!("{path}:{line}"))
                .unwrap_or_else(|| "<none>".to_string()),
            last_action_result.as_deref().map(|s| s.len()).unwrap_or(0),
            last_action_result
                .as_deref()
                .and_then(derive_next_action_hint)
                .unwrap_or_else(|| "<none>".to_string()),
            delta.len(),
            derive_next_action_hint(&delta).unwrap_or_else(|| "<none>".to_string()),
        ));
        // Tier 1 + 2 sent only on the first call; stateful endpoints skip them on subsequent turns.
        let send_system = step == 0;
        let send_base = step == 0;
        let planner_result = call_planner(
            workspace,
            &delta,
            send_system.then_some(PLANNER_SYSTEM_INSTRUCTIONS),
            &system_prompt_id,
            send_base.then_some(context_base.as_str()),
            &context_base_id,
            last_request_id.as_deref(),
            &emitter,
            &rx,
        );

        let actions = if let Ok((actions, req_id)) = planner_result {
                if req_id.starts_with("local-fallback-") {
                    let fingerprint = actions
                        .iter()
                        .map(|a| a.raw.to_string())
                        .collect::<Vec<_>>()
                        .join("\n");
                    if last_local_fallback_action.as_deref() == Some(fingerprint.as_str()) {
                        repeated_local_fallbacks += 1;
                    } else {
                        repeated_local_fallbacks = 1;
                        last_local_fallback_action = Some(fingerprint);
                    }
                    if repeated_local_fallbacks >= 3 {
                        let deterministic_hint = last_action_result
                            .as_deref()
                            .and_then(derive_next_action_hint)
                            .or_else(|| derive_next_action_hint(failure_output));
                        if let Some(fallback) = deterministic_hint {
                            logger.log(&format!(
                                "step={} planner_transport_degraded repeated_local_fallbacks={} deterministic_hint={}",
                                step + 1,
                                repeated_local_fallbacks,
                                fallback
                            ));
                            let fallback_json = format!("[{}]", fallback);
                            let actions =
                                parse_planner_actions(&serde_json::Value::String(fallback_json))?;
                            repeated_local_fallbacks = 0;
                            last_local_fallback_action = None;
                            last_request_id = Some(format!("local-fallback-recovery-{step}"));
                            actions
                        } else {
                            let message = format!(
                                "planner transport unavailable: repeated identical local fallback action {} times; relay={}",
                                repeated_local_fallbacks,
                                std::env::var("CANON_LLM_RELAY_ADDR")
                                    .unwrap_or_else(|_| RELAY_ADDR.to_string())
                            );
                            logger.log(&format!(
                                "step={} planner_failure reason={}",
                                step + 1,
                                message
                            ));
                            bail!("{message}");
                        }
                    } else {
                        consecutive_planner_transport_failures = 0;
                        last_request_id = Some(req_id);
                        actions
                    }
                } else {
                    repeated_local_fallbacks = 0;
                    last_local_fallback_action = None;
                    consecutive_planner_transport_failures = 0;
                    last_request_id = Some(req_id);
                    actions
                }
        } else {
            let err = planner_result.err().expect("planner_result checked as Err");
                let err_str = err.to_string();
                if let Some(req_id) = extract_last_request_id(&err_str) {
                    last_request_id = Some(req_id);
                }
                if err_str.starts_with(BAD_RESPONSE_PREFIX) {
                    // LLM responded with emoji/prose instead of JSON.
                    // Inject an explicit correction so the next turn demands valid JSON.
                    let correction = format!(
                        "INVALID RESPONSE — your last reply was not a JSON action array.\n\
                        {err_str}\n\n\
                        Return exactly one action in a JSON array wrapped in a `json` code block.\n\
                        No emoji. No prose outside the code block."
                    );
                    logger.log(&format!("step={} bad_response injecting_correction", step + 1));
                    *failure_output = correction.clone();
                    last_action_result = Some(correction);
                    continue;
                } else {
                    let message = format!("planner call failed: {err_str}");
                    logger.log(&format!("step={} planner_failure reason={}", step + 1, message));
                    if is_planner_transport_failure(&err_str) {
                        consecutive_planner_transport_failures += 1;
                        if consecutive_planner_transport_failures
                            >= MAX_CONSECUTIVE_PLANNER_TRANSPORT_FAILURES
                        {
                            let hint_from_last = last_action_result
                                .as_deref()
                                .and_then(derive_next_action_hint);
                            let hint_from_failure = derive_next_action_hint(failure_output);
                            let deterministic_hint =
                                hint_from_last.clone().or(hint_from_failure.clone());
                            if let Some(fallback) = deterministic_hint {
                                logger.log(&format!(
                                    "step={} planner_transport_degraded consecutive_failures={} hint_source={} deterministic_hint={}",
                                    step + 1,
                                    consecutive_planner_transport_failures,
                                    if hint_from_last.is_some() {
                                        "last_action_result"
                                    } else if hint_from_failure.is_some() {
                                        "failure_output"
                                    } else {
                                        "none"
                                    },
                                    fallback
                                ));
                                consecutive_planner_transport_failures = 0;
                                repeated_local_fallbacks = 0;
                                last_local_fallback_action = None;
                                last_request_id =
                                    Some(format!("local-fallback-transport-recovery-{step}"));
                                *failure_output = format!(
                                    "{}\n\nNEXT ACTION HINT:\n{}",
                                    message,
                                    fallback
                                );
                                last_action_result = Some(failure_output.clone());
                                continue;
                            } else {
                                logger.log(&format!(
                                    "step={} planner_transport_no_hint consecutive_failures={} last_action_result_present={} failure_output_bytes={}",
                                    step + 1,
                                    consecutive_planner_transport_failures,
                                    last_action_result.is_some(),
                                    failure_output.len()
                                ));
                                *failure_output = message.clone();
                                last_action_result = Some(message);
                                continue;
                            }
                        } else {
                            *failure_output = message.clone();
                            last_action_result = Some(message);
                            continue;
                        }
                    } else {
                        consecutive_planner_transport_failures = 0;
                        *failure_output = message.clone();
                        last_action_result = Some(message);
                        continue;
                    }
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
        logger.log(&format!(
            "step={} action={} crate={crate_name} test={test_name} action_raw={}",
            step + 1,
            action_kind,
            truncate(&action.raw.to_string(), 300),
        ));
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
                    let normalized_line = normalize_read_file_line(workspace, path, line_offset)?;
                    logger.log(&format!(
                        "step={} read_file_request path={} line={} normalized_line={}",
                        step + 1,
                        path,
                        line_offset.unwrap_or(1),
                        normalized_line.unwrap_or(1),
                    ));
                    let output = run_read_file(workspace, path, normalized_line)?;
                    logger.log(&format!(
                        "step={} read_file_output bytes={} preview={}",
                        step + 1,
                        output.len(),
                        truncate(&output.replace('\n', "\\n"), 240),
                    ));
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
                                if let Ok(content) = run_read_file_for_patch_anchor(workspace, &anchor_path, &err.to_string()) {
                                    logger.log(&format!("step={} auto_read anchor_fail_path={anchor_path}", step + 1));
                                    delta_msg = format!(
                                        "apply_patch failed: {err}\n\n{content}"
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
                    if command_requires_verify(cmd) {
                        let verify = verify_after_mutation(workspace, crate_name, test_name)?;
                        print_verify_status(crate_name, test_name, &verify, logger);
                        if verify.success {
                            logger.log("harness repair complete: crate test suite passed");
                            return Ok((true, String::new()));
                        }
                        *failure_output = verify.output.clone();
                        return Ok((false, format!("run_command ok; verify failed:\n{}", truncate(&verify.output, 2000))));
                    }
                    let source_path = extract_rs_path_from_command(cmd).unwrap_or_default();
                    return Ok((
                        false,
                        format!(
                            "run_command ok:\npath={}\n{}",
                            source_path,
                            truncate(&result.output, 2000)
                        ),
                    ));
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
                logger.log(&format!(
                    "step={} action_result bytes={} primary={} derived_hint={} preview={}",
                    step + 1,
                    result.len(),
                    extract_primary_file_line(&result)
                        .map(|(path, line)| format!("{path}:{line}"))
                        .unwrap_or_else(|| "<none>".to_string()),
                    derive_next_action_hint(&result).unwrap_or_else(|| "<none>".to_string()),
                    truncate(&result.replace('\n', "\\n"), 240),
                ));
                // Preserve the original failing-test signal across non-mutating
                // discovery steps. If we overwrite failure_output with read/list
                // output, the deterministic fallback loses the cited file:line
                // and collapses into repeated list_dir actions.
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

fn normalize_read_file_line(
    workspace: &Path,
    relative_path: &str,
    line_offset: Option<usize>,
) -> Result<Option<usize>> {
    let Some(requested) = line_offset else {
        return Ok(None);
    };

    let full_path = workspace.join(relative_path);
    let content = std::fs::read_to_string(&full_path)
        .with_context(|| format!("failed to read file for normalization: {}", full_path.display()))?;
    let lines: Vec<&str> = content.lines().collect();
    if lines.is_empty() {
        return Ok(Some(requested));
    }

    let idx = requested.saturating_sub(1).min(lines.len().saturating_sub(1));

    for scan in (0..=idx).rev() {
        let line = lines[scan].trim_start();
        if line.starts_with("fn ")
            || line.starts_with("pub fn ")
            || line.starts_with("#[test]")
        {
            if line.starts_with("#[test]") {
                let next = (scan + 1).min(lines.len().saturating_sub(1));
                return Ok(Some(next + 1));
            }
            return Ok(Some(scan + 1));
        }
    }

    Ok(Some(requested))
}

// deterministic result → action mapping
#[cfg(test)]
mod harness_repair_transition_tests {
    use super::{derive_next_action_hint, derive_post_run_command_action};

    #[test]
    fn derive_post_run_command_action_reads_function_line() {
        let result = "\
run_command ok:
2420:    fn deterministic_route_for_event_observes_when_no_progress_has_no_actionable_failure() {
1624:    #[test]";
        let action = derive_post_run_command_action(result);
        assert_eq!(action, None);
    }

    #[test]
    fn derive_next_action_hint_prefers_post_run_command_rule() {
        let result = "\
run_command ok:
2420:    fn repeated_missing_target_failures_promote_force_plan_invariant() {";
        assert_eq!(derive_next_action_hint(result), None);
    }
}

fn should_force_control_path_read(result: &str, incident_context: Option<&str>) -> bool {
    const NEEDLES: &[&str] = &[
        "noop_spam",
        "missing_target_plan",
        "invariant_violation",
        "route_executor_missing_target_plan",
        "llm call timed out",
        "capability_failed",
        "status\":\"llm_failed",
        "status=llm_failed",
    ];

    let contains_forced_signal = |text: &str| {
        let lower = text.to_ascii_lowercase();
        NEEDLES.iter().any(|needle| lower.contains(needle))
            || (lower.contains("planning_completed") && lower.contains("llm_failed"))
    };

    contains_forced_signal(result)
        || incident_context
            .map(contains_forced_signal)
            .unwrap_or(false)
}

fn build_forced_planner_constraint(result: &str, incident_context: Option<&str>) -> String {
    if !should_force_control_path_read(result, incident_context) {
        return "none".to_string();
    }

    let mut lines = vec![
        "- You are in a repeated control-path failure region.".to_string(),
        "- Your next action MUST be `read_file`.".to_string(),
        "- Do NOT emit `run_command`, `apply_patch`, or `done` on this turn.".to_string(),
        "- Read the control-path file/function cited by the current failure or incident context.".to_string(),
        "- After reading, repair the route/invariant logic that caused the repeated plan/noop loop.".to_string(),
    ];

    if let Some((file, line)) = extract_primary_file_line(result) {
        lines.push(format!(
            "- Required target: read_file path=`{}` line={}",
            file, line
        ));
    } else if let Some(incident) = incident_context {
        for anchor in extract_incident_anchors(incident).into_iter().take(2) {
            lines.push(format!("- Incident anchor: {}", anchor));
        }
    }

    lines.join("\n")
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
    incident_context: Option<&str>,
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
    let incident_section = match incident_context {
        Some(text) if !text.trim().is_empty() => format!(
            "\nEvent-log incident context:\n{}\n",
            truncate(&clean_failure_output(text), 4000)
        ),
        _ => String::new(),
    };
    let cleaned = clean_failure_output(initial_failure);
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
- If event-log incident context is present, prioritize repairing the cited control-path and adding a regression for that exact incident shape.\n\
- Your response must be a JSON action array only. No prose.\n\
\n\
{incident_section}\
Initial failure output:\n{cleaned}\n\
\n\
Response contract:\n\
- Return exactly one action in a JSON array.\n\
- Do not return prose outside the JSON array.",
        workspace = workspace.display(),
        crate_name = crate_name,
        test_name = test_name,
        cleaned = truncate(&cleaned, 4000),
        guidance_section = guidance_section,
        fn_index_section = fn_index_section,
        incident_section = incident_section,
    )
}

/// Tier 3: per-step delta. First turn = repair directive. Subsequent turns = action result only.
fn build_turn_delta(
    crate_name: &str,
    test_name: &str,
    failure_output: &str,
    directive_reason: &str,
    last_action_result: Option<&str>,
    incident_context: Option<&str>,
) -> String {
    if let Some(result) = last_action_result {
        let guidance = build_action_guidance(result);
        let incident_guidance = build_incident_action_guidance(incident_context);
        let forced_constraint = build_forced_planner_constraint(result, incident_context);
        let failure_focus = build_failure_focus(failure_output, incident_context);

        format!(
            "Action result:\n{result}\n\n\
Failure focus:\n{failure_focus}\n\n\
Incident guidance:\n{incident_guidance}\n\n\
FORCED PLANNER CONSTRAINT:\n{forced_constraint}\n\n\
Next action guidance:\n{guidance}\n\n\
Emit exactly one action.",
            result = truncate(result, 2000),
            failure_focus = failure_focus,
            incident_guidance = incident_guidance,
            forced_constraint = forced_constraint,
            guidance = guidance,
        )
    } else {
        let failure_focus = build_failure_focus(failure_output, incident_context);
        let forced_constraint = build_forced_planner_constraint(failure_output, incident_context);
        format!(
            "Repair directive:\n\
- {directive_reason}\n\
- Emit exactly one action.\n\
\n\
Failure focus:\n{failure_focus}\n\
\n\
FORCED PLANNER CONSTRAINT:\n\
{forced_constraint}\n\
\n\
Preferred verifier after mutation:\n\
- `cargo check -p {crate_name}`\n\
- then `cargo test -p {crate_name} {test_name} -- --nocapture`",
            directive_reason = directive_reason,
            failure_focus = failure_focus,
            forced_constraint = forced_constraint,
            crate_name = crate_name,
            test_name = test_name,
        )
    }
}

fn build_failure_focus(failure_output: &str, incident_context: Option<&str>) -> String {
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
    if let Some(incident) = incident_context {
        if let Some(kind) = extract_tagged_line_value(incident, "incident_kind=") {
            lines.push(format!("- incident_kind: {kind}"));
        }
        for anchor in extract_incident_anchors(incident).into_iter().take(6) {
            lines.push(format!("- incident_anchor: {anchor}"));
        }
    }

    if lines.is_empty() {
        "none".to_string()
    } else {
        lines.join("\n")
    }
}

fn build_incident_action_guidance(incident_context: Option<&str>) -> String {
    let Some(incident) = incident_context else {
        return "none".to_string();
    };
    let anchors = extract_incident_anchors(incident);
    let mut lines = Vec::new();
    if let Some(kind) = extract_tagged_line_value(incident, "incident_kind=") {
        lines.push(format!(
            "- prioritize the event-log incident kind `{kind}` before unrelated cleanup"
        ));
    }
    if !anchors.is_empty() {
        lines.push(format!(
            "- read these cited files/functions first: {}",
            anchors.into_iter().take(4).collect::<Vec<_>>().join(", ")
        ));
    }
    lines.push(
        "- after mutation, verify both the target test and the synthetic regression for the incident shape"
            .to_string(),
    );
    lines.join("\n")
}

fn extract_incident_anchors(text: &str) -> Vec<String> {
    let mut anchors = Vec::new();
    let mut in_anchor_section = false;
    for line in text.lines() {
        let trimmed = line.trim();
        if trimmed == "Likely file:line anchors:" {
            in_anchor_section = true;
            continue;
        }
        if in_anchor_section {
            if trimmed.is_empty() {
                break;
            }
            if trimmed.ends_with(".rs") || trimmed.contains(".rs:") {
                anchors.push(trimmed.to_string());
            }
        }
    }
    anchors
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

fn extract_last_request_id(text: &str) -> Option<String> {
    let marker = "[last_request_id=";
    let start = text.find(marker)? + marker.len();
    let rest = &text[start..];
    let end = rest.find(']')?;
    let value = rest[..end].trim();
    if value.is_empty() {
        None
    } else {
        Some(value.to_string())
    }
}

#[cfg(test)]
mod planner_contract_tests {
    use super::{
        extract_last_request_id, parse_planner_actions, validate_patch_attempt,
    };
    use std::path::Path;

    #[test]
    fn planner_parses_single_action_object() {
        let actions = parse_planner_actions(&serde_json::json!({
            "action": "done",
            "reason": "green"
        }))
        .expect("single action object should parse");
        assert_eq!(actions.len(), 1);
        assert_eq!(
            actions[0]
                .raw
                .get("action")
                .and_then(|value| value.as_str()),
            Some("done")
        );
    }

    #[test]
    fn planner_parses_json_code_fence_from_nested_message_field() {
        let actions = parse_planner_actions(&serde_json::json!({
            "message": {
                "content": "```json\n[{\"action\":\"read_file\",\"path\":\"canon-utils/canon-route/src/policy.rs\"}]\n```"
            }
        }))
        .expect("nested fenced JSON should parse");
        assert_eq!(actions.len(), 1);
        assert_eq!(
            actions[0]
                .raw
                .get("action")
                .and_then(|value| value.as_str()),
            Some("read_file")
        );
    }

    #[test]
    fn planner_rejects_request_metadata_only_payload() {
        let err = parse_planner_actions(&serde_json::json!({
            "request_id": "abc-123"
        }))
        .expect_err("metadata-only payload must be rejected");
        assert!(err.to_string().contains("response only contained request metadata"));
    }

    #[test]
    fn planner_rejects_empty_action_array() {
        let err = parse_planner_actions(&serde_json::json!([]))
            .expect_err("empty action arrays must be rejected");
        assert!(err.to_string().contains("empty JSON array"));
    }

    #[test]
    fn patch_validation_rejects_absolute_paths() {
        let patch = "\
*** Begin Patch
*** Update File: /workspace/ai_sandbox/canon/canon-utils/canon-route/src/policy.rs
@@
-old
+new
*** End Patch";
        let err = validate_patch_attempt(Path::new("/workspace/ai_sandbox/canon"), patch)
            .expect_err("absolute patch paths must be rejected");
        assert!(err.contains("workspace-relative"));
    }

    #[test]
    fn patch_validation_rejects_unexpected_repo_root_targets() {
        let patch = "\
*** Begin Patch
*** Update File: Cargo.toml
@@
-old
+new
*** End Patch";
        let err = validate_patch_attempt(Path::new("/workspace/ai_sandbox/canon"), patch)
            .expect_err("repo-root harness should reject non canon-utils targets");
        assert!(err.contains("unexpected patch path"));
    }

    #[test]
    fn extract_last_request_id_reads_bracketed_tag() {
        let text = "planner failed [last_request_id=req-42]";
        assert_eq!(extract_last_request_id(text).as_deref(), Some("req-42"));
    }
}

fn call_planner(
    _workspace: &Path,
    delta: &str,
    system: Option<&str>,
    _system_prompt_id: &str,
    context_base: Option<&str>,
    _context_base_id: &str,
    _prev_prompt_id: Option<&str>,
    _emitter: &EventEmitterHandle,
    _rx: &Receiver<RuntimeEvent>,
) -> Result<(Vec<PlannerAction>, String)> {
    // Tier-1 (system instructions) go in role_schema — the relay endpoint is
    // stateful so the LlmWorker sends this only on the first TURN per tab.
    let role_schema = system.unwrap_or("").to_string();

    // Tier-2 (context_base) + Tier-3 (delta) are assembled into the prompt.
    let prompt = match context_base {
        Some(base) if !base.is_empty() => format!("{base}\n\n{delta}"),
        _ => delta.to_string(),
    };

    if !harness_should_use_relay() {
        return local_planner_fallback(delta, &prompt, context_base);
    }

    let relay_addr = std::env::var("CANON_LLM_RELAY_ADDR")
        .unwrap_or_else(|_| RELAY_ADDR.to_string());
    let request_id = Uuid::new_v4().to_string();
    eprintln!(
        "[canon-harness-repair] relay_dispatch request_id={} relay={} prompt_bytes={}",
        request_id,
        relay_addr,
        prompt.len(),
    );

    let req = RelayRequest {
        role: "harness_repair".to_string(),
        endpoint_id: None,
        prompt: prompt.clone(),
        role_schema,
        request_tag: Some(request_id.clone()),
    };

    let resp = match relay_client_call(&relay_addr, &req)
        .with_context(|| format!("relay call to {relay_addr} failed"))
    {
        Ok(resp) => resp,
        Err(err) => {
            if let Some(fallback) =
                derive_next_action_hint(delta).or_else(|| derive_next_action_hint(&prompt))
            {
                eprintln!(
                    "[canon-harness-repair] relay_failure request_id={} fallback_action={}",
                    request_id,
                    fallback
                );
                let fallback_json = format!("[{}]", fallback);
                let actions = parse_planner_actions(&serde_json::Value::String(fallback_json))?;
                return Ok((actions, format!("local-fallback-{request_id}")));
            }
            return Err(err);
        }
    };

    if !resp.ok {
        let relay_error = resp.error.unwrap_or_default();
        if let Some(fallback) =
            derive_next_action_hint(delta).or_else(|| derive_next_action_hint(&prompt))
        {
            eprintln!(
                "[canon-harness-repair] relay_error request_id={} fallback_action={} error={}",
                request_id,
                fallback,
                relay_error
            );
            let fallback_json = format!("[{}]", fallback);
            let actions = parse_planner_actions(&serde_json::Value::String(fallback_json))?;
            return Ok((actions, format!("local-fallback-{request_id}")));
        }
        bail!(
            "relay returned error for request_id={}: {}",
            request_id,
            relay_error
        );
    }

    let raw = resp.response.unwrap_or_default();
    eprintln!(
        "[canon-harness-repair] relay_response request_id={} response_bytes={}",
        request_id,
        raw.len(),
    );

    let actions = parse_planner_actions(&serde_json::Value::String(raw))?;
    Ok((actions, request_id))
}

fn parse_planner_actions(value: &serde_json::Value) -> anyhow::Result<Vec<PlannerAction>> {
    let array = if let Some(array) = value.as_array() {
        array.clone()
    } else if value.is_object() {
        if value.get("action").is_some() || value.get("kind").is_some() {
            vec![value.clone()]
        } else if value.get("request_id").is_some() && value.as_object().map(|o| o.len()) == Some(1) {
            anyhow::bail!("bad_response: response only contained request metadata");
        } else if let Some(text) = value.get("text").and_then(|v| v.as_str()) {
            parse_planner_actions_from_text(text)?
        } else if let Some(text) = value
            .get("response")
            .and_then(extract_response_text)
            .or_else(|| value.get("message").and_then(extract_response_text))
            .or_else(|| value.get("content").and_then(extract_response_text))
        {
            parse_planner_actions_from_text(&text)?
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

fn extract_response_text(value: &serde_json::Value) -> Option<String> {
    if let Some(text) = value.as_str() {
        let trimmed = text.trim();
        if !trimmed.is_empty()
            && !(trimmed.starts_with("{\"request_id\":") && trimmed.ends_with('}'))
        {
            return Some(text.to_string());
        }
    }
    if let Some(obj) = value.as_object() {
        if obj.len() == 1 && obj.get("request_id").and_then(|v| v.as_str()).is_some() {
            return None;
        }
        for key in ["text", "response", "message", "content"] {
            if let Some(inner) = obj.get(key).and_then(extract_response_text) {
                return Some(inner);
            }
        }
    }
    if let Some(arr) = value.as_array() {
        for item in arr {
            if let Some(inner) = extract_response_text(item) {
                return Some(inner);
            }
        }
    }
    None
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

fn command_requires_verify(cmd: &str) -> bool {
    let trimmed = cmd.trim();

    let exploratory_prefixes = [
        "rg ", "sed ", "awk ", "perl ", "cat ", "head ", "tail ", "ls ", "find ",
        "bat ", "git diff", "git status", "cargo test ", "cargo check ", "cargo build ",
        "cargo run ", "pwd", "tree ", "stat ", "wc ", "echo ",
    ];
    if exploratory_prefixes.iter().any(|p| trimmed.starts_with(p)) {
        return false;
    }

    let mutating_markers = [
        "apply_patch",
        "sed -i",
        "perl -pi",
        "tee ",
        ">",
        ">>",
        "mv ",
        "cp ",
        "touch ",
        "mkdir ",
        "rmdir ",
        "chmod ",
        "chown ",
        "truncate ",
        "xargs rm",
    ];
    if mutating_markers.iter().any(|m| trimmed.contains(m)) {
        return true;
    }

    false
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

fn run_read_file_for_patch_anchor(workspace: &Path, relative: &str, err_msg: &str) -> Result<String> {
    let path = resolve_workspace_path(workspace, relative)?;
    let full = std::fs::read_to_string(&path)
        .with_context(|| format!("read_file failed for {}", path.display()))?;

    if let Some((start, end, excerpt)) = extract_anchor_context_excerpt(&full, err_msg) {
        return Ok(format!(
            "Current content near likely match of failed anchor in {relative} (lines {start}-{end}):\n{excerpt}"
        ));
    }

    let fallback = run_read_file(workspace, relative, None)?;
    Ok(format!("Current content of {relative}:\n{fallback}"))
}

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
        if let Some(idx) = file_lines.iter().position(|line| line.contains(needle)) {
            best_idx = Some(idx);
            break;
        }
    }

    let idx = best_idx?;
    let start_idx = idx.saturating_sub(AUTO_READ_CONTEXT_BEFORE);
    let end_idx = (idx + AUTO_READ_CONTEXT_AFTER + 1).min(file_lines.len());
    let start_line = start_idx + 1;
    let end_line = end_idx;
    let excerpt = file_lines[start_idx..end_idx]
        .iter()
        .enumerate()
        .map(|(offset, line)| format!("{}: {}", start_line + offset, line))
        .collect::<Vec<_>>()
        .join("\n");

    Some((start_line, end_line, excerpt))
}

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
