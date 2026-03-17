use crate::smt::loader::{AnalysisGraph, EdgeKind, NodeKind};
use std::collections::HashMap;
use z3::ast::{Ast, Bool, BV};
use z3::Context;

pub struct EncodedGraph<'ctx> {
    ctx: &'ctx Context,
    pub bb: HashMap<u32, Bool<'ctx>>,
    pub var: HashMap<u32, BV<'ctx>>,
    pub err: HashMap<u32, Bool<'ctx>>,
    pub flow_imps: Vec<Bool<'ctx>>,
    pub assign_eqs: Vec<Bool<'ctx>>,
    pub prop_eqs: Vec<Bool<'ctx>>,
    pub err_imps: Vec<Bool<'ctx>>,
}

impl<'ctx> EncodedGraph<'ctx> {
    pub fn build(graph: &AnalysisGraph, ctx: &'ctx Context) -> Self {
        let mut bb = HashMap::new();
        let mut var = HashMap::new();
        let mut err = HashMap::new();

        for node in &graph.nodes {
            match node.kind {
                NodeKind::BasicBlock => {
                    bb.insert(node.id, Bool::new_const(ctx, format!("bb_{}", node.id)));
                }
                NodeKind::Variable | NodeKind::Param => {
                    var.insert(node.id, BV::new_const(ctx, format!("val_{}", node.id), 32));
                }
                NodeKind::Error => {
                    err.insert(node.id, Bool::new_const(ctx, format!("err_{}", node.id)));
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
                        assign_eqs.push(dst._eq(src));
                    }
                }
                EdgeKind::Propagates => {
                    if let (Some(src), Some(dst)) = (var.get(&edge.src), var.get(&edge.dst)) {
                        prop_eqs.push(dst._eq(src));
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
            ctx,
            bb,
            var,
            err,
            flow_imps,
            assign_eqs,
            prop_eqs,
            err_imps,
        }
    }

    pub fn build_scoped(graph: &AnalysisGraph, ctx: &'ctx Context, fn_id: u32) -> Self {
        let mut bb = HashMap::new();
        let mut var = HashMap::new();
        let mut err = HashMap::new();

        let mut block_ids = Vec::new();
        let mut block_set = std::collections::HashSet::new();
        for e in &graph.edges {
            if e.kind == EdgeKind::HasBlock && e.src == fn_id {
                if let Some(node) = graph.id_to_index.get(&e.dst).and_then(|&i| graph.nodes.get(i)) {
                    if node.kind == NodeKind::BasicBlock {
                        block_ids.push(node.id);
                        block_set.insert(node.id);
                    }
                }
            }
        }

        for id in &block_ids {
            bb.insert(*id, Bool::new_const(ctx, format!("bb_{}", id)));
        }
        for node in &graph.nodes {
            if matches!(node.kind, NodeKind::Variable | NodeKind::Param) {
                var.insert(node.id, BV::new_const(ctx, format!("val_{}", node.id), 32));
            }
            if node.kind == NodeKind::Error {
                err.insert(node.id, Bool::new_const(ctx, format!("err_{}", node.id)));
            }
        }

        let mut flow_imps = Vec::new();
        let mut assign_eqs = Vec::new();
        let mut prop_eqs = Vec::new();
        let mut err_imps = Vec::new();

        for edge in &graph.edges {
            match edge.kind {
                EdgeKind::Flow => {
                    if block_set.contains(&edge.src) && block_set.contains(&edge.dst) {
                        if let (Some(src), Some(dst)) = (bb.get(&edge.src), bb.get(&edge.dst)) {
                            flow_imps.push(src.implies(dst));
                        }
                    }
                }
                EdgeKind::Assign => {
                    if let (Some(src), Some(dst)) = (var.get(&edge.src), var.get(&edge.dst)) {
                        assign_eqs.push(dst._eq(src));
                    }
                }
                EdgeKind::Propagates => {
                    if let (Some(src), Some(dst)) = (var.get(&edge.src), var.get(&edge.dst)) {
                        prop_eqs.push(dst._eq(src));
                    }
                }
                EdgeKind::ErrorToBlock => {
                    if block_set.contains(&edge.dst) {
                        if let (Some(block), Some(err_node)) = (bb.get(&edge.dst), err.get(&edge.src)) {
                            err_imps.push(block.implies(err_node));
                        }
                    }
                }
                _ => {}
            }
        }

        Self {
            ctx,
            bb,
            var,
            err,
            flow_imps,
            assign_eqs,
            prop_eqs,
            err_imps,
        }
    }

    pub fn assert_all(&self, solver: &z3::Solver<'ctx>) {
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

    pub fn ctx(&self) -> &'ctx Context {
        self.ctx
    }
}
