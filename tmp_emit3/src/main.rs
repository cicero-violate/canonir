use std::fs::OpenOptions;
use std::io::Write;

fn main() {
    let path = "../state/sub_agents/dispatch-0/event.tlog";
    let mut f = OpenOptions::new().create(true).append(true).open(path).unwrap();

    let event = r#"{"id":"manual-seed","parent_ids":[],"actor":"user","kind":"prompt_loaded","ts":0,"payload":{"input":{},"output":{"payload":{"content":"manual_json_seed"}},"delta":{},"meta":{},"data":{"payload":{"content":"manual_json_seed"}}}}"#;

    writeln!(f, "{}", event).unwrap();
}
