use std::path::PathBuf;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Mutex};
use std::thread;
use std::time::{Duration, Instant};

use crate::consumers::capability_executor::CapabilityExecutor;
use crate::EventRuntime;
use canon_event::{EventConsumer, EventEmitterHandle, EventFilter, EventId, EventOutcome, GoalNodeRetracted, LoopObserved, RequestDispatch, RuntimeEvent, SubTaskResult};
use canon_loop::LoopStageExecutor;
use canon_proc_macros::must_emit;
use canon_route::RouteExecutor;

/// Load exec endpoint IDs from capability_config.toml at startup.
fn load_exec_endpoint_ids() -> Vec<String> {
    canon_llm::config::CapabilityConfig::snapshot_store_load().ok().map(|c| c.llm_endpoints.iter().filter(|e| e.role.as_deref() == Some("exec")).map(|e| e.id.clone()).collect()).unwrap_or_default()
}

const SUB_AGENT_TIMEOUT_SECS: u64 = 300;
const TICK_INTERVAL_MS: u64 = 100;

// ---------------------------------------------------------------------------
// HaltDetectorConsumer
// ---------------------------------------------------------------------------

struct HaltDetectorConsumer {
    halted: Arc<AtomicBool>,
}

impl EventConsumer for HaltDetectorConsumer {
    fn filter(&self) -> EventFilter {
        EventFilter::All
    }
    fn is_synchronous(&self) -> bool { true }
    fn consumer_name(&self) -> &'static str { "halt_detector" }
    fn set_emitter(&mut self, _: EventEmitterHandle) {}
    #[must_emit]
    fn on_event(&mut self, event: &RuntimeEvent, _trigger_id: EventId) -> EventOutcome {
        if let RuntimeEvent::LoopRewarded(r) = event {
            if r.halt {
                self.halted.store(true, Ordering::Relaxed);
            }
        }
        EventOutcome::NoOp("halt_detector_checked")
    }
}

// ---------------------------------------------------------------------------
// ForwardConsumer — re-emits LoopPlanned/Acted/Verified to parent bus
// and collects action IDs for SubTaskResult.actions_taken
// ---------------------------------------------------------------------------

struct ForwardConsumer {
    parent: EventEmitterHandle,
    actions_taken: Arc<Mutex<Vec<String>>>,
    halted: Arc<AtomicBool>,
}

impl EventConsumer for ForwardConsumer {
    fn filter(&self) -> EventFilter {
        EventFilter::All
    }
    fn is_synchronous(&self) -> bool { true }
    fn consumer_name(&self) -> &'static str { "forward_consumer" }
    fn set_emitter(&mut self, _: EventEmitterHandle) {}
    #[must_emit]
    fn on_event(&mut self, event: &RuntimeEvent, trigger_id: EventId) -> EventOutcome {
        let forward = |parent: &EventEmitterHandle, e: RuntimeEvent| {
            parent.emit_with_parents(e, vec![trigger_id.clone()], file!(), line!());
        };
        match event {
            RuntimeEvent::LoopObserved(_) => {}
            RuntimeEvent::LoopPlanned(p) => {
                if let Some(id) = &p.action_id {
                    if let Ok(mut v) = self.actions_taken.lock() {
                        v.push(id.clone());
                    }
                }
                forward(&self.parent, event.clone());
            }
            RuntimeEvent::LoopActed(a) => {
                if let Some(id) = &a.action_id {
                    if let Ok(mut v) = self.actions_taken.lock() {
                        v.push(id.clone());
                    }
                }
                forward(&self.parent, event.clone());
            }
            RuntimeEvent::PlanningCompleted(_)
            | RuntimeEvent::LoopVerified(_)
            | RuntimeEvent::ToolCall(_)
            | RuntimeEvent::ToolResult(_)
            | RuntimeEvent::ToolBatchSettled(_) => {
                forward(&self.parent, event.clone());
            }
            RuntimeEvent::LoopRewarded(r) => {
                if r.halt {
                    self.halted.store(true, Ordering::Relaxed);
                }
                forward(&self.parent, event.clone());
            }
            RuntimeEvent::Code(_)
            | RuntimeEvent::Debug(_)
            | RuntimeEvent::Edit(_)
            | RuntimeEvent::ErrorOccurred(_)
            | RuntimeEvent::Tick(_)
            | RuntimeEvent::GoodnessSnapshot(_)
            | RuntimeEvent::RouteTick(_)
            | RuntimeEvent::RouteSelected(_)
            | RuntimeEvent::Cargo(_)
            | RuntimeEvent::File(_)
            | RuntimeEvent::Bash(_)
            | RuntimeEvent::Llm(_)
            | RuntimeEvent::RequestDispatch(_)
            | RuntimeEvent::SubTaskResult(_)
            | RuntimeEvent::Analysis(_)
            | RuntimeEvent::RuntimeStateUpdated(_)
            | RuntimeEvent::NodeReady(_)
            | RuntimeEvent::NodeStarted(_)
            | RuntimeEvent::NodeCompleted(_)
            | RuntimeEvent::NodeFailed(_)
            | RuntimeEvent::CapabilityCompleted(_)
            | RuntimeEvent::CapabilityFailed(_)
            | RuntimeEvent::PolicyBaselineUpdated(_)
            | RuntimeEvent::GoalSelected(_)
            | RuntimeEvent::SystemConfigLoaded(_)
            | RuntimeEvent::AgentRegistered(_)
            | RuntimeEvent::PromptLoaded(_)
            | RuntimeEvent::GoalNodeCreated(_)
            | RuntimeEvent::GoalNodeRetracted(_)
            | RuntimeEvent::GoalNodeRewritten(_)
            | RuntimeEvent::GoalEdgeDefined(_)
            | RuntimeEvent::GoalGraphCheckpointed(_)
            | RuntimeEvent::CapabilityInvoked(_)
            | RuntimeEvent::CapabilityResolved(_)
            | RuntimeEvent::InvariantDiscovered(_)
            | RuntimeEvent::RustcCaptureStarted(_)
            | RuntimeEvent::RustcGraphArtifactWritten(_)
            | RuntimeEvent::RustcCaptureCompleted(_)
            | RuntimeEvent::RustcCaptureFailed(_)
            | RuntimeEvent::VerifierPolicyUpdated(_) => {}
        }
        EventOutcome::NoOp("forward_consumer_forwarded")
    }
}

// ---------------------------------------------------------------------------
// Sub-agent loop — owns a full EventRuntime with isolated tlog
// ---------------------------------------------------------------------------

fn run_sub_agent(req: RequestDispatch, parent_emitter: EventEmitterHandle, base_workspace: PathBuf, trigger_id: EventId) {
    let workspace = base_workspace.join("sub_agents").join(&req.dispatch_id);
    std::fs::create_dir_all(&workspace).ok();
    let tlog = workspace.join("event.tlog.d");

    let halted = Arc::new(AtomicBool::new(false));
    let actions_taken: Arc<Mutex<Vec<String>>> = Arc::new(Mutex::new(Vec::new()));

    let consumers: Vec<Box<dyn canon_event::EventConsumer>> = vec![
        Box::new(LoopStageExecutor::new(workspace.clone(), tlog.clone()).with_agent_id(req.agent_id.clone())),
        Box::new(RouteExecutor::new(workspace.clone())),
        Box::new(CapabilityExecutor::new(workspace.clone())),
        Box::new(HaltDetectorConsumer { halted: halted.clone() }),
        Box::new(ForwardConsumer { parent: parent_emitter.clone(), actions_taken: actions_taken.clone(), halted: halted.clone() }),
    ];

    let mut runtime = EventRuntime::new(consumers);
    runtime.set_tlog_path(tlog);

    // Prime the sub-agent with its goal inside the sub-agent runtime, not the parent bus.
    let _ = runtime.emit_event_with_parents(RuntimeEvent::LoopObserved(LoopObserved {
            tick: 0,
            goal_text: Some(req.task_prompt.clone()),
            error_count: 0,
            warning_count: 0,
            compiler_errors: vec![],
            semantic_summary: canon_semantic_state::SemanticStateSummary::default(),
            observe_diagnostics: vec![],
        }), vec![trigger_id.clone()], file!(), line!());

    let deadline = Instant::now() + Duration::from_secs(SUB_AGENT_TIMEOUT_SECS);
    while !halted.load(Ordering::Relaxed) && Instant::now() < deadline {
        thread::sleep(Duration::from_millis(TICK_INTERVAL_MS));
    }

    let success = halted.load(Ordering::Relaxed);
    let taken = actions_taken.lock().map(|v| v.clone()).unwrap_or_default();

    if !success {
        parent_emitter.emit_with_parents(RuntimeEvent::GoalNodeRetracted(GoalNodeRetracted { node_id: req.dispatch_id.clone(), retracted: true }), vec![trigger_id.clone()], file!(), line!());
    }

    parent_emitter.emit_with_parents(RuntimeEvent::SubTaskResult(SubTaskResult {
        dispatch_id: req.dispatch_id,
        agent_id: req.agent_id,
        parent_request_id: req.parent_request_id,
        success,
        output: serde_json::json!({}),
        actions_taken: taken,
        error: if success { None } else { Some("sub-agent timeout".to_string()) },
    }), vec![trigger_id], file!(), line!());
}

// ---------------------------------------------------------------------------
// DispatchConsumer — public API
// ---------------------------------------------------------------------------

pub struct DispatchConsumer {
    emitter: Option<EventEmitterHandle>,
    workspace: PathBuf,
    /// Exec endpoint IDs loaded from capability_config.toml (e.g., exec_chatgpt_a…f).
    exec_endpoints: Vec<String>,
    /// Round-robin cursor for endpoint assignment.
    next_exec_idx: usize,
}

impl DispatchConsumer {
    pub fn new() -> Self {
        let workspace = std::env::current_dir().unwrap_or_else(|_| PathBuf::from("."));
        let exec_endpoints = load_exec_endpoint_ids();
        Self { emitter: None, workspace, exec_endpoints, next_exec_idx: 0 }
    }

    /// Map a generic role string (e.g., "exec") to a specific endpoint ID
    /// from the configured pool, round-robin. If `agent_id` is already a
    /// known endpoint ID it is returned unchanged.
    fn assign_endpoint(&mut self, agent_id: &str) -> String {
        // Already a concrete endpoint ID — use it directly.
        if self.exec_endpoints.contains(&agent_id.to_string()) {
            return agent_id.to_string();
        }
        // Pool is empty (config not loaded or no exec endpoints) — fall through.
        if self.exec_endpoints.is_empty() {
            return agent_id.to_string();
        }
        // Round-robin pick from pool.
        let idx = self.next_exec_idx % self.exec_endpoints.len();
        self.next_exec_idx += 1;
        self.exec_endpoints[idx].clone()
    }
}

impl EventConsumer for DispatchConsumer {
    fn filter(&self) -> EventFilter {
        EventFilter::All
    }

    fn is_synchronous(&self) -> bool { true }

    fn consumer_name(&self) -> &'static str { "dispatch_consumer" }

    fn set_emitter(&mut self, emitter: EventEmitterHandle) {
        self.emitter = Some(emitter);
    }

    #[must_emit]
    fn on_event(&mut self, event: &RuntimeEvent, trigger_id: EventId) -> EventOutcome {
        let RuntimeEvent::RequestDispatch(req) = event else {
            return EventOutcome::NoOp("dispatch_consumer_non_dispatch");
        };
        let Some(emitter) = self.emitter.clone() else {
            return EventOutcome::NoOp("dispatch_consumer_no_emitter");
        };
        // Resolve the generic role ("exec") to a specific endpoint ID so that
        // all LlmCalls within this sub-agent use the same tab (stateful conversation).
        let mut req = req.clone();
        req.agent_id = self.assign_endpoint(&req.agent_id);
        let base = self.workspace.clone();
        thread::Builder::new()
            .name(format!("dispatch-worker-{}", req.dispatch_id))
            .spawn(move || {
                run_sub_agent(req, emitter, base, trigger_id);
            })
            .expect("dispatch worker thread");
        EventOutcome::NoOp("dispatch_consumer_spawned")
    }
}
