// Routing module for capability execution
// This module will handle the routing of capability requests to the appropriate executor

use crate::{CapabilityRegistry, CapabilityContext, CapabilityResult};
use anyhow::Result;

/// Routes capability execution through the registry
pub fn route_capability(
    registry: &CapabilityRegistry,
    name: &str,
    ctx: CapabilityContext,
) -> Result<CapabilityResult> {
    registry.execute(name, ctx)
}
