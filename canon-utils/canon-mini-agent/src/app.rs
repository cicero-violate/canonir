use anyhow::{anyhow, bail, Context, Result};
use canon_llm::{
    config::LlmEndpoint,
    endpoint_worker::{
        llm_worker_new_tabs, llm_worker_send_request_timeout, llm_worker_send_request_with_req_id_timeout,
    },
    tab_management::TabManagerHandle,
    ws_server,
    ws_server::WsBridge,
};
use serde_json::{json, Value};
use std::collections::{HashMap, VecDeque};
use std::path::{Path, PathBuf};
use std::sync::{Arc, OnceLock};

use crate::engine::process_action_and_execute;
use crate::logging::{
    append_action_log_record, append_action_result_log, append_message_log, append_orchestration_trace,
    compact_log_record, make_command_id, now_ms, init_log_paths,
};
use crate::prompts::{
    action_observation, action_rationale, action_result_prompt, diagnostics_cycle_prompt,
    diagnostics_python_reads_event_logs, executor_cycle_prompt, is_explicit_idle_action,
    normalize_action, parse_actions, planner_cycle_prompt, system_instructions,
    truncate, validate_action, verifier_cycle_prompt, AgentPromptKind,
};
use crate::constants::{
    DEFAULT_RESPONSE_TIMEOUT_SECS, DIAGNOSTICS_FILE_PATH, ENDPOINT_SPECS, INVARIANTS_FILE, MASTER_PLAN_FILE, MAX_SNIPPET,
    MAX_STEPS, OBJECTIVES_FILE, ROLE_TIMEOUT_SECS, SPEC_FILE, VIOLATIONS_FILE, WORKSPACE, WS_PORT_CANDIDATES,
};

fn ws_port_arg(args: &[String]) -> Option<&str> {
    args.windows(2)
        .find(|w| w[0] == "--port")
        .map(|w| w[1].as_str())
}

fn instance_arg(args: &[String]) -> Option<&str> {
    args.windows(2)
        .find(|w| w[0] == "--instance")
        .map(|w| w[1].as_str())
}

fn ws_port_is_available(port: u16) -> bool {
    std::net::TcpListener::bind((std::net::Ipv4Addr::LOCALHOST, port)).is_ok()
}

fn choose_ws_port(args: &[String]) -> Result<(u16, bool)> {
    if let Some(raw) = ws_port_arg(args) {
        let port = raw
            .parse::<u16>()
            .with_context(|| format!("invalid --port value: {raw}"))?;
        return Ok((port, true));
    }

    for &port in WS_PORT_CANDIDATES {
        if ws_port_is_available(port) {
            return Ok((port, false));
        }
    }

    bail!(
        "no free ws port available in {:?}; pass --port explicitly or extend WS_PORT_CANDIDATES",
        WS_PORT_CANDIDATES
    );
}

fn role_key(role: &str) -> &str {
    if role.starts_with("executor") {
        "executor"
    } else {
        role
    }
}

fn response_timeout_for_role(role: &str) -> u64 {
    ROLE_TIMEOUT_SECS
        .iter()
        .find(|(key, _)| *key == role_key(role))
        .map(|(_, val)| *val)
        .unwrap_or(DEFAULT_RESPONSE_TIMEOUT_SECS)
}

#[derive(Clone)]
struct LaneConfig {
    index: usize,
    endpoint: LlmEndpoint,
    plan_file: String,
    label: String,
    tabs: TabManagerHandle,
}

async fn continue_executor_completion(
    submitted: &SubmittedExecutorTurn,
    completion_text: &str,
    turn_id: u64,
    endpoint: &LlmEndpoint,
    bridge: &WsBridge,
    workspace: &Path,
    tabs: &TabManagerHandle,
) -> Result<String> {
    let role = submitted.actor.as_str();
    let prompt_kind = "executor";
    let step = 1usize;
    let command_id = submitted.command_id.as_str();

    let actions = match parse_actions(completion_text) {
        Ok(actions) => actions,
        Err(e) => {
            if let Err(log_err) = append_message_log(
                role,
                endpoint,
                prompt_kind,
                step,
                command_id,
                "llm_parse_error",
                json!({
                    "error": e.to_string(),
                    "raw": truncate(completion_text, MAX_SNIPPET),
                }),
            ) {
                eprintln!("[{role}] step={} action_log_error: {log_err}", step);
            }
            append_orchestration_trace(
                "executor_tool_result_forwarded",
                json!({
                    "lane_name": submitted.lane_label,
                    "tab_id": submitted.tab_id,
                    "turn_id": command_id,
                    "status": "parse_error",
                }),
            );
            return Err(anyhow!("executor parse_error: {e}"));
        }
    };

    if actions.len() != 1 {
        let msg = format!("Got {} actions — emit exactly one action per turn.", actions.len());
        if let Err(log_err) = append_message_log(
            role,
            endpoint,
            prompt_kind,
            step,
            command_id,
            "llm_invalid_action_count",
            json!({
                "action_count": actions.len(),
                "raw": truncate(completion_text, MAX_SNIPPET),
            }),
        ) {
            eprintln!("[{role}] step={} action_log_error: {log_err}", step);
        }
        append_orchestration_trace(
            "executor_tool_result_forwarded",
            json!({
                "lane_name": submitted.lane_label,
                "tab_id": submitted.tab_id,
                "turn_id": command_id,
                "status": "invalid_action_count",
                "action_count": actions.len(),
            }),
        );
        return Err(anyhow!("executor invalid_action_count: {msg}"));
    }

    let mut action = actions[0].clone();
    if let Err(e) = normalize_action(&mut action) {
        let msg = format!(
            "Invalid action: {e}\nReturn exactly one action with a non-empty `observation`, a non-empty `rationale`, and any required fields."
        );
        if let Err(log_err) = append_message_log(
            role,
            endpoint,
            prompt_kind,
            step,
            command_id,
            "llm_invalid_action",
            json!({
                "stage": "normalize_action",
                "error": e.to_string(),
                "raw": truncate(completion_text, MAX_SNIPPET),
            }),
        ) {
            eprintln!("[{role}] step={} action_log_error: {log_err}", step);
        }
        return Err(anyhow!("executor invalid_action: {msg}"));
    }

    if let Err(e) = validate_action(&action) {
        let msg = format!(
            "Invalid action: {e}\nReturn exactly one action with a non-empty `observation`, a non-empty `rationale`, and any required fields."
        );
        if let Err(log_err) = append_message_log(
            role,
            endpoint,
            prompt_kind,
            step,
            command_id,
            "llm_invalid_action",
            json!({
                "stage": "validate_action",
                "error": e.to_string(),
                "raw": truncate(completion_text, MAX_SNIPPET),
                "action": action.clone(),
            }),
        ) {
            eprintln!("[{role}] step={} action_log_error: {log_err}", step);
        }
        return Err(anyhow!("executor invalid_action: {msg}"));
    }

    let (done, out) = process_action_and_execute(
        role,
        prompt_kind,
        endpoint,
        workspace,
        step,
        command_id,
        &action,
        false,
    )?;
    if done {
        return Ok(out);
    }

    append_orchestration_trace(
        "executor_tool_result_forwarded",
        json!({
            "lane_name": submitted.lane_label,
            "tab_id": submitted.tab_id,
            "command_id": command_id,
            "action": action.get("action").and_then(|v| v.as_str()),
            "result_bytes": out.len(),
        }),
    );

    let agent_type = role.to_uppercase();
    run_agent(
        role,
        prompt_kind,
        "",
        action_result_prompt(
            Some(submitted.tab_id),
            Some(turn_id),
            agent_type.as_str(),
            &out,
            action.get("action").and_then(|v| v.as_str()),
        ),
        endpoint,
        bridge,
        workspace,
        tabs,
        false,
        false,
        false,
    )
    .await
}

// ── Agent loop ─────────────────────────────────────────────────────────────────

/// Run one agent role until it calls `message` with status=complete or exhausts MAX_STEPS.
/// Returns the completion summary on success, or an error on hard failure.
/// `check_on_done`: if true, run cargo build + test before accepting completion.
async fn run_agent(
    role: &str,
    prompt_kind: &str,
    system_instructions: &str,
    initial_prompt: String,
    endpoint: &LlmEndpoint,
    bridge: &WsBridge,
    workspace: &Path,
    tabs: &TabManagerHandle,
    submit_only: bool,
    check_on_done: bool,
    send_system_prompt: bool,
) -> Result<String> {
    eprintln!(
        "[{role}] endpoint_id={} url={} prompt_kind={} submit_only={}",
        endpoint.id,
        endpoint.pick_url(0),
        prompt_kind,
        submit_only
    );
    let mut step = 0usize;
    let mut last_result: Option<String> = None;
    let mut last_tab_id: Option<u32> = None;
    let mut last_turn_id: Option<u64> = None;
    let mut last_action: Option<String> = None;
    let mut diagnostics_eventlog_python_done = false;
    let mut idle_streak = 0usize;

    loop {
        if step >= MAX_STEPS {
            bail!("[{role}] exhausted {MAX_STEPS} steps without completing");
        }

        let (role_schema, prompt) = if step == 0 {
            (
                if send_system_prompt {
                    system_instructions.to_string()
                } else {
                    String::new()
                },
                initial_prompt.clone(),
            )
        } else {
            let result = last_result.as_deref().unwrap_or("");
            let agent_type = role_key(role).to_uppercase();
            (
                String::new(),
                action_result_prompt(
                    last_tab_id,
                    last_turn_id,
                    agent_type.as_str(),
                    result,
                    last_action.as_deref(),
                ),
            )
        };
        let exchange_id = make_command_id(role, prompt_kind, step + 1);

        eprintln!("[{role}] step={} prompt_bytes={}", step + 1, prompt.len());
        if let Err(e) = append_message_log(
            role,
            endpoint,
            prompt_kind,
            step + 1,
            &exchange_id,
            "llm_request",
            json!({
                "submit_only": submit_only,
                "prompt_bytes": prompt.len(),
                "role_schema_bytes": role_schema.len(),
                "prompt": truncate(&prompt, MAX_SNIPPET),
            }),
        ) {
            eprintln!("[{role}] step={} action_log_error: {e}", step + 1);
        }
        append_orchestration_trace(
            "llm_message_forwarded",
            json!({
                "role": role,
                "prompt_kind": prompt_kind,
                "step": step + 1,
                "endpoint_id": endpoint.id,
                "submit_only": submit_only,
                "prompt_bytes": prompt.len(),
            }),
        );

        let response_timeout_secs = response_timeout_for_role(role);
        let (req_id, resp) = match llm_worker_send_request_with_req_id_timeout(
            bridge,
            &endpoint.id,
            &endpoint.url,
            endpoint.stateful,
            &prompt,
            &role_schema,
            None,
            None,
            false,
            true,
            role,
            tabs,
            endpoint.max_tabs,
            submit_only,
            Some(response_timeout_secs),
        )
        .await
        {
            Ok(r) => r,
            Err(e) => {
                eprintln!("[{role}] step={} llm_error: {e}", step + 1);
                if let Err(log_err) = append_message_log(
                    role,
                    endpoint,
                    prompt_kind,
                    step + 1,
                    &exchange_id,
                    "llm_error",
                    json!({
                        "error": e.to_string(),
                    }),
                ) {
                    eprintln!("[{role}] step={} action_log_error: {log_err}", step + 1);
                }
                last_result = Some(format!("LLM error: {e}\nReturn exactly one action as a single JSON object in a ```json code block."));
                step += 1;
                continue;
            }
        };
        let _ = req_id;
        last_tab_id = resp.tab_id;
        last_turn_id = resp.turn_id;
        let raw = resp.raw;

        append_orchestration_trace(
            "llm_message_received",
            json!({
                "role": role,
                "prompt_kind": prompt_kind,
                "step": step + 1,
                "endpoint_id": endpoint.id,
                "submit_only": submit_only,
                "response_bytes": raw.len(),
            }),
        );
        if let Err(e) = append_message_log(
            role,
            endpoint,
            prompt_kind,
            step + 1,
            &exchange_id,
            "llm_response",
            json!({
                "submit_only": submit_only,
                "response_bytes": raw.len(),
                "raw": truncate(&raw, MAX_SNIPPET),
            }),
        ) {
            eprintln!("[{role}] step={} action_log_error: {e}", step + 1);
        }

        if submit_only {
            if let Ok(mut ack) = serde_json::from_str::<Value>(&raw) {
                if ack.get("submit_ack").and_then(|v| v.as_bool()) == Some(true) {
                    ack["command_id"] = Value::String(exchange_id.clone());
                    eprintln!("[{role}] step={} submit_ack={}", step + 1, raw);
                    if let Err(e) = append_message_log(
                        role,
                        endpoint,
                        prompt_kind,
                        step + 1,
                        &exchange_id,
                        "llm_submit_ack",
                        ack.clone(),
                    ) {
                        eprintln!("[{role}] step={} action_log_error: {e}", step + 1);
                    }
                    append_orchestration_trace(
                        "llm_message_processed",
                        json!({
                            "role": role,
                            "prompt_kind": prompt_kind,
                            "step": step + 1,
                            "endpoint_id": endpoint.id,
                            "submit_ack": ack,
                        }),
                    );
                    return Ok(ack.to_string());
                }
            }
        }

        eprintln!("[{role}] step={} response_bytes={}", step + 1, raw.len());

        let actions = match parse_actions(&raw) {
            Ok(a) => a,
            Err(e) => {
                eprintln!("[{role}] step={} parse_error: {e}", step + 1);
                if let Err(log_err) = append_message_log(
                    role,
                    endpoint,
                    prompt_kind,
                    step + 1,
                    &exchange_id,
                    "llm_parse_error",
                    json!({
                        "error": e.to_string(),
                        "raw": truncate(&raw, MAX_SNIPPET),
                    }),
                ) {
                    eprintln!("[{role}] step={} action_log_error: {log_err}", step + 1);
                }
                last_result = Some(format!("Parse error: {e}\nReturn exactly one action as a single JSON object in a ```json code block. No prose outside it."));
                step += 1;
                continue;
            }
        };

        if actions.len() != 1 {
            let msg = format!("Got {} actions — emit exactly one action per turn.", actions.len());
            eprintln!("[{role}] step={} {msg}", step + 1);
            if let Err(log_err) = append_message_log(
                role,
                endpoint,
                prompt_kind,
                step + 1,
                &exchange_id,
                "llm_invalid_action_count",
                json!({
                    "action_count": actions.len(),
                    "raw": truncate(&raw, MAX_SNIPPET),
                }),
            ) {
                eprintln!("[{role}] step={} action_log_error: {log_err}", step + 1);
            }
            last_result = Some(msg);
            step += 1;
            continue;
        }

        let mut action = actions[0].clone();
        if let Err(e) = normalize_action(&mut action) {
            if let Err(log_err) = append_message_log(
                role,
                endpoint,
                prompt_kind,
                step + 1,
                &exchange_id,
                "llm_invalid_action",
                json!({
                    "stage": "normalize_action",
                    "error": e.to_string(),
                    "raw": truncate(&raw, MAX_SNIPPET),
                }),
            ) {
                eprintln!("[{role}] step={} action_log_error: {log_err}", step + 1);
            }
            last_result = Some(format!("Invalid action: {e}\nReturn exactly one action with a non-empty `observation`, a non-empty `rationale`, and any required fields."));
            step += 1;
            continue;
        }
        if let Err(e) = validate_action(&action) {
            if let Err(log_err) = append_message_log(
                role,
                endpoint,
                prompt_kind,
                step + 1,
                &exchange_id,
                "llm_invalid_action",
                json!({
                    "stage": "validate_action",
                    "error": e.to_string(),
                    "raw": truncate(&raw, MAX_SNIPPET),
                    "action": action.clone(),
                }),
            ) {
                eprintln!("[{role}] step={} action_log_error: {log_err}", step + 1);
            }
            last_result = Some(format!("Invalid action: {e}\nReturn exactly one action with a non-empty `observation`, a non-empty `rationale`, and any required fields."));
            step += 1;
            continue;
        }

        let kind = action.get("action").and_then(|v| v.as_str()).unwrap_or("unknown").to_string();
        eprintln!("[{role}] step={} action={}", step + 1, kind);
        last_action = Some(kind.clone());
        append_orchestration_trace(
            "llm_message_processed",
            json!({
                "role": role,
                "prompt_kind": prompt_kind,
                "step": step + 1,
                "endpoint_id": endpoint.id,
                "action": kind,
            }),
        );

        let command_id = exchange_id.clone();
        action["command_id"] = Value::String(command_id.clone());

        if role == "diagnostics" && !diagnostics_eventlog_python_done {
            if diagnostics_python_reads_event_logs(&action) {
                diagnostics_eventlog_python_done = true;
            } else if step == 0 {
                last_result = Some(
                    "Diagnostics must begin with a `python` action that analyzes /workspace/ai_sandbox/canon/state/event_log/event.tlog.d to diagnose problems, detect inconsistencies, and extract concrete failure signals."
                        .to_string(),
                );
                step += 1;
                continue;
            } else if matches!(kind.as_str(), "apply_patch" | "message") {
                last_result = Some(
                    "Before writing diagnostics or finishing, run a `python` action that analyzes /workspace/ai_sandbox/canon/state/event_log/event.tlog.d to find errors, inconsistencies, invariant violations, repeated failure patterns, and concrete repair targets. Diagnostics is for finding what is broken."
                        .to_string(),
                );
                step += 1;
                continue;
            }
        }

        if is_explicit_idle_action(&action) {
            idle_streak += 1;
            if idle_streak >= 3 {
                bail!("[{role}] stuck: no progress in 3 steps (repeated explicit idle commands)");
            }
        } else {
            idle_streak = 0;
        }

        let step_result = process_action_and_execute(
            role,
            prompt_kind,
            endpoint,
            workspace,
            step + 1,
            &command_id,
            &action,
            check_on_done,
        )?;

        match step_result {
            (true, reason) => {
                eprintln!("[{role}] message complete: {reason}");
                return Ok(reason);
            }
            (false, out) => {
                last_result = Some(out);
            }
        }
        step += 1;
    }
}

fn find_endpoint<'a>(endpoints: &'a [LlmEndpoint], role: &str) -> Result<&'a LlmEndpoint> {
    endpoints
        .iter()
        .find(|e| e.role.as_deref() == Some(role))
        .ok_or_else(|| anyhow!("no endpoint with role '{role}' in constants"))
}

fn build_endpoints() -> Vec<LlmEndpoint> {
    ENDPOINT_SPECS
        .iter()
        .map(|spec| LlmEndpoint {
            id: spec.id.to_string(),
            url: spec.urls.iter().map(|s| s.to_string()).collect(),
            role_markdown: spec.role_markdown.to_string(),
            role: Some(spec.role.to_string()),
            stateful: spec.stateful,
            max_tabs: spec.max_tabs,
        })
        .collect()
}

#[derive(Clone, Debug, Default)]
struct DispatchLaneState {
    plan_text: String,
    pending: bool,
    in_progress_by: Option<String>,
    latest_verifier_result: String,
}

#[derive(Clone)]
struct PendingSubmitState {
    job: PendingExecutorSubmit,
    started_ms: u64,
    command_id: String,
    endpoint_id: String,
    tabs: TabManagerHandle,
}

#[derive(Clone)]
struct DeferredExecutorCompletion {
    submitted: SubmittedExecutorTurn,
    turn_id: u64,
    tab_id: u32,
    exec_result: String,
}

#[derive(Clone)]
struct DispatchState {
    lanes: HashMap<usize, DispatchLaneState>,
    submitted_turns: std::collections::HashMap<(u32, u64), SubmittedExecutorTurn>,
    pending_submits: HashMap<usize, PendingSubmitState>,
    tab_id_to_lane: HashMap<u32, usize>,
    lane_active_tab: HashMap<usize, u32>,
    lane_prompt_in_flight: HashMap<usize, bool>,
    deferred_completions: HashMap<usize, VecDeque<DeferredExecutorCompletion>>,
    diagnostics_dirty: bool,
    planner_dirty: bool,
    diagnostics_text: String,
    lane_next_submit_at_ms: HashMap<usize, u64>,
    lane_submit_in_flight: HashMap<usize, bool>,
}

fn new_dispatch_state(lanes: &[LaneConfig]) -> DispatchState {
    let mut lanes_state = HashMap::new();
    let mut lane_prompt_in_flight = HashMap::new();
    let mut deferred_completions = HashMap::new();
    let mut lane_next_submit_at_ms = HashMap::new();
    let mut lane_submit_in_flight = HashMap::new();
    for lane in lanes {
        lanes_state.insert(lane.index, DispatchLaneState::default());
        lane_prompt_in_flight.insert(lane.index, false);
        deferred_completions.insert(lane.index, VecDeque::new());
        lane_next_submit_at_ms.insert(lane.index, 0);
        lane_submit_in_flight.insert(lane.index, false);
    }
    DispatchState {
        lanes: lanes_state,
        submitted_turns: std::collections::HashMap::new(),
        pending_submits: HashMap::new(),
        tab_id_to_lane: HashMap::new(),
        lane_active_tab: HashMap::new(),
        lane_prompt_in_flight,
        deferred_completions,
        diagnostics_dirty: false,
        planner_dirty: false,
        diagnostics_text: String::new(),
        lane_next_submit_at_ms,
        lane_submit_in_flight,
    }
}

#[derive(Clone)]
struct SubmittedExecutorTurn {
    tab_id: u32,
    lane: usize,
    lane_label: String,
    command_id: String,
    actor: String,
    endpoint_id: String,
    tabs: TabManagerHandle,
}

#[derive(Clone, Debug)]
struct PendingExecutorSubmit {
    executor_name: String,
    executor_display: String,
    lane_index: usize,
    lane_plan_file: String,
    label: String,
    latest_verify_result: String,
    executor_role: String,
}

fn parse_submit_ack(raw: &str) -> Option<(u32, u64, Option<String>)> {
    let v: Value = serde_json::from_str(raw).ok()?;
    if v.get("submit_ack").and_then(|x| x.as_bool()) != Some(true) {
        return None;
    }
    let tab_id = v.get("tab_id").and_then(|x| x.as_u64())? as u32;
    let turn_id = v.get("turn_id").and_then(|x| x.as_u64())?;
    let command_id = v.get("command_id").and_then(|x| x.as_str()).map(str::to_string);
    Some((tab_id, turn_id, command_id))
}

fn append_executor_completion_log(
    submitted: &SubmittedExecutorTurn,
    step: usize,
    turn_id: u64,
    tab_id: u32,
    text: &str,
) -> Result<()> {
    let parsed = parse_actions(text)
        .ok()
        .and_then(|actions| actions.into_iter().next());
    let observation = parsed
        .as_ref()
        .and_then(|action| action_observation(action))
        .map(str::to_string);
    let rationale = parsed
        .as_ref()
        .and_then(|action| action_rationale(action))
        .map(str::to_string);
    let parsed_action = parsed
        .as_ref()
        .and_then(|action| action.get("action").and_then(|v| v.as_str()))
        .map(str::to_string);
    let parsed_command = parsed
        .as_ref()
        .map(|action| {
            let kind = action.get("action").and_then(|v| v.as_str()).unwrap_or("unknown");
            match kind {
                "run_command" => action.get("cmd").and_then(|v| v.as_str()).unwrap_or("").to_string(),
                "python" => "python".to_string(),
                "read_file" => {
                    let path = action.get("path").and_then(|v| v.as_str()).unwrap_or("");
                    let line = action.get("line").and_then(|v| v.as_u64());
                    match line {
                        Some(n) => format!("read_file {}:{}", path, n),
                        None => format!("read_file {}", path),
                    }
                }
                "list_dir" => format!("list_dir {}", action.get("path").and_then(|v| v.as_str()).unwrap_or("")),
                "apply_patch" => "apply_patch".to_string(),
                "message" => {
                    let status = action.get("status").and_then(|v| v.as_str()).unwrap_or("");
                    let summary = action
                        .get("payload")
                        .and_then(|v| v.get("summary"))
                        .and_then(|v| v.as_str())
                        .unwrap_or("");
                    format!("message {} {}", status, summary)
                }
                _ => kind.to_string(),
            }
        })
        .filter(|s| !s.is_empty());
    let record = compact_log_record(
        "llm",
        "completion",
        Some(&submitted.actor),
        Some(submitted.lane_label.as_str()),
        Some(&submitted.endpoint_id),
        Some(step),
        Some(turn_id),
        Some(&submitted.command_id),
        parsed_action.map(|name| {
            let summary = parsed_command.clone().unwrap_or_else(|| name.clone());
            json!({
                "name": name,
                "summary": summary,
            })
        }),
        None,
        observation,
        rationale,
        Some(text.to_string()),
        Some(json!({ "tab_id": tab_id })),
    );
    append_action_log_record(&record)
}

fn parse_completed_turn(value: &Value) -> Option<(u32, u64, String)> {
    let tab_id = value.get("tab_id").and_then(|x| x.as_u64())? as u32;
    let turn_id = value.get("turn_id").and_then(|x| x.as_u64())?;
    let text = value.get("text").and_then(|x| x.as_str())?.to_string();
    Some((tab_id, turn_id, text))
}

fn completed_turn_is_complete(text: &str) -> bool {
    parse_actions(text)
        .ok()
        .and_then(|actions| actions.into_iter().next())
        .and_then(|action| action.get("action").and_then(|v| v.as_str()).map(str::to_string))
        .as_deref()
        == Some("message")
}

fn handle_executor_completion(
    submitted: SubmittedExecutorTurn,
    tab_id: u32,
    turn_id: u64,
    exec_result: String,
    dispatch_state: &mut DispatchState,
    lanes: &[LaneConfig],
    bridge: &WsBridge,
    workspace: &PathBuf,
    continuation_joinset: &mut tokio::task::JoinSet<(SubmittedExecutorTurn, u64, Result<String>)>,
    verifier_queue: &mut VecDeque<(SubmittedExecutorTurn, u64, String)>,
) -> bool {
    let lane_cfg = &lanes[submitted.lane];
    let lane_name = lane_cfg.label.as_str();
    if *dispatch_state
        .lane_prompt_in_flight
        .get(&submitted.lane)
        .unwrap_or(&false)
    {
        dispatch_state
            .deferred_completions
            .entry(submitted.lane)
            .or_default()
            .push_back(DeferredExecutorCompletion {
                submitted,
                turn_id,
                tab_id,
                exec_result,
            });
        append_orchestration_trace(
            "executor_completion_deferred",
            json!({
                "lane_name": lane_name,
                "tab_id": tab_id,
                "turn_id": turn_id,
            }),
        );
        return false;
    }

    append_orchestration_trace(
        "llm_message_processed",
        json!({
            "tab_id": tab_id,
            "turn_id": turn_id,
            "lane_name": lane_name,
        }),
    );
    if let Err(e) = append_executor_completion_log(&submitted, 1, turn_id, tab_id, &exec_result) {
        eprintln!("[orchestrate] executor_completion_log_error: {e}");
    }
    if completed_turn_is_complete(&exec_result) {
        if let Ok(mut actions) = parse_actions(&exec_result) {
            if let Some(action) = actions.pop() {
                if let Err(e) = append_action_result_log(
                    &submitted.actor,
                    &lane_cfg.endpoint,
                    "executor",
                    1,
                    &submitted.command_id,
                    &action,
                    true,
                    &exec_result,
                ) {
                    eprintln!("[orchestrate] executor_message_result_log_error: {e}");
                }
            }
        }
    }
    if submitted.tab_id != tab_id {
        eprintln!(
            "[orchestrate] completed turn tab mismatch: turn_id={} expected_tab={} actual_tab={}",
            turn_id, submitted.tab_id, tab_id
        );
        let lane = dispatch_lane_mut(dispatch_state, submitted.lane);
        lane.in_progress_by = None;
        lane.pending = true;
        return true;
    }
    let final_exec_result = if completed_turn_is_complete(&exec_result) {
        exec_result
    } else {
        eprintln!(
            "[orchestrate] executor turn requires tool execution: lane={} turn_id={}",
            lane_name,
            turn_id
        );
        append_orchestration_trace(
            "executor_completion_requires_tool",
            json!({
                "lane_name": lane_name,
                "tab_id": tab_id,
                "turn_id": turn_id,
                "endpoint_id": lane_cfg.endpoint.id,
            }),
        );
        let executor_endpoint = lane_cfg.endpoint.clone();
        let bridge = bridge.clone();
        let workspace = workspace.clone();
        let exec_result = exec_result.clone();
        let submitted_clone = submitted.clone();
        let tabs = submitted.tabs.clone();
        dispatch_state
            .lane_prompt_in_flight
            .insert(submitted.lane, true);
        continuation_joinset.spawn(async move {
            let result = continue_executor_completion(
                &submitted_clone,
                &exec_result,
                turn_id,
                &executor_endpoint,
                &bridge,
                &workspace,
                &tabs,
            )
            .await;
            (submitted_clone, turn_id, result)
        });
        return true;
    };
    if completed_turn_is_complete(&final_exec_result) {
        verifier_queue.push_back((submitted, turn_id, final_exec_result));
        true
    } else {
        eprintln!(
            "[orchestrate] executor completion not complete: lane={} turn_id={}",
            lane_name,
            turn_id
        );
        let lane = dispatch_lane_mut(dispatch_state, submitted.lane);
        lane.in_progress_by = None;
        lane.pending = true;
        true
    }
}

fn verifier_confirmed(reason: &str) -> bool {
    if let Ok(v) = serde_json::from_str::<Value>(reason) {
        if let Some(verified) = v.get("verified").and_then(|x| x.as_bool()) {
            return verified;
        }
    }
    false
}

fn dispatch_lane_mut<'a>(state: &'a mut DispatchState, lane_id: usize) -> &'a mut DispatchLaneState {
    state
        .lanes
        .get_mut(&lane_id)
        .unwrap_or_else(|| panic!("missing lane state for {:?}", lane_id))
}

fn claim_next_lane(state: &mut DispatchState, lane: &LaneConfig) -> Option<(usize, String)> {
    let lane_id = lane.index;
    let lane_state = dispatch_lane_mut(state, lane_id);
    if lane_state.pending && lane_state.in_progress_by.is_none() && !lane_state.plan_text.trim().is_empty() {
        lane_state.pending = false;
        lane_state.in_progress_by = Some(lane.label.clone());
        return Some((lane_id, lane_state.latest_verifier_result.clone()));
    }
    None
}

fn claim_executor_submit(state: &mut DispatchState, lane: &LaneConfig) -> Option<PendingExecutorSubmit> {
    let (lane_id, latest_verify_result) = claim_next_lane(state, lane)?;
    let executor_display = format!("executor {}", lane.label);
    let executor_role = format!("executor[{}]", lane.label);
    Some(PendingExecutorSubmit {
        executor_name: "executor".to_string(),
        executor_display,
        lane_index: lane_id,
        lane_plan_file: lane.plan_file.clone(),
        label: lane.label.clone(),
        latest_verify_result,
        executor_role,
    })
}

async fn submit_executor_turn(
    job: &PendingExecutorSubmit,
    endpoint: &LlmEndpoint,
    bridge: &WsBridge,
    tabs: &TabManagerHandle,
    send_system_prompt: bool,
    command_id: &str,
    response_timeout_secs: u64,
) -> Result<String> {
    let lane_plan_text = std::fs::read_to_string(Path::new(WORKSPACE).join(&job.lane_plan_file)).unwrap_or_default();
    let exec_prompt = executor_cycle_prompt(
        job.executor_display.as_str(),
        job.label.as_str(),
        job.lane_plan_file.as_str(),
        lane_plan_text.as_str(),
        &job.latest_verify_result,
    );
    let executor_system = system_instructions(AgentPromptKind::Executor);
    let role_schema = if send_system_prompt {
        executor_system
    } else {
        String::new()
    };
    let prompt = exec_prompt;
    eprintln!(
        "[{}] step=1 prompt_bytes={}",
        job.executor_role,
        prompt.len()
    );
    if let Err(e) = append_message_log(
        &job.executor_role,
        endpoint,
        "executor",
        1,
        command_id,
        "llm_request",
        json!({
            "submit_only": true,
            "prompt_bytes": prompt.len(),
            "role_schema_bytes": role_schema.len(),
            "prompt": truncate(&prompt, MAX_SNIPPET),
        }),
    ) {
        eprintln!("[{}] step=1 action_log_error: {e}", job.executor_role);
    }
    append_orchestration_trace(
        "llm_message_forwarded",
        json!({
            "role": job.executor_role,
            "prompt_kind": "executor",
            "step": 1,
            "endpoint_id": endpoint.id,
            "submit_only": true,
            "prompt_bytes": prompt.len(),
        }),
    );
    let raw = llm_worker_send_request_timeout(
        bridge,
        &endpoint.id,
        &endpoint.url,
        endpoint.stateful,
        &prompt,
        &role_schema,
        None,
        None,
        false,
        true,
        &job.executor_role,
        tabs,
        endpoint.max_tabs,
        true,
        Some(response_timeout_secs),
    )
    .await?;
    append_orchestration_trace(
        "llm_message_received",
        json!({
            "role": job.executor_role,
            "prompt_kind": "executor",
            "step": 1,
            "endpoint_id": endpoint.id,
            "submit_only": true,
            "response_bytes": raw.len(),
        }),
    );
    if let Err(e) = append_message_log(
        &job.executor_role,
        endpoint,
        "executor",
        1,
        command_id,
        "llm_response",
        json!({
            "submit_only": true,
            "response_bytes": raw.len(),
            "raw": truncate(&raw, MAX_SNIPPET),
        }),
    ) {
        eprintln!("[{}] step=1 action_log_error: {e}", job.executor_role);
    }
    if let Ok(mut ack) = serde_json::from_str::<Value>(&raw) {
        if ack.get("submit_ack").and_then(|v| v.as_bool()) == Some(true) {
            ack["command_id"] = Value::String(command_id.to_string());
            eprintln!("[{}] step=1 submit_ack={}", job.executor_role, raw);
            if let Err(e) = append_message_log(
                &job.executor_role,
                endpoint,
                "executor",
                1,
                command_id,
                "llm_submit_ack",
                ack.clone(),
            ) {
                eprintln!("[{}] step=1 action_log_error: {e}", job.executor_role);
            }
            append_orchestration_trace(
                "llm_message_processed",
                json!({
                    "role": job.executor_role,
                    "prompt_kind": "executor",
                    "step": 1,
                    "endpoint_id": endpoint.id,
                    "submit_ack": ack,
                }),
            );
        }
    }
    Ok(raw)
}

// ── Main ───────────────────────────────────────────────────────────────────────

pub async fn run() -> Result<()> {
    let args: Vec<String> = std::env::args().collect();
    let orchestrate = args.iter().any(|a| a == "--orchestrate");
    let start_role = args.windows(2).find(|w| w[0] == "--start").map(|w| w[1].as_str()).unwrap_or("executor");
    if !matches!(start_role, "executor" | "verifier" | "planner" | "diagnostics") {
        bail!("invalid --start value: {start_role} (expected executor|verifier|planner|diagnostics)");
    }
    let is_verifier = !orchestrate && args.iter().any(|a| a == "--verifier");
    let is_planner = !orchestrate && args.iter().any(|a| a == "--planner");
    let is_diagnostics = !orchestrate && args.iter().any(|a| a == "--diagnostics");
    let (ws_port, ws_port_explicit) = choose_ws_port(&args)?;
    if ws_port_explicit {
        eprintln!("[canon-mini-agent] ws_port={} (explicit)", ws_port);
    } else {
        eprintln!(
            "[canon-mini-agent] ws_port={} (auto-selected from {:?})",
            ws_port,
            WS_PORT_CANDIDATES
        );
    }

    let workspace = PathBuf::from(WORKSPACE);
    let spec_path = workspace.join(SPEC_FILE);
    let master_plan_path = workspace.join(MASTER_PLAN_FILE);
    let violations_path = workspace.join(VIOLATIONS_FILE);
    let instance_id = instance_arg(&args).map(str::to_string);
    let path_prefix = instance_id.clone().unwrap_or_else(|| "default".to_string());
    init_log_paths(&path_prefix);
    let diagnostics_rel = format!("PLANS/{}/diagnostics-{}.md", path_prefix, path_prefix);
    let diagnostics_path = workspace.join(&diagnostics_rel);
    let _ = DIAGNOSTICS_FILE_PATH.set(diagnostics_rel.clone());

    let endpoints = build_endpoints();
    let mut executor_endpoints: Vec<LlmEndpoint> = endpoints
        .iter()
        .filter(|e| e.role.as_deref() == Some("executor"))
        .cloned()
        .collect();
    executor_endpoints.sort_by(|a, b| a.id.cmp(&b.id));
    let lanes: Vec<LaneConfig> = executor_endpoints
        .into_iter()
        .enumerate()
        .map(|(index, ep)| LaneConfig {
            index,
            plan_file: format!("PLANS/{}/executor-{}.json", path_prefix, ep.id),
            label: ep.id.clone(),
            endpoint: ep,
            tabs: llm_worker_new_tabs(),
        })
        .collect();
    if lanes.is_empty() {
        bail!("no executor endpoints with role = \"executor\" found in constants");
    }
    let plans_dir = workspace.join("PLANS").join(&path_prefix);
    let _ = std::fs::create_dir_all(&plans_dir);
    if !diagnostics_path.exists() {
        let legacy_path = workspace.join("DIAGNOSTICS.md");
        if let Ok(contents) = std::fs::read_to_string(&legacy_path) {
            let _ = std::fs::write(&diagnostics_path, contents);
        } else {
            let _ = std::fs::write(&diagnostics_path, "");
        }
    }
    for lane in &lanes {
        let plan_path = workspace.join(&lane.plan_file);
        if plan_path.exists() {
            continue;
        }
        let legacy_json = workspace.join(format!("PLANS/executor-{}.json", lane.endpoint.id));
        let legacy_md = workspace.join(format!("PLANS/executor-{}.md", lane.endpoint.id));
        if let Ok(contents) = std::fs::read_to_string(&legacy_json) {
            if let Some(parent) = plan_path.parent() {
                let _ = std::fs::create_dir_all(parent);
            }
            let _ = std::fs::write(&plan_path, contents);
        } else if let Ok(contents) = std::fs::read_to_string(&legacy_md) {
            if let Some(parent) = plan_path.parent() {
                let _ = std::fs::create_dir_all(parent);
            }
            let _ = std::fs::write(&plan_path, contents);
        } else {
            if let Some(parent) = plan_path.parent() {
                let _ = std::fs::create_dir_all(parent);
            }
            let _ = std::fs::write(&plan_path, "");
        }
    }

    let ws_addr: std::net::SocketAddr = format!("127.0.0.1:{ws_port}").parse()?;
    let bridge = ws_server::spawn(ws_addr, DEFAULT_RESPONSE_TIMEOUT_SECS, Arc::new(OnceLock::new()));
    eprintln!("[canon-mini-agent] waiting for Chrome extension on ws://127.0.0.1:{ws_port}");
    bridge.wait_for_connection().await;
    eprintln!("[canon-mini-agent] Chrome extension connected");

    let tabs = llm_worker_new_tabs();

    if orchestrate {
        const SERVICE_POLL_MS: u64 = 500;
        const PENDING_SUBMIT_TIMEOUT_MS: u64 = 10_000;

        eprintln!("[orchestrate] start_role={start_role}");

        let diagnostics_ep = find_endpoint(&endpoints, "diagnostics")?.clone();
        let planner_ep = find_endpoint(&endpoints, "mini_planner")?.clone();
        let verifier_ep = find_endpoint(&endpoints, "verifier")?.clone();

        let tabs_diagnostics = llm_worker_new_tabs();
        let tabs_planner = llm_worker_new_tabs();
        let tabs_verify = llm_worker_new_tabs();
        let mut verifier_summary: Vec<String> = vec!["(none yet)".to_string(); lanes.len()];
        let mut dispatch_state = {
            let mut state = new_dispatch_state(&lanes);
            state.planner_dirty = true;
            state
        };
        let mut planner_bootstrapped = false;
        let mut diagnostics_bootstrapped = false;
        let mut verifier_bootstrapped = false;
        let mut submit_joinset: tokio::task::JoinSet<(usize, PendingExecutorSubmit, Result<String>)> =
            tokio::task::JoinSet::new();
        let mut continuation_joinset: tokio::task::JoinSet<(SubmittedExecutorTurn, u64, Result<String>)> =
            tokio::task::JoinSet::new();
        let mut verifier_joinset: tokio::task::JoinSet<(usize, String)> = tokio::task::JoinSet::new();
        let mut verifier_queue: VecDeque<(SubmittedExecutorTurn, u64, String)> = VecDeque::new();

        eprintln!("[orchestrate] pipeline started: planner -> background executors -> verifier/diagnostics -> planner");

        loop {
            let mut cycle_progress = false;

            if dispatch_state.planner_dirty {
                let summary_text = lanes
                    .iter()
                    .map(|lane| format!("{}={}", lane.label, verifier_summary[lane.index]))
                    .collect::<Vec<_>>()
                    .join("\n");
                let lane_plan_list = lanes
                    .iter()
                    .map(|lane| lane.plan_file.as_str())
                    .collect::<Vec<_>>()
                    .join(", ");
                let objectives_text =
                    std::fs::read_to_string(workspace.join(OBJECTIVES_FILE)).unwrap_or_default();
                let invariants_text =
                    std::fs::read_to_string(workspace.join(INVARIANTS_FILE)).unwrap_or_default();
                let violations_text =
                    std::fs::read_to_string(&violations_path).unwrap_or_default();
                let diagnostics_text =
                    std::fs::read_to_string(&diagnostics_path).unwrap_or_default();
                let planner_prompt = planner_cycle_prompt(
                    &summary_text,
                    &lane_plan_list,
                    &objectives_text,
                    &invariants_text,
                    &violations_text,
                    &diagnostics_text,
                );
                append_orchestration_trace(
                    "llm_message_forwarded",
                    json!({
                        "from": "orchestrator",
                        "to": "planner",
                        "phase": "planner",
                    }),
                );
                let planner_system = system_instructions(AgentPromptKind::Planner);
                let result = run_agent(
                    "planner",
                    "planner",
                    &planner_system,
                    planner_prompt,
                    &planner_ep,
                    &bridge,
                    &workspace,
                    &tabs_planner,
                    false,
                    false,
                    !planner_bootstrapped,
                )
                .await;
                match result {
                    Ok(result) => {
                        eprintln!("[orchestrate] planner ok bytes={}", result.len());
                        for lane in &lanes {
                            let mut plan_text = std::fs::read_to_string(workspace.join(&lane.plan_file)).unwrap_or_default();
                            if plan_text.trim().is_empty() {
                                let legacy_paths = match lane.index {
                                    0 => vec!["PLANS/executor-a.json", "PLANS/executor-a.md"],
                                    1 => vec!["PLANS/executor-b.json", "PLANS/executor-b.md"],
                                    _ => Vec::new(),
                                };
                                for legacy in legacy_paths {
                                    let legacy_text =
                                        std::fs::read_to_string(workspace.join(legacy)).unwrap_or_default();
                                    if !legacy_text.trim().is_empty() {
                                        eprintln!(
                                            "[orchestrate] legacy lane plan fallback: {} -> {}",
                                            legacy,
                                            lane.plan_file
                                        );
                                        plan_text = legacy_text;
                                        break;
                                    }
                                }
                            }
                            let lane_state = dispatch_lane_mut(&mut dispatch_state, lane.index);
                            let changed = lane_state.plan_text != plan_text;
                            lane_state.plan_text = plan_text;
                            if lane_state.in_progress_by.is_none()
                                && (changed || !verifier_confirmed(&lane_state.latest_verifier_result))
                            {
                                lane_state.pending = !lane_state.plan_text.trim().is_empty();
                            }
                        }

                        dispatch_state.planner_dirty = false;
                        cycle_progress = true;
                    }
                    Err(err) => {
                        eprintln!("[orchestrate] planner error: {err:#}");
                    }
                }
                planner_bootstrapped = true;
            }

            let now = now_ms();
            if !dispatch_state.pending_submits.is_empty() {
                let mut timed_out = Vec::new();
                for (lane_id, pending) in dispatch_state.pending_submits.iter() {
                    if now.saturating_sub(pending.started_ms) >= PENDING_SUBMIT_TIMEOUT_MS {
                        timed_out.push(*lane_id);
                    }
                }
                for lane_id in timed_out {
                    if let Some(pending) = dispatch_state.pending_submits.remove(&lane_id) {
                        eprintln!(
                            "[orchestrate] pending submit timeout: lane={} command_id={}",
                            lanes[lane_id].label,
                            pending.command_id
                        );
                        append_orchestration_trace(
                            "executor_submit_timeout",
                            json!({
                                "lane_name": lanes[lane_id].label,
                                "command_id": pending.command_id,
                            }),
                        );
                    }
                   dispatch_state.lane_submit_in_flight.insert(lane_id, false);
                   let lane = dispatch_lane_mut(&mut dispatch_state, lane_id);
                    lane.in_progress_by = None;
                    lane.pending = true;
                }
            }
            for lane in &lanes {
                let in_flight = *dispatch_state
                    .lane_submit_in_flight
                    .get(&lane.index)
                    .unwrap_or(&false);
                let next_at = *dispatch_state
                    .lane_next_submit_at_ms
                    .get(&lane.index)
                    .unwrap_or(&0);
                if in_flight || next_at > now {
                    continue;
                }
                if let Some(job) = claim_executor_submit(&mut dispatch_state, lane) {
                    let lane_index = lane.index;
                    let endpoint = lane.endpoint.clone();
                    let bridge = bridge.clone();
                    let tabs = lane.tabs.clone();
                    let command_id = make_command_id(&job.executor_role, "executor", 1);
                    let response_timeout_secs = response_timeout_for_role(&job.executor_role);
                    dispatch_state.pending_submits.insert(
                        lane_index,
                        PendingSubmitState {
                            job: job.clone(),
                            started_ms: now_ms(),
                            command_id: command_id.clone(),
                            endpoint_id: endpoint.id.clone(),
                            tabs: tabs.clone(),
                        },
                    );
                    dispatch_state.lane_submit_in_flight.insert(lane_index, true);
                    submit_joinset.spawn(async move {
                        let result = submit_executor_turn(
                            &job,
                            &endpoint,
                            &bridge,
                            &tabs,
                            true,
                            &command_id,
                            response_timeout_secs,
                        )
                        .await;
                        (lane_index, job, result)
                    });
                }
            }

            while let Some(joined) = submit_joinset.try_join_next() {
                match joined {
                    Ok((lane_id, job, result)) => {
                        match result {
                            Ok(exec_result) => {
                                if let Some((tab_id, turn_id, command_id)) = parse_submit_ack(&exec_result) {
                                    let Some(pending) = dispatch_state.pending_submits.remove(&lane_id) else {
                                        eprintln!(
                                            "[orchestrate] submit ack without pending submit: lane={} tab_id={} turn_id={}",
                                            lanes[lane_id].label,
                                            tab_id,
                                            turn_id
                                        );
                                        continue;
                                    };
                                    if now_ms().saturating_sub(pending.started_ms) >= PENDING_SUBMIT_TIMEOUT_MS {
                                        eprintln!(
                                            "[orchestrate] submit ack arrived after timeout: lane={} tab_id={} turn_id={}",
                                            lanes[lane_id].label,
                                            tab_id,
                                            turn_id
                                        );
                                        dispatch_state.lane_submit_in_flight.insert(lane_id, false);
                                        dispatch_state.lane_prompt_in_flight.insert(lane_id, false);
                                        continue;
                                    }
                                    if let Some(active_tab) = dispatch_state.lane_active_tab.get(&lane_id) {
                                        if *active_tab != tab_id {
                                            eprintln!(
                                                "[orchestrate] submit ack tab mismatch: lane={} active_tab={} ack_tab={} (overwriting active tab)",
                                                lanes[lane_id].label,
                                                active_tab,
                                                tab_id
                                            );
                                        }
                                    }
                                    dispatch_state.lane_active_tab.insert(lane_id, tab_id);
                                    dispatch_state
                                        .tab_id_to_lane
                                        .entry(tab_id)
                                        .or_insert(lane_id);
                                    dispatch_state.submitted_turns.insert(
                                        (tab_id, turn_id),
                                        SubmittedExecutorTurn {
                                            tab_id,
                                            lane: job.lane_index,
                                            lane_label: job.label.clone(),
                                            command_id: command_id.unwrap_or_else(|| pending.command_id.clone()),
                                            actor: job.executor_role.clone(),
                                            endpoint_id: pending.endpoint_id.clone(),
                                            tabs: pending.tabs.clone(),
                                        },
                                    );
                                    dispatch_state.lane_next_submit_at_ms.insert(lane_id, now_ms());
                                    dispatch_state.lane_submit_in_flight.insert(lane_id, false);
                                    cycle_progress = true;
                                } else {
                                    eprintln!("[orchestrate] {} missing submit_ack: {exec_result}", job.executor_name);
                                    let lane = dispatch_lane_mut(&mut dispatch_state, job.lane_index);
                                    lane.in_progress_by = None;
                                    lane.pending = true;
                                    dispatch_state.pending_submits.remove(&job.lane_index);
                   dispatch_state.lane_submit_in_flight.insert(job.lane_index, false);
                               }
                           }
                           Err(err) => {
                               eprintln!("[orchestrate] {} submit error: {err:#}", job.executor_name);
                               let lane = dispatch_lane_mut(&mut dispatch_state, job.lane_index);
                               lane.in_progress_by = None;
                               lane.pending = true;
                               dispatch_state.pending_submits.remove(&job.lane_index);
                               dispatch_state.lane_submit_in_flight.insert(job.lane_index, false);
                            }
                        }
                    }
                    Err(err) => {
                        eprintln!("[orchestrate] submit join error: {err:#}");
                    }
                }
            }

            let completed_turns = bridge.take_completed_turns().await;
            let mut verifier_changed = false;
            for item in completed_turns {
                append_orchestration_trace("llm_message_received", item.clone());
                let Some((tab_id, turn_id, exec_result)) = parse_completed_turn(&item) else {
                    continue;
                };
                let submitted = if let Some(submitted) =
                    dispatch_state.submitted_turns.remove(&(tab_id, turn_id))
                {
                    submitted
                } else {
                    let lane_id = dispatch_state
                        .tab_id_to_lane
                        .get(&tab_id)
                        .copied()
                        .or_else(|| {
                            if dispatch_state.pending_submits.len() == 1 {
                                let (&lane_id, _) = dispatch_state.pending_submits.iter().next()?;
                                dispatch_state.tab_id_to_lane.insert(tab_id, lane_id);
                                Some(lane_id)
                            } else {
                                None
                            }
                        });
                    let Some(lane_id) = lane_id else {
                        append_orchestration_trace(
                            "executor_completion_unmatched",
                            json!({
                                "tab_id": tab_id,
                                "turn_id": turn_id,
                                "text": truncate(&exec_result, MAX_SNIPPET),
                            }),
                        );
                        continue;
                    };
                    if let Some(active_tab) = dispatch_state.lane_active_tab.get(&lane_id) {
                        if *active_tab != tab_id {
                            append_orchestration_trace(
                                "executor_completion_tab_mismatch",
                                json!({
                                    "lane_name": lanes[lane_id].label,
                                    "active_tab": active_tab,
                                    "tab_id": tab_id,
                                    "turn_id": turn_id,
                                }),
                            );
                            continue;
                        }
                    } else {
                        dispatch_state.lane_active_tab.insert(lane_id, tab_id);
                    }
                    let Some(pending) = dispatch_state.pending_submits.remove(&lane_id) else {
                        append_orchestration_trace(
                            "executor_completion_unmatched",
                            json!({
                                "tab_id": tab_id,
                                "turn_id": turn_id,
                                "text": truncate(&exec_result, MAX_SNIPPET),
                            }),
                        );
                        continue;
                    };
                dispatch_state.lane_submit_in_flight.insert(lane_id, false);
                dispatch_state.lane_next_submit_at_ms.insert(lane_id, now_ms());
                SubmittedExecutorTurn {
                    tab_id,
                    lane: lane_id,
                    lane_label: lanes[lane_id].label.clone(),
                    command_id: pending.command_id,
                    actor: pending.job.executor_role,
                    endpoint_id: pending.endpoint_id,
                    tabs: pending.tabs,
                }
            };
            dispatch_state.lane_prompt_in_flight.insert(submitted.lane, false);
            if handle_executor_completion(
                submitted,
                tab_id,
                turn_id,
                exec_result,
                &mut dispatch_state,
                &lanes,
                &bridge,
                &workspace,
                &mut continuation_joinset,
                &mut verifier_queue,
            ) {
                cycle_progress = true;
            }
        }

            while let Some(joined) = continuation_joinset.try_join_next() {
                match joined {
                    Ok((submitted, turn_id, result)) => match result {
                        Ok(final_exec_result) => {
                            dispatch_state.lane_prompt_in_flight.insert(submitted.lane, false);
                            // Continuations only return once the executor has reached completion,
                            // and the returned value is the completion summary (not the raw action JSON).
                            verifier_queue.push_back((submitted, turn_id, final_exec_result));
                            cycle_progress = true;
                        }
                        Err(err) => {
                            eprintln!(
                                "[orchestrate] executor continuation error: lane={} err={err:#}",
                                submitted.lane_label
                            );
                            dispatch_state.lane_prompt_in_flight.insert(submitted.lane, false);
                            let lane = dispatch_lane_mut(&mut dispatch_state, submitted.lane);
                            lane.in_progress_by = None;
                            lane.pending = true;
                            cycle_progress = true;
                        }
                    },
                    Err(err) => {
                        eprintln!("[orchestrate] continuation join error: {err:#}");
                    }
                }
            }

            for lane_id in 0..lanes.len() {
                let in_flight = *dispatch_state
                    .lane_prompt_in_flight
                    .get(&lane_id)
                    .unwrap_or(&false);
                if in_flight {
                    continue;
                }
                while let Some(deferred) = dispatch_state
                    .deferred_completions
                    .get_mut(&lane_id)
                    .and_then(|queue| queue.pop_front())
                {
                    if handle_executor_completion(
                        deferred.submitted,
                        deferred.tab_id,
                        deferred.turn_id,
                        deferred.exec_result,
                        &mut dispatch_state,
                        &lanes,
                        &bridge,
                        &workspace,
                        &mut continuation_joinset,
                        &mut verifier_queue,
                    ) {
                        cycle_progress = true;
                    }
                    let now_in_flight = *dispatch_state
                        .lane_prompt_in_flight
                        .get(&lane_id)
                        .unwrap_or(&false);
                    if now_in_flight {
                        break;
                    }
                }
            }

            while let Some((submitted, turn_id, final_exec_result)) = verifier_queue.pop_front() {
                let lane_plan_file = lanes[submitted.lane].plan_file.clone();
                let verifier_prompt =
                    verifier_cycle_prompt(submitted.lane_label.as_str(), lane_plan_file.as_str(), &final_exec_result);
                append_orchestration_trace(
                    "llm_message_forwarded",
                    json!({
                        "from": format!("executor:{}", submitted.lane_label),
                        "to": "verifier",
                        "tab_id": submitted.tab_id,
                        "turn_id": turn_id,
                        "lane_name": submitted.lane_label.as_str(),
                        "lane_plan_file": lane_plan_file,
                    }),
                );
                let verifier_system = system_instructions(AgentPromptKind::Verifier);
                let verifier_ep = verifier_ep.clone();
                let bridge = bridge.clone();
                let workspace = workspace.clone();
                let send_system = !verifier_bootstrapped;
                verifier_bootstrapped = true;
                let tabs_verify = tabs_verify.clone();
                verifier_joinset.spawn(async move {
                    let verify_result = match run_agent(
                        "verifier",
                        "verifier",
                        &verifier_system,
                        verifier_prompt,
                        &verifier_ep,
                        &bridge,
                        &workspace,
                        &tabs_verify,
                        false,
                        false,
                        send_system,
                    )
                    .await
                    {
                        Ok(result) => result,
                        Err(err) => format!(
                            "{{\"verified\":false,\"summary\":\"verifier error: {}\"}}",
                            err.to_string().replace('"', "'")
                        ),
                    };
                    (submitted.lane, verify_result)
                });
            }

            while let Some(joined) = verifier_joinset.try_join_next() {
                match joined {
                    Ok((lane_id, verify_result)) => {
                        let lane = dispatch_lane_mut(&mut dispatch_state, lane_id);
                        let changed = lane.latest_verifier_result != verify_result;
                        lane.latest_verifier_result = verify_result.clone();
                        lane.in_progress_by = None;
                        lane.pending = !verifier_confirmed(&verify_result);
                        verifier_changed |= changed;
                        verifier_summary[lane_id] = verify_result;
                        cycle_progress = true;
                    }
                    Err(err) => {
                        eprintln!("[orchestrate] verifier join error: {err:#}");
                    }
                }
            }

            if verifier_changed {
                dispatch_state.diagnostics_dirty = true;
            }

            if dispatch_state.diagnostics_dirty {
                let summary_text = lanes
                    .iter()
                    .map(|lane| format!("{}={}", lane.label, verifier_summary[lane.index]))
                    .collect::<Vec<_>>()
                    .join("\n");
                let prompt = diagnostics_cycle_prompt(&summary_text);
                append_orchestration_trace(
                    "llm_message_forwarded",
                    json!({
                        "from": "verifier",
                        "to": "diagnostics",
                        "phase": "diagnostics",
                    }),
                );
                let diagnostics_system = system_instructions(AgentPromptKind::Diagnostics);
                match run_agent(
                    "diagnostics",
                    "diagnostics",
                    &diagnostics_system,
                    prompt,
                    &diagnostics_ep,
                    &bridge,
                    &workspace,
                    &tabs_diagnostics,
                    false,
                    false,
                    !diagnostics_bootstrapped,
                )
                .await
                {
                    Ok(result) => {
                        eprintln!("[orchestrate] diagnostics ok bytes={}", result.len());
                        let new_diagnostics_text = std::fs::read_to_string(&diagnostics_path).unwrap_or_default();
                        let diagnostics_changed = dispatch_state.diagnostics_text != new_diagnostics_text;
                        dispatch_state.diagnostics_text = new_diagnostics_text;
                        dispatch_state.diagnostics_dirty = false;
                        dispatch_state.planner_dirty = diagnostics_changed || verifier_changed;
                        cycle_progress = true;
                    }
                    Err(err) => {
                        eprintln!("[orchestrate] diagnostics error: {err:#}");
                    }
                }
                diagnostics_bootstrapped = true;
            }

            if !cycle_progress {
                tokio::time::sleep(std::time::Duration::from_millis(SERVICE_POLL_MS)).await;
            }
        }
    } else {
        // Single-role mode
        let (role, prompt_kind) = if is_verifier {
            ("verifier", AgentPromptKind::Verifier)
        } else if is_diagnostics {
            ("diagnostics", AgentPromptKind::Diagnostics)
        } else if is_planner {
            ("mini_planner", AgentPromptKind::Planner)
        } else {
            ("executor", AgentPromptKind::Executor)
        };
        let instructions = system_instructions(prompt_kind);

        let primary_input_path = if is_verifier || is_planner {
            &spec_path
        } else {
            &workspace.join(&lanes[0].plan_file)
        };
        let primary_input_name = if is_verifier || is_planner {
            SPEC_FILE
        } else {
            lanes[0].plan_file.as_str()
        };
        let primary_input = std::fs::read_to_string(primary_input_path).with_context(|| format!("failed to read {primary_input_name}"))?;
        if primary_input.trim().is_empty() {
            bail!("input file is empty — write content into {primary_input_name} before running");
        }
        eprintln!("[canon-mini-agent] role={role} input loaded ({} bytes)", primary_input.len());

        let endpoint = find_endpoint(&endpoints, role)?.clone();
        eprintln!("[canon-mini-agent] endpoint id={} url={}", endpoint.id, endpoint.pick_url(0));

        let initial_prompt = if is_verifier {
            let invariants = std::fs::read_to_string(workspace.join(INVARIANTS_FILE)).unwrap_or_default();
            let objectives = std::fs::read_to_string(workspace.join(OBJECTIVES_FILE)).unwrap_or_default();
            format!(
                "WORKSPACE: {WORKSPACE}\nAll relative paths resolve against WORKSPACE.\n\nCanonical spec (from {SPEC_FILE}):\n{primary_input}\n\nObjectives (from {OBJECTIVES_FILE}):\n{objectives}\n\nInvariants (from {INVARIANTS_FILE}):\n{invariants}\n\nVerify that objectives in {OBJECTIVES_FILE} are completed properly.\nWrite violations to {VIOLATIONS_FILE} if any are found.\nWhen complete, report verified/unverified/false items in `message.payload`.\nEmit exactly one action to begin."
            )
        } else if is_diagnostics {
            let violations = std::fs::read_to_string(&violations_path).unwrap_or_default();
            let objectives = std::fs::read_to_string(workspace.join(OBJECTIVES_FILE)).unwrap_or_default();
            format!(
                "WORKSPACE: {WORKSPACE}\nAll relative paths resolve against WORKSPACE.\n\nAlways inspect state/event_log/event.tlog.d and the relevant canon system files.\nRead files and search the source code for the bugs (use read_file + run_command/ripgrep).\nRun 5+ python analysis actions over event logs and code evidence.\nInfer the root cause from the evidence and cite detailed sources of errors (file paths, functions, and log evidence).\nPrioritize canon-route, canon-loop, canon-runtime, canon-semantic-state, and canon-mini-agent when control flow or prompt contracts are implicated.\nLatest verifier summary:\n(none yet)\n\nViolations (from {VIOLATIONS_FILE}):\n{violations}\n\nObjectives (from {OBJECTIVES_FILE}):\n{objectives}\n\nVerify whether objectives in {OBJECTIVES_FILE} are being met and note gaps.\nUse {SPEC_FILE}, {OBJECTIVES_FILE}, and {INVARIANTS_FILE} as the contract, not lane plans.\nInfer failures from code, logs, runtime state, and verifier findings.\nCanonical law:\n- SemanticStateSummary is the single source of truth for routing.\n- scheduler_len / planned_pending are not routing authority.\nFocus on route/control-flow correctness, event successor discharge, duplicate fanout, state-authority drift, queue-driven routing, synthetic dispatch bypasses, and prompt-shell mismatches.\n\nWrite a ranked diagnostics report to {diagnostics_rel}. Emit exactly one action to begin."
            )
        } else if is_planner {
            let violations = std::fs::read_to_string(&violations_path).unwrap_or_default();
            let diagnostics = std::fs::read_to_string(&diagnostics_path).unwrap_or_default();
            let objectives = std::fs::read_to_string(workspace.join(OBJECTIVES_FILE)).unwrap_or_default();
            let invariants = std::fs::read_to_string(workspace.join(INVARIANTS_FILE)).unwrap_or_default();
            let lane_plan_list = lanes
                .iter()
                .map(|lane| lane.plan_file.as_str())
                .collect::<Vec<_>>()
                .join(", ");
            format!(
                "WORKSPACE: {WORKSPACE}\nAll relative paths resolve against WORKSPACE.\n\nCanonical spec (from {SPEC_FILE}):\n{primary_input}\n\nObjectives (from {OBJECTIVES_FILE}):\n{objectives}\n\nInvariants (from {INVARIANTS_FILE}):\n{invariants}\n\nViolations (from {VIOLATIONS_FILE}):\n{violations}\n\nDiagnostics report (from {diagnostics_rel}):\n{diagnostics}\n\nCanonical law:\n- SemanticStateSummary is the single source of truth for routing.\n- scheduler_len / planned_pending are not routing authority.\n- Prioritize migration to state-authority before edge patches.\n\nUse {INVARIANTS_FILE} when deriving plan constraints.\nRead files and search the source code before issuing plan changes.\nWrite imperative, actionable instructions in {MASTER_PLAN_FILE} and derive lane plans: {lane_plan_list}.\nEmit exactly one action to begin."
            )
        } else {
            let spec = std::fs::read_to_string(&spec_path).with_context(|| format!("failed to read {SPEC_FILE}"))?;
            let master_plan = std::fs::read_to_string(&master_plan_path).unwrap_or_default();
            let violations = std::fs::read_to_string(&violations_path).unwrap_or_default();
            let diagnostics = std::fs::read_to_string(&diagnostics_path).unwrap_or_default();
            let invariants = std::fs::read_to_string(workspace.join(INVARIANTS_FILE)).unwrap_or_default();
            format!(
                "WORKSPACE: {WORKSPACE}\nAll relative paths resolve against WORKSPACE.\n\nCanonical spec (from {SPEC_FILE}):\n{spec}\n\nMaster plan (from {MASTER_PLAN_FILE}):\n{master_plan}\n\nViolations (from {VIOLATIONS_FILE}):\n{violations}\n\nDiagnostics (from {diagnostics_rel}):\n{diagnostics}\n\nInvariants (from {INVARIANTS_FILE}):\n{invariants}\n\nAssigned lane plan (from {primary_input_name}):\n{primary_input}\n\nDo not modify spec, plan, lane plans, violations, or diagnostics. Use `message.payload` to report evidence for verifier review. Emit exactly one action to begin."
            )
        };

        let submit_only = role == "executor";
        let reason = run_agent(
            role,
            if is_verifier { "verifier" } else if is_diagnostics { "diagnostics" } else if is_planner { "planner" } else { "executor" },
            &instructions,
            initial_prompt,
            if role == "executor" { &lanes[0].endpoint } else { &endpoint },
            &bridge,
            &workspace,
            &tabs,
            submit_only,
            false,
            true,
        ).await?;
        println!("message: {reason}");
        Ok(())
    }
}
