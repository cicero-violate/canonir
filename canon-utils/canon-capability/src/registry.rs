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

    /// Route a typed capability event to handlers.
    pub fn route(&self, ctx: CapabilityExecutionContext) -> Result<CapabilityExecutionResult> {
        let is_cap_event = matches!(
            ctx.event,
            canon_event::CanonEvent::Edit(_)
                | canon_event::CanonEvent::Cargo(_)
                | canon_event::CanonEvent::File(_)
                | canon_event::CanonEvent::Bash(_)
                | canon_event::CanonEvent::Llm(_)
                | canon_event::CanonEvent::Analysis(_)
        );
        if !is_cap_event {
            return Ok(CapabilityExecutionResult::NoOp);
        }

        for handler in self.map.values() {
            let result = handler.handle(ctx.clone())?;
            if !matches!(result, CapabilityExecutionResult::NoOp) {
                return Ok(result);
            }
        }
        Ok(CapabilityExecutionResult::NoOp)
    }
}
