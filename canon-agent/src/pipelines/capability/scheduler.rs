use std::collections::HashMap;

use super::authority::AuthorityContext;
use super::capability::Capability;
use super::dag::{Status, TaskGraph, TaskNode};

pub fn resolve_ready(graph: &mut TaskGraph) {
    let status_map: HashMap<String, Status> = graph.nodes.iter().map(|n| (n.id.clone(), n.status)).collect();
    for node in &mut graph.nodes {
        if node.status == Status::Pending || node.status == Status::Blocked {
            let any_failed = node.deps.iter().any(|d| status_map.get(d) == Some(&Status::Failed));
            let all_complete = node.deps.iter().all(|d| status_map.get(d) == Some(&Status::Completed));
            if any_failed {
                node.status = Status::Blocked;
            } else if all_complete {
                node.status = Status::Ready;
            }
        }
    }
}

pub fn grant_authority(node: &TaskNode) -> Result<AuthorityContext, String> {
    let caps: std::collections::HashSet<Capability> = node.required_capabilities.iter().copied().collect();
    AuthorityContext::new(node.id.clone(), caps)
}
