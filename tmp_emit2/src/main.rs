use canon_event::canon_emit;
use canon_prompt_events::runtime_goal_prompt_loaded;
use std::path::PathBuf;
use serde_json::json;

fn main() {
    let tlog = PathBuf::from("../state/event_log");
    let _event = runtime_goal_prompt_loaded("# Test Goal\n\nGenerate a project.");
    let payload = json!({"goal": "# Test Goal\n\nGenerate a project."});
    match canon_emit!(root; "event-runtime", "runtime_goal_prompt_loaded", payload, &tlog) {
        Ok(_) => println!("emit ok"),
        Err(e) => eprintln!("emit failed: {:#}", e),
    }
}
