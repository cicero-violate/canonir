use crate::loader::{AnalysisGraph, EdgeKind, NodeKind};
use std::collections::HashMap;
use z3::ast::{Bool, BV};

pub struct EncodedGraph {
    pub bb: HashMap<u32, Bool>,
    pub var: HashMap<u32, BV>,
    pub err: HashMap<u32, Bool>,
    pub flow_imps: Vec<Bool>,
    pub assign_eqs: Vec<Bool>,
    pub prop_eqs: Vec<Bool>,
    pub err_imps: Vec<Bool>,
}

impl EncodedGraph {
    pub fn build(graph: &AnalysisGraph) -> Self {
        let mut bb = HashMap::new();
        let mut var = HashMap::new();
        let mut err = HashMap::new();

        for node in &graph.nodes {
            match node.kind {
                NodeKind::BasicBlock => {
                    bb.insert(node.id, Bool::new_const(format!("bb_{}", node.id)));
                }
                NodeKind::Variable => {
                    var.insert(node.id, BV::new_const(format!("val_{}", node.id), 32));
                }
                NodeKind::Error => {
                    err.insert(node.id, Bool::new_const(format!("err_{}", node.id)));
                }
                _ => {}
            }
        }

        let mut flow_imps = Vec::new();
        let mut assign_eqs = Vec::new();
        let mut prop_eqs = Vec::new();
        let mut err_imps = Vec::new();

        for edge in &graph.edges {
            match edge.kind {
                EdgeKind::Flow => {
                    if let (Some(src), Some(dst)) = (bb.get(&edge.src), bb.get(&edge.dst)) {
                        flow_imps.push(src.implies(dst));
                    }
                }
                EdgeKind::Assign => {
                    if let (Some(src), Some(dst)) = (var.get(&edge.src), var.get(&edge.dst)) {
                        assign_eqs.push(dst.eq(src));
                    }
                }
                EdgeKind::Propagates => {
                    if let (Some(src), Some(dst)) = (var.get(&edge.src), var.get(&edge.dst)) {
                        prop_eqs.push(dst.eq(src));
                    }
                }
                EdgeKind::ErrorToBlock => {
                    if let (Some(block), Some(err_node)) = (bb.get(&edge.dst), err.get(&edge.src)) {
                        err_imps.push(block.implies(err_node));
                    }
                }
                _ => {}
            }
        }

        Self {
            bb,
            var,
            err,
            flow_imps,
            assign_eqs,
            prop_eqs,
            err_imps,
        }
    }

    pub fn assert_all(&self, solver: &z3::Solver) {
        for imp in &self.flow_imps {
            solver.assert(imp);
        }
        for eq in &self.assign_eqs {
            solver.assert(eq);
        }
        for eq in &self.prop_eqs {
            solver.assert(eq);
        }
        for imp in &self.err_imps {
            solver.assert(imp);
        }
    }
}
