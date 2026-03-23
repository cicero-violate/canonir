use std::collections::HashMap;
use std::path::PathBuf;
use std::sync::{Arc, Mutex};
use std::sync::atomic::{AtomicBool, Ordering};
use std::thread;
use std::time::{Duration, Instant};

use canon_event::{
    EventConsumer, EventEmitterHandle, EventFilter, LoopObserved,
    RuntimeEvent, RequestDispatch, SubTaskResult, GoalNodeRetracted,
};
use canon_loop::LoopStageExecutor;
use canon_route::RouteExecutor;
use crate::consumers::capability_executor::CapabilityExecutor;
use crate::EventRuntime;
use crossbeam_channel as cc;

const SUB_AGENT_TIMEOUT_SECS: u64 = 300;
const TICK_INTERVAL_MS: u64 = 100;

// ---------------------------------------------------------------------------
// HaltDetectorConsumer
// ---------------------------------------------------------------------------

struct HaltDetectorConsumer {
    halted: Arc<AtomicBool>,
}

impl EventConsumer for HaltDetectorConsumer {
    fn filter(&self) -> EventFilter { EventFilter::All }
    fn set_emitter(&mut self, _: EventEmitterHandle) {}
    fn on_event(&mut self, event: &RuntimeEvent) {
        if let RuntimeEvent::LoopRewarded(r) = event {
            if r.halt {
                self.halted.store(true, Ordering::Relaxed);
            }
        }
    }
}

// ---------------------------------------------------------------------------
// ForwardConsumer — re-emits LoopPlanned/Acted/Verified to parent bus
// and collects action IDs for SubTaskResult.actions_taken
// ---------------------------------------------------------------------------

struct ForwardConsumer {
    parent: EventEmitterHandle,
    actions_taken: Arc<Mutex<Vec<String>>>,
}

impl EventConsumer for ForwardConsumer {
    fn filter(&self) -> EventFilter { EventFilter::All }
    fn set_emitter(&mut self, _: EventEmitterHandle) {}
    fn on_event(&mut self, event: &RuntimeEvent) {
        match event {
            RuntimeEvent::LoopPlanned(p) => {
                if let Some(id) = &p.action_id {
                    if let Ok(mut v) = self.actions_taken.lock() {
                        v.push(id.clone());
                    }
                }
                self.parent.emit(event.clone());
            }
            RuntimeEvent::LoopActed(_) | RuntimeEvent::LoopVerified(_) => {
                self.parent.emit(event.clone());
            }
            _ => {}
        }
    }
}

// ---------------------------------------------------------------------------
// Sub-agent loop — owns a full EventRuntime with isolated tlog
// ---------------------------------------------------------------------------

fn run_sub_agent(req: RequestDispatch, parent_emitter: EventEmitterHandle, base_workspace: PathBuf) {
    let workspace = base_workspace.join("sub_agents").join(&req.dispatch_id);
    std::fs::create_dir_all(&workspace).ok();
    let tlog = workspace.join("event.tlog.d");

    let halted = Arc::new(AtomicBool::new(false));
    let actions_taken: Arc<Mutex<Vec<String>>> = Arc::new(Mutex::new(Vec::new()));

    let consumers: Vec<Box<dyn canon_event::EventConsumer>> = vec![
        Box::new(LoopStageExecutor::new(workspace.clone(), tlog.clone())
            .with_agent_id(req.agent_id.clone())),
        Box::new(RouteExecutor::new(workspace.clone())),
        Box::new(CapabilityExecutor::new(workspace.clone())),
        Box::new(HaltDetectorConsumer { halted: halted.clone() }),
        Box::new(ForwardConsumer {
            parent: parent_emitter.clone(),
            actions_taken: actions_taken.clone(),
        }),
    ];

    let mut runtime = EventRuntime::new(consumers);
    runtime.set_tlog_path(tlog);

    // Prime the sub-agent with its goal.
    runtime.emit_event(RuntimeEvent::LoopObserved(LoopObserved {
        tick: 0,
        goal_text: Some(req.task_prompt.clone()),
        error_count: 0,
        warning_count: 0,
        compiler_errors: vec![],
        workspace_facts: vec![],
    })).ok();

    let deadline = Instant::now() + Duration::from_secs(SUB_AGENT_TIMEOUT_SECS);
    while !halted.load(Ordering::Relaxed) && Instant::now() < deadline {
        runtime.emit_tick().ok();
        thread::sleep(Duration::from_millis(TICK_INTERVAL_MS));
    }

    let success = halted.load(Ordering::Relaxed);
    let taken = actions_taken.lock().map(|v| v.clone()).unwrap_or_default();

    if !success {
        parent_emitter.emit(RuntimeEvent::GoalNodeRetracted(GoalNodeRetracted {
            node_id: req.dispatch_id.clone(),
        }));
    }

    parent_emitter.emit(RuntimeEvent::SubTaskResult(SubTaskResult {
        dispatch_id: req.dispatch_id,
        agent_id: req.agent_id,
        parent_request_id: req.parent_request_id,
        success,
        output: serde_json::json!({}),
        actions_taken: taken,
        error: if success { None } else { Some("sub-agent timeout".to_string()) },
    }));
}

// ---------------------------------------------------------------------------
// DispatchConsumer — public API
// ---------------------------------------------------------------------------

pub struct DispatchConsumer {
    emitter: Option<EventEmitterHandle>,
    workers: HashMap<String, cc::Sender<RequestDispatch>>,
    workspace: PathBuf,
}

impl DispatchConsumer {
    pub fn new() -> Self {
        let workspace = std::env::current_dir().unwrap_or_else(|_| PathBuf::from("."));
        Self { emitter: None, workers: HashMap::new(), workspace }
    }

    fn ensure_worker(&mut self, agent_id: &str, parent_emitter: EventEmitterHandle) -> &cc::Sender<RequestDispatch> {
        let base = self.workspace.clone();
        self.workers.entry(agent_id.to_string()).or_insert_with(|| {
            let (tx, rx) = cc::unbounded::<RequestDispatch>();
            thread::Builder::new()
                .name(format!("dispatch-worker-{agent_id}"))
                .spawn(move || {
                    for req in rx.iter() {
                        run_sub_agent(req, parent_emitter.clone(), base.clone());
                    }
                })
                .expect("dispatch worker thread");
            tx
        })
    }
}

impl EventConsumer for DispatchConsumer {
    fn filter(&self) -> EventFilter { EventFilter::All }

    fn set_emitter(&mut self, emitter: EventEmitterHandle) {
        self.emitter = Some(emitter);
    }

    fn on_event(&mut self, event: &RuntimeEvent) {
        let RuntimeEvent::RequestDispatch(req) = event else { return; };
        let Some(emitter) = self.emitter.clone() else { return; };
        let tx = self.ensure_worker(&req.agent_id, emitter);
        let _ = tx.send(req.clone());
    }
}
