use crate::bootstrap::PromptRegistryHandle;
use canon_planner::planner::config::CapabilityConfig;
use canon_planner::planner::engine;
use canon_planner::planner::endpoint_worker;
use canon_event::emit_debug::{error, info};
use canon_event::{RuntimeConsumer, RuntimeEmitterHandle, RuntimeEvent, RuntimeEventFilter};
use serde_json::json;
use std::sync::{Arc, Mutex};
use std::thread;
use std::time::Instant;
use std::env;

enum LlmWork {
    Request {
        request_id: String,
        name: String,
        prompt: String,
        role: Option<String>,
        raw: bool,
    },
}

pub struct LlmExecutorConsumer {
    emitter: Arc<Mutex<Option<RuntimeEmitterHandle>>>,
    work_tx: std::sync::mpsc::Sender<LlmWork>,
}

impl LlmExecutorConsumer {
    pub fn new(registry: PromptRegistryHandle) -> Self {
        let (work_tx, work_rx) = std::sync::mpsc::channel();
        let emitter = Arc::new(Mutex::new(None::<RuntimeEmitterHandle>));
        let emitter_handle = Arc::clone(&emitter);
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
                    Err(err) => {
                        error(
                            "llm_executor",
                            "tokio_runtime_failed",
                            serde_json::json!({ "error": err.to_string() }),
                        );
                        return;
                    }
                };
                let config = match CapabilityConfig::snapshot_store_load() {
                    Ok(config) => config,
                    Err(err) => {
                        error(
                            "llm_executor",
                            "config_load_failed",
                            serde_json::json!({ "error": err.to_string() }),
                        );
                        return;
                    }
                };
                config.apply_env_flags();
                let bridge_addr = env::var("CANON_LLM_BRIDGE_ADDR")
                    .unwrap_or_else(|_| "127.0.0.1:9100".to_string());
                let addr: std::net::SocketAddr = match bridge_addr.parse() {
                    Ok(addr) => addr,
                    Err(err) => {
                        let err_msg = err.to_string();
                        error(
                            "llm_executor",
                            "invalid_bridge_addr",
                            serde_json::json!({ "addr": bridge_addr, "error": err_msg }),
                        );
                        "127.0.0.1:9100".parse().expect("fallback llm bridge addr")
                    }
                };
                info(
                    "llm_executor",
                    "bridge_start",
                    serde_json::json!({ "addr": addr.to_string() }),
                );
                let bridge = runtime.block_on(async {
                    canon_planner::planner::ws_server::spawn(addr, config.response_timeout_secs)
                });
                runtime.block_on(async {
                    let wait = bridge.wait_for_connection();
                    if tokio::time::timeout(std::time::Duration::from_secs(5), wait)
                        .await
                        .is_err()
                    {
                        error(
                            "llm_executor",
                            "bridge_timeout",
                            serde_json::json!({ "timeout_s": 5 }),
                        );
                    }
                });
                let tabs = engine::llm_worker_new_tabs();
                runtime.block_on(endpoint_worker::llm_worker_init_workers(
                    &bridge,
                    &config,
                    &tabs,
                ));
                info("llm_executor", "bridge_ready", serde_json::json!({}));

                for job in work_rx.iter() {
                    let emitter = emitter_handle.lock().ok().and_then(|e| e.clone());
                    let Some(emitter) = emitter else { continue };
                    match job {
                        LlmWork::Request {
                            request_id,
                            name,
                            prompt,
                            role,
                            raw,
                        } => {
                            let start = Instant::now();
                            let request_id_log = request_id.clone();
                            let name_log = name.clone();
                            info(
                                "llm_executor",
                                "request_start",
                                serde_json::json!({ "request_id": request_id_log.as_str(), "name": name_log.as_str() }),
                            );
                            let Some(endpoint) = (if let Some(role) = role.as_deref() {
                                config
                                    .llm_endpoints
                                    .iter()
                                    .find(|e| e.role.as_deref() == Some(role))
                            } else {
                                config.llm_endpoints.first()
                            }) else {
                                emitter.emit(RuntimeEvent::CapabilityFailed(
                                    canon_event::CapabilityFailed {
                                        request_id,
                                        name,
                                        error: "no llm endpoints configured".to_string(),
                                    },
                                ));
                                continue;
                            };
                            let retries = config.llm_retry_count.max(1);
                            let delay = config.llm_retry_delay_secs;
                            // role_markdown is a filename relative to the prompts dir; look up from registry
                            let role_content = registry_handle
                                .read()
                                .ok()
                                .and_then(|r| r.get(&endpoint.role_markdown).map(str::to_string))
                                .unwrap_or_default();
                            info(
                                "llm_executor",
                                "request_dispatch",
                                serde_json::json!({
                                    "request_id": request_id,
                                    "endpoint": endpoint.id,
                                    "url": endpoint.url
                                }),
                            );
                            let result = runtime.block_on(async {
                                let call = async {
                                    if raw {
                                        engine::module_call_llm_raw_with_retry_allow_mismatch(
                                            &bridge,
                                            &endpoint.id,
                                            &endpoint.url,
                                            endpoint.stateful,
                                            &prompt,
                                            &role_content,
                                            "llm_executor",
                                            None,
                                            &tabs,
                                            endpoint.max_tabs,
                                            config.tab_cooldown_ms,
                                            retries,
                                            delay,
                                        )
                                        .await
                                        .map(|text| json!({ "text": text }))
                                    } else {
                                        engine::module_call_llm_json_with_retry_allow_mismatch(
                                            &bridge,
                                            &endpoint.id,
                                            &endpoint.url,
                                            endpoint.stateful,
                                            &prompt,
                                            &role_content,
                                            "llm_executor",
                                            None,
                                            &tabs,
                                            endpoint.max_tabs,
                                            config.tab_cooldown_ms,
                                            retries,
                                            delay,
                                        )
                                        .await
                                    }
                                };
                                match tokio::time::timeout(
                                    std::time::Duration::from_secs(config.response_timeout_secs),
                                    call,
                                )
                                .await
                                {
                                    Ok(result) => result,
                                    Err(_) => Err(anyhow::anyhow!("llm call timed out")),
                                }
                            });

                            match result {
                                Ok(payload) => {
                                    emitter.emit(RuntimeEvent::CapabilityCompleted(
                                        canon_event::CapabilityCompleted {
                                            request_id,
                                            name,
                                            result: json!({
                                                "status": 0,
                                                "success": true,
                                                "duration_ms": start.elapsed().as_millis(),
                                                "result": payload,
                                            }),
                                        },
                                    ));
                                    info(
                                        "llm_executor",
                                        "request_completed",
                                        serde_json::json!({ "request_id": request_id_log.as_str() }),
                                    );
                                }
                                Err(err) => {
                                    emitter.emit(RuntimeEvent::CapabilityFailed(
                                        canon_event::CapabilityFailed {
                                            request_id,
                                            name,
                                            error: err.to_string(),
                                        },
                                    ));
                                    error(
                                        "llm_executor",
                                        "request_failed",
                                        serde_json::json!({ "request_id": request_id_log.as_str(), "error": err.to_string() }),
                                    );
                                }
                            }
                        }
                    }
                }
                }));
                if let Err(e) = result {
                    let msg = e
                        .downcast_ref::<&str>()
                        .copied()
                        .or_else(|| e.downcast_ref::<String>().map(|s| s.as_str()))
                        .unwrap_or("unknown panic");
                    error(
                        "llm_executor",
                        "worker_panicked",
                        serde_json::json!({ "error": msg }),
                    );
                }
            })
            .expect("llm executor worker thread");

        Self { emitter, work_tx }
    }
}

impl RuntimeConsumer for LlmExecutorConsumer {
    fn filter(&self) -> RuntimeEventFilter {
        RuntimeEventFilter::CapabilityOnly
    }

    fn on_event(&mut self, event: &RuntimeEvent) {
        let RuntimeEvent::CapabilityRequested(request) = event else {
            return;
        };
        info(
            "llm_executor",
            "event_received",
            serde_json::json!({ "name": request.name, "request_id": request.request_id }),
        );
        if request.name != "llm.call" {
            return;
        }
        let prompt = match request.args.get("prompt").and_then(|v| v.as_str()) {
            Some(v) => v.to_string(),
            None => return,
        };
        let role = request.args.get("role").and_then(|v| v.as_str()).map(|v| v.to_string());
        let raw = request
            .args
            .get("raw")
            .and_then(|v| v.as_bool())
            .unwrap_or(false);
        info(
            "llm_executor",
            "enqueue_request",
            serde_json::json!({ "request_id": request.request_id }),
        );
        let send_result = self.work_tx.send(LlmWork::Request {
            request_id: request.request_id.clone(),
            name: request.name.clone(),
            prompt,
            role,
            raw,
        });
        if send_result.is_err() {
            error(
                "llm_executor",
                "enqueue_failed",
                serde_json::json!({ "request_id": request.request_id }),
            );
        }
    }

    fn set_emitter(&mut self, emitter: RuntimeEmitterHandle) {
        if let Ok(mut slot) = self.emitter.lock() {
            *slot = Some(emitter);
        }
    }
}

