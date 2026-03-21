use anyhow::{anyhow, Result};
use canon_act::ActConsumer;
use canon_capability::CapabilityExecutionContext;
use canon_decision::{JournalLine, RouteKind};
use canon_event::{CanonEvent, CapabilityRequested, EventEmitter, EventEmitterHandle, LoopActed, LoopObserved, LoopPlanned, LoopRewarded, LoopVerified, ToolCall, ToolResult, EVENT_SCHEMA_VERSION};
use canon_event_store::replay_graph_from_tlog;
use canon_event_store::AnyEvent;
use canon_event_store::{extract_rustc_event, read_any_events_from_path, read_any_events_from_path_with_start_seq};
use canon_goal::{parse_agent_goal_markdown, summarize_goal, GoalSpec};
use canon_judgment::{GuardConfig, RuntimeSignals};
use canon_observe::ObserveConsumer;
use canon_plan::PlanConsumer;
use canon_reward::RewardConsumer;
use canon_runtime::bootstrap::{bootstrap_config, new_prompt_registry, prompts_dir, reload_prompt_file};
use canon_runtime::consumers::capability_executor::CapabilityExecutor;
use canon_runtime::consumers::error_logger::ErrorLogger;
use canon_runtime::consumers::llm_executor::LlmCapabilityHandler;
use canon_runtime::{register_default_capabilities, spawn_kernel_processor, EventRuntime, KernelMsg};
use canon_runtime_supervisor::judgment_loop::RouteController;
use canon_verify::VerifyConsumer;
use crossbeam_channel as cc;
use notify::{Config as NotifyConfig, RecommendedWatcher, RecursiveMode, Watcher};
use std::collections::HashSet;
use std::env;
use std::fs::{self, File, OpenOptions};
use std::io::{Read, Write};
use std::path::{Path, PathBuf};
use std::sync::Arc;
use std::time::{Duration, Instant, SystemTime, UNIX_EPOCH};
use uuid::Uuid;

// ---------------------------------------------------------------------------
// Lock guard — ensures only one instance runs against a given tlog path.
// ---------------------------------------------------------------------------

struct LockGuard {
    path: PathBuf,
    _file: File,
}

impl Drop for LockGuard {
    fn drop(&mut self) {
        let _ = fs::remove_file(&self.path);
    }
}

fn parse_pid(lock_contents: &str) -> Option<u32> {
    lock_contents.lines().find_map(|line| line.strip_prefix("pid=")).and_then(|value| value.trim().parse::<u32>().ok())
}

fn pid_is_alive(pid: u32) -> Result<bool> {
    let stat_path = PathBuf::from("/proc").join(pid.to_string()).join("stat");
    let mut file = match File::open(&stat_path) {
        Ok(file) => file,
        Err(err) if err.kind() == std::io::ErrorKind::NotFound => return Ok(false),
        Err(err) => return Err(err.into()),
    };
    let mut contents = String::new();
    file.read_to_string(&mut contents)?;
    let close_paren = match contents.rfind(')') {
        Some(idx) => idx,
        None => return Ok(true),
    };
    let state = contents[close_paren + 1..].trim_start().chars().next().unwrap_or(' ');
    Ok(state != 'Z')
}

fn acquire_lock(path: &Path) -> Result<Option<LockGuard>> {
    match OpenOptions::new().write(true).create_new(true).open(path) {
        Ok(mut file) => {
            let _ = file.write_all(format!("pid={}\n", std::process::id()).as_bytes());
            return Ok(Some(LockGuard { path: path.to_path_buf(), _file: file }));
        }
        Err(err) if err.kind() == std::io::ErrorKind::AlreadyExists => {}
        Err(err) => return Err(err.into()),
    }

    let mut contents = String::new();
    if let Ok(mut file) = File::open(path) {
        let _ = file.read_to_string(&mut contents);
    }
    let Some(pid) = parse_pid(&contents) else {
        eprintln!("[event_runtime] another instance is running (lock: {})", path.display());
        return Ok(None);
    };
    let alive = pid_is_alive(pid)?;
    if alive {
        eprintln!("[event_runtime] another instance is running (lock: {})", path.display());
        return Ok(None);
    }

    let _ = fs::remove_file(path);
    let mut file = OpenOptions::new().write(true).create_new(true).open(path)?;
    let _ = file.write_all(format!("pid={}\n", std::process::id()).as_bytes());
    Ok(Some(LockGuard { path: path.to_path_buf(), _file: file }))
}

// ---------------------------------------------------------------------------
// Queue message type — P → Q_e / Q_c
//
// Producers push EventMsg into Q_e and ControlMsg into Q_c.
// W=1 (the main loop) is the sole receiver of both; it defines commit order and is
// the only writer to L (log/tlog).  Consumers (C ≥ 1) are driven from W
// via bus dispatch and track their own offsets; they never mutate L.
// ---------------------------------------------------------------------------

enum EventMsg {
    /// New inbound event delivered directly in memory — no filesystem poll.
    /// Produced by the notify-watcher thread (P2) and the bootstrap replayer (P1).
    Event(AnyEvent),
    /// Tlog was truncated/recreated (observed by P2).
    /// W must reset its state and replay the provided events from scratch
    /// to maintain deterministic order (Rule 10).
    Reset(Vec<AnyEvent>),
}

enum ControlMsg {
    /// Periodic housekeeping tick from the timer producer (P3).
    Tick,
}

fn is_kernel_canon_event(event: &AnyEvent) -> bool {
    if let AnyEvent::Canon(canon) = event {
        extract_rustc_event(canon).is_some()
    } else {
        false
    }
}

#[derive(Default)]
struct RouteRuntimeState {
    scheduler_tick: u64,
    mission_raw: String,
    mission_summary: String,
    mission_goal_spec: Option<GoalSpec>,
    context_ready: bool,
    workspace_dirty: bool,
    planned_pending: usize,
    acted_unverified: bool,
    last_action_failed: bool,
    pending_tool_result_ids: HashSet<String>,
    latest_tool_result: Option<serde_json::Value>,
    finish_ready: bool,
    last_action_kind: String,
    journal: Vec<JournalLine>,
}

struct DirectEventEmitter {
    tx: cc::Sender<CanonEvent>,
}

impl EventEmitter for DirectEventEmitter {
    fn emit(&self, event: CanonEvent) {
        let _ = self.tx.send(event);
    }
}

impl RouteRuntimeState {
    fn signals(&self) -> RuntimeSignals {
        RuntimeSignals {
            context_ready: self.context_ready,
            has_queued_plan: self.planned_pending > 0,
            workspace_dirty: self.workspace_dirty,
            performed_recently: self.acted_unverified,
            last_action_failed: self.last_action_failed,
            finish_ready: self.finish_ready,
        }
    }

    fn snapshot_text(&self) -> String {
        format!(
            "tick={tick}\ncontext_ready={context}\nworkspace_dirty={dirty}\nplanned_pending={pending}\nacted_unverified={unverified}\nfinish_ready={finish}\nlast_action_kind={action}",
            tick = self.scheduler_tick,
            context = self.context_ready,
            dirty = self.workspace_dirty,
            pending = self.planned_pending,
            unverified = self.acted_unverified,
            finish = self.finish_ready,
            action = self.last_action_kind,
        )
    }

    fn push_journal(&mut self, lane: impl Into<String>, summary: impl Into<String>) {
        self.journal.push(JournalLine { lane: lane.into(), summary: summary.into(), data: serde_json::Value::Null });
        if self.journal.len() > 32 {
            let drop_n = self.journal.len() - 32;
            self.journal.drain(0..drop_n);
        }
    }
}

fn heuristic_route_json(state: &RouteRuntimeState) -> String {
    let route = if state.finish_ready {
        RouteKind::Conclude
    } else if state.planned_pending > 0 {
        RouteKind::Execute
    } else if state.acted_unverified {
        RouteKind::Validate
    } else if state.workspace_dirty {
        RouteKind::Validate
    } else if state.context_ready {
        RouteKind::Shape
    } else {
        RouteKind::Scan
    };
    serde_json::json!({
        "route": route.as_str(),
        "rationale": "heuristic proposal from runtime state",
        "confidence": 0.75,
        "signals": {
            "context_ready": state.context_ready,
            "workspace_dirty": state.workspace_dirty,
            "planned_pending": state.planned_pending,
            "acted_unverified": state.acted_unverified,
            "finish_ready": state.finish_ready,
        }
    })
    .to_string()
}

fn request_route_via_llm_call(
    registry: &std::sync::Arc<std::sync::Mutex<canon_capability::CapabilityRegistry>>, workspace: &Path, prompt: String, timeout: Duration, last_tool_result: Option<serde_json::Value>,
) -> Result<String> {
    let request_id = format!("route-{}", Uuid::new_v4());
    let request = CapabilityRequested {
        request_id: request_id.clone(),
        name: "llm.call".to_string(),
        args: serde_json::json!({
            "prompt": prompt,
            "role": "router",
            "last_tool_result": last_tool_result,
        }),
    };
    let (tx, rx) = cc::unbounded::<CanonEvent>();
    let emitter: EventEmitterHandle = Arc::new(DirectEventEmitter { tx });
    let ctx = CapabilityExecutionContext { workspace: workspace.to_path_buf(), event: CanonEvent::CapabilityRequested(request.clone()), emitter: Some(emitter) };

    {
        let guard = registry.lock().map_err(|_| anyhow!("capability registry lock poisoned"))?;
        guard.execute("llm.call", ctx)?;
    }

    let deadline = Instant::now() + timeout;
    loop {
        let now = Instant::now();
        if now >= deadline {
            return Err(anyhow!("route llm.call timed out"));
        }
        let remaining = deadline.saturating_duration_since(now);
        let event = rx.recv_timeout(remaining).map_err(|_| anyhow!("route llm.call timed out"))?;
        match event {
            CanonEvent::CapabilityCompleted(done) if done.request_id == request_id && done.name == "llm.call" => {
                let value = done.result.get("result").cloned().unwrap_or(done.result);
                if let Some(text) = value.get("text").and_then(|v| v.as_str()) {
                    return Ok(text.to_string());
                }
                return Ok(value.to_string());
            }
            CanonEvent::CapabilityFailed(failed) if failed.request_id == request_id && failed.name == "llm.call" => {
                return Err(anyhow!("route llm.call failed: {}", failed.error));
            }
            _ => {}
        }
    }
}

fn count_loc(dir: &Path) -> usize {
    let Ok(entries) = std::fs::read_dir(dir) else {
        return 0;
    };
    let mut total = 0;
    for entry in entries.flatten() {
        let path = entry.path();
        if path.is_dir() {
            total += count_loc(&path);
        } else if path.extension().and_then(|s| s.to_str()) == Some("rs") {
            if let Ok(content) = std::fs::read_to_string(&path) {
                total += content.lines().count();
            }
        }
    }
    total
}

fn extract_loc_requirement(spec: &GoalSpec) -> usize {
    for req in &spec.requirements {
        let lower = req.to_lowercase();
        if lower.contains("loc") {
            let digits: String = req.chars().filter(|c| c.is_ascii_digit()).collect();
            if let Ok(n) = digits.parse::<usize>() {
                return n;
            }
        }
    }
    0
}

fn evaluate_goal_satisfied(spec: Option<&GoalSpec>, workspace: &Path) -> bool {
    let Some(spec) = spec else {
        return false;
    };
    let target = spec.target_path.clone().unwrap_or_else(|| workspace.join("test_rust_project_v3"));
    if !target.is_dir() {
        return false;
    }

    let readme = target.join("README.md");
    let readme_non_empty = std::fs::metadata(&readme).ok().map(|m| m.is_file() && m.len() > 0).unwrap_or(false);
    if !readme_non_empty {
        return false;
    }

    let required_loc = extract_loc_requirement(spec);
    if required_loc > 0 {
        let actual_loc = count_loc(&target);
        if actual_loc < required_loc {
            return false;
        }
    }

    std::process::Command::new("cargo").arg("build").current_dir(&target).stdout(std::process::Stdio::null()).stderr(std::process::Stdio::null()).status().map(|s| s.success()).unwrap_or(false)
}

fn update_route_runtime_state(route_state: &mut RouteRuntimeState, event: &CanonEvent, workspace: &Path) {
    match event {
        CanonEvent::LoopObserved(LoopObserved { goal_text, error_count, .. }) => {
            let goal_present = goal_text.as_ref().map(|v| !v.trim().is_empty()).unwrap_or(false);
            route_state.context_ready = goal_present || *error_count > 0;
            if let Some(goal_text) = goal_text {
                if !goal_text.trim().is_empty() {
                    route_state.mission_raw = goal_text.clone();
                    route_state.mission_summary = summarize_goal(&parse_agent_goal_markdown(goal_text));
                    route_state.mission_goal_spec = Some(parse_agent_goal_markdown(goal_text));
                }
            }
            route_state.push_journal("observe", format!("tick={} goal_present={} errors={}", route_state.scheduler_tick, goal_present, error_count));
        }
        CanonEvent::LoopPlanned(LoopPlanned { action_kind, plan_id, action_id, llm_request_id, .. }) => {
            route_state.planned_pending = route_state.planned_pending.saturating_add(1);
            let mut summary = format!("planned action={action_kind}");
            if let Some(plan_id) = plan_id {
                summary.push_str(&format!(" plan_id={plan_id}"));
            }
            if let Some(action_id) = action_id {
                summary.push_str(&format!(" action_id={action_id}"));
            }
            if let Some(llm_request_id) = llm_request_id {
                summary.push_str(&format!(" llm_request_id={llm_request_id}"));
            }
            route_state.push_journal("plan", summary);
        }
        CanonEvent::LoopActed(LoopActed { action_kind, capability_request_id, tool_call_id, tool_result_id, success, stderr, .. }) => {
            route_state.planned_pending = route_state.planned_pending.saturating_sub(1);
            route_state.acted_unverified = true;
            if stderr != "skipped:batch_aborted" {
                route_state.last_action_failed = !success;
            }
            if let Some(tool_call_id) = tool_call_id {
                if tool_result_id.is_some() {
                    route_state.pending_tool_result_ids.remove(tool_call_id);
                }
            }
            route_state.workspace_dirty = true;
            route_state.last_action_kind = action_kind.clone();
            let mut summary = format!("executed action={} success={} capability_request_id={}", route_state.last_action_kind, success, capability_request_id);
            if let Some(tool_call_id) = tool_call_id {
                summary.push_str(&format!(" tool_call_id={tool_call_id}"));
            }
            if let Some(tool_result_id) = tool_result_id {
                summary.push_str(&format!(" tool_result_id={tool_result_id}"));
            }
            route_state.push_journal("act", summary);
        }
        CanonEvent::LoopVerified(LoopVerified { passed, diagnostics, .. }) => {
            route_state.acted_unverified = false;
            route_state.workspace_dirty = false;
            let system_satisfied = evaluate_goal_satisfied(route_state.mission_goal_spec.as_ref(), workspace);
            route_state.finish_ready = *passed && system_satisfied;
            route_state.push_journal("verify", format!("passed={} system_satisfied={} diagnostics={}", passed, system_satisfied, diagnostics.join("|")));
        }
        CanonEvent::LoopRewarded(LoopRewarded { halt, .. }) => {
            if *halt {
                route_state.finish_ready = true;
            }
            route_state.push_journal("reward", format!("halt={halt}"));
        }
        CanonEvent::ToolCall(ToolCall { tool_call_id, .. }) => {
            // Track in-flight tool calls as soon as dispatch occurs.
            route_state.pending_tool_result_ids.insert(tool_call_id.clone());
            route_state.latest_tool_result = None;
        }
        CanonEvent::ToolResult(ToolResult { node_id, kind, success, request_id, tool_call_id, tool_result_id, output, .. }) => {
            route_state.pending_tool_result_ids.remove(tool_call_id);
            let mut output_text = output.to_string();
            if output_text.len() > 512 {
                output_text.truncate(512);
                output_text.push_str("...<truncated>");
            }
            route_state.latest_tool_result = Some(serde_json::json!({
                "node_id": node_id,
                "kind": kind,
                "success": success,
                "request_id": request_id,
                "tool_call_id": tool_call_id,
                "tool_result_id": tool_result_id,
                "output": output,
            }));
            route_state.push_journal("tool", format!("tool_result kind={kind} success={success} tool_call_id={tool_call_id} tool_result_id={tool_result_id} output={output_text}"));
        }
        CanonEvent::RuntimeStateUpdated(updated) => {
            let dirty = updated.payload.get("workspace_dirty").and_then(|v| v.as_bool()).unwrap_or(false);
            if dirty {
                route_state.workspace_dirty = true;
                let crate_name = updated.payload.get("crate").and_then(|v| v.as_str()).unwrap_or("unknown");
                route_state.push_journal("observe", format!("workspace_dirty=true crate={crate_name}"));
            }
        }
        _ => {}
    }
}

fn apply_observed_events(runtime: &mut EventRuntime, route_state: &mut RouteRuntimeState, workspace: &Path) -> Result<()> {
    let observed = runtime.take_observed_events();
    if observed.is_empty() {
        return Ok(());
    }
    for emitted in observed {
        update_route_runtime_state(route_state, &emitted, workspace);
    }
    Ok(())
}

fn handle_event_msg(
    msg: EventMsg, runtime: &mut EventRuntime, route_state: &mut RouteRuntimeState, workspace: &Path, processed: &mut usize, cursor_path: &Path, tlog_path: &Path, start_seq: u64, session_id: &str,
    last_saved: &mut Instant, last_saved_processed: &mut usize,
) -> Result<()> {
    match msg {
        EventMsg::Event(event) => {
            runtime.process_events(std::slice::from_ref(&event))?;
            apply_observed_events(runtime, route_state, workspace)?;
            *processed = processed.saturating_add(1);
            if *processed != *last_saved_processed
                && last_saved.elapsed() >= Duration::from_secs(1)
                && save_cursor(cursor_path, tlog_path, *processed, start_seq, session_id, runtime.next_id()).is_ok()
            {
                *last_saved = Instant::now();
                *last_saved_processed = *processed;
            }
        }
        EventMsg::Reset(events) => {
            runtime.reset();
            runtime.process_events(&events)?;
            apply_observed_events(runtime, route_state, workspace)?;
            *processed = events.len();
        }
    }
    Ok(())
}

fn drain_event_queue_with_grace(
    q_event_rx: &cc::Receiver<EventMsg>, runtime: &mut EventRuntime, route_state: &mut RouteRuntimeState, workspace: &Path, processed: &mut usize, cursor_path: &Path, tlog_path: &Path,
    start_seq: u64, session_id: &str, last_saved: &mut Instant, last_saved_processed: &mut usize, grace: Duration,
) -> Result<()> {
    while let Ok(event_msg) = q_event_rx.try_recv() {
        handle_event_msg(event_msg, runtime, route_state, workspace, processed, cursor_path, tlog_path, start_seq, session_id, last_saved, last_saved_processed)?;
    }

    let deadline = Instant::now() + grace;
    while Instant::now() < deadline {
        match q_event_rx.recv_timeout(Duration::from_millis(5)) {
            Ok(event_msg) => {
                handle_event_msg(event_msg, runtime, route_state, workspace, processed, cursor_path, tlog_path, start_seq, session_id, last_saved, last_saved_processed)?;
            }
            Err(cc::RecvTimeoutError::Timeout) => {
                if q_event_rx.is_empty() {
                    break;
                }
            }
            Err(cc::RecvTimeoutError::Disconnected) => break,
        }
    }
    Ok(())
}

fn handle_control_msg(
    msg: ControlMsg, q_event_rx: &cc::Receiver<EventMsg>, runtime: &mut EventRuntime, route_controller: &mut RouteController, route_state: &mut RouteRuntimeState,
    registry: &std::sync::Arc<std::sync::Mutex<canon_capability::CapabilityRegistry>>, workspace: &Path, processed: &mut usize, cursor_path: &Path, tlog_path: &Path, start_seq: u64, session_id: &str,
    last_saved: &mut Instant, last_saved_processed: &mut usize,
) -> Result<bool> {
    match msg {
        ControlMsg::Tick => {
            runtime.flush_emitted_events()?;
            apply_observed_events(runtime, route_state, workspace)?;

            if !route_state.pending_tool_result_ids.is_empty() {
                runtime.emit_debug_event(
                    "supervisor".to_string(),
                    "route_blocked_waiting_tool_result".to_string(),
                    serde_json::json!({
                        "tick": route_state.scheduler_tick,
                        "pending_tool_call_ids": route_state.pending_tool_result_ids.iter().cloned().collect::<Vec<_>>(),
                        "planned_pending": route_state.planned_pending,
                        "acted_unverified": route_state.acted_unverified,
                        "last_action_kind": route_state.last_action_kind,
                    }),
                )?;
                runtime.flush_emitted_events()?;
                apply_observed_events(runtime, route_state, workspace)?;
                return Ok(false);
            }

            route_state.scheduler_tick = route_state.scheduler_tick.saturating_add(1);
            runtime.emit_debug_event(
                "supervisor".to_string(),
                "signals_snapshot".to_string(),
                serde_json::json!({
                    "tick": route_state.scheduler_tick,
                    "context_ready": route_state.context_ready,
                    "planned_pending": route_state.planned_pending,
                    "has_queued_plan": route_state.planned_pending > 0,
                    "acted_unverified": route_state.acted_unverified,
                    "last_action_kind": route_state.last_action_kind,
                    "last_action_failed": route_state.last_action_failed,
                    "workspace_dirty": route_state.workspace_dirty,
                    "finish_ready": route_state.finish_ready,
                    "ltr_present": route_state.latest_tool_result.is_some(),
                    "pending_tool_count": route_state.pending_tool_result_ids.len(),
                }),
            )?;
            let snapshot = route_state.snapshot_text();
            let prompt = route_controller.build_prompt(&route_state.mission_summary, &snapshot, route_state.latest_tool_result.as_ref(), &route_state.journal);
            let model_json = match request_route_via_llm_call(registry, workspace, prompt.clone(), Duration::from_secs(90), route_state.latest_tool_result.clone()) {
                Ok(json) => json,
                Err(err) => {
                    runtime.emit_debug_event(
                        "supervisor".to_string(),
                        "route_llm_fallback".to_string(),
                        serde_json::json!({
                            "tick": route_state.scheduler_tick,
                            "error": err.to_string(),
                        }),
                    )?;
                    heuristic_route_json(route_state)
                }
            };

            drain_event_queue_with_grace(q_event_rx, runtime, route_state, workspace, processed, cursor_path, tlog_path, start_seq, session_id, last_saved, last_saved_processed, Duration::ZERO)?;
            apply_observed_events(runtime, route_state, workspace)?;
            if !route_state.pending_tool_result_ids.is_empty() {
                runtime.emit_debug_event(
                    "supervisor".to_string(),
                    "route_blocked_waiting_tool_result".to_string(),
                    serde_json::json!({
                        "tick": route_state.scheduler_tick,
                        "reason": "tools_dispatched_during_llm_call",
                        "pending_tool_call_ids": route_state.pending_tool_result_ids.iter().cloned().collect::<Vec<_>>(),
                    }),
                )?;
                runtime.flush_emitted_events()?;
                apply_observed_events(runtime, route_state, workspace)?;
                return Ok(false);
            }

            let signals = route_state.signals();
            let (selection, gate) = match route_controller.evaluate_model_output(&model_json, &signals) {
                Ok(v) => v,
                Err(err) => {
                    runtime.emit_debug_event(
                        "supervisor".to_string(),
                        "route_error".to_string(),
                        serde_json::json!({
                            "tick": route_state.scheduler_tick,
                            "error": err,
                            "fallback": "heuristic",
                        }),
                    )?;
                    let fallback_json = heuristic_route_json(route_state);
                    match route_controller.evaluate_model_output(&fallback_json, &signals) {
                        Ok(v) => v,
                        Err(fallback_err) => {
                            runtime.emit_debug_event(
                                "supervisor".to_string(),
                                "route_error".to_string(),
                                serde_json::json!({
                                    "tick": route_state.scheduler_tick,
                                    "error": fallback_err,
                                    "fallback": "failed",
                                }),
                            )?;
                            return Ok(false);
                        }
                    }
                }
            };

            let lane = gate.lane.as_str();
            runtime.emit_debug_event(
                "supervisor".to_string(),
                "route_selected".to_string(),
                serde_json::json!({
                    "tick": route_state.scheduler_tick,
                    "suggested_route": selection.route.as_str(),
                    "approved_route": lane,
                    "rationale": selection.rationale,
                    "confidence": selection.confidence,
                    "changed": gate.changed,
                    "note": gate.note,
                    "gate_rules_fired": gate.note.split("; ").filter(|s| !s.is_empty() && *s != "accepted").collect::<Vec<_>>(),
                    "ltr_present": route_state.latest_tool_result.is_some(),
                    "last_action_kind": route_state.last_action_kind,
                    "last_action_failed": route_state.last_action_failed,
                    "last_action_success": !route_state.last_action_failed,
                    "prompt": prompt,
                }),
            )?;

            if gate.should_stop {
                runtime.flush_emitted_events()?;
                apply_observed_events(runtime, route_state, workspace)?;
                let _ = save_cursor(cursor_path, tlog_path, *processed, start_seq, session_id, runtime.next_id());
                return Ok(true);
            }

            if matches!(gate.lane, RouteKind::Scan) {
                runtime.emit_tick()?;
            }
            runtime.flush_emitted_events()?;
            apply_observed_events(runtime, route_state, workspace)?;
        }
    }
    Ok(false)
}

// ---------------------------------------------------------------------------
// main
// ---------------------------------------------------------------------------

fn main() -> Result<()> {
    let args: Vec<String> = env::args().collect();
    let mut tlog_path: Option<PathBuf> = None;
    let mut once = false;
    let start_at_tail = env::var("CANON_EVENT_RUNTIME_START_AT_TAIL").ok().map(|v| v != "0" && v.to_lowercase() != "false").unwrap_or(false);
    let cursor_path = env::var("CANON_EVENT_RUNTIME_CURSOR").map(PathBuf::from).unwrap_or_else(|_| PathBuf::from("/workspace/ai_sandbox/canon/state/event_runtime.cursor.json"));

    let mut i = 1;
    while i < args.len() {
        match args[i].as_str() {
            "--tlog" => {
                i += 1;
                tlog_path = args.get(i).map(PathBuf::from);
            }
            "--once" => once = true,
            _ => {}
        }
        i += 1;
    }

    let tlog_path = tlog_path.ok_or_else(|| anyhow!("missing --tlog"))?;
    let event_execution_enabled = std::env::var("CANON_EVENT_EXECUTION").ok().map(|v| v != "0" && v.to_lowercase() != "false").unwrap_or(true);
    let lock_path = env::var("CANON_EVENT_RUNTIME_LOCK").map(PathBuf::from).unwrap_or_else(|_| PathBuf::from("/workspace/ai_sandbox/canon/state/event_runtime.lock"));
    let _lock_guard = match acquire_lock(&lock_path)? {
        Some(guard) => guard,
        None => return Ok(()),
    };
    let system_id = load_or_create_system_id();

    if std::env::var("CANON_VERIFY_TLOG_EQUIV").ok().as_deref() == Some("1") {
        let _ = maybe_verify_tlog_equivalence(&tlog_path);
    }

    // Use the cursor's start_seq to skip reading old tlog segments, but always
    // start processing from the tail — consumers rebuild from scratch on each run.
    // Goal is recovered via scan_tlog_for_goal() on first tick. This prevents
    // stale CapabilityRequested events from being re-dispatched and avoids
    // orphaned-pending deadlocks when the process was killed mid LLM call.
    let _ = (start_at_tail,);
    let mut cursor_state = load_cursor_state(&cursor_path, &tlog_path);
    let latest_tlog_session_id = find_last_runtime_started_session_id(&tlog_path);
    if let (Some(cursor), Some(tlog_session_id)) = (&cursor_state, &latest_tlog_session_id) {
        if let Some(cursor_session_id) = &cursor.session_id {
            if cursor_session_id != tlog_session_id {
                eprintln!("[event_runtime] cursor session_id mismatch; ignoring stale cursor (cursor={} tlog={})", cursor_session_id, tlog_session_id);
                cursor_state = None;
            }
        }
    }
    let start_seq: u64 = cursor_state.as_ref().map(|c| c.start_seq).unwrap_or(0);
    let resumed_next_id: u64 = cursor_state.as_ref().map(|c| c.next_id).unwrap_or(0);
    let session_id = cursor_state.as_ref().and_then(|c| c.session_id.clone()).or(latest_tlog_session_id).unwrap_or_else(|| Uuid::new_v4().to_string());
    if let Some(schema_id) = find_last_runtime_started_schema_id(&tlog_path) {
        if schema_id != EVENT_SCHEMA_VERSION {
            eprintln!("[event_runtime] unsupported schema_id in tlog: {} (runtime supports {})", schema_id, EVENT_SCHEMA_VERSION);
        }
    }

    // --- Build runtime (W owns this exclusively) ---
    let registry = std::sync::Arc::new(std::sync::Mutex::new(canon_capability::CapabilityRegistry::new()));
    let prompt_registry = new_prompt_registry();
    bootstrap_config(&tlog_path, &prompt_registry);

    let workspace = std::env::current_dir().unwrap_or_else(|_| std::path::PathBuf::from("."));
    let mut consumers: Vec<Box<dyn canon_event::EventConsumer>> = vec![
        Box::new(ObserveConsumer::new(workspace.clone(), tlog_path.clone())),
        Box::new(PlanConsumer::new(workspace.clone())),
        Box::new(ActConsumer::new(workspace.clone())),
        Box::new(VerifyConsumer::new(workspace.clone(), tlog_path.clone())),
        Box::new(RewardConsumer::new()),
        Box::new(ErrorLogger::new(None)),
    ];
    if event_execution_enabled {
        consumers.push(Box::new(CapabilityExecutor::new(registry.clone(), workspace.clone())));
    }
    let mut runtime = EventRuntime::new_with_registry(consumers, registry.clone());
    {
        let mut reg = registry.lock().expect("capability registry lock");
        register_default_capabilities(&mut reg);
        reg.register(Arc::new(LlmCapabilityHandler::new(prompt_registry.clone())));
    }
    runtime.set_execute_capabilities(false);
    // set_tlog_path tells W where to append (L).  Only W calls this; only W writes L.
    runtime.set_tlog_path(tlog_path.clone());
    runtime.set_next_id(resumed_next_id);

    // Read events that already exist in L at startup (in-memory after this point).
    let bootstrap_events: Vec<AnyEvent> = if tlog_path.exists() { read_any_events_from_path_with_start_seq(&tlog_path, start_seq).unwrap_or_default() } else { vec![] };

    // Always start at tail — never replay into consumers.
    let mut processed: usize = bootstrap_events.len();

    // --- Once mode: W processes the current snapshot of L, then exits ---
    if once {
        if !tlog_path.exists() {
            return Err(anyhow!("tlog not found: {}", tlog_path.display()));
        }
        if processed < bootstrap_events.len() {
            runtime.process_events(&bootstrap_events[processed..])?;
        }
        processed = bootstrap_events.len();
        let _ = save_cursor(&cursor_path, &tlog_path, processed, start_seq, &session_id, runtime.next_id());
        return Ok(());
    }

    // =========================================================================
    // P → Q → W=1 → L
    //
    // Q  is an unbounded MPMC crossbeam channel.
    //    Multiple producers push concurrently without blocking each other.
    //
    // W=1  is the loop below.  It is the sole receiver of Q, the sole caller
    //    of process_events/emit_tick, and the sole appender to L.
    //    Order = arrival at W (Rule 5).  Determinism = single commit path (Rule 9).
    //
    // C ≥ 1  are the EventRuntime bus consumers.  They receive events dispatched
    //    by W, track their own state (offsets), and never write L (Rule 7).
    // =========================================================================
    // Q_c: control-plane queue (ticks/routing cadence).
    // Q_e: event-plane queue (tlog events/replay/reset).
    let (q_control_tx, q_control_rx) = cc::unbounded::<ControlMsg>();
    let (q_event_tx, q_event_rx) = cc::unbounded::<EventMsg>();
    let (q_kernel_tx, q_kernel_rx) = cc::unbounded::<KernelMsg>();
    let event_budget_per_cycle = std::env::var("CANON_EVENT_RUNTIME_EVENT_BUDGET").ok().and_then(|v| v.parse::<usize>().ok()).filter(|v| *v > 0).unwrap_or(256);

    let kernel_emitter = runtime.emitter_handle();
    let _kernel_processor = spawn_kernel_processor(q_kernel_rx, kernel_emitter);

    // --- P1: bootstrap replayer ---
    // Unprocessed events already in memory — push directly into Q, no file re-read.
    for event in bootstrap_events.into_iter().skip(processed) {
        if is_kernel_canon_event(&event) {
            q_kernel_tx.send(KernelMsg::Event(event)).ok();
        } else {
            q_event_tx.send(EventMsg::Event(event)).ok();
        }
    }

    // --- P2: notify watcher ---
    // Uses OS-level inotify/kqueue to detect tlog changes.
    // On notification: reads new entries into memory, delivers each as EventMsg::Event
    // directly into Q.  Zero polling, zero sleep — events arrive in real time.
    // File is L (durable log); Q is the live in-memory delivery pipe.
    {
        let watcher_tlog = tlog_path.clone();
        let watcher_tx = q_event_tx.clone();
        let kernel_tx = q_kernel_tx.clone();
        let mut watcher_start_seq = start_seq;
        // watcher_seen tracks how many events from L this producer has already forwarded.
        let mut watcher_seen: usize = processed;

        let (fs_tx, fs_rx) = cc::unbounded::<notify::Result<notify::Event>>();
        let mut fs_watcher = RecommendedWatcher::new(
            move |res| {
                let _ = fs_tx.send(res);
            },
            NotifyConfig::default(),
        )?;

        // For a segmented binary tlog (dir) watch the dir itself; otherwise watch
        // the parent so we also catch the file being created for the first time.
        let watch_target = if watcher_tlog.is_dir() { watcher_tlog.clone() } else { watcher_tlog.parent().map(|p| p.to_path_buf()).unwrap_or_else(|| PathBuf::from(".")) };
        if watch_target.exists() {
            fs_watcher.watch(&watch_target, RecursiveMode::NonRecursive)?;
        }

        std::thread::Builder::new().name("canon-p2-watcher".to_string()).spawn(move || {
            let _watcher = fs_watcher; // keep alive for thread lifetime
            while let Ok(res) = fs_rx.recv() {
                if res.is_err() {
                    continue;
                }
                let all = match read_any_events_from_path_with_start_seq(&watcher_tlog, watcher_start_seq) {
                    Ok(e) => e,
                    Err(_) => continue,
                };
                if all.len() < watcher_seen {
                    // L was truncated or recreated — W must reset (Rule 10).
                    watcher_seen = 0;
                    if kernel_tx.send(KernelMsg::Reset).is_err() {
                        break;
                    }
                    let mut non_kernel = Vec::new();
                    for event in all {
                        if is_kernel_canon_event(&event) {
                            if kernel_tx.send(KernelMsg::Event(event)).is_err() {
                                break;
                            }
                        } else {
                            non_kernel.push(event);
                        }
                    }
                    if watcher_tx.send(EventMsg::Reset(non_kernel)).is_err() {
                        break;
                    }
                } else {
                    // Deliver only the new suffix: each event enters Q individually
                    // so W can interleave other message types between them.
                    for event in all.into_iter().skip(watcher_seen) {
                        watcher_seen += 1;
                        if is_kernel_canon_event(&event) {
                            if kernel_tx.send(KernelMsg::Event(event)).is_err() {
                                break;
                            }
                        } else if watcher_tx.send(EventMsg::Event(event)).is_err() {
                            break;
                        }
                    }
                    // Advance start_seq to the latest segment so future reads
                    // only scan the current segment instead of the full history.
                    if let Some(latest_seq) = latest_segment_seq(&watcher_tlog) {
                        if latest_seq > watcher_start_seq {
                            let from_latest = read_any_events_from_path_with_start_seq(&watcher_tlog, latest_seq).unwrap_or_default().len();
                            watcher_start_seq = latest_seq;
                            watcher_seen = from_latest;
                        }
                    }
                }
            }
        })?;
    }

    // --- P3: tick timer ---
    // A lightweight background producer that sends a housekeeping Tick into Q
    // every second.  W dispatches emit_tick(); consumers never see this as a
    // log entry (Tick is not appended to L).
    {
        let tick_tx = q_control_tx.clone();
        std::thread::Builder::new().name("canon-p3-tick".to_string()).spawn(move || loop {
            std::thread::sleep(Duration::from_secs(1));
            if tick_tx.send(ControlMsg::Tick).is_err() {
                break;
            }
        })?;
    }

    // --- P4: prompt-directory watcher ---
    // Watches canon-agent-prompts/ for .md file changes. On change: re-reads
    // the file, updates the in-memory PromptRegistry (so LlmCapabilityHandler
    // picks up the new content immediately), and writes a prompt_loaded event
    // directly to the tlog. P2 then delivers it as EventMsg::Event to W, which
    // dispatches CanonEvent::PromptLoaded to all consumers (including ObserveConsumer).
    {
        let prompts_path = PathBuf::from(prompts_dir());
        let tlog_for_prompts = tlog_path.clone();
        let registry_for_prompts = prompt_registry.clone();

        if prompts_path.exists() {
            let (prompt_fs_tx, prompt_fs_rx) = cc::unbounded::<notify::Result<notify::Event>>();
            let mut prompt_watcher = RecommendedWatcher::new(
                move |res| {
                    let _ = prompt_fs_tx.send(res);
                },
                NotifyConfig::default(),
            )?;
            prompt_watcher.watch(&prompts_path, RecursiveMode::NonRecursive)?;

            std::thread::Builder::new().name("canon-p4-prompts".to_string()).spawn(move || {
                let _watcher = prompt_watcher;
                let mut last_reload: std::collections::HashMap<PathBuf, Instant> = std::collections::HashMap::new();
                while let Ok(Ok(event)) = prompt_fs_rx.recv() {
                    for path in &event.paths {
                        // Debounce: skip if same file reloaded within 500ms
                        let now = Instant::now();
                        if last_reload.get(path).map_or(false, |t| now.duration_since(*t) < Duration::from_millis(500)) {
                            continue;
                        }
                        last_reload.insert(path.clone(), now);
                        reload_prompt_file(path, &tlog_for_prompts, &registry_for_prompts);
                    }
                }
            })?;
        }
    }

    // Emit runtime_started so watch_log.py and the tlog show when a new
    // process begins. Written after P2 watcher_seen is fixed so P2 delivers it.
    runtime.emit_debug_event(
        "event-runtime".to_string(),
        "runtime_started".to_string(),
        serde_json::json!({
            "pid": std::process::id(),
            "tlog": tlog_path.display().to_string(),
            "event_stream_id": tlog_path.display().to_string(),
            "session_id": session_id.clone(),
            "schema_id": EVENT_SCHEMA_VERSION,
            "build_id": env!("CANON_BUILD_ID"),
            "commit_id": env!("CANON_COMMIT_ID"),
            "system_id": system_id,
        }),
    )?;
    if env!("CANON_COMMIT_ID").starts_with("unknown") {
        eprintln!("[event_runtime] warning: CANON_COMMIT_ID is unknown; build metadata is incomplete");
    }
    let mut route_controller = RouteController::new(GuardConfig::default());
    let mut route_state = RouteRuntimeState::default();
    route_state.mission_raw = std::fs::read_to_string("/workspace/ai_sandbox/canon/canon-agent-prompts/AGENT_GOAL.md").unwrap_or_default();
    let initial_spec = parse_agent_goal_markdown(&route_state.mission_raw);
    route_state.mission_summary = summarize_goal(&initial_spec);
    route_state.mission_goal_spec = Some(initial_spec);
    apply_observed_events(&mut runtime, &mut route_state, &workspace)?;

    // =========================================================================
    // W = 1 — single writer loop
    //
    // Interleaved schedule with guarantees:
    // 1) Process at most one control message each cycle (Q_c).
    // 2) Then process at most N event messages (Q_e).
    //
    // This bounds control latency under bursty event load while preserving
    // eventual event convergence.
    // =========================================================================
    let mut last_saved = Instant::now();
    let mut last_saved_processed = processed;
    let pre_control_grace_ms = std::env::var("CANON_PRE_CONTROL_EVENT_GRACE_MS").ok().and_then(|v| v.parse::<u64>().ok()).unwrap_or(50);
    let pre_control_grace = Duration::from_millis(pre_control_grace_ms);

    loop {
        // Step 1: process one control message when available.
        let mut processed_control = false;
        match q_control_rx.try_recv() {
            Ok(control_msg) => {
                processed_control = true;
                drain_event_queue_with_grace(
                    &q_event_rx,
                    &mut runtime,
                    &mut route_state,
                    &workspace,
                    &mut processed,
                    &cursor_path,
                    &tlog_path,
                    start_seq,
                    &session_id,
                    &mut last_saved,
                    &mut last_saved_processed,
                    pre_control_grace,
                )?;
                if handle_control_msg(
                    control_msg,
                    &q_event_rx,
                    &mut runtime,
                    &mut route_controller,
                    &mut route_state,
                    &registry,
                    &workspace,
                    &mut processed,
                    &cursor_path,
                    &tlog_path,
                    start_seq,
                    &session_id,
                    &mut last_saved,
                    &mut last_saved_processed,
                )? {
                    return Ok(());
                }
            }
            Err(cc::TryRecvError::Empty) => {
                // If no control is immediately ready, block for one item from either
                // queue so we don't spin when idle.
                cc::select! {
                    recv(q_control_rx) -> msg => {
                        if let Ok(control_msg) = msg {
                            processed_control = true;
                            drain_event_queue_with_grace(
                                &q_event_rx,
                                &mut runtime,
                                &mut route_state,
                                &workspace,
                                &mut processed,
                                &cursor_path,
                                &tlog_path,
                                start_seq,
                                &session_id,
                                &mut last_saved,
                                &mut last_saved_processed,
                                pre_control_grace,
                            )?;
                            if handle_control_msg(
                                control_msg,
                                &q_event_rx,
                                &mut runtime,
                                &mut route_controller,
                                &mut route_state,
                                &registry,
                                &workspace,
                                &mut processed,
                                &cursor_path,
                                &tlog_path,
                                start_seq,
                                &session_id,
                                &mut last_saved,
                                &mut last_saved_processed,
                            )? {
                                return Ok(());
                            }
                        }
                    }
                    recv(q_event_rx) -> msg => {
                        if let Ok(event_msg) = msg {
                            handle_event_msg(
                                event_msg,
                                &mut runtime,
                                &mut route_state,
                                &workspace,
                                &mut processed,
                                &cursor_path,
                                &tlog_path,
                                start_seq,
                                &session_id,
                                &mut last_saved,
                                &mut last_saved_processed,
                            )?;
                        }
                    }
                }
            }
            Err(cc::TryRecvError::Disconnected) => {}
        }

        // Step 2: bounded event processing.
        let mut handled_events = 0usize;
        while handled_events < event_budget_per_cycle {
            match q_event_rx.try_recv() {
                Ok(event_msg) => {
                    handle_event_msg(
                        event_msg,
                        &mut runtime,
                        &mut route_state,
                        &workspace,
                        &mut processed,
                        &cursor_path,
                        &tlog_path,
                        start_seq,
                        &session_id,
                        &mut last_saved,
                        &mut last_saved_processed,
                    )?;
                    handled_events = handled_events.saturating_add(1);
                }
                Err(cc::TryRecvError::Empty) => break,
                Err(cc::TryRecvError::Disconnected) => break,
            }
        }

        // If we did not process control and had no event work, block on control
        // so routing cadence remains responsive.
        if !processed_control && handled_events == 0 {
            if let Ok(control_msg) = q_control_rx.recv() {
                drain_event_queue_with_grace(
                    &q_event_rx,
                    &mut runtime,
                    &mut route_state,
                    &workspace,
                    &mut processed,
                    &cursor_path,
                    &tlog_path,
                    start_seq,
                    &session_id,
                    &mut last_saved,
                    &mut last_saved_processed,
                    pre_control_grace,
                )?;
                if handle_control_msg(
                    control_msg,
                    &q_event_rx,
                    &mut runtime,
                    &mut route_controller,
                    &mut route_state,
                    &registry,
                    &workspace,
                    &mut processed,
                    &cursor_path,
                    &tlog_path,
                    start_seq,
                    &session_id,
                    &mut last_saved,
                    &mut last_saved_processed,
                )? {
                    return Ok(());
                }
            }
        }
    }
}

// ---------------------------------------------------------------------------
// Cursor helpers — track W's read offset into L for crash recovery / replay.
// ---------------------------------------------------------------------------

/// Returns the base sequence number of the latest `.log` segment in `dir`.
/// Used by the P2 watcher to advance `watcher_start_seq` after each batch so
/// that subsequent reads only scan the current segment rather than full history.
fn latest_segment_seq(dir: &Path) -> Option<u64> {
    std::fs::read_dir(dir)
        .ok()?
        .filter_map(|e| e.ok())
        .filter_map(|e| {
            let p = e.path();
            if p.extension().and_then(|s| s.to_str()) != Some("log") {
                return None;
            }
            p.file_stem().and_then(|s| s.to_str()).and_then(|s| s.parse::<u64>().ok())
        })
        .max()
}

struct CursorState {
    start_seq: u64,
    session_id: Option<String>,
    next_id: u64,
}

fn load_cursor_state(path: &Path, tlog_path: &Path) -> Option<CursorState> {
    let text = std::fs::read_to_string(path).ok()?;
    let value: serde_json::Value = serde_json::from_str(&text).ok()?;
    let stored_path = value.get("tlog_path")?.as_str()?;
    if stored_path != tlog_path.display().to_string() {
        return None;
    }
    Some(CursorState {
        start_seq: value.get("start_seq")?.as_u64()?,
        session_id: value.get("session_id").and_then(|v| v.as_str()).map(|s| s.to_string()),
        next_id: value.get("next_id").and_then(|v| v.as_u64()).unwrap_or(0),
    })
}

fn save_cursor(path: &Path, tlog_path: &Path, state_version: usize, start_seq: u64, session_id: &str, next_id: u64) -> Result<()> {
    let state = serde_json::json!({
        "tlog_path": tlog_path.display().to_string(),
        "state_version": state_version,
        "processed": state_version,
        "start_seq": start_seq,
        "session_id": session_id,
        "next_id": next_id,
        "updated_ms": now_ms(),
    });
    let tmp_path = path.with_extension("tmp");
    if let Some(parent) = path.parent() {
        std::fs::create_dir_all(parent)?;
    }
    std::fs::write(&tmp_path, serde_json::to_string(&state)?)?;
    std::fs::rename(&tmp_path, path)?;
    Ok(())
}

fn load_or_create_system_id() -> String {
    if let Ok(system_id) = std::env::var("CANON_SYSTEM_ID") {
        if !system_id.trim().is_empty() {
            return system_id;
        }
    }
    let path = system_id_path();
    if let Ok(existing) = std::fs::read_to_string(&path) {
        let trimmed = existing.trim();
        if !trimmed.is_empty() {
            return trimmed.to_string();
        }
    }
    let system_id = Uuid::new_v4().to_string();
    if let Some(parent) = path.parent() {
        let _ = std::fs::create_dir_all(parent);
    }
    let _ = std::fs::write(&path, &system_id);
    system_id
}

fn system_id_path() -> PathBuf {
    if let Ok(path) = std::env::var("CANON_SYSTEM_ID_PATH") {
        let trimmed = path.trim();
        if !trimmed.is_empty() {
            return PathBuf::from(trimmed);
        }
    }
    if let Ok(workspace) = std::env::var("CANON_WORKSPACE") {
        let trimmed = workspace.trim();
        if !trimmed.is_empty() {
            return PathBuf::from(trimmed).join("state").join("system_id");
        }
    }
    PathBuf::from("/workspace/ai_sandbox/canon/state/system_id")
}

fn find_last_runtime_started_session_id(tlog_path: &Path) -> Option<String> {
    find_last_runtime_started_payload(tlog_path).and_then(|payload| payload.get("session_id").and_then(|v| v.as_str()).map(|s| s.to_string()))
}

fn find_last_runtime_started_schema_id(tlog_path: &Path) -> Option<String> {
    find_last_runtime_started_payload(tlog_path).and_then(|payload| payload.get("schema_id").and_then(|v| v.as_str()).map(|s| s.to_string()))
}

fn find_last_runtime_started_payload(tlog_path: &Path) -> Option<serde_json::Value> {
    let events = read_any_events_from_path(tlog_path).ok()?;
    events.into_iter().rev().find_map(|event| {
        let AnyEvent::Canon(canon) = event else {
            return None;
        };
        if canon.kind != "runtime_started" {
            return None;
        }
        Some(canon.payload)
    })
}

fn now_ms() -> u128 {
    SystemTime::now().duration_since(UNIX_EPOCH).unwrap_or_default().as_millis()
}

// ---------------------------------------------------------------------------
// Tlog equivalence verification (optional, debug mode only)
// ---------------------------------------------------------------------------

fn maybe_verify_tlog_equivalence(tlog_path: &Path) -> Result<()> {
    let (json_path, bin_path) = if tlog_path.is_dir() { (tlog_path.with_extension("tlog"), tlog_path.to_path_buf()) } else { (tlog_path.to_path_buf(), tlog_path.with_extension("tlog.d")) };
    if !json_path.exists() || !bin_path.exists() {
        return Ok(());
    }
    let _diffs = verify_tlog_equivalence(json_path.as_path(), bin_path.as_path())?;
    Ok(())
}

fn verify_tlog_equivalence(json_path: &Path, bin_path: &Path) -> Result<Vec<String>> {
    let json_graph = replay_graph_from_tlog(json_path)?;
    let bin_graph = replay_graph_from_tlog(bin_path)?;
    let mut diffs = Vec::new();
    if json_graph.nodes.len() != bin_graph.nodes.len() {
        diffs.push(format!("node_count json={} binary={}", json_graph.nodes.len(), bin_graph.nodes.len()));
    }
    if json_graph.edges.len() != bin_graph.edges.len() {
        diffs.push(format!("edge_count json={} binary={}", json_graph.edges.len(), bin_graph.edges.len()));
    }
    let json_nodes: HashSet<(u32, String, String, Option<u32>, Option<u32>)> = json_graph.nodes.iter().map(|n| (n.id, n.kind.clone(), n.symbol.clone(), n.file_id, n.line)).collect();
    let bin_nodes: HashSet<(u32, String, String, Option<u32>, Option<u32>)> = bin_graph.nodes.iter().map(|n| (n.id, n.kind.clone(), n.symbol.clone(), n.file_id, n.line)).collect();
    if json_nodes != bin_nodes {
        diffs.push(format!("node_set mismatch: json_only={} binary_only={}", json_nodes.difference(&bin_nodes).count(), bin_nodes.difference(&json_nodes).count()));
    }
    let json_edges: HashSet<(u32, u32, String)> = json_graph.edges.iter().map(|e| (e.src, e.dst, e.kind.clone())).collect();
    let bin_edges: HashSet<(u32, u32, String)> = bin_graph.edges.iter().map(|e| (e.src, e.dst, e.kind.clone())).collect();
    if json_edges != bin_edges {
        diffs.push(format!("edge_set mismatch: json_only={} binary_only={}", json_edges.difference(&bin_edges).count(), bin_edges.difference(&json_edges).count()));
    }
    Ok(diffs)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn route_state_transitions_after_loop_events() {
        let mut state = RouteRuntimeState::default();
        let workspace = PathBuf::from("/tmp");

        update_route_runtime_state(
            &mut state,
            &CanonEvent::LoopObserved(LoopObserved { tick: 1, error_count: 1, warning_count: 0, compiler_errors: vec![], goal_text: Some("# Agent Goal\n- Project path: `/tmp/nope`\n".to_string()) }),
            &workspace,
        );
        assert!(state.context_ready);

        update_route_runtime_state(
            &mut state,
            &CanonEvent::LoopPlanned(LoopPlanned {
                tick: 1,
                action_kind: "run_command".to_string(),
                action_payload: serde_json::json!({}),
                reason: "test".to_string(),
                llm_request_id: Some("llm-1".to_string()),
                trace_id: None,
                execution_id: None,
                span_id: None,
                parent_span_id: None,
                plan_id: Some("plan-1".to_string()),
                plan_step_id: None,
                action_id: Some("action-1".to_string()),
            }),
            &workspace,
        );
        assert_eq!(state.planned_pending, 1);

        update_route_runtime_state(
            &mut state,
            &CanonEvent::LoopActed(LoopActed {
                tick: 1,
                action_kind: "run_command".to_string(),
                capability_request_id: "cap-1".to_string(),
                tool_call_id: Some("tool-call-1".to_string()),
                tool_result_id: Some("tool-result-1".to_string()),
                stdout: String::new(),
                stderr: String::new(),
                exit_code: Some(0),
                duration_ms: 10,
                success: true,
                trace_id: None,
                execution_id: None,
                span_id: None,
                parent_span_id: None,
                plan_id: Some("plan-1".to_string()),
                plan_step_id: None,
                action_id: Some("action-1".to_string()),
            }),
            &workspace,
        );
        assert_eq!(state.planned_pending, 0);
        assert!(state.acted_unverified);
        assert_eq!(state.last_action_kind, "run_command");

        update_route_runtime_state(
            &mut state,
            &CanonEvent::LoopVerified(LoopVerified {
                tick: 1,
                passed: true,
                compiler_clean: true,
                tlog_clean: true,
                error_count: 0,
                diagnostics: vec!["ok".to_string()],
                trace_id: None,
                execution_id: None,
                span_id: None,
                parent_span_id: None,
            }),
            &workspace,
        );
        assert!(!state.acted_unverified);
        assert!(!state.finish_ready);
        assert!(!state.journal.is_empty());
    }

    #[test]
    fn journal_is_bounded_to_32_lines() {
        let mut state = RouteRuntimeState::default();
        for i in 0..64 {
            state.push_journal("test", format!("line-{i}"));
        }
        assert_eq!(state.journal.len(), 32);
        assert_eq!(state.journal[0].summary, "line-32");
    }
}
