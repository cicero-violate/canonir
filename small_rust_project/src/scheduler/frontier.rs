use crate::dag::NodeId;
use std::collections::VecDeque;

/// FrontierQueue represents the ordered set of runnable nodes.
#[derive(Debug, Clone)]
pub struct FrontierQueue {
    queue: VecDeque<NodeId>,
}

impl FrontierQueue {
    pub fn new() -> Self {
        Self { queue: VecDeque::new() }
    }

    pub fn from_nodes(nodes: Vec<NodeId>) -> Self {
        Self { queue: nodes.into() }
    }

    pub fn push(&mut self, id: NodeId) {
        self.queue.push_back(id);
    }

    pub fn pop(&mut self) -> Option<NodeId> {
        self.queue.pop_front()
    }

    pub fn is_empty(&self) -> bool {
        self.queue.is_empty()
    }

    pub fn len(&self) -> usize {
        self.queue.len()
    }

    pub fn iter(&self) -> impl Iterator<Item=&NodeId> {
        self.queue.iter()
    }
}
