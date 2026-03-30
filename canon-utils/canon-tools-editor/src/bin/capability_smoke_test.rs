use std::path::PathBuf;
use std::sync::Arc;

use anyhow::Result;
use canon_event::{EditEvent, EventEmitter, EventEmitterHandle, EventId, RenameSymbol, RuntimeEvent};
use canon_exec::{ExecutableEvent, ExecutionContext};

struct NullEmitter;

impl EventEmitter for NullEmitter {
    fn emit_with_parents(&self, _event: RuntimeEvent, _parents: Vec<canon_event::EventId>, _file: &'static str, _line: u32) {}
}

fn main() -> Result<()> {
    let emitter: EventEmitterHandle = Arc::new(NullEmitter);
    let ctx = ExecutionContext { workspace: PathBuf::from("/workspace/ai_sandbox/canon"), emitter, trigger_id: EventId::new("root") };

    let event = RuntimeEvent::Edit(EditEvent::RenameSymbol(RenameSymbol { project: "p".into(), old: "a".into(), new: "b".into() }));
    let exec = ExecutableEvent::try_from(event).expect("edit should be executable");
    let _ = exec.execute(ctx)?;

    println!("capability_invariant_test: PASS (1)");
    Ok(())
}
