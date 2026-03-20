use crate::bootstrap::PromptRegistryHandle;
use canon_llm::config::CapabilityConfig;
use canon_llm::endpoint_worker;
use canon_llm::llm;
use canon_llm::ws_server;
use canon_capability::{CapabilityExecutionContext, CapabilityExecutionResult, CapabilityHandler};
use canon_event::{new_error_occurred, CanonEvent, EventEmitterHandle, canon_emit};
use serde_json::json;
use std::sync::{Arc, OnceLock};
use std::thread;
use std::time::{Instant, SystemTime, UNIX_EPOCH};
use std::env;
use std::path::Path;

#[derive(Clone, Copy)]
struct ParseDiagnostics {
    parse_ok: bool,
    parse_mode: &'static str,
    block_count: usize,
    valid_action_count: usize,
}

struct LlmWork {
    request_id: String,
    name: String,
    prompt: String,
    role: Option<String>,
    raw: bool,
    args: serde_json::Value,
    emitter: EventEmitterHandle,
}

pub struct LlmCapabilityHandler {
    work_tx: std::sync::mpsc::Sender<LlmWork>,
}

impl LlmCapabilityHandler {
    pub fn new(_registry: PromptRegistryHandle) -> Self {
        let (work_tx, work_rx) = std::sync::mpsc::channel::<LlmWork>();

        // Shared emitter cell: populated from the first LLM job so ws_server
        // can emit bridge-level events (connection, tab lifecycle) as P→Q
        // producers independent of any single capability request.
        let ws_emitter: Arc<OnceLock<EventEmitterHandle>> = Arc::new(OnceLock::new());
        let ws_emitter_thread = Arc::clone(&ws_emitter);

        thread::Builder::new()
            .name("llm_executor_worker".to_string())
            .spawn(move || {
                let result = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
                    let runtime = match tokio::runtime::Builder::new_multi_thread()
                        .enable_all()
                        .build()
                    {
                        Ok(rt) => rt,
                        Err(_err) => {
                            return;
                        }
                    };
                    let config = match CapabilityConfig::snapshot_store_load() {
                        Ok(config) => config,
                        Err(_err) => {
                            return;
                        }
                    };
                    config.apply_env_flags();
                    let bridge_addr = env::var("CANON_LLM_BRIDGE_ADDR")
                        .unwrap_or_else(|_| "127.0.0.1:9100".to_string());
                    let addr: std::net::SocketAddr = match bridge_addr.parse() {
                        Ok(addr) => addr,
                        Err(_err) => {
                            "127.0.0.1:9100".parse().expect("fallback llm bridge addr")
                        }
                    };
                    let bridge = runtime.block_on(async {
                        ws_server::spawn(addr, config.response_timeout_secs, Arc::clone(&ws_emitter_thread))
                    });
                    runtime.block_on(async {
                        let wait = bridge.wait_for_connection();
                        let _ = tokio::time::timeout(std::time::Duration::from_secs(5), wait).await;
                    });
                    let tabs = endpoint_worker::llm_worker_new_tabs();
                    runtime.block_on(endpoint_worker::llm_worker_init_workers(
                        &bridge,
                        &config,
                        &tabs,
                    ));
                    let llm_log_dir = env::var("CANON_LLM_LOG_DIR")
                        .unwrap_or_else(|_| "/workspace/ai_sandbox/canon/canon-utils/state/reports_out/llm".to_string());
                    let _ = std::fs::create_dir_all(&llm_log_dir);
                    let mut llm_call_counter: u32 = next_llm_call_counter(&llm_log_dir);
                    for job in work_rx.iter() {
                        let LlmWork { request_id, name, prompt, role, raw, args, emitter } = job;
                        // Capture emitter on first job; ws_server uses it for
                        // bridge-level events for the rest of the process lifetime.
                        let _ = ws_emitter_thread.set(emitter.clone());
                        let start = Instant::now();
                        canon_emit!(emitter; "llm_executor", "request_start",
                            json!({ "request_id": request_id, "name": name }));
                        let is_planner_role = role.as_deref() == Some("planner");
                        let bust_cache = config.planner_refine_on_cache && is_planner_role;
                        let requested_role = role.clone().unwrap_or_default();
                        let selected = if let Some(role_name) = role.as_deref() {
                            config
                                .llm_endpoints
                                .iter()
                                .find(|e| e.role.as_deref() == Some(role_name))
                                // Backward-compatible fallback: router requests can use planner.
                                .or_else(|| {
                                    if role_name == "router" {
                                        config
                                            .llm_endpoints
                                            .iter()
                                            .find(|e| e.role.as_deref() == Some("planner"))
                                    } else {
                                        None
                                    }
                                })
                        } else {
                            config.llm_endpoints.first()
                        };
                        let Some(endpoint) = selected else {
                            let error_msg = if requested_role.is_empty() {
                                "no llm endpoints configured".to_string()
                            } else {
                                format!("no llm endpoint configured for role={}", requested_role)
                            };
                            emitter.emit(CanonEvent::ErrorOccurred(new_error_occurred(
                                "llm_config",
                                "llm_executor",
                                &error_msg,
                                "error",
                                json!({
                                    "request_id": request_id.clone(),
                                    "capability": name.clone(),
                                    "role": role,
                                }),
                                Some(request_id.clone()),
                            )));
                            emitter.emit(CanonEvent::CapabilityFailed(canon_event::CapabilityFailed {
                                request_id,
                                name,
                                error: error_msg,
                            }));
                            continue;
                        };
                        let retries = config.llm_retry_count.max(1);
                        let delay = config.llm_retry_delay_secs;
                        let role_content = default_role_content(
                            role.as_deref().or(endpoint.role.as_deref()),
                            &endpoint.id,
                        );
                        let prompt_with_request_id = format!(
                            "{{\"request_id\":\"{}\"}}\n{}",
                            request_id,
                            prompt
                        );
                        canon_emit!(emitter; "llm_executor", "request_dispatch",
                            json!({
                                "request_id": request_id,
                                "endpoint": endpoint.id,
                                "url": endpoint.url
                            }));
                        let dispatched_ms = now_ms();
                        let call_n = llm_call_counter;
                        llm_call_counter += 1;
                        let req_tag = format!("{dispatched_ms}_{call_n:04}");
                        let req_path = format!("{}/{}_request.json", llm_log_dir, req_tag);
                        let res_path = format!("{}/{}_response.json", llm_log_dir, req_tag);
                        let req_obj = json!({
                            "n": call_n,
                            "request_id": request_id,
                            "capability": name,
                            "endpoint": endpoint.id,
                            "url": endpoint.url,
                            "role": role_content,
                            "prompt": prompt_with_request_id,
                            "args": args,
                            "dispatched_ms": dispatched_ms,
                            "finalized_ms": serde_json::Value::Null,
                        });
                        let _ = std::fs::write(&req_path, serde_json::to_string_pretty(&req_obj).unwrap_or_default());
                        // Ensure every request has a terminal artifact path immediately.
                        // If the process crashes mid-call, this remains as a durable marker
                        // instead of leaving a missing *_response.json file.
                        let pending_obj = json!({
                            "n": call_n,
                            "request_id": request_id,
                            "endpoint": endpoint.id,
                            "status": "pending",
                            "started_ms": now_ms(),
                        });
                        let _ = std::fs::write(&res_path, serde_json::to_string_pretty(&pending_obj).unwrap_or_default());
                        let result = runtime.block_on(async {
                            let call = async {
                                if raw {
                                    llm::llm_client_call_agent_raw_with_retry_allow_mismatch(
                                        &bridge, &endpoint.id, &endpoint.url,
                                        endpoint.stateful, &prompt_with_request_id, &role_content,
                                        "llm_executor", None, &tabs, endpoint.max_tabs,
                                        config.tab_cooldown_ms, retries, delay, bust_cache,
                                    ).await
                                } else {
                                    llm::llm_client_call_agent_json_with_retry_allow_mismatch(
                                        &bridge, &endpoint.id, &endpoint.url,
                                        endpoint.stateful, &prompt_with_request_id, &role_content,
                                        "llm_executor", None, &tabs, endpoint.max_tabs,
                                        config.tab_cooldown_ms, retries, delay, bust_cache,
                                    ).await
                                }
                            };
                            match tokio::time::timeout(
                                std::time::Duration::from_secs(config.response_timeout_secs),
                                call,
                            ).await {
                                Ok(result) => result,
                                Err(_) => Err(anyhow::anyhow!("llm call timed out")),
                            }
                        });

                        match result {
                            Ok(payload) => {
                                let finalized_ms = now_ms();
                                // E_in → L → E_out boundary: emit exactly once per LLM
                                // response arrival, before the capability result is sealed.
                                // Gives real-time visibility into: which endpoint replied,
                                // how long it took, response size, and whether JSON parsed.
                                let elapsed_ms = start.elapsed().as_millis() as u64;
                                let parse = analyze_llm_payload(&payload, is_planner_role);
                                let res_obj = json!({
                                    "n": call_n,
                                    "request_id": request_id,
                                    "endpoint": endpoint.id,
                                    "duration_ms": elapsed_ms,
                                    "parse_ok": parse.parse_ok,
                                    "parse_mode": parse.parse_mode,
                                    "parse_blocks": parse.block_count,
                                    "valid_action_count": parse.valid_action_count,
                                    "response": payload,
                                });
                                let _ = std::fs::write(&res_path, serde_json::to_string_pretty(&res_obj).unwrap_or_default());
                                let req_done = json!({
                                    "n": call_n,
                                    "request_id": request_id,
                                    "capability": name,
                                    "endpoint": endpoint.id,
                                    "url": endpoint.url,
                                    "role": role_content,
                                    "prompt": prompt_with_request_id,
                                    "args": args,
                                    "dispatched_ms": dispatched_ms,
                                    "finalized_ms": finalized_ms,
                                });
                                let _ = std::fs::write(&req_path, serde_json::to_string_pretty(&req_done).unwrap_or_default());
                                canon_emit!(emitter; "llm_executor", "llm_response",
                                    json!({
                                        "request_id": request_id,
                                        "endpoint":   endpoint.id,
                                        "duration_ms": elapsed_ms,
                                        "bytes":      payload.to_string().len(),
                                        "parse_ok":   parse.parse_ok,
                                        "parse_mode": parse.parse_mode,
                                        "parse_blocks": parse.block_count,
                                        "valid_action_count": parse.valid_action_count,
                                    }));
                                emitter.emit(CanonEvent::CapabilityCompleted(
                                    canon_event::CapabilityCompleted {
                                        request_id: request_id.clone(),
                                        name,
                                        result: json!({
                                            "status": 0,
                                            "success": true,
                                            "duration_ms": elapsed_ms,
                                            "result": payload,
                                        }),
                                    },
                                ));
                                canon_emit!(emitter; "llm_executor", "request_completed",
                                    json!({ "request_id": request_id }));
                            }
                            Err(err) => {
                                let finalized_ms = now_ms();
                                let res_obj = json!({
                                    "n": call_n,
                                    "request_id": request_id,
                                    "endpoint": endpoint.id,
                                    "error": err.to_string(),
                                });
                                let _ = std::fs::write(&res_path, serde_json::to_string_pretty(&res_obj).unwrap_or_default());
                                let req_done = json!({
                                    "n": call_n,
                                    "request_id": request_id,
                                    "capability": name,
                                    "endpoint": endpoint.id,
                                    "url": endpoint.url,
                                    "role": role_content,
                                    "prompt": prompt_with_request_id,
                                    "args": args,
                                    "dispatched_ms": dispatched_ms,
                                    "finalized_ms": finalized_ms,
                                });
                                let _ = std::fs::write(&req_path, serde_json::to_string_pretty(&req_done).unwrap_or_default());
                                emitter.emit(CanonEvent::ErrorOccurred(new_error_occurred(
                                    "llm_call",
                                    "llm_executor",
                                    err.to_string(),
                                    "error",
                                    json!({
                                        "request_id": request_id.clone(),
                                        "capability": name.clone(),
                                    }),
                                    Some(request_id.clone()),
                                )));
                                emitter.emit(CanonEvent::CapabilityFailed(
                                    canon_event::CapabilityFailed {
                                        request_id: request_id.clone(),
                                        name,
                                        error: err.to_string(),
                                    },
                                ));
                                canon_emit!(emitter; "llm_executor", "request_failed",
                                    json!({ "request_id": request_id, "error": err.to_string() }));
                            }
                        }
                    }
                }));
                let _ = result;
            })
            .expect("llm executor worker thread");

        Self { work_tx }
    }
}

fn now_ms() -> u128 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap_or_default()
        .as_millis()
}

fn analyze_llm_payload(payload: &serde_json::Value, is_planner_role: bool) -> ParseDiagnostics {
    let Some(obj) = payload.as_object() else {
        return ParseDiagnostics {
            parse_ok: true,
            parse_mode: "strict_json",
            block_count: 0,
            valid_action_count: 0,
        };
    };

    let Some(text) = (obj.len() == 1 && obj.contains_key("text"))
        .then(|| obj.get("text").and_then(|v| v.as_str()))
        .flatten()
    else {
        return ParseDiagnostics {
            parse_ok: !is_planner_role || is_valid_planner_action(payload),
            parse_mode: "strict_json",
            block_count: 0,
            valid_action_count: usize::from(is_valid_planner_action(payload)),
        };
    };

    let blocks = extract_fenced_json_blocks(text);
    if !blocks.is_empty() {
        let mode = if blocks.len() > 1 { "multi_block" } else { "fenced_block" };
        let mut parsed_count = 0usize;
        let mut valid_action_count = 0usize;
        for block in &blocks {
            if let Ok(value) = serde_json::from_str::<serde_json::Value>(block) {
                parsed_count += 1;
                if !is_planner_role || is_valid_planner_action(&value) {
                    valid_action_count += 1;
                }
            }
        }
        let parse_ok = if is_planner_role {
            parsed_count > 0 && parsed_count == valid_action_count && parsed_count == blocks.len()
        } else {
            parsed_count > 0
        };
        return ParseDiagnostics {
            parse_ok,
            parse_mode: mode,
            block_count: blocks.len(),
            valid_action_count,
        };
    }

    if let Ok(value) = serde_json::from_str::<serde_json::Value>(text.trim()) {
        return ParseDiagnostics {
            parse_ok: !is_planner_role || is_valid_planner_action(&value),
            parse_mode: "strict_json",
            block_count: 0,
            valid_action_count: usize::from(is_valid_planner_action(&value)),
        };
    }

    ParseDiagnostics {
        parse_ok: false,
        parse_mode: "strict_json",
        block_count: 0,
        valid_action_count: 0,
    }
}

fn extract_fenced_json_blocks(text: &str) -> Vec<String> {
    let mut blocks = Vec::new();
    let mut in_fence = false;
    let mut current = String::new();

    for line in text.lines() {
        let trimmed = line.trim_start();
        if !in_fence {
            if trimmed.starts_with("```") {
                in_fence = true;
                current.clear();
            }
            continue;
        }

        if trimmed.starts_with("```") {
            blocks.push(current.trim().to_string());
            in_fence = false;
            current.clear();
            continue;
        }

        current.push_str(line);
        current.push('\n');
    }

    blocks
}

fn is_valid_planner_action(value: &serde_json::Value) -> bool {
    if value.get("done").and_then(|v| v.as_bool()) == Some(true) {
        return true;
    }
    if value.get("cmd").and_then(|v| v.as_str()).is_some() {
        return true;
    }
    if value.get("write").and_then(|v| v.as_str()).is_some()
        && value.get("content").and_then(|v| v.as_str()).is_some()
    {
        return true;
    }
    value.get("path").and_then(|v| v.as_str()).is_some()
        && value.get("old").and_then(|v| v.as_str()).is_some()
        && value.get("new").and_then(|v| v.as_str()).is_some()
}

fn next_llm_call_counter(log_dir: &str) -> u32 {
    let mut max_seen: Option<u32> = None;
    let Ok(entries) = std::fs::read_dir(Path::new(log_dir)) else {
        return 0;
    };
    for entry in entries.flatten() {
        let Some(name) = entry.file_name().to_str().map(|s| s.to_string()) else {
            continue;
        };
        let Some((n, suffix)) = parse_artifact_reqnum_and_suffix(&name) else {
            continue;
        };
        if suffix != "request.json" && suffix != "response.json" {
            continue;
        }
        max_seen = Some(max_seen.map_or(n, |m| m.max(n)));
    }
    max_seen.map_or(0, |m| m.saturating_add(1))
}

fn parse_artifact_reqnum_and_suffix(name: &str) -> Option<(u32, String)> {
    let mut parts = name.splitn(3, '_');
    let first = parts.next()?;
    let second = parts.next()?;
    let third = parts.next()?;

    if let Ok(n) = first.parse::<u32>() {
        // Legacy: <REQNUM>_<suffix>.json
        return Some((n, format!("{}_{}", second, third)));
    }
    // New: <TS>_<REQNUM>_<suffix>.json
    let n = second.parse::<u32>().ok()?;
    Some((n, third.to_string()))
}

fn default_role_content(role: Option<&str>, endpoint_id: &str) -> String {
    if endpoint_id == "planner_chatgpt_group" {
        return String::new();
    }
    match role.unwrap_or("exec") {
        "planner" => "You are a planning agent. Return only JSON inside fenced ```json code block(s) with no prose.".to_string(),
        "router" => "You are a routing selector. Choose exactly one next route and return only one fenced ```json code block with no prose.".to_string(),
        _ => "You are an execution agent. Return only JSON inside fenced ```json code block(s) with no prose.".to_string(),
    }
}

impl CapabilityHandler for LlmCapabilityHandler {
    fn name(&self) -> &'static str {
        "llm.call"
    }

    fn execute(&self, ctx: CapabilityExecutionContext) -> anyhow::Result<CapabilityExecutionResult> {
        let CanonEvent::CapabilityRequested(request) = ctx.event else {
            return Ok(CapabilityExecutionResult::NoOp);
        };
        let Some(emitter) = ctx.emitter else {
            return Err(anyhow::anyhow!("llm.call: no emitter in context"));
        };
        let prompt = match request.args.get("prompt").and_then(|v| v.as_str()) {
            Some(v) => v.to_string(),
            None => return Ok(CapabilityExecutionResult::NoOp),
        };
        let role = request.args.get("role").and_then(|v| v.as_str()).map(|v| v.to_string());
        let raw = request.args.get("raw").and_then(|v| v.as_bool()).unwrap_or(false);
        canon_emit!(emitter; "llm_executor", "enqueue_request",
            json!({ "request_id": request.request_id }));
        if self.work_tx.send(LlmWork {
            request_id: request.request_id,
            name: request.name,
            prompt,
            role,
            raw,
            args: request.args,
            emitter,
        }).is_err() {
            return Err(anyhow::anyhow!("llm executor worker channel closed"));
        }
        Ok(CapabilityExecutionResult::Deferred)
    }
}
