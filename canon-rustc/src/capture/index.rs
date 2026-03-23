use rustc_hash::FxHashMap;
use rustc_middle::ty::TyCtxt;
use rustc_span::def_id::DefId;

use crate::capture::types::NodeId;

/// Stable mapping from DefId -> NodeId (sorted order for determinism).
#[derive(Debug, Default, Clone)]
pub struct Index {
    pub def_ids: Vec<DefId>,
    pub def_to_node: FxHashMap<DefId, NodeId>,
}

/// Build deterministic DefId -> NodeId index.
/// Ordering: sort by def_path_str for stability across runs.
pub fn build_index(tcx: TyCtxt<'_>) -> Index {
    // Collect all local item DefIds from HIR crate items query.
    let crate_items = tcx.hir_crate_items(());
    // Filter: exclude compiler-generated items (derive impls, synthetic MIR).
    // These produce Impl nodes with qualified names like "<Point as Clone>"
    // that the invariant solver cannot resolve to user-defined structs.
    let mut pairs: Vec<(String, DefId)> =
        crate_items
            .definitions()
            .map(|id| id.to_def_id())
            .filter(|&d| !tcx.is_automatically_derived(d))
            .filter(|&d| !tcx.is_synthetic_mir(d))
            .map(|d| (tcx.def_path_str(d), d))
            .collect();
    pairs.sort_by(|a, b| a.0.cmp(&b.0));
    let mut def_ids: Vec<DefId> = pairs.into_iter().map(|(_, d)| d).collect();

    // Enforce parent-chain closure: remove any def whose parent is not indexed.
    let mut set: std::collections::HashSet<DefId> = def_ids.iter().copied().collect();
    let mut changed = true;
    while changed {
        changed = false;
        let current: Vec<DefId> = set.iter().copied().collect();
        for def_id in current {
            if let Some(parent) = tcx.opt_parent(def_id) {
                if !set.contains(&parent) {
                    set.remove(&def_id);
                    changed = true;
                }
            }
        }
    }
    def_ids.retain(|d| set.contains(d));

    // Assign NodeIds by rank.
    let mut def_to_node = FxHashMap::default();
    for (rank, def_id) in def_ids.iter().enumerate() {
        def_to_node.insert(*def_id, NodeId(rank as u32));
    }

    Index { def_ids, def_to_node }
}
