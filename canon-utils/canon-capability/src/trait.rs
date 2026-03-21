use crate::{CapabilityExecutionContext, CapabilityExecutionResult};
use anyhow::Result;

#[derive(Debug, Clone)]
pub struct CapabilitySchema {
    pub name: &'static str,
    /// Deprecated. Args are encoded in typed CanonEvent variants, not here.
    #[allow(deprecated)]
    pub args: Vec<ArgSpec>,
}

#[deprecated(note = "use typed CanonEvent fields directly")]
#[derive(Debug, Clone)]
pub struct ArgSpec {
    pub key: &'static str,
    #[allow(deprecated)]
    pub kind: ArgKind,
    pub required: bool,
}

#[deprecated(note = "use typed CanonEvent fields directly")]
#[derive(Debug, Clone)]
pub enum ArgKind {
    String,
    Path,
    Symbol,
    Json,
}

pub trait CapabilityHandler: Send + Sync {
    fn name(&self) -> &'static str;

    /// Primary entrypoint. Override this; default bridges to deprecated execute().
    fn handle(&self, ctx: CapabilityExecutionContext) -> Result<CapabilityExecutionResult> {
        #[allow(deprecated)]
        self.execute(ctx)
    }

    /// Deprecated. Implement handle() instead.
    #[deprecated(note = "implement handle() instead")]
    fn execute(&self, ctx: CapabilityExecutionContext) -> Result<CapabilityExecutionResult> {
        self.handle(ctx)
    }

    /// Deprecated. Schema is derived from typed CanonEvent variants.
    #[deprecated(note = "schema is derived from CanonEvent variants")]
    fn schema(&self) -> CapabilitySchema {
        CapabilitySchema { name: self.name(), args: Vec::new() }
    }
}
