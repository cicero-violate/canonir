use canon_event_store::{read_any_events_from_path, AnyEvent};
use std::path::PathBuf;

#[derive(Default)]
struct Args {
    tlog: Option<PathBuf>,
    kind: Option<String>,
    trace_id: Option<String>,
    session_id: Option<String>,
    tick: Option<u64>,
    check_event_id: bool,
}

fn parse_args() -> anyhow::Result<Args> {
    let mut args = Args::default();
    let mut iter = std::env::args().skip(1);
    while let Some(arg) = iter.next() {
        match arg.as_str() {
            "--tlog" => args.tlog = iter.next().map(PathBuf::from),
            "--kind" => args.kind = iter.next(),
            "--trace-id" => args.trace_id = iter.next(),
            "--session-id" => args.session_id = iter.next(),
            "--tick" => {
                args.tick = iter.next().and_then(|v| v.parse::<u64>().ok());
            }
            "--check-event-id" => args.check_event_id = true,
            "--help" | "-h" => {
                print_help();
                std::process::exit(0);
            }
            other => {
                return Err(anyhow::anyhow!("unknown arg: {other}"));
            }
        }
    }
    Ok(args)
}

fn print_help() {
    eprintln!("Usage: read_tlog --tlog <path> [--kind <kind>] [--trace-id <id>] [--session-id <id>] [--tick <n>] [--check-event-id]");
}

fn event_trace_id(payload: &serde_json::Value) -> Option<&str> {
    payload.get("trace_id").and_then(|v| v.as_str()).or_else(|| payload.get("context").and_then(|v| v.get("trace_id")).and_then(|v| v.as_str()))
}

fn event_tick(payload: &serde_json::Value) -> Option<u64> {
    payload.get("tick").and_then(|v| v.as_u64())
}

fn event_runtime_session(payload: &serde_json::Value) -> Option<String> {
    payload.get("session_id").and_then(|v| v.as_str()).map(|s| s.to_string())
}

fn main() -> anyhow::Result<()> {
    let args = parse_args()?;
    let tlog = args.tlog.ok_or_else(|| anyhow::anyhow!("missing --tlog"))?;
    let events = read_any_events_from_path(&tlog)?;

    let mut current_session: Option<String> = None;
    let mut last_event_by_session: std::collections::HashMap<String, u64> = std::collections::HashMap::new();

    for event in events {
        let AnyEvent::Canon(canon) = event else {
            continue;
        };
        let kind = canon.payload.kind_str();
        let Some(payload) = canon.payload.as_value() else { continue };
        if kind == "runtime_started" {
            if let Some(session) = event_runtime_session(&payload) {
                current_session = Some(session);
            }
        }

        let session_for_event = current_session.clone();
        if let Some(want) = args.session_id.as_deref() {
            if session_for_event.as_deref() != Some(want) {
                continue;
            }
        }
        if let Some(want) = args.kind.as_deref() {
            if kind != want {
                continue;
            }
        }
        if let Some(want) = args.trace_id.as_deref() {
            if event_trace_id(&payload) != Some(want) {
                continue;
            }
        }
        if let Some(want) = args.tick {
            if event_tick(&payload) != Some(want) {
                continue;
            }
        }

        if args.check_event_id {
            if let (Some(session), Some(event_id)) = (session_for_event.as_deref(), canon.event_id) {
                if let Some(last) = last_event_by_session.get(session) {
                    if event_id <= *last {
                        eprintln!("event_id regression for session {}: current={} previous={}", session, event_id, last);
                    }
                }
                last_event_by_session.insert(session.to_string(), event_id);
            }
        }

        let out = serde_json::json!({
            "session_id": session_for_event,
            "event_id": canon.event_id,
            "ts": canon.meta.ts,
            "source": canon.meta.source,
            "kind": kind,
            "payload": payload,
        });
        println!("{}", serde_json::to_string(&out)?);
    }

    Ok(())
}
