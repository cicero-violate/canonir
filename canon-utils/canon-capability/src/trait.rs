use crate::{CapabilityExecutionContext, CapabilityExecutionResult};
use anyhow::Result;

pub trait CapabilityHandler: Send + Sync {
    fn name(&self) -> &'static str;

    fn execute(&self, ctx: CapabilityExecutionContext) -> Result<CapabilityExecutionResult>;
}
