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
use std::time::Instant;
use std::env;
use std::path::Path;

struct LlmWork {
    request_id: String,
    name: String,
    prompt: String,
    role: Option<String>,
    raw: bool,
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
                        let LlmWork { request_id, name, prompt, role, raw, emitter } = job;
                        // Capture emitter on first job; ws_server uses it for
                        // bridge-level events for the rest of the process lifetime.
                        let _ = ws_emitter_thread.set(emitter.clone());
                        let start = Instant::now();
                        canon_emit!(emitter; "llm_executor", "request_start",
                            json!({ "request_id": request_id, "name": name }));
                        let is_planner_role = role.as_deref() == Some("planner");
                        let bust_cache = config.planner_refine_on_cache && is_planner_role;
                        let Some(endpoint) = (if let Some(role) = role.as_deref() {
                            config.llm_endpoints.iter().find(|e| e.role.as_deref() == Some(role))
                        } else {
                            config.llm_endpoints.first()
                        }) else {
                            emitter.emit(CanonEvent::ErrorOccurred(new_error_occurred(
                                "llm_config",
                                "llm_executor",
                                "no llm endpoints configured",
                                "error",
                                json!({
                                    "request_id": request_id.clone(),
                                    "capability": name.clone(),
                                }),
                                Some(request_id.clone()),
                            )));
                            emitter.emit(CanonEvent::CapabilityFailed(canon_event::CapabilityFailed {
                                request_id,
                                name,
                                error: "no llm endpoints configured".to_string(),
                            }));
                            continue;
                        };
                        let retries = config.llm_retry_count.max(1);
                        let delay = config.llm_retry_delay_secs;
                        let role_content = default_role_content(
                            role.as_deref().or(endpoint.role.as_deref()),
                        );
                        canon_emit!(emitter; "llm_executor", "request_dispatch",
                            json!({
                                "request_id": request_id,
                                "endpoint": endpoint.id,
                                "url": endpoint.url
                            }));
                        let call_n = llm_call_counter;
                        llm_call_counter += 1;
                        let req_path = format!("{}/{:04}_request.json", llm_log_dir, call_n);
                        let res_path = format!("{}/{:04}_response.json", llm_log_dir, call_n);
                        let req_obj = json!({
                            "n": call_n,
                            "request_id": request_id,
                            "endpoint": endpoint.id,
                            "url": endpoint.url,
                            "role": role_content,
                            "prompt": prompt,
                        });
                        let _ = std::fs::write(&req_path, serde_json::to_string_pretty(&req_obj).unwrap_or_default());
                        let result = runtime.block_on(async {
                            let call = async {
                                if raw {
                                    llm::llm_client_call_agent_raw_with_retry_allow_mismatch(
                                        &bridge, &endpoint.id, &endpoint.url,
                                        endpoint.stateful, &prompt, &role_content,
                                        "llm_executor", None, &tabs, endpoint.max_tabs,
                                        config.tab_cooldown_ms, retries, delay, bust_cache,
                                    ).await
                                } else {
                                    llm::llm_client_call_agent_json_with_retry_allow_mismatch(
                                        &bridge, &endpoint.id, &endpoint.url,
                                        endpoint.stateful, &prompt, &role_content,
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
                                // E_in → L → E_out boundary: emit exactly once per LLM
                                // response arrival, before the capability result is sealed.
                                // Gives real-time visibility into: which endpoint replied,
                                // how long it took, response size, and whether JSON parsed.
                                let elapsed_ms = start.elapsed().as_millis() as u64;
                                let parse_ok = !payload.as_object()
                                    .map(|o| o.len() == 1 && o.contains_key("text"))
                                    .unwrap_or(false);
                                let res_obj = json!({
                                    "n": call_n,
                                    "request_id": request_id,
                                    "endpoint": endpoint.id,
                                    "duration_ms": elapsed_ms,
                                    "parse_ok": parse_ok,
                                    "response": payload,
                                });
                                let _ = std::fs::write(&res_path, serde_json::to_string_pretty(&res_obj).unwrap_or_default());
                                canon_emit!(emitter; "llm_executor", "llm_response",
                                    json!({
                                        "request_id": request_id,
                                        "endpoint":   endpoint.id,
                                        "duration_ms": elapsed_ms,
                                        "bytes":      payload.to_string().len(),
                                        "parse_ok":   parse_ok,
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
                                let res_obj = json!({
                                    "n": call_n,
                                    "request_id": request_id,
                                    "endpoint": endpoint.id,
                                    "error": err.to_string(),
                                });
                                let _ = std::fs::write(&res_path, serde_json::to_string_pretty(&res_obj).unwrap_or_default());
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

fn next_llm_call_counter(log_dir: &str) -> u32 {
    let mut max_seen: Option<u32> = None;
    let Ok(entries) = std::fs::read_dir(Path::new(log_dir)) else {
        return 0;
    };
    for entry in entries.flatten() {
        let Some(name) = entry.file_name().to_str().map(|s| s.to_string()) else {
            continue;
        };
        let Some((prefix, suffix)) = name.split_once('_') else {
            continue;
        };
        if suffix != "request.json" && suffix != "response.json" {
            continue;
        }
        let Ok(n) = prefix.parse::<u32>() else {
            continue;
        };
        max_seen = Some(max_seen.map_or(n, |m| m.max(n)));
    }
    max_seen.map_or(0, |m| m.saturating_add(1))
}

fn default_role_content(role: Option<&str>) -> String {
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
            emitter,
        }).is_err() {
            return Err(anyhow::anyhow!("llm executor worker channel closed"));
        }
        Ok(CapabilityExecutionResult::Deferred)
    }
}
