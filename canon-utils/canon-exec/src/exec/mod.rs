use canon_event::{AnalysisEvent, BashInvoke, RuntimeEvent, CargoEvent, EditEvent, EventId, FileEvent, LlmCall};
use canon_event::EventEmitterHandle;
use std::path::PathBuf;

pub mod analysis;
pub mod bash;
pub mod cargo;
pub mod edit;
pub mod file;
pub mod llm;

#[derive(Clone)]
pub struct ExecutionContext {
    pub workspace: PathBuf,
    pub emitter: EventEmitterHandle,
    pub trigger_id: EventId,
}

#[derive(Debug)]
pub enum ExecutionResult {
    Emit(RuntimeEvent),
    EmitMany(Vec<RuntimeEvent>),
    Deferred,
}

pub trait Executable {
    fn execute(self, ctx: ExecutionContext) -> anyhow::Result<ExecutionResult>;
}

pub enum ExecutableEvent {
    Edit(EditEvent),
    Cargo(CargoEvent),
    File(FileEvent),
    Bash(BashInvoke),
    Llm(LlmCall),
    Analysis(AnalysisEvent),
}

impl ExecutableEvent {
    pub fn execute(self, ctx: ExecutionContext) -> anyhow::Result<ExecutionResult> {
        match self {
            ExecutableEvent::Edit(e) => e.execute(ctx),
            ExecutableEvent::Cargo(e) => e.execute(ctx),
            ExecutableEvent::File(e) => e.execute(ctx),
            ExecutableEvent::Bash(e) => e.execute(ctx),
            ExecutableEvent::Llm(e) => e.execute(ctx),
            ExecutableEvent::Analysis(e) => e.execute(ctx),
        }
    }
}

impl TryFrom<RuntimeEvent> for ExecutableEvent {
    type Error = RuntimeEvent;
    fn try_from(e: RuntimeEvent) -> Result<Self, RuntimeEvent> {
        match e {
            RuntimeEvent::Edit(e) => Ok(ExecutableEvent::Edit(e)),
            RuntimeEvent::Cargo(e) => Ok(ExecutableEvent::Cargo(e)),
            RuntimeEvent::File(e) => Ok(ExecutableEvent::File(e)),
            RuntimeEvent::Bash(e) => Ok(ExecutableEvent::Bash(e)),
            RuntimeEvent::Llm(e) => Ok(ExecutableEvent::Llm(e)),
            RuntimeEvent::Analysis(e) => Ok(ExecutableEvent::Analysis(e)),
            other => Err(other),
        }
    }
}
