//! Use-def chain analysis for local variable tracking.
//!
//! Variables:
//!   defs[v] : set of block indices where variable v is defined
//!   uses[v] : set of block indices where variable v is used
//!   chains[v] : for each use of v, the set of reaching definitions
//!
//! Equations:
//!   chains[v][use_site] = { d in defs[v] | d reaches use_site }
//!   reaches(d, u) = u in reachable(d, CFG) and no intervening def of v
//!
//! Used by: canon-capture to track closure locals and prevent unit collapse.

use std::collections::{HashMap, HashSet};

pub type VarId = usize;
pub type BlockId = usize;

#[derive(Debug, Clone, Default)]
pub struct UseDefFacts {
    /// defs[var] = set of blocks that define var
    pub defs: HashMap<VarId, HashSet<BlockId>>,
    /// uses[var] = set of blocks that use var
    pub uses: HashMap<VarId, HashSet<BlockId>>,
}

#[derive(Debug, Clone, Default)]
pub struct UseDefChains {
    /// chains[var][use_block] = set of definition blocks that reach this use
    pub chains: HashMap<VarId, HashMap<BlockId, HashSet<BlockId>>>,
}

/// Build use-def chains for all variables given CFG adjacency and facts.
pub fn build_use_def_chains(
    adj: &[Vec<BlockId>],
    facts: &UseDefFacts,
) -> UseDefChains {
    let n = adj.len();
    let mut result = UseDefChains::default();

    for (&var, def_blocks) in &facts.defs {
        let use_blocks = facts.uses.get(&var).cloned().unwrap_or_default();
        let mut var_chains: HashMap<BlockId, HashSet<BlockId>> = HashMap::new();

        for &use_block in &use_blocks {
            let mut reaching = HashSet::new();
            for &def_block in def_blocks {
                if def_block == use_block || can_reach_without_redef(def_block, use_block, adj, n, def_blocks) {
                    reaching.insert(def_block);
                }
            }
            var_chains.insert(use_block, reaching);
        }
        result.chains.insert(var, var_chains);
    }
    result
}

fn can_reach_without_redef(
    from: BlockId,
    to: BlockId,
    adj: &[Vec<BlockId>],
    n: usize,
    redefs: &HashSet<BlockId>,
) -> bool {
    if from == to { return true; }
    let mut visited = vec![false; n];
    let mut stack = vec![from];
    while let Some(u) = stack.pop() {
        if visited[u] { continue; }
        visited[u] = true;
        for &v in &adj[u] {
            if v >= n { continue; }
            if v == to { return true; }
            // Stop at re-definitions of the variable (kills this def chain).
            if redefs.contains(&v) { continue; }
            stack.push(v);
        }
    }
    false
}
