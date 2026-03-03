//! Scheduler — resolves ready nodes via topo constraints.

use super::dag::{Status, TaskGraph};

pub fn update_ready_states(graph: &mut TaskGraph) {
    let mut status_map = std::collections::HashMap::new();
    for n in &graph.nodes {
        status_map.insert(n.id.clone(), n.status);
    }
    for node in &mut graph.nodes {
        if node.status == Status::Pending || node.status == Status::Blocked {
            let mut any_failed = false;
            let mut all_completed = true;
            for dep in &node.deps {
                if let Some(dep_status) = status_map.get(dep) {
                    if *dep_status == Status::Failed {
                        any_failed = true;
                    }
                    if *dep_status != Status::Completed {
                        all_completed = false;
                    }
                }
            }
            if any_failed {
                node.status = Status::Blocked;
            } else if all_completed {
                node.status = Status::Ready;
            }
        }
    }
}
