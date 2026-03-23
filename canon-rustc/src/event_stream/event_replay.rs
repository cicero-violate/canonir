use anyhow::Result;

use crate::event_stream::delta::EventDelta;
use crate::event_stream::event_engine::apply_delta;
use crate::event_stream::event::RustcEvent;
use canon_event::{CanonEvent, CanonPayload};
use std::fs::File;
use std::io::{BufRead, BufReader};
use std::path::Path;
use crate::rustc::state::RustcState;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum DispatchMode {
    Batch,
    Streaming,
}

pub fn replay_events(
    state: &mut RustcState,
    deltas: &[EventDelta],
    mode: DispatchMode,
) -> Result<()> {
    match mode {
        DispatchMode::Batch => {
            for delta in deltas {
                apply_delta(state, delta)?;
            }
        }
        DispatchMode::Streaming => {
            for delta in deltas {
                apply_delta(state, delta)?;
            }
        }
    }
    Ok(())
}

pub fn replay_tlog(
    state: &mut RustcState,
    tlog_path: &Path,
    mode: DispatchMode,
) -> Result<()> {
    let deltas = read_tlog_deltas(tlog_path)?;
    replay_events(state, &deltas, mode)
}

fn read_tlog_deltas(path: &Path) -> Result<Vec<EventDelta>> {
    let file = File::open(path)?;
    let reader = BufReader::new(file);
    let mut out = Vec::new();
    let mut next_id: u64 = 1;
    let mut tick: u64 = 0;

    for line in reader.lines() {
        let line = line?;
        if line.trim().is_empty() {
            continue;
        }
        let canon: CanonEvent = match serde_json::from_str(&line) {
            Ok(v) => v,
            Err(_) => continue,
        };
        let event = match canon.payload {
            CanonPayload::RustcEvent(v) => serde_json::from_value::<RustcEvent>(v).ok(),
            _ => None,
        };
        let Some(event) = event else { continue; };
        if matches!(event, RustcEvent::SessionStart(_)) {
            next_id = 1;
            tick = 0;
            out.push(EventDelta { id: 0, tick: 0, event });
            continue;
        }
        tick = tick.saturating_add(1);
        out.push(EventDelta { id: next_id, tick, event });
        next_id = next_id.saturating_add(1);
    }
    Ok(out)
}
