use crate::bootstrap::PromptRegistryHandle;
use canon_agent::config::CapabilityConfig;
use canon_llm::endpoint_worker;
use canon_llm::llm;
use canon_capability::{CapabilityExecutionContext, CapabilityExecutionResult, CapabilityHandler};
use canon_event::{CanonEvent, EventEmitterHandle, canon_emit};
use serde_json::json;
use std::sync::Arc;
use std::thread;
use std::time::Instant;
use std::env;

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
    pub fn new(registry: PromptRegistryHandle) -> Self {
        let (work_tx, work_rx) = std::sync::mpsc::channel::<LlmWork>();
        let registry_handle = Arc::clone(&registry);

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
                        canon_agent::ws_server::spawn(addr, config.response_timeout_secs)
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
                    for job in work_rx.iter() {
                        let LlmWork { request_id, name, prompt, role, raw, emitter } = job;
                        let start = Instant::now();
                        canon_emit!(emitter; "llm_executor", "request_start",
                            json!({ "request_id": request_id, "name": name }));
                        let Some(endpoint) = (if let Some(role) = role.as_deref() {
                            config.llm_endpoints.iter().find(|e| e.role.as_deref() == Some(role))
                        } else {
                            config.llm_endpoints.first()
                        }) else {
                            emitter.emit(CanonEvent::CapabilityFailed(canon_event::CapabilityFailed {
                                request_id,
                                name,
                                error: "no llm endpoints configured".to_string(),
                            }));
                            continue;
                        };
                        let retries = config.llm_retry_count.max(1);
                        let delay = config.llm_retry_delay_secs;
                        let role_content = registry_handle
                            .read()
                            .ok()
                            .and_then(|r| r.get(&endpoint.role_markdown).map(str::to_string))
                            .unwrap_or_default();
                        canon_emit!(emitter; "llm_executor", "request_dispatch",
                            json!({
                                "request_id": request_id,
                                "endpoint": endpoint.id,
                                "url": endpoint.url
                            }));
                        let result = runtime.block_on(async {
                            let call = async {
                                if raw {
                                    llm::llm_client_call_agent_raw_with_retry_allow_mismatch(
                                        &bridge, &endpoint.id, &endpoint.url,
                                        endpoint.stateful, &prompt, &role_content,
                                        "llm_executor", None, &tabs, endpoint.max_tabs,
                                        config.tab_cooldown_ms, retries, delay,
                                    ).await
                                } else {
                                    llm::llm_client_call_agent_json_with_retry_allow_mismatch(
                                        &bridge, &endpoint.id, &endpoint.url,
                                        endpoint.stateful, &prompt, &role_content,
                                        "llm_executor", None, &tabs, endpoint.max_tabs,
                                        config.tab_cooldown_ms, retries, delay,
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
                                emitter.emit(CanonEvent::CapabilityCompleted(
                                    canon_event::CapabilityCompleted {
                                        request_id: request_id.clone(),
                                        name,
                                        result: json!({
                                            "status": 0,
                                            "success": true,
                                            "duration_ms": start.elapsed().as_millis(),
                                            "result": payload,
                                        }),
                                    },
                                ));
                                canon_emit!(emitter; "llm_executor", "request_completed",
                                    json!({ "request_id": request_id }));
                            }
                            Err(err) => {
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
