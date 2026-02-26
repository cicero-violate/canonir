use canon::csr_graph::CsrGraph;
use canon::edge::EdgeKind;
use canon::node::{CanonId, CanonNodeKind};
use canon::CanonIR;

pub struct TypeGraphBuilder {
    v: usize,
    edges: Vec<(u32, u32, EdgeKind)>,
}

impl TypeGraphBuilder {
    pub fn new(v: usize) -> Self {
        Self { v, edges: Vec::new() }
    }

    pub fn add_type_of(&mut self, expr: CanonId, ty: CanonId) {
        self.edges.push((expr.0, ty.0, EdgeKind::TypeOf));
    }

    pub fn add_unifies(&mut self, a: CanonId, b: CanonId) {
        self.edges.push((a.0, b.0, EdgeKind::TypeUnifies));
    }

    pub fn derive_from_ir(&mut self, ir: &CanonIR) {
        for n in &ir.nodes {
            match &n.kind {
                CanonNodeKind::Fn { sig_id, .. } => {
                    if let CanonNodeKind::FnSig { params, ret, .. } = &ir.node(*sig_id).kind {
                        for p in params {
                            if let CanonNodeKind::Param { ty, .. } = &ir.node(*p).kind {
                                self.add_type_of(n.id, *ty);
                            }
                        }
                        self.add_type_of(n.id, *ret);
                    }
                }
                CanonNodeKind::Struct { fields, .. } => {
                    for f in fields {
                        if let CanonNodeKind::Field { ty, .. } = &ir.node(*f).kind {
                            self.add_type_of(*f, *ty);
                        }
                    }
                }
                CanonNodeKind::Field { ty, .. } | CanonNodeKind::Param { ty, .. } | CanonNodeKind::Const { ty, .. } | CanonNodeKind::Static { ty, .. } | CanonNodeKind::TypeAlias { ty, .. } => {
                    self.add_type_of(n.id, *ty);
                }
                CanonNodeKind::Impl { for_ty, for_trait, .. } => {
                    self.add_type_of(n.id, *for_ty);
                    if let Some(t) = for_trait {
                        self.edges.push((n.id.0, t.0, EdgeKind::ImplTrait));
                    }
                }
                _ => {}
            }
        }
    }

    pub fn edges(&self) -> &[(u32, u32, EdgeKind)] {
        &self.edges
    }

    pub fn build(self) -> CsrGraph<CanonId, EdgeKind> {
        let node_ids: Vec<CanonId> = (0..self.v as u32).map(CanonId).collect();
        CsrGraph::from_edges(node_ids, self.edges)
    }
}
