use canon_event::{AnalysisEvent, BashInvoke, CanonEvent, CargoEvent, EditEvent, FileEvent, LlmCall};
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
}

#[derive(Debug)]
pub enum ExecutionResult {
    Emit(CanonEvent),
    EmitMany(Vec<CanonEvent>),
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

impl TryFrom<CanonEvent> for ExecutableEvent {
    type Error = CanonEvent;
    fn try_from(e: CanonEvent) -> Result<Self, CanonEvent> {
        match e {
            CanonEvent::Edit(e) => Ok(ExecutableEvent::Edit(e)),
            CanonEvent::Cargo(e) => Ok(ExecutableEvent::Cargo(e)),
            CanonEvent::File(e) => Ok(ExecutableEvent::File(e)),
            CanonEvent::Bash(e) => Ok(ExecutableEvent::Bash(e)),
            CanonEvent::Llm(e) => Ok(ExecutableEvent::Llm(e)),
            CanonEvent::Analysis(e) => Ok(ExecutableEvent::Analysis(e)),
            other => Err(other),
        }
    }
}
