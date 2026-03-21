use crate::r#trait::CapabilitySchema;
use crate::{CapabilityExecutionContext, CapabilityExecutionResult, CapabilityHandler};
use anyhow::{anyhow, Result};
use std::collections::HashMap;
use std::sync::Arc;

#[derive(Default)]
pub struct CapabilityRegistry {
    map: HashMap<String, Arc<dyn CapabilityHandler>>,
}

impl CapabilityRegistry {
    pub fn new() -> Self {
        Self { map: HashMap::new() }
    }

    pub fn register(&mut self, capability: Arc<dyn CapabilityHandler>) {
        self.map.insert(capability.name().to_string(), capability);
    }

    pub fn lookup(&self, name: &str) -> Option<Arc<dyn CapabilityHandler>> {
        self.map.get(name).cloned()
    }

    pub fn execute(&self, name: &str, ctx: CapabilityExecutionContext) -> Result<CapabilityExecutionResult> {
        let capability = self.map.get(name).ok_or_else(|| anyhow!("capability not registered: {name}"))?;
        capability.handle(ctx)
    }

    pub fn names(&self) -> Vec<String> {
        let mut names: Vec<String> = self.map.keys().cloned().collect();
        names.sort();
        names
    }

    #[allow(deprecated)]
    pub fn schemas(&self) -> Vec<CapabilitySchema> {
        self.map.values().map(|h| h.schema()).collect()
    }

    /// Route a CanonEvent (during migration) via decode bridge.
    pub fn route(&self, ctx: CapabilityExecutionContext) -> Result<CapabilityExecutionResult> {
        let name = match &ctx.event {
            canon_event::CanonEvent::CapabilityRequested(req) => req.name.clone(),
            _ => return Ok(CapabilityExecutionResult::NoOp),
        };

        // Decode CapabilityRequested args into typed CanonEvent
        let typed_event = crate::decode::decode_capability_event(&ctx.event)?;
        let ctx = CapabilityExecutionContext { event: typed_event, ..ctx };
        self.execute(&name, ctx)
    }
}
