use canon_event::canon_emit;
use serde_json::json;
use std::path::PathBuf;

fn main() {
    let tlog = PathBuf::from("../state/event_log");
    let payload = json!({
        "node_id": "g1",
        "description": "test goal",
        "deps": [],
        "caps": ["cap_a"],
        "node_type": "root",
        "priority": 1,
        "created": true
    });
    let _ = canon_emit!(root; "event-runtime", "goal_node_created", payload, &tlog);
}
