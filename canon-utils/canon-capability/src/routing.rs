// Routing module for capability execution
// This module will handle the routing of capability requests to the appropriate executor

use crate::{CapabilityRegistry, CapabilityExecutionContext, CapabilityExecutionResult};
use anyhow::Result;

/// Routes capability execution through the registry
pub fn route_capability(
    registry: &CapabilityRegistry,
    name: &str,
    ctx: CapabilityExecutionContext,
) -> Result<CapabilityExecutionResult> {
    registry.execute(name, ctx)
}
