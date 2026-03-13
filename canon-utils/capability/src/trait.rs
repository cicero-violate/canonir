use crate::{CapabilityContext, CapabilityResult};
use anyhow::Result;

pub trait Capability: Send + Sync {
    fn name(&self) -> &'static str;

    fn execute(&self, ctx: CapabilityContext) -> Result<CapabilityResult>;
}
