//! Seal pass: ModelIR -> CanonIR
//!
//! Variables:
//!   model : &ModelIR          — source fat-node IR
//!   canon : CanonIR           — destination thin-node IR (built in place)
//!   id_map: Vec<CanonId>      — model NodeId.index() -> CanonId
//!
//! Equations:
//!   id_map[i] = canon.push_node(...)   for each model.nodes[i]
//!   ty_str -> intern_type(TypeKind::Extern(path_id))   — all string types become Extern refs
//!   edge_hints -> 8 CsrGraphs via from_edges()
//!   emit_order[i] = id_map[model.emit_order[i].index()]

use model::ir::{
    csr_graph::CsrGraph,
    edge::{EdgeHint, EdgeKind},
    model_ir::ModelIR,
    node::{Body, EnumVariant, Field, GenericParam, NodeId, NodeKind, StructKind, TraitMethod, Visibility},
};

use crate::{
    intern::{NameId, PathId},
    ir::CanonIR,
    node::{flags, CanonId, CanonNodeKind, CfgOp, PrimTy, TypeKind},
};

// ── Visibility -> flags bits ──────────────────────────────────────────────────

fn vis_flags(v: &Visibility) -> u32 {
    match v {
        Visibility::Public => flags::PUB,
        Visibility::PubCrate => flags::PUB_CRATE,
        Visibility::PubSuper => flags::PUB_SUPER,
        Visibility::PubIn(_) => flags::PUB_CRATE, // conservative fallback
        Visibility::Private => 0,
    }
}

// ── String type -> TypeKind ───────────────────────────────────────────────────
// Resolves known primitives; everything else becomes Extern(path_id).

fn str_to_type_kind(canon: &mut CanonIR, ty: &str) -> TypeKind {
    match ty.trim() {
        "bool" => TypeKind::Primitive(PrimTy::Bool),
        "char" => TypeKind::Primitive(PrimTy::Char),
        "str" => TypeKind::Primitive(PrimTy::Str),
        "u8" => TypeKind::Primitive(PrimTy::U8),
        "u16" => TypeKind::Primitive(PrimTy::U16),
        "u32" => TypeKind::Primitive(PrimTy::U32),
        "u64" => TypeKind::Primitive(PrimTy::U64),
        "u128" => TypeKind::Primitive(PrimTy::U128),
        "usize" => TypeKind::Primitive(PrimTy::Usize),
        "i8" => TypeKind::Primitive(PrimTy::I8),
        "i16" => TypeKind::Primitive(PrimTy::I16),
        "i32" => TypeKind::Primitive(PrimTy::I32),
        "i64" => TypeKind::Primitive(PrimTy::I64),
        "i128" => TypeKind::Primitive(PrimTy::I128),
        "isize" => TypeKind::Primitive(PrimTy::Isize),
        "f32" => TypeKind::Primitive(PrimTy::F32),
        "f64" => TypeKind::Primitive(PrimTy::F64),
        "()" => TypeKind::Primitive(PrimTy::Unit),
        "!" => TypeKind::Primitive(PrimTy::Never),
        other => {
            let pid = PathId(canon.path_intern.intern(other));
            TypeKind::Extern(pid)
        }
    }
}

fn intern_ty(canon: &mut CanonIR, ty: &str) -> CanonId {
    let kind = str_to_type_kind(canon, ty);
    canon.intern_type(kind)
}

fn unit_ty(canon: &mut CanonIR) -> CanonId {
    canon.intern_type(crate::node::TypeKind::Primitive(PrimTy::Unit))
}

fn bool_ty(canon: &mut CanonIR) -> CanonId {
    canon.intern_type(crate::node::TypeKind::Primitive(PrimTy::Bool))
}

fn seal_generic_param(canon: &mut CanonIR, gp: &GenericParam) -> CanonId {
    let name_id = NameId(canon.name_intern.intern(&gp.name));
    let bounds = gp
        .bounds
        .iter()
        .map(|b| {
            let bid = NameId(canon.name_intern.intern(b));
            canon.push_node(CanonNodeKind::TypeRef { name_id: bid })
        })
        .collect();
    let default_ty = gp.default_ty.as_deref().map(|t| intern_ty(canon, t));
    canon.push_node(CanonNodeKind::GenericParam { name_id, bounds, is_lifetime: gp.is_lifetime, default_ty })
}

fn seal_field(canon: &mut CanonIR, f: &Field) -> CanonId {
    let name_id = f.name.as_deref().map(|n| NameId(canon.name_intern.intern(n)));
    let ty = intern_ty(canon, &f.ty);
    canon.push_node(CanonNodeKind::Field { name_id, ty, flags: vis_flags(&f.vis) })
}

fn seal_variant(canon: &mut CanonIR, v: &EnumVariant) -> CanonId {
    let name_id = NameId(canon.name_intern.intern(&v.name));
    let fields = v.fields.iter().map(|f| seal_field(canon, f)).collect();
    canon.push_node(CanonNodeKind::Variant { name_id, fields })
}

fn seal_trait_method(canon: &mut CanonIR, m: &TraitMethod) -> CanonId {
    let name_id = NameId(canon.name_intern.intern(&m.name));
    let ret_id = intern_ty(canon, &m.ret);
    let param_ids: Vec<CanonId> = m
        .params
        .iter()
        .map(|p| {
            let pname = NameId(canon.name_intern.intern(&p.name));
            let pty = intern_ty(canon, &p.ty);
            let mut pf = 0u32;
            if p.mutable {
                pf |= flags::MUT;
            }
            canon.push_node(CanonNodeKind::Param { name_id: pname, ty: pty, flags: pf })
        })
        .collect();
    let generics = m.generics.iter().map(|g| seal_generic_param(canon, g)).collect();
    let sig_id = canon.push_node(CanonNodeKind::FnSig { generics, params: param_ids, ret: ret_id, where_clauses: vec![] });
    let body = seal_body(canon, &m.body);
    let mut f = vis_flags(&m.vis);
    if m.unsafe_ {
        f |= flags::UNSAFE;
    }
    if m.async_ {
        f |= flags::ASYNC;
    }
    canon.push_node(CanonNodeKind::Fn { name_id, sig_id, body, attrs: vec![], flags: f })
}

// ── Body -> CanonId (Body node + BasicBlock nodes) ───────────────────────────

fn seal_body(canon: &mut CanonIR, body: &Body) -> Option<CanonId> {
    match body {
        Body::None => None,
        Body::Raw(src) => {
            // Escape hatch: store raw source as a single-block body with one
            // Raw op represented as a named local. We keep the string in the
            // name intern table so it isn't lost.
            let raw_id = NameId(canon.name_intern.intern(src));
            let bb_id = canon.push_node(CanonNodeKind::BasicBlock { ops: vec![CfgOp::Raw(raw_id)], next: None });
            let body_id = canon.push_node(CanonNodeKind::Body { blocks: vec![bb_id] });
            Some(body_id)
        }
        Body::Blocks(blocks) => {
            let mut block_ids = Vec::with_capacity(blocks.len());
            for bb in blocks {
                let mut ops = Vec::new();
                for stmt in &bb.stmts {
                    use model::ir::node::Stmt;
                    let op = match stmt {
                        Stmt::Let { pat, ty, init } => {
                            let name_id = NameId(canon.name_intern.intern(pat));
                            let ty_id = ty.as_deref().map(|t| intern_ty(canon, t)).unwrap_or_else(|| intern_ty(canon, "()"));
                            let rhs = init.as_deref().map(|e| {
                                let eid = NameId(canon.name_intern.intern(e));
                                canon.push_node(CanonNodeKind::Local { name_id: eid, ty: ty_id, flags: 0 })
                            });
                            let lhs = canon.push_node(CanonNodeKind::Local { name_id, ty: ty_id, flags: 0 });
                            CfgOp::Let { lhs, ty: ty_id, rhs }
                        }
                        Stmt::Expr(e) => {
                            let eid = NameId(canon.name_intern.intern(e));
                            let ty = unit_ty(canon);
                            let loc = canon.push_node(CanonNodeKind::Local { name_id: eid, ty, flags: 0 });
                            CfgOp::Expr(loc)
                        }
                        Stmt::Return(val) => {
                            let v = val.as_deref().map(|e| {
                                let eid = NameId(canon.name_intern.intern(e));
                                let ty = unit_ty(canon);
                                canon.push_node(CanonNodeKind::Local { name_id: eid, ty, flags: 0 })
                            });
                            CfgOp::Return(v)
                        }
                        Stmt::Raw(src) => {
                            let rid = NameId(canon.name_intern.intern(src));
                            CfgOp::Raw(rid)
                        }
                    };
                    ops.push(op);
                }
                // Terminator -> final CfgOp
                use model::ir::node::Terminator;
                match &bb.terminator {
                    Terminator::Goto(t) => ops.push(CfgOp::Goto(*t)),
                    Terminator::Branch { cond, true_bb, false_bb } => {
                        let cid = NameId(canon.name_intern.intern(cond));
                        let bty = bool_ty(canon);
                        let cloc = canon.push_node(CanonNodeKind::Local { name_id: cid, ty: bty, flags: 0 });
                        ops.push(CfgOp::Branch { cond: cloc, true_bb: *true_bb, false_bb: *false_bb });
                    }
                    Terminator::Return => ops.push(CfgOp::Return(None)),
                    Terminator::None => {}
                }
                let bb_id = canon.push_node(CanonNodeKind::BasicBlock { ops, next: None });
                block_ids.push(bb_id);
            }
            let body_id = canon.push_node(CanonNodeKind::Body { blocks: block_ids });
            Some(body_id)
        }
    }
}

// ── Main seal pass ────────────────────────────────────────────────────────────

pub fn seal(model: &ModelIR) -> CanonIR {
    let mut canon = CanonIR::new();

    // id_map[model_node_index] = canon_id
    let mut id_map: Vec<CanonId> = Vec::with_capacity(model.nodes.len());

    // ── Pass 1: allocate one CanonNode per ModelIR node ───────────────────────
    for node in &model.nodes {
        let canon_kind = match &node.kind {
            NodeKind::Crate { name, edition } => {
                let name_id = NameId(canon.name_intern.intern(name));
                let ed: u32 = edition.parse().unwrap_or(2021);
                CanonNodeKind::Crate { name_id, edition: ed }
            }

            NodeKind::Module { path, vis, inline, .. } => {
                let path_id = PathId(canon.path_intern.intern(path));
                let mut f = vis_flags(vis);
                if *inline {
                    f |= flags::INLINE;
                }
                CanonNodeKind::Module { path_id, flags: f }
            }

            NodeKind::Struct { name, vis, generics, fields, derives, attrs, where_clauses, struct_kind } => {
                let name_id = NameId(canon.name_intern.intern(name));
                let sk: u8 = match struct_kind {
                    StructKind::Named => 0,
                    StructKind::Tuple => 1,
                    StructKind::Unit => 2,
                };
                // generics, fields, derives, attrs — sealed as sub-nodes below in pass 2
                // for now store empty vecs; pass 2 fills them
                CanonNodeKind::Struct { name_id, generics: vec![], fields: vec![], derives: vec![], attrs: vec![], flags: vis_flags(vis), struct_kind: sk }
            }

            NodeKind::Enum { name, vis, .. } => {
                let name_id = NameId(canon.name_intern.intern(name));
                CanonNodeKind::Enum { name_id, generics: vec![], variants: vec![], derives: vec![], attrs: vec![], flags: vis_flags(vis) }
            }

            NodeKind::Trait { name, vis, unsafe_, .. } => {
                let name_id = NameId(canon.name_intern.intern(name));
                let mut f = vis_flags(vis);
                if *unsafe_ {
                    f |= flags::UNSAFE;
                }
                CanonNodeKind::Trait { name_id, generics: vec![], methods: vec![], attrs: vec![], flags: f }
            }

            NodeKind::Impl { for_struct, for_trait, unsafe_, .. } => {
                let for_ty = intern_ty(&mut canon, for_struct);
                let trait_ty = for_trait.as_deref().map(|t| intern_ty(&mut canon, t));
                let mut f = 0u32;
                if *unsafe_ {
                    f |= flags::UNSAFE;
                }
                CanonNodeKind::Impl { for_ty, for_trait: trait_ty, generics: vec![], attrs: vec![], flags: f }
            }

            NodeKind::Function { name, vis, params, ret, body, unsafe_, async_, .. } => {
                let name_id = NameId(canon.name_intern.intern(name));
                let ret_id = intern_ty(&mut canon, ret);
                let param_ids: Vec<CanonId> = params
                    .iter()
                    .map(|p| {
                        let pname = NameId(canon.name_intern.intern(&p.name));
                        let pty = intern_ty(&mut canon, &p.ty);
                        let mut pf = 0u32;
                        if p.mutable {
                            pf |= flags::MUT;
                        }
                        canon.push_node(CanonNodeKind::Param { name_id: pname, ty: pty, flags: pf })
                    })
                    .collect();
                let sig_id = canon.push_node(CanonNodeKind::FnSig { generics: vec![], params: param_ids, ret: ret_id, where_clauses: vec![] });
                let body_id = seal_body(&mut canon, body);
                let mut f = vis_flags(vis);
                if *unsafe_ {
                    f |= flags::UNSAFE;
                }
                if *async_ {
                    f |= flags::ASYNC;
                }
                CanonNodeKind::Fn { name_id, sig_id, body: body_id, attrs: vec![], flags: f }
            }

            NodeKind::Method { name, vis, params, ret, body, unsafe_, async_, .. } => {
                let name_id = NameId(canon.name_intern.intern(name));
                let ret_id = intern_ty(&mut canon, ret);
                let param_ids: Vec<CanonId> = params
                    .iter()
                    .map(|p| {
                        let pname = NameId(canon.name_intern.intern(&p.name));
                        let pty = intern_ty(&mut canon, &p.ty);
                        let mut pf = 0u32;
                        if p.mutable {
                            pf |= flags::MUT;
                        }
                        canon.push_node(CanonNodeKind::Param { name_id: pname, ty: pty, flags: pf })
                    })
                    .collect();
                let sig_id = canon.push_node(CanonNodeKind::FnSig { generics: vec![], params: param_ids, ret: ret_id, where_clauses: vec![] });
                let body_id = seal_body(&mut canon, body);
                let mut f = vis_flags(vis);
                if *unsafe_ {
                    f |= flags::UNSAFE;
                }
                if *async_ {
                    f |= flags::ASYNC;
                }
                CanonNodeKind::Fn { name_id, sig_id, body: body_id, attrs: vec![], flags: f }
            }

            NodeKind::Const { name, vis, ty, value, .. } => {
                let name_id = NameId(canon.name_intern.intern(name));
                let ty_id = intern_ty(&mut canon, ty);
                let value_id = NameId(canon.name_intern.intern(value));
                CanonNodeKind::Const { name_id, ty: ty_id, value_id, attrs: vec![], flags: vis_flags(vis) }
            }

            NodeKind::Static { name, vis, ty, value, mutable, .. } => {
                let name_id = NameId(canon.name_intern.intern(name));
                let ty_id = intern_ty(&mut canon, ty);
                let value_id = NameId(canon.name_intern.intern(value));
                let mut f = vis_flags(vis);
                if *mutable {
                    f |= flags::MUT;
                }
                CanonNodeKind::Static { name_id, ty: ty_id, value_id, attrs: vec![], flags: f }
            }

            NodeKind::Use { vis, path, alias, glob } => {
                let path_id = PathId(canon.path_intern.intern(path));
                let alias_id = alias.as_deref().map(|a| NameId(canon.name_intern.intern(a)));
                let mut f = vis_flags(vis);
                if *glob {
                    f |= flags::GLOB;
                }
                CanonNodeKind::Use { path_id, alias: alias_id, flags: f }
            }

            NodeKind::ExternCrate { name, alias, vis } => {
                let name_id = NameId(canon.name_intern.intern(name));
                let alias_id = alias.as_deref().map(|a| NameId(canon.name_intern.intern(a)));
                CanonNodeKind::ExternCrate { name_id, alias: alias_id, flags: vis_flags(vis) }
            }

            NodeKind::TypeAlias { name, vis, ty, .. } => {
                let name_id = NameId(canon.name_intern.intern(name));
                let ty_id = intern_ty(&mut canon, ty);
                CanonNodeKind::TypeAlias { name_id, generics: vec![], ty: ty_id, attrs: vec![], flags: vis_flags(vis) }
            }

            NodeKind::TypeRef { name } => {
                let name_id = NameId(canon.name_intern.intern(name));
                CanonNodeKind::TypeRef { name_id }
            }

            NodeKind::Lifetime { name } => {
                let name_id = NameId(canon.name_intern.intern(name));
                CanonNodeKind::Lifetime { name_id }
            }

            NodeKind::MacroCall { path, tokens } => {
                let path_id = PathId(canon.path_intern.intern(path));
                let tokens_id = NameId(canon.name_intern.intern(tokens));
                CanonNodeKind::MacroCall { path_id, tokens_id }
            }
        };

        let cid = canon.push_node(canon_kind);
        id_map.push(cid);
    }

    // ── Pass 2: enrich composite nodes from ModelIR payload ──────────────────
    for (i, node) in model.nodes.iter().enumerate() {
        let cid = id_map[i];
        match &node.kind {
            NodeKind::Struct { generics, fields, derives, .. } => {
                let gen_ids: Vec<CanonId> = generics.iter().map(|g| seal_generic_param(&mut canon, g)).collect();
                let field_ids: Vec<CanonId> = fields.iter().map(|f| seal_field(&mut canon, f)).collect();
                let derive_ids: Vec<CanonId> = derives
                    .iter()
                    .map(|d| {
                        let did = NameId(canon.name_intern.intern(d));
                        canon.push_node(CanonNodeKind::TypeRef { name_id: did })
                    })
                    .collect();
                if let CanonNodeKind::Struct { generics, fields, derives, .. } = &mut canon.nodes[cid.0 as usize].kind {
                    *generics = gen_ids;
                    *fields = field_ids;
                    *derives = derive_ids;
                }
            }
            NodeKind::Enum { generics, variants, derives, .. } => {
                let gen_ids: Vec<CanonId> = generics.iter().map(|g| seal_generic_param(&mut canon, g)).collect();
                let variant_ids: Vec<CanonId> = variants.iter().map(|v| seal_variant(&mut canon, v)).collect();
                let derive_ids: Vec<CanonId> = derives
                    .iter()
                    .map(|d| {
                        let did = NameId(canon.name_intern.intern(d));
                        canon.push_node(CanonNodeKind::TypeRef { name_id: did })
                    })
                    .collect();
                if let CanonNodeKind::Enum { generics, variants, derives, .. } = &mut canon.nodes[cid.0 as usize].kind {
                    *generics = gen_ids;
                    *variants = variant_ids;
                    *derives = derive_ids;
                }
            }
            NodeKind::Trait { generics, methods, .. } => {
                let gen_ids: Vec<CanonId> = generics.iter().map(|g| seal_generic_param(&mut canon, g)).collect();
                let method_ids: Vec<CanonId> = methods.iter().map(|m| seal_trait_method(&mut canon, m)).collect();
                if let CanonNodeKind::Trait { generics, methods, .. } = &mut canon.nodes[cid.0 as usize].kind {
                    *generics = gen_ids;
                    *methods = method_ids;
                }
            }
            NodeKind::Impl { generics, .. } => {
                let gen_ids: Vec<CanonId> = generics.iter().map(|g| seal_generic_param(&mut canon, g)).collect();
                if let CanonNodeKind::Impl { generics, .. } = &mut canon.nodes[cid.0 as usize].kind {
                    *generics = gen_ids;
                }
            }
            NodeKind::Function { generics, .. } | NodeKind::Method { generics, .. } => {
                let gen_ids: Vec<CanonId> = generics.iter().map(|g| seal_generic_param(&mut canon, g)).collect();
                let sig_id = match &canon.nodes[cid.0 as usize].kind {
                    CanonNodeKind::Fn { sig_id, .. } => Some(*sig_id),
                    _ => None,
                };
                if let Some(sig_id) = sig_id {
                    if let CanonNodeKind::FnSig { generics, .. } = &mut canon.nodes[sig_id.0 as usize].kind {
                        *generics = gen_ids;
                    }
                }
            }
            NodeKind::TypeAlias { generics, .. } => {
                let gen_ids: Vec<CanonId> = generics.iter().map(|g| seal_generic_param(&mut canon, g)).collect();
                if let CanonNodeKind::TypeAlias { generics, .. } = &mut canon.nodes[cid.0 as usize].kind {
                    *generics = gen_ids;
                }
            }
            _ => {}
        }
    }

    // ── Pass 3: emit_order ────────────────────────────────────────────────────
    canon.emit_order = model.emit_order.iter().map(|nid| id_map[nid.index()]).collect();

    // ── Pass 4: edges -> 8 CSR graphs ─────────────────────────────────────────
    let mut name_edges = vec![];
    let mut type_edges = vec![];
    let mut call_edges = vec![];
    let mut module_edges = vec![];
    let mut cfg_edges = vec![];
    let mut region_edges = vec![];
    let mut value_edges = vec![];
    let mut macro_edges = vec![];

    for hint in &model.edge_hints {
        let src = id_map[hint.src as usize];
        let dst = id_map[hint.dst as usize];
        let k = hint.kind.clone();
        match &hint.kind {
            EdgeKind::Renames | EdgeKind::Resolves => name_edges.push((src, dst, k)),
            EdgeKind::TypeOf | EdgeKind::TypeUnifies | EdgeKind::ImplTrait | EdgeKind::DynTrait => type_edges.push((src, dst, k)),
            EdgeKind::Calls => call_edges.push((src, dst, k)),
            EdgeKind::Contains | EdgeKind::ImplFor => module_edges.push((src, dst, k)),
            EdgeKind::CfgEdge | EdgeKind::CfgBranch { .. } => cfg_edges.push((src, dst, k)),
            EdgeKind::Outlives => region_edges.push((src, dst, k)),
            EdgeKind::ConstDep => value_edges.push((src, dst, k)),
            EdgeKind::Expands => macro_edges.push((src, dst, k)),
        }
    }

    // CsrGraph::from_edges(node_data: Vec<ND>, edges: Vec<(u32,u32,ED)>)
    // node_data is per-node payload — CanonIR uses CanonId as the node data,
    // so we pass identity: node_data[i] = CanonId(i).
    let n = canon.nodes.len();
    let node_data: Vec<CanonId> = (0..n as u32).map(CanonId).collect();

    // Unwrap CanonId to u32 for the edge tuples.
    let to_raw = |v: Vec<(CanonId, CanonId, EdgeKind)>| -> Vec<(u32, u32, EdgeKind)> { v.into_iter().map(|(s, d, k)| (s.0, d.0, k)).collect() };

    canon.name_graph = CsrGraph::from_edges(node_data.clone(), to_raw(name_edges));
    canon.type_graph = CsrGraph::from_edges(node_data.clone(), to_raw(type_edges));
    canon.call_graph = CsrGraph::from_edges(node_data.clone(), to_raw(call_edges));
    canon.module_graph = CsrGraph::from_edges(node_data.clone(), to_raw(module_edges));
    canon.cfg_graph = CsrGraph::from_edges(node_data.clone(), to_raw(cfg_edges));
    canon.region_graph = CsrGraph::from_edges(node_data.clone(), to_raw(region_edges));
    canon.value_graph = CsrGraph::from_edges(node_data.clone(), to_raw(value_edges));
    canon.macro_graph = CsrGraph::from_edges(node_data, to_raw(macro_edges));

    canon
}
