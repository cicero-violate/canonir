use std::collections::HashSet;

use super::capability::{assert_mut_verify_disjoint, Capability};

#[derive(Debug, Clone)]
pub struct AuthorityContext {
    pub node_id: String,
    pub capabilities: HashSet<Capability>,
}

impl AuthorityContext {
    pub fn new(node_id: String, caps: HashSet<Capability>) -> Result<Self, String> {
        assert_mut_verify_disjoint(&caps)?;
        Ok(Self { node_id, capabilities: caps })
    }

    pub fn has(&self, cap: Capability) -> bool {
        self.capabilities.contains(&cap)
    }

    pub fn require(&self, cap: Capability) -> Result<(), String> {
        if self.has(cap) {
            Ok(())
        } else {
            Err(format!("node {} missing capability {:?}", self.node_id, cap))
        }
    }

    pub fn is_verify_context(&self) -> bool {
        self.capabilities.contains(&Capability::StatusUpdateOnly)
    }

    pub fn is_mutation_context(&self) -> bool {
        self.capabilities.contains(&Capability::FileWrite) || self.capabilities.contains(&Capability::ApplyPatch)
    }
}
