use std::collections::HashMap;
use canon_event::{
    EventConsumer, EventEmitterHandle, EventFilter, EventOutcome, GoalGraphCheckpointed, RuntimeEvent,
};

#[derive(Clone, Debug, Default)]
pub struct GoalNode {
    pub node_id:     String,
    pub description: String,
    pub deps:        Vec<String>,
    pub caps:        Vec<String>,
    pub node_type:   String,
    pub priority:    u8,
    pub retracted:   bool,
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
                self.nodes.insert(e.node_id.clone(), GoalNode {
                    node_id:     e.node_id.clone(),
                    description: e.description.clone(),
                    deps:        e.deps.clone(),
                    caps:        e.caps.clone(),
                    node_type:   e.node_type.clone(),
                    priority:    e.priority,
                    retracted:   false,
                });
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

    pub fn graph(&self) -> &GoalGraph { &self.graph }
}

impl EventConsumer for GoalGraphConsumer {
    fn filter(&self) -> EventFilter { EventFilter::All }

    fn set_emitter(&mut self, _emitter: EventEmitterHandle) {}

    fn on_event(&mut self, event: &RuntimeEvent) -> EventOutcome {
        self.graph.apply(event);
        match event {
            RuntimeEvent::GoalNodeCreated(_)
            | RuntimeEvent::GoalNodeRetracted(_)
            | RuntimeEvent::GoalNodeRewritten(_)
            | RuntimeEvent::GoalEdgeDefined(_) => {
                self.last_checkpoint_seq += 1;
                return EventOutcome::Emit(RuntimeEvent::GoalGraphCheckpointed(GoalGraphCheckpointed {
                    tlog_seq: self.last_checkpoint_seq,
                }));
            }
            _ => {}
        }
        EventOutcome::NoOp("goal_graph_noop")
    }
}
