use crate::{ChangeSet, GraphEdge};
use canon::{edge::EdgeKind, id::NodeId};
use canon::{node::CanonId, CanonIR};
use std::collections::HashSet;

fn extract_edges(ir: &CanonIR) -> Vec<GraphEdge> {
    fn from_graph(src: &canon::csr_graph::CsrGraph<CanonId, EdgeKind>) -> Vec<GraphEdge> {
        let mut edges = Vec::new();
        for i in 0..src.vertex_count() {
            for (dst, kind) in src.neighbours(NodeId(i as u32)) {
                edges.push(GraphEdge { src: CanonId(i as u32), dst: CanonId(dst.0), kind: kind.clone() });
            }
        }
        edges
    }

    let mut out = Vec::new();
    out.extend(from_graph(&ir.name_graph));
    out.extend(from_graph(&ir.type_graph));
    out.extend(from_graph(&ir.call_graph));
    out.extend(from_graph(&ir.module_graph));
    out.extend(from_graph(&ir.cfg_graph));
    out.extend(from_graph(&ir.region_graph));
    out.extend(from_graph(&ir.value_graph));
    out.extend(from_graph(&ir.macro_graph));
    out
}

#[derive(PartialEq, Eq, Hash)]
struct EdgeKey(String);

impl From<&GraphEdge> for EdgeKey {
    fn from(e: &GraphEdge) -> Self {
        EdgeKey(format!("{}-{}-{:?}", e.src.0, e.dst.0, e.kind))
    }
}

pub fn diff(before: &CanonIR, after: &CanonIR) -> ChangeSet {
    let bn = before.nodes.len();
    let an = after.nodes.len();

    let added_nodes: Vec<CanonId> = (bn..an).map(|i| CanonId(i as u32)).collect();
    let removed_nodes: Vec<CanonId> = (an..bn).map(|i| CanonId(i as u32)).collect();

    let mut changed_nodes = Vec::new();
    for i in 0..bn.min(an) {
        let bk = &before.nodes[i].kind;
        let ak = &after.nodes[i].kind;
        if bk != ak {
            changed_nodes.push((CanonId(i as u32), bk.clone(), ak.clone()));
        }
    }

    let before_edges = extract_edges(before);
    let after_edges = extract_edges(after);
    let before_keys: HashSet<EdgeKey> = before_edges.iter().map(EdgeKey::from).collect();
    let after_keys: HashSet<EdgeKey> = after_edges.iter().map(EdgeKey::from).collect();

    let added_edges: Vec<GraphEdge> = after_edges.into_iter().filter(|e| !before_keys.contains(&EdgeKey::from(e))).collect();
    let removed_edges: Vec<GraphEdge> = before_edges.into_iter().filter(|e| !after_keys.contains(&EdgeKey::from(e))).collect();

    ChangeSet { added_nodes, removed_nodes, changed_nodes, added_edges, removed_edges }
}
