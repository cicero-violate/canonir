use model::ir::{
    edge::{EdgeHint, EdgeKind},
    model_ir::ModelIR,
    node::{Node, NodeId, NodeKind},
};
use rustc_middle::ty::TyCtxt;
use rustc_span::def_id::LOCAL_CRATE;

use crate::{index::Index, Partial};

/// Deterministic merge of partial projections into a ModelIR.
/// A synthetic Crate node is prepended at NodeId(0); all index NodeIds are
/// shifted up by 1 so the index remains dense and the Crate is the root.
pub fn assemble(tcx: TyCtxt<'_>, index: Index, parts: Vec<Partial>) -> ModelIR {
    let mut ir = ModelIR::new();

    // ── Crate node at NodeId(0) ──────────────────────────────────────────────
    let crate_name = tcx.crate_name(LOCAL_CRATE).to_string();
    let edition = format!("{}", tcx.sess.edition());
    let crate_node = Node {
        id: NodeId(0),
        kind: NodeKind::Crate {
            name: crate_name,
            edition,
        },
        span: None,
    };

    // ── Merge partials, shifting all NodeIds by +1 ───────────────────────────
    let mut nodes: Vec<Node> = vec![crate_node];
    for part in parts {
        for mut node in part.nodes {
            node.id = NodeId(node.id.0 + 1);
            nodes.push(node);
        }
        for mut hint in part.edge_hints {
            hint.src += 1;
            hint.dst += 1;
            ir.edge_hints.push(hint);
        }
    }

    ir.nodes = nodes;

    // ── Contains edges: Crate(0) -> every top-level node ────────────────────
    // "Top-level" = nodes whose parent in the index is the crate root DefId,
    // i.e. tcx.opt_parent(def_id) is None.
    // We approximate this by emitting Contains from 0 to every Module node
    // whose parent has no entry in def_to_node (the crate root itself has no
    // DefId in our index).
    for (def_id, &node_id) in &index.def_to_node {
        if tcx.opt_parent(*def_id).map_or(true, |p| !index.def_to_node.contains_key(&p)) {
            ir.edge_hints.push(EdgeHint {
                src: 0,
                dst: node_id.0 + 1,
                kind: EdgeKind::Contains,
            });
        }
    }

    // Graphs will be built by analyzer; capture only produces edge_hints + nodes.
    ir
}
