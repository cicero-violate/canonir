use crate::{CapabilityExecutionContext, CapabilityExecutionResult};
use anyhow::Result;

pub trait CapabilityHandler: Send + Sync {
    fn name(&self) -> &'static str;
    fn handle(&self, ctx: CapabilityExecutionContext) -> Result<CapabilityExecutionResult>;
}
