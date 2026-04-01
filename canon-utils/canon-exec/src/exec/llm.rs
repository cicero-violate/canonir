use super::{Executable, ExecutionContext, ExecutionResult};
use canon_event::{new_error_occurred, CapabilityResult, EventEmitterHandle, LlmCall, LlmResult};
use canon_llm::config::CapabilityConfig;
use canon_llm::endpoint_worker;
use canon_llm::llm;
use canon_llm::ws_server;
use serde_json::json;
use std::collections::HashMap;
use std::env;
use std::sync::{Arc, OnceLock};
use std::time::{Duration, Instant, SystemTime, UNIX_EPOCH};

pub(crate) struct LlmWork {
    request_id: String,
    name: &'static str,
    /// Fast-changing delta (LOC, errors, recent actions/results) — sent every call.
    prompt: String,
    /// Static system instructions — Some on first send, None when already cached.
    system: Option<String>,
    /// Cache key for the static system instructions.
    system_prompt_id: Option<String>,
    /// Slow-changing context (GOAL, workspace tree, facts) — Some only when changed.
    context_base: Option<String>,
    /// Cache key for the slow-changing context base.
    context_base_id: Option<String>,
    role: Option<String>,
    agent_id: Option<String>,
    raw: bool,
    emitter: EventEmitterHandle,
    trigger_id: canon_event::EventId,
}

static LLM_WORKER_TX: std::sync::RwLock<Option<std::sync::mpsc::Sender<LlmWork>>> = std::sync::RwLock::new(None);
const HARNESS_FAILFAST_TIMEOUT_SECS: u64 = 12;

pub fn init_llm_worker() {
    let tx = spawn_llm_worker();
    *LLM_WORKER_TX.write().unwrap() = Some(tx);
}

pub fn shutdown_llm_worker() {
    *LLM_WORKER_TX.write().unwrap() = None;
}

fn spawn_llm_worker() -> std::sync::mpsc::Sender<LlmWork> {
    let (work_tx, work_rx) = std::sync::mpsc::channel::<LlmWork>();

    let ws_emitter: Arc<OnceLock<EventEmitterHandle>> = Arc::new(OnceLock::new());
    let ws_emitter_thread = Arc::clone(&ws_emitter);

    std::thread::Builder::new()
        .name("llm_executor_worker".to_string())
        .spawn(move || {
            let result = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
                let runtime = match tokio::runtime::Builder::new_multi_thread().enable_all().build() {
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
                let bridge_addr = env::var("CANON_LLM_BRIDGE_ADDR").unwrap_or_else(|_| "127.0.0.1:9100".to_string());
                let addr: std::net::SocketAddr = match bridge_addr.parse() {
                    Ok(addr) => addr,
                    Err(_err) => "127.0.0.1:9100".parse().expect("fallback llm bridge addr"),
                };
                let bridge = runtime.block_on(async { ws_server::spawn(addr, config.response_timeout_secs, Arc::clone(&ws_emitter_thread)) });
                // NON-BLOCKING: do not gate execution on WS connection
                let bridge_bg = bridge.clone();
                runtime.spawn(async move {
                    let _ = bridge_bg.wait_for_connection().await;
                });
                let tabs = endpoint_worker::llm_worker_new_tabs();
                runtime.block_on(endpoint_worker::llm_worker_init_workers(&bridge, &config, &tabs));

                // Start relay server so standalone harness binaries can route LLM calls
                // through this worker's WsBridge without binding port 9100 themselves.
                {
                    let bridge_r = bridge.clone();
                    let config_r = config.clone();
                    let tabs_r = tabs.clone();
                    let relay_addr = env::var("CANON_LLM_RELAY_ADDR").unwrap_or_else(|_| "127.0.0.1:9101".to_string());
                    runtime.spawn(async move {
                        match canon_llm::relay::relay_server_start(&relay_addr, move |req| {
                            let bridge = bridge_r.clone();
                            let config = config_r.clone();
                            let tabs = tabs_r.clone();
                            Box::pin(async move {
                                let endpoint = config
                                    .llm_endpoints
                                    .iter()
                                    .find(|e| req.endpoint_id.as_deref().map_or(false, |id| e.id == id) || e.role.as_deref() == Some(req.role.as_str()))
                                    .cloned()
                                    .ok_or_else(|| anyhow::anyhow!("relay: no endpoint for role={}", req.role))?;
                                endpoint_worker::llm_worker_send_request(
                                    &bridge,
                                    &endpoint.id,
                                    &endpoint.url,
                                    endpoint.stateful,
                                    &req.prompt,
                                    &req.role_schema,
                                    None,
                                    None,
                                    false,
                                    true,
                                    "relay",
                                    &tabs,
                                    endpoint.max_tabs,
                                    false,
                                )
                                .await
                            })
                        })
                        .await
                        {
                            Ok(handle) => {
                                eprintln!("[llm-worker] relay server listening on {}", handle.local_addr());
                                std::future::pending::<()>().await;
                                drop(handle);
                            }
                            Err(e) => eprintln!("[llm-worker] relay server failed to start: {e}"),
                        }
                    });
                }

                let llm_log_dir = env::var("CANON_LLM_LOG_DIR").unwrap_or_else(|_| "/workspace/ai_sandbox/canon/canon-utils/state/reports_out/llm".to_string());
                let _ = std::fs::create_dir_all(&llm_log_dir);
                let mut llm_call_counter: u32 = next_llm_call_counter(&llm_log_dir);
                // System prompt cache: keyed by system_prompt_id (hash of static instructions).
                // Populated on first call that includes `system`; subsequent calls omit it.
                let mut system_cache: HashMap<String, String> = HashMap::new();
                // Context-base cache: keyed by context_base_id (hash of slow-changing context).
                // Populated when `context_base` is Some; subsequent calls with same id omit it.
                let mut context_base_cache: HashMap<String, String> = HashMap::new();
                for job in work_rx.iter() {
                    let LlmWork { request_id, name, prompt, system, system_prompt_id, context_base, context_base_id, role, agent_id, raw, emitter, trigger_id } = job;

                    // Cache new system prompt when provided (first call or reset).
                    let system_was_sent = system.is_some();
                    if let (Some(id), Some(sys)) = (system_prompt_id.as_ref(), system) {
                        system_cache.insert(id.clone(), sys);
                    }

                    // Cache new context base when provided (changes when GOAL/workspace changes).
                    if let (Some(id), Some(base)) = (context_base_id.as_ref(), context_base) {
                        context_base_cache.insert(id.clone(), base);
                    }

                    let _ = ws_emitter_thread.set(emitter.clone());
                    let start = Instant::now();
                    let is_planner_role = role.as_deref() == Some("planner");
                    let bust_cache = config.planner_refine_on_cache && is_planner_role;
                    let requested_role = role.clone().unwrap_or_default();
                    let selected = if let Some(aid) = agent_id.as_deref() {
                        config
                            .llm_endpoints
                            .iter()
                            .find(|e| e.id == aid)
                            .or_else(|| config.llm_endpoints.iter().find(|e| e.url.contains(aid)))
                            // Fallback: treat agent_id as a role name (e.g., "exec" → first exec endpoint).
                            .or_else(|| config.llm_endpoints.iter().find(|e| e.role.as_deref() == Some(aid)))
                    } else if let Some(role_name) = role.as_deref() {
                        config.llm_endpoints.iter().find(|e| e.role.as_deref() == Some(role_name)).or_else(|| {
                            if role_name == "router" {
                                config.llm_endpoints.iter().find(|e| e.role.as_deref() == Some("planner"))
                            } else {
                                None
                            }
                        })
                    } else {
                        config.llm_endpoints.first()
                    };
                    let Some(endpoint) = selected else {
                        let error_msg = if requested_role.is_empty() { "no llm endpoints configured".to_string() } else { format!("no llm endpoint configured for role={}", requested_role) };
                        emitter.emit_child(
                            canon_event::RuntimeEvent::ErrorOccurred(new_error_occurred(
                                "llm_config",
                                "llm_executor",
                                &error_msg,
                                "error",
                                json!({
                                    "request_id": request_id.clone(),
                                    "capability": name,
                                    "role": role,
                                }),
                                Some(request_id.clone()),
                            )),
                            vec![trigger_id.clone()],
                            file!(),
                            line!(),
                        );
                        emitter.emit_child(
                            canon_event::RuntimeEvent::CapabilityFailed(canon_event::CapabilityFailed { request_id, capability: name, error: error_msg }),
                            vec![trigger_id.clone()],
                            file!(),
                            line!(),
                        );
                        continue;
                    };
                    // Reconstruct the full prompt to send to the LLM API (three-tier hierarchy):
                    //
                    //   Tier 1 (system): static instructions — prepended once, cached.
                    //   Tier 2 (context_base): GOAL + workspace — prepended when changed, cached.
                    //   Tier 3 (prompt / delta): LOC, errors, actions — always present.
                    //
                    //   • First call (system_was_sent=true): reconstruct all three tiers.
                    //   • Stateful endpoint, nothing changed: send only delta (LLM has tiers 1+2
                    //     in session history already).
                    //   • Stateless endpoint: always reconstruct from cache so LLM has full context.
                    let sys = system_prompt_id.as_deref().and_then(|id| system_cache.get(id)).map(String::as_str).unwrap_or("");
                    let base = context_base_id.as_deref().and_then(|id| context_base_cache.get(id)).map(String::as_str).unwrap_or("");
                    let full_prompt = if system_was_sent {
                        // First call: assemble all tiers; system and base were just cached.
                        match (sys.is_empty(), base.is_empty()) {
                            (false, false) => format!("{sys}\n\n{base}\n\n{prompt}"),
                            (false, true) => format!("{sys}\n\n{prompt}"),
                            (true, false) => format!("{base}\n\n{prompt}"),
                            (true, true) => prompt.clone(),
                        }
                    } else if endpoint.stateful {
                        // Stateful session: LLM already has system + base in history.
                        // Send only the delta (prompt).
                        prompt.clone()
                    } else {
                        // Stateless endpoint: reconstruct everything from cache on every call.
                        match (sys.is_empty(), base.is_empty()) {
                            (false, false) => format!("{sys}\n\n{base}\n\n{prompt}"),
                            (false, true) => format!("{sys}\n\n{prompt}"),
                            (true, false) => format!("{base}\n\n{prompt}"),
                            (true, true) => prompt.clone(),
                        }
                    };
                    let retries = config.llm_retry_count.max(1);
                    let delay = config.llm_retry_delay_secs;
                    let role_content = default_role_content(role.as_deref().or(endpoint.role.as_deref()), &endpoint.id);
                    let prompt_with_request_id = format!("{{\"request_id\":\"{}\"}}\n{}", request_id, full_prompt);
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
                        "args": json!({ "prompt": prompt_with_request_id, "role": role }),
                        "dispatched_ms": dispatched_ms,
                        "finalized_ms": serde_json::Value::Null,
                    });
                    let _ = std::fs::write(&req_path, serde_json::to_string_pretty(&req_obj).unwrap_or_default());
                    let pending_obj = json!({
                        "n": call_n,
                        "request_id": request_id,
                        "endpoint": endpoint.id,
                        "status": "pending",
                        "started_ms": now_ms(),
                    });
                    let _ = std::fs::write(&res_path, serde_json::to_string_pretty(&pending_obj).unwrap_or_default());
                    let (result_tx, result_rx) = std::sync::mpsc::channel();
                    let bridge_cloned = bridge.clone();
                    let endpoint_id = endpoint.id.clone();
                    let endpoint_url = endpoint.url.clone();
                    let endpoint_stateful = endpoint.stateful;
                    let endpoint_max_tabs = endpoint.max_tabs;
                    // removed tab_cooldown_ms: field no longer exists in CapabilityConfig
                    let role_content_cloned = role_content.clone();
                    let prompt_with_request_id_cloned = prompt_with_request_id.clone();
                    let tabs_cloned = tabs.clone();
                    std::thread::spawn(move || {
                        let thread_runtime = match tokio::runtime::Builder::new_current_thread().enable_all().build() {
                            Ok(rt) => rt,
                            Err(err) => {
                                let _ = result_tx.send(Err(anyhow::anyhow!("llm worker request runtime init failed: {err}")));
                                return;
                            }
                        };
                        let result = thread_runtime.block_on(async {
                            if raw {
                                llm::llm_client_call_agent_raw_with_retry_allow_mismatch(
                                    &bridge_cloned,
                                    &endpoint_id,
                                    &endpoint_url,
                                    endpoint_stateful,
                                    &prompt_with_request_id_cloned,
                                    &role_content_cloned,
                                    "llm_executor",
                                    None,
                                    &tabs_cloned,
                                    endpoint_max_tabs,
                                    0,
                                    retries,
                                    delay,
                                    bust_cache,
                                )
                                .await
                            } else {
                                llm::llm_client_call_agent_json_with_retry_allow_mismatch(
                                    &bridge_cloned,
                                    &endpoint_id,
                                    &endpoint_url,
                                    endpoint_stateful,
                                    &prompt_with_request_id_cloned,
                                    &role_content_cloned,
                                    "llm_executor",
                                    None,
                                    &tabs_cloned,
                                    endpoint_max_tabs,
                                    0,
                                    retries,
                                    delay,
                                    bust_cache,
                                )
                                .await
                            }
                        });
                        let _ = result_tx.send(result);
                    });
                    let worker_timeout =
                        if is_planner_role { Duration::from_secs(config.response_timeout_secs.min(HARNESS_FAILFAST_TIMEOUT_SECS)) } else { Duration::from_secs(config.response_timeout_secs) };
                    let result = match result_rx.recv_timeout(worker_timeout) {
                        Ok(result) => result,
                        Err(std::sync::mpsc::RecvTimeoutError::Timeout) => Err(anyhow::anyhow!("llm call timed out")),
                        Err(std::sync::mpsc::RecvTimeoutError::Disconnected) => Err(anyhow::anyhow!("llm worker request thread disconnected")),
                    };

                    match result {
                        Ok(payload) => {
                            let finalized_ms = now_ms();
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
                                "args": json!({ "prompt": prompt_with_request_id, "role": role }),
                                "dispatched_ms": dispatched_ms,
                                "finalized_ms": finalized_ms,
                            });
                            let _ = std::fs::write(&req_path, serde_json::to_string_pretty(&req_done).unwrap_or_default());
                            emitter.emit_child(
                                canon_event::RuntimeEvent::CapabilityCompleted(canon_event::CapabilityCompleted {
                                    request_id: request_id.clone(),
                                    capability: name,
                                    result: CapabilityResult::Llm(LlmResult { success: true, duration_ms: elapsed_ms, response: payload.clone() }),
                                }),
                                vec![trigger_id.clone()],
                                file!(),
                                line!(),
                            );
                        }
                        Err(err) => {
                            let finalized_ms = now_ms();
                            let res_obj = json!({
                                "n": call_n,
                                "request_id": request_id,
                                "endpoint": endpoint.id,
                                "status": "failed",
                                "finalized_ms": finalized_ms,
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
                                "args": json!({ "prompt": prompt_with_request_id, "role": role }),
                                "dispatched_ms": dispatched_ms,
                                "finalized_ms": finalized_ms,
                            });
                            let _ = std::fs::write(&req_path, serde_json::to_string_pretty(&req_done).unwrap_or_default());
                            emitter.emit_child(
                                canon_event::RuntimeEvent::ErrorOccurred(new_error_occurred(
                                    "llm_call",
                                    "llm_executor",
                                    err.to_string(),
                                    "error",
                                    json!({
                                        "request_id": request_id.clone(),
                                        "capability": name,
                                    }),
                                    Some(request_id.clone()),
                                )),
                                vec![trigger_id.clone()],
                                file!(),
                                line!(),
                            );
                            emitter.emit_child(
                                canon_event::RuntimeEvent::CapabilityFailed(canon_event::CapabilityFailed { request_id: request_id.clone(), capability: name, error: err.to_string() }),
                                vec![trigger_id.clone()],
                                file!(),
                                line!(),
                            );
                            // Failure is already surfaced via ErrorOccurred + CapabilityFailed.
                            // Do not emit a fake follow-up event here.
                        }
                    }
                }
            }));
            let _ = result;
        })
        .expect("llm executor worker thread");

    work_tx
}

impl Executable for LlmCall {
    fn execute(self, ctx: ExecutionContext) -> anyhow::Result<ExecutionResult> {
        let guard = LLM_WORKER_TX.read().unwrap();
        let tx = guard.as_ref().ok_or_else(|| anyhow::anyhow!("llm worker not initialized"))?;
        tx.send(LlmWork {
            request_id: self.request_id,
            name: "llm.call",
            prompt: self.prompt,
            system: self.system,
            system_prompt_id: self.system_prompt_id,
            context_base: self.context_base,
            context_base_id: self.context_base_id,
            role: self.role,
            agent_id: self.agent_id,
            raw: false,
            emitter: ctx.emitter,
            trigger_id: ctx.trigger_id,
        })
        .map_err(|e| anyhow::anyhow!("llm worker channel closed: {e}"))?;
        Ok(ExecutionResult::Deferred)
    }
}

fn now_ms() -> u128 {
    SystemTime::now().duration_since(UNIX_EPOCH).unwrap_or_default().as_millis()
}

#[derive(Clone, Copy)]
struct ParseDiagnostics {
    parse_ok: bool,
    parse_mode: &'static str,
    block_count: usize,
    valid_action_count: usize,
}

fn analyze_llm_payload(payload: &serde_json::Value, is_planner_role: bool) -> ParseDiagnostics {
    let Some(obj) = payload.as_object() else {
        return ParseDiagnostics { parse_ok: true, parse_mode: "strict_json", block_count: 0, valid_action_count: 0 };
    };

    let Some(text) = (obj.len() == 1 && obj.contains_key("text")).then(|| obj.get("text").and_then(|v| v.as_str())).flatten() else {
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
        let parse_ok = if is_planner_role { parsed_count > 0 && parsed_count == valid_action_count && parsed_count == blocks.len() } else { parsed_count > 0 };
        return ParseDiagnostics { parse_ok, parse_mode: mode, block_count: blocks.len(), valid_action_count };
    }

    if let Ok(value) = serde_json::from_str::<serde_json::Value>(text.trim()) {
        return ParseDiagnostics {
            parse_ok: !is_planner_role || is_valid_planner_action(&value),
            parse_mode: "strict_json",
            block_count: 0,
            valid_action_count: usize::from(is_valid_planner_action(&value)),
        };
    }

    ParseDiagnostics { parse_ok: false, parse_mode: "strict_json", block_count: 0, valid_action_count: 0 }
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
    if value.get("write").and_then(|v| v.as_str()).is_some() && value.get("content").and_then(|v| v.as_str()).is_some() {
        return true;
    }
    value.get("path").and_then(|v| v.as_str()).is_some() && value.get("old").and_then(|v| v.as_str()).is_some() && value.get("new").and_then(|v| v.as_str()).is_some()
}

fn next_llm_call_counter(log_dir: &str) -> u32 {
    let mut max_seen: Option<u32> = None;
    let Ok(entries) = std::fs::read_dir(std::path::Path::new(log_dir)) else {
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
        return Some((n, format!("{}_{}", second, third)));
    }
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
        "analyst" => String::new(),
        _ => "You are an execution agent. Return only JSON inside fenced ```json code block(s) with no prose.".to_string(),
    }
}
