use crate::{CapabilityHandler, CapabilityExecutionContext, CapabilityExecutionResult};
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
        let capability = self
            .map
            .get(name)
            .ok_or_else(|| anyhow!("capability not registered: {name}"))?;
        capability.execute(ctx)
    }

    pub fn names(&self) -> Vec<String> {
        let mut names: Vec<String> = self.map.keys().cloned().collect();
        names.sort();
        names
    }
}
