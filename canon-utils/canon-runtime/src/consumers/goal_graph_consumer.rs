use canon_event::{EventConsumer, EventEmitterHandle, EventFilter, EventId, EventOutcome, GoalGraphCheckpointed, RuntimeEvent};
use canon_proc_macros::must_emit;
use std::collections::HashMap;

#[derive(Clone, Debug, Default)]
pub struct GoalNode {
    pub node_id: String,
    pub description: String,
    pub deps: Vec<String>,
    pub caps: Vec<String>,
    pub node_type: String,
    pub priority: u8,
    pub retracted: bool,
}

#[derive(Default)]
pub struct GoalGraph {
    pub nodes: HashMap<String, GoalNode>,
    pub edges: Vec<(String, String)>,
}

impl GoalGraph {
    pub fn apply(&mut self, event: &RuntimeEvent) {
        match event {
            RuntimeEvent::GoalNodeCreated(e) => {
                self.nodes.insert(
                    e.node_id.clone(),
                    GoalNode {
                        node_id: e.node_id.clone(),
                        description: e.description.clone(),
                        deps: e.deps.clone(),
                        caps: e.caps.clone(),
                        node_type: e.node_type.clone(),
                        priority: e.priority,
                        retracted: false,
                    },
                );
            }
            RuntimeEvent::GoalNodeRetracted(e) => {
                if let Some(n) = self.nodes.get_mut(&e.node_id) {
                    n.retracted = true;
                }
            }
            RuntimeEvent::GoalNodeRewritten(e) => {
                if let Some(n) = self.nodes.get_mut(&e.node_id) {
                    n.description = e.new_description.clone();
                    n.caps = e.new_caps.clone();
                }
            }
            RuntimeEvent::GoalEdgeDefined(e) => {
                self.edges.push((e.from_node_id.clone(), e.to_node_id.clone()));
            }
            _ => {}
        }
    }

    pub fn active_nodes(&self) -> impl Iterator<Item = &GoalNode> {
        self.nodes.values().filter(|n| !n.retracted)
    }
}

pub struct GoalGraphConsumer {
    graph: GoalGraph,
    last_checkpoint_seq: u64,
}

impl GoalGraphConsumer {
    pub fn new() -> Self {
        Self { graph: GoalGraph::default(), last_checkpoint_seq: 0 }
    }

    pub fn graph(&self) -> &GoalGraph {
        &self.graph
    }
}

impl EventConsumer for GoalGraphConsumer {
    fn filter(&self) -> EventFilter {
        EventFilter::All
    }

    fn is_synchronous(&self) -> bool {
        true
    }

    fn consumer_name(&self) -> &'static str {
        "goal_graph_consumer"
    }

    fn set_emitter(&mut self, _emitter: EventEmitterHandle) {}

    #[must_emit]
    fn on_event(&mut self, event: &RuntimeEvent, _trigger_id: EventId) -> EventOutcome {
        self.graph.apply(event);
        match event {
            RuntimeEvent::CapabilityRequested(_) => {
                return EventOutcome::NoOp("goal_graph_capability_requested");
            }
            RuntimeEvent::GoalNodeCreated(_) | RuntimeEvent::GoalNodeRetracted(_) | RuntimeEvent::GoalNodeRewritten(_) | RuntimeEvent::GoalEdgeDefined(_) => {
                self.last_checkpoint_seq += 1;
                return EventOutcome::emit(RuntimeEvent::GoalGraphCheckpointed(GoalGraphCheckpointed { tlog_seq: self.last_checkpoint_seq, checkpointed: true }), file!(), line!());
            }
            RuntimeEvent::Code(_)
            | RuntimeEvent::Debug(_)
            | RuntimeEvent::Edit(_)
            | RuntimeEvent::ErrorOccurred(_)
            | RuntimeEvent::Tick(_)
            | RuntimeEvent::LoopObserved(_)
            | RuntimeEvent::LoopPlanned(_)
            | RuntimeEvent::PlanningCompleted(_)
            | RuntimeEvent::LoopActed(_)
            | RuntimeEvent::LoopVerified(_)
            | RuntimeEvent::LoopRewarded(_)
            | RuntimeEvent::GoodnessSnapshot(_)
            | RuntimeEvent::RouteTick(_)
            | RuntimeEvent::RouteSelected(_)
            | RuntimeEvent::Cargo(_)
            | RuntimeEvent::File(_)
            | RuntimeEvent::Bash(_)
            | RuntimeEvent::Llm(_)
            => {
                return EventOutcome::NoOp("non_actionable_event");
            }
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
            | RuntimeEvent::ToolCall(_)
            | RuntimeEvent::ToolResult(_)
            | RuntimeEvent::ToolBatchSettled(_)
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
        EventOutcome::NoOp("goal_graph_noop")
    }
}
