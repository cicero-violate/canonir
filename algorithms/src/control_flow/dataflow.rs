//! Reaching definitions (forward dataflow).

use std::collections::{HashMap, HashSet};

pub type DefId = String;
pub type BlockId = usize;

pub struct DataflowFacts {
    pub r#gen: HashMap<BlockId, HashSet<DefId>>,
    pub kill: HashMap<BlockId, HashSet<DefId>>,
}

pub struct DataflowResult {
    pub r#in: HashMap<BlockId, HashSet<DefId>>,
    pub out: HashMap<BlockId, HashSet<DefId>>,
}

pub fn reaching_definitions(blocks: &[BlockId], pred: &HashMap<BlockId, Vec<BlockId>>, facts: &DataflowFacts) -> DataflowResult {
    let mut r#in: HashMap<BlockId, HashSet<DefId>> = HashMap::new();
    let mut out: HashMap<BlockId, HashSet<DefId>> = HashMap::new();
    for &b in blocks {
        r#in.insert(b, HashSet::new());
        out.insert(b, facts.r#gen.get(&b).cloned().unwrap_or_default());
    }
    let mut changed = true;
    while changed {
        changed = false;
        for &b in blocks {
            let new_in: HashSet<DefId> = pred.get(&b).map(|ps| ps.iter().flat_map(|p| out[p].iter().cloned()).collect()).unwrap_or_default();
            let kill = facts.kill.get(&b).cloned().unwrap_or_default();
            let r#gen = facts.r#gen.get(&b).cloned().unwrap_or_default();
            let new_out: HashSet<DefId> = r#gen.union(&new_in.difference(&kill).cloned().collect::<HashSet<_>>()).cloned().collect();
            if new_out != out[&b] {
                out.insert(b, new_out);
                changed = true;
            }
            r#in.insert(b, new_in);
        }
    }
    DataflowResult { r#in, out }
}
