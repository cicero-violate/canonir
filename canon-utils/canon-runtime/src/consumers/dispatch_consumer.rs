use std::path::PathBuf;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Mutex};
// REMOVED: unused thread import
// REMOVED: unused timing imports

// REMOVED: unused runtime + capability executor
use canon_event::{EventConsumer, EventEmitterHandle, EventFilter, EventId, EventOutcome, RuntimeEvent};
// REMOVED: unused LoopStageExecutor
use canon_proc_macros::must_emit;
// REMOVED: unused RouteExecutor

/// Load exec endpoint IDs from capability_config.toml at startup.
fn load_exec_endpoint_ids() -> Vec<String> {
    canon_llm::config::CapabilityConfig::snapshot_store_load().ok().map(|c| c.llm_endpoints.iter().filter(|e| e.role.as_deref() == Some("exec")).map(|e| e.id.clone()).collect()).unwrap_or_default()
}

#[allow(dead_code)]
const SUB_AGENT_TIMEOUT_SECS: u64 = 300;
#[allow(dead_code)]
const TICK_INTERVAL_MS: u64 = 100;

// ---------------------------------------------------------------------------
// HaltDetectorConsumer
// ---------------------------------------------------------------------------

#[allow(dead_code)]
struct HaltDetectorConsumer {
    halted: Arc<AtomicBool>,
}

impl EventConsumer for HaltDetectorConsumer {
    fn filter(&self) -> EventFilter {
        EventFilter::All
    }
    fn is_synchronous(&self) -> bool {
        // 🔥 CRITICAL FIX: make async so it is registered in async consumer path
        false
    }
    fn consumer_name(&self) -> &'static str {
        "halt_detector"
    }
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

#[allow(dead_code)]
struct ForwardConsumer {
    parent: EventEmitterHandle,
    actions_taken: Arc<Mutex<Vec<String>>>,
    halted: Arc<AtomicBool>,
}

impl EventConsumer for ForwardConsumer {
    fn filter(&self) -> EventFilter {
        EventFilter::All
    }
    fn is_synchronous(&self) -> bool {
        true
    }
    fn consumer_name(&self) -> &'static str {
        "forward_consumer"
    }
    fn set_emitter(&mut self, _: EventEmitterHandle) {}
    #[must_emit]
    fn on_event(&mut self, event: &RuntimeEvent, trigger_id: EventId) -> EventOutcome {
        eprintln!("[DISPATCH TRACE] ForwardConsumer received event={:?} trigger_id={:?}", event, trigger_id);

        // REMOVED: duplicate pre-forward for LoopPlanned (handled in match below)
        // FIX: do NOT force LoopObserved here — must respect invariant ordering
        if let RuntimeEvent::ErrorOccurred(_err) = event {
            eprintln!("[DISPATCH FIX] ErrorOccurred received — no forced observe");
            return EventOutcome::NoOp("error_passthrough");
        }
        let forward = |parent: &EventEmitterHandle, e: RuntimeEvent| {
            parent.emit_with_parents(e, vec![trigger_id.clone()], file!(), line!());
        };
        match event {
            RuntimeEvent::RouteSelected(_) => {
                forward(&self.parent, event.clone());
            }
            RuntimeEvent::LoopObserved(_) => {
                // CRITICAL FIX: do NOT forward LoopObserved here
                // Forwarding at this layer creates duplicate delivery paths
                // LoopObserved must propagate only via canonical runtime/bus path
                return EventOutcome::NoOp("loop_observed_no_forward");
            }
            RuntimeEvent::LoopPlanned(p) => {
                if let Some(id) = &p.action_id {
                    if let Ok(mut v) = self.actions_taken.lock() {
                        v.push(id.clone());
                    }
                }
                eprintln!("[DISPATCH TRACE] forwarding LoopPlanned without bash rewrite");
                forward(&self.parent, event.clone());
                return EventOutcome::NoOp("loop_planned_forwarded");
            }
            RuntimeEvent::LoopActed(a) => {
                if let Some(id) = &a.action_id {
                    if let Ok(mut v) = self.actions_taken.lock() {
                        v.push(id.clone());
                    }
                }
                // forward exactly once
                forward(&self.parent, event.clone());
                return EventOutcome::NoOp("loop_acted_forwarded_once");
            }
            RuntimeEvent::PlanningCompleted(_) => {
                // FIX: do NOT auto-execute — let RouteSelected decide next step
                return EventOutcome::NoOp("planning_completed_passthrough");
            }
            RuntimeEvent::LoopVerified(_) | RuntimeEvent::ToolCall(_) | RuntimeEvent::ToolResult(_) | RuntimeEvent::ToolBatchSettled(_) => {
                forward(&self.parent, event.clone());
            }
            RuntimeEvent::CapabilityCompleted(_) => {
                // FIX: LLM completes but PlanningCompleted is not reaching here reliably
                // DO NOT force RouteSelected here — routing must be handled by RouteExecutor
                return EventOutcome::NoOp("capability_completed_passthrough");
            }
            RuntimeEvent::LoopRewarded(r) => {
                if r.halt {
                    self.halted.store(true, Ordering::Relaxed);
                }
                forward(&self.parent, event.clone());
            }
            RuntimeEvent::Debug(d) if d.kind == "observe_suppressed_due_to_pending_successor" => {
                // DROP: legacy suppression signal
                return EventOutcome::NoOp("dropped_legacy_observe_suppression");
            }
            RuntimeEvent::Code(_)
            | RuntimeEvent::Debug(_)
            | RuntimeEvent::Edit(_)
            | RuntimeEvent::ErrorOccurred(_)
            | RuntimeEvent::Tick(_)
            | RuntimeEvent::GoodnessSnapshot(_)
            | RuntimeEvent::RouteTick(_)
            | RuntimeEvent::Cargo(_)
            | RuntimeEvent::File(_)
            | RuntimeEvent::Llm(_) => {
                // Only LLM events should be forwarded
                forward(&self.parent, event.clone());
            }
            | RuntimeEvent::Bash(_) => {
                // FIX: DO NOT forward Bash → prevents duplicate execution
                eprintln!("[FORWARD FIX] skipping Bash forward");
            }
            | RuntimeEvent::RequestDispatch(_)
            | RuntimeEvent::SubTaskResult(_)
            | RuntimeEvent::Analysis(_)
            | RuntimeEvent::RuntimeStateUpdated(_)
            | RuntimeEvent::NodeReady(_)
            | RuntimeEvent::NodeStarted(_)
            | RuntimeEvent::NodeCompleted(_)
            | RuntimeEvent::NodeFailed(_)
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

// REMOVED: run_sub_agent(RequestDispatch)
// Entire synthetic sub-agent execution block removed — canonical routing only
// REMOVED: dangling LoopObserved payload from deleted RequestDispatch flow

// REMOVED: RequestDispatch execution block
// Synthetic dispatch execution is eliminated; canonical routing must drive all execution

// ---------------------------------------------------------------------------
// DispatchConsumer — public API
// ---------------------------------------------------------------------------

#[allow(dead_code)]
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
    #[allow(dead_code)]
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

    fn is_synchronous(&self) -> bool {
        true
    }

    fn consumer_name(&self) -> &'static str {
        "dispatch_consumer"
    }

    fn set_emitter(&mut self, emitter: EventEmitterHandle) {
        self.emitter = Some(emitter);
    }

    #[must_emit]
    fn on_event(&mut self, event: &RuntimeEvent, _trigger_id: EventId) -> EventOutcome {
        // 🔥 CRITICAL: prove DispatchConsumer actually sees RouteSelected
        if let RuntimeEvent::RouteSelected(route) = event {
            eprintln!("[DISPATCH TRACE] DispatchConsumer RECEIVED RouteSelected tick={}", route.tick);
            // INVARIANT: DISPATCH MUST FLOW ONLY THROUGH ROUTE EXECUTOR
            // No synthetic RequestDispatch is allowed here — this consumer must remain passive
            // and only react to canonical RequestDispatch events emitted downstream.
            return EventOutcome::NoOp("route_selected_no_synthetic_dispatch");
        }
        // REMOVED: RequestDispatch handling entirely
        EventOutcome::NoOp("dispatch_consumer_non_dispatch")
    }
}
