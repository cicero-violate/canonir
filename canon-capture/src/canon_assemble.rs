use crate::types::{
    Body, EdgeHint, EdgeKind as ModelEdgeKind, EnumVariant, Field, GenericParam, Node, NodeId, NodeKind, PrimType, StructKind, TraitMethod, TypeExpr,
    Visibility,
};
use crate::{index::Index, Partial};
use canon::{
    csr_graph::CsrGraph,
    edge::EdgeKind as CanonEdgeKind,
    intern::{NameId, PathId},
    ir::CanonIR,
    node::{flags, CanonId, CanonNodeKind, CfgOp, DependencySpec, PrimTy, TypeKind},
};
use rustc_middle::ty::TyCtxt;
use rustc_span::def_id::LOCAL_CRATE;
use std::collections::{HashMap, HashSet};
use std::path::PathBuf;

#[derive(Default)]
struct ModelLike {
    nodes: Vec<Node>,
    edge_hints: Vec<EdgeHint>,
    emit_order: Vec<NodeId>,
}

fn load_declared_dependency_specs(canon: &mut CanonIR) -> Vec<DependencySpec> {
    let manifest_dir = match std::env::var("CARGO_MANIFEST_DIR") {
        Ok(v) => v,
        Err(_) => return Vec::new(),
    };
    let manifest_path = PathBuf::from(manifest_dir).join("Cargo.toml");
    let manifest = match std::fs::read_to_string(manifest_path) {
        Ok(v) => v,
        Err(_) => return Vec::new(),
    };
    let value: toml::Value = match toml::from_str(&manifest) {
        Ok(v) => v,
        Err(_) => return Vec::new(),
    };
    let Some(deps) = value.get("dependencies").and_then(|v| v.as_table()) else {
        return Vec::new();
    };

    let mut out = Vec::new();
    let mut seen: HashSet<String> = HashSet::new();
    for (key, dep_value) in deps {
        let crate_root = key.replace('-', "_");
        if !seen.insert(crate_root.clone()) {
            continue;
        }
        let package_name: Option<String> = match dep_value {
            toml::Value::String(_) => {
                if key.contains('-') {
                    Some(key.clone())
                } else {
                    None
                }
            }
            toml::Value::Table(t) => {
                if let Some(pkg) = t.get("package").and_then(|v| v.as_str()) {
                    Some(pkg.to_string())
                } else if key.contains('-') {
                    Some(key.clone())
                } else {
                    None
                }
            }
            _ => None,
        };
        let crate_root = canon.intern_path(&crate_root);
        let package_name = package_name
            .as_deref()
            .filter(|pkg| !pkg.is_empty())
            .map(|pkg| NameId(canon.name_intern.intern(pkg)));
        out.push(DependencySpec {
            crate_root,
            package_name,
        });
    }

    out
}

fn map_edge_kind(k: &ModelEdgeKind) -> CanonEdgeKind {
    match k {
        ModelEdgeKind::Renames => CanonEdgeKind::Renames,
        ModelEdgeKind::Resolves => CanonEdgeKind::Resolves,
        ModelEdgeKind::ImplRef => CanonEdgeKind::ImplRef,
        ModelEdgeKind::TypeOf => CanonEdgeKind::TypeOf,
        ModelEdgeKind::TypeUnifies => CanonEdgeKind::TypeUnifies,
        ModelEdgeKind::ImplTrait => CanonEdgeKind::ImplTrait,
        ModelEdgeKind::DynTrait => CanonEdgeKind::DynTrait,
        ModelEdgeKind::Calls => CanonEdgeKind::Calls,
        ModelEdgeKind::Contains => CanonEdgeKind::Contains,
        ModelEdgeKind::ImplFor => CanonEdgeKind::ImplFor,
        ModelEdgeKind::CfgEdge => CanonEdgeKind::CfgEdge,
        ModelEdgeKind::CfgBranch { label } => CanonEdgeKind::CfgBranch { label: label.clone() },
        ModelEdgeKind::Outlives => CanonEdgeKind::Outlives,
        ModelEdgeKind::ConstDep => CanonEdgeKind::ConstDep,
        ModelEdgeKind::Expands => CanonEdgeKind::Expands,
        ModelEdgeKind::AssocItem => CanonEdgeKind::AssocItem,
        ModelEdgeKind::Instantiates => CanonEdgeKind::Instantiates,
        ModelEdgeKind::Reexports => CanonEdgeKind::Reexports,
    }
}

fn vis_flags(v: &Visibility) -> u32 {
    match v {
        Visibility::Public => flags::PUB,
        Visibility::PubCrate => flags::PUB_CRATE,
        Visibility::PubSuper => flags::PUB_SUPER,
        Visibility::PubIn(_) => flags::PUB_IN,
        Visibility::Private => 0,
    }
}

fn prim_to_canon(prim: &PrimType) -> PrimTy {
    match prim {
        PrimType::Bool => PrimTy::Bool,
        PrimType::Char => PrimTy::Char,
        PrimType::Str => PrimTy::Str,
        PrimType::U8 => PrimTy::U8,
        PrimType::U16 => PrimTy::U16,
        PrimType::U32 => PrimTy::U32,
        PrimType::U64 => PrimTy::U64,
        PrimType::U128 => PrimTy::U128,
        PrimType::Usize => PrimTy::Usize,
        PrimType::I8 => PrimTy::I8,
        PrimType::I16 => PrimTy::I16,
        PrimType::I32 => PrimTy::I32,
        PrimType::I64 => PrimTy::I64,
        PrimType::I128 => PrimTy::I128,
        PrimType::Isize => PrimTy::Isize,
        PrimType::F32 => PrimTy::F32,
        PrimType::F64 => PrimTy::F64,
        PrimType::Unit => PrimTy::Unit,
        PrimType::Never => PrimTy::Never,
    }
}

fn render_type_expr(expr: &TypeExpr) -> String {
    match expr {
        TypeExpr::Primitive(prim) => match prim {
            PrimType::Bool => "bool".to_string(),
            PrimType::Char => "char".to_string(),
            PrimType::Str => "str".to_string(),
            PrimType::U8 => "u8".to_string(),
            PrimType::U16 => "u16".to_string(),
            PrimType::U32 => "u32".to_string(),
            PrimType::U64 => "u64".to_string(),
            PrimType::U128 => "u128".to_string(),
            PrimType::Usize => "usize".to_string(),
            PrimType::I8 => "i8".to_string(),
            PrimType::I16 => "i16".to_string(),
            PrimType::I32 => "i32".to_string(),
            PrimType::I64 => "i64".to_string(),
            PrimType::I128 => "i128".to_string(),
            PrimType::Isize => "isize".to_string(),
            PrimType::F32 => "f32".to_string(),
            PrimType::F64 => "f64".to_string(),
            PrimType::Unit => "()".to_string(),
            PrimType::Never => "!".to_string(),
        },
        TypeExpr::Ref { lifetime, inner, mutable } => {
            let mut out = String::from("&");
            if let Some(lt) = lifetime {
                out.push_str(lt);
                out.push(' ');
            }
            if *mutable {
                out.push_str("mut ");
            }
            out.push_str(&render_type_expr(inner));
            out
        }
        TypeExpr::RawPtr { inner, mutable } => {
            if *mutable {
                format!("*mut {}", render_type_expr(inner))
            } else {
                format!("*const {}", render_type_expr(inner))
            }
        }
        TypeExpr::Array { inner, len } => match len {
            Some(n) => format!("[{}; {n}]", render_type_expr(inner)),
            None => format!("[{}; _]", render_type_expr(inner)),
        },
        TypeExpr::Slice(inner) => format!("[{}]", render_type_expr(inner)),
        TypeExpr::Tuple(items) => {
            if items.is_empty() {
                "()".to_string()
            } else {
                let mut rendered: Vec<String> = items.iter().map(render_type_expr).collect();
                if rendered.len() == 1 {
                    rendered[0].push(',');
                }
                format!("({})", rendered.join(", "))
            }
        }
        TypeExpr::FnPtr { params, ret } => {
            let params = params.iter().map(render_type_expr).collect::<Vec<_>>().join(", ");
            format!("fn({params}) -> {}", render_type_expr(ret))
        }
        TypeExpr::Param(name) => name.clone(),
        TypeExpr::DynTrait(path) => format!("dyn {path}"),
        TypeExpr::ImplTrait(path) => format!("impl {path}"),
        TypeExpr::AppliedPath { base, args } => {
            let rendered = args.iter().map(render_type_expr).collect::<Vec<_>>().join(", ");
            format!("{base}<{rendered}>")
        }
        TypeExpr::Path(path) => path.clone(),
    }
}

fn intern_ty_expr(canon: &mut CanonIR, ty: &TypeExpr) -> CanonId {
    let kind = match ty {
        TypeExpr::Primitive(prim) => TypeKind::Primitive(prim_to_canon(prim)),
        TypeExpr::Ref { lifetime, inner, mutable } => {
            let lifetime = lifetime.as_ref().map(|lt| {
                let name_id = NameId(canon.name_intern.intern(lt));
                canon.push_node(CanonNodeKind::Lifetime { name_id })
            });
            let inner = intern_ty_expr(canon, inner);
            TypeKind::Ref { lifetime, inner, mutable: *mutable }
        }
        TypeExpr::RawPtr { inner, mutable } => {
            let inner = intern_ty_expr(canon, inner);
            TypeKind::RawPtr { inner, mutable: *mutable }
        }
        TypeExpr::Array { inner, len } => {
            let inner = intern_ty_expr(canon, inner);
            if let Some(len) = len {
                TypeKind::Array { inner, len: *len }
            } else {
                TypeKind::Unresolved(canon.intern_path(&render_type_expr(ty)))
            }
        }
        TypeExpr::Slice(inner) => TypeKind::Slice(intern_ty_expr(canon, inner)),
        TypeExpr::Tuple(items) => {
            let elems = items.iter().map(|item| intern_ty_expr(canon, item)).collect();
            TypeKind::Tuple(elems)
        }
        TypeExpr::FnPtr { params, ret } => {
            let params: Vec<CanonId> = params
                .iter()
                .enumerate()
                .map(|(i, p)| {
                    let name_id = NameId(canon.name_intern.intern(&format!("__fnptr_arg{i}")));
                    let ty = intern_ty_expr(canon, p);
                    canon.push_node(CanonNodeKind::Param { name_id, ty, flags: 0 })
                })
                .collect();
            let ret = intern_ty_expr(canon, ret);
            let sig_id = canon.push_node(CanonNodeKind::FnSig {
                generics: vec![],
                params,
                ret,
                where_clauses: vec![],
            });
            TypeKind::FnPtr(sig_id)
        }
        TypeExpr::Param(name) => TypeKind::Param(NameId(canon.name_intern.intern(name))),
        TypeExpr::DynTrait(path) => {
            let name_id = NameId(canon.name_intern.intern(path));
            let trait_id = canon.push_node(CanonNodeKind::TypeRef { name_id });
            TypeKind::DynTrait(trait_id)
        }
        TypeExpr::ImplTrait(path) => {
            let name_id = NameId(canon.name_intern.intern(path));
            let trait_id = canon.push_node(CanonNodeKind::TypeRef { name_id });
            TypeKind::ImplTrait(trait_id)
        }
        TypeExpr::AppliedPath { base, args } => {
            let base_path = canon.intern_path(base);
            let base_ty = canon.intern_type(TypeKind::Extern(base_path));
            let args: Vec<CanonId> = args.iter().map(|arg| intern_ty_expr(canon, arg)).collect();
            TypeKind::Applied { base: base_ty, args }
        }
        TypeExpr::Path(path) => TypeKind::Extern(canon.intern_path(path)),
    };
    canon.intern_type(kind)
}

fn unit_ty(canon: &mut CanonIR) -> CanonId {
    canon.intern_type(TypeKind::Primitive(PrimTy::Unit))
}

fn bool_ty(canon: &mut CanonIR) -> CanonId {
    canon.intern_type(TypeKind::Primitive(PrimTy::Bool))
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
    let default_ty = gp.default_ty.as_ref().map(|t| intern_ty_expr(canon, t));
    canon.push_node(CanonNodeKind::GenericParam { name_id, bounds, is_lifetime: gp.is_lifetime, default_ty })
}

fn seal_field(canon: &mut CanonIR, f: &Field) -> CanonId {
    let name_id = f.name.as_deref().map(|n| NameId(canon.name_intern.intern(n)));
    let ty = intern_ty_expr(canon, &f.ty);
    canon.push_node(CanonNodeKind::Field { name_id, ty, flags: vis_flags(&f.vis) })
}

fn seal_variant(canon: &mut CanonIR, v: &EnumVariant) -> CanonId {
    let name_id = NameId(canon.name_intern.intern(&v.name));
    let fields = v.fields.iter().map(|f| seal_field(canon, f)).collect();
    canon.push_node(CanonNodeKind::Variant { name_id, fields })
}

fn seal_trait_method(canon: &mut CanonIR, m: &TraitMethod) -> CanonId {
    let name_id = NameId(canon.name_intern.intern(&m.name));
    let ret_id = intern_ty_expr(canon, &m.ret);
    let param_ids: Vec<CanonId> = m
        .params
        .iter()
        .map(|p| {
            let pname = NameId(canon.name_intern.intern(&p.name));
            let pty = intern_ty_expr(canon, &p.ty);
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

fn seal_body(canon: &mut CanonIR, body: &Body) -> Option<CanonId> {
    match body {
        Body::None => None,
        Body::Blocks(blocks) => {
            let mut block_ids = Vec::with_capacity(blocks.len());
            for bb in blocks {
                let mut ops = Vec::new();
                for stmt in &bb.stmts {
                    use crate::types::Stmt;
                    let op = match stmt {
                        Stmt::Let { pat, ty, init } => {
                            let name_id = NameId(canon.name_intern.intern(pat));
                            let ty_id = ty.as_ref().map(|t| intern_ty_expr(canon, t)).unwrap_or_else(|| unit_ty(canon));
                            let rhs = init.as_deref().map(|e| {
                                let eid = NameId(canon.name_intern.intern(e));
                                canon.push_node(CanonNodeKind::Local { name_id: eid, ty: ty_id, flags: 0 })
                            });
                            let lhs = canon.push_node(CanonNodeKind::Local { name_id, ty: ty_id, flags: 0 });
                            CfgOp::Let { lhs, ty: ty_id, rhs }
                        }
                        Stmt::Assign { lhs, rhs } => {
                            let ty = unit_ty(canon);
                            let lhs_name = NameId(canon.name_intern.intern(lhs));
                            let rhs_name = NameId(canon.name_intern.intern(rhs));
                            let lhs_id = canon.push_node(CanonNodeKind::Local { name_id: lhs_name, ty, flags: 0 });
                            let rhs_id = canon.push_node(CanonNodeKind::Local { name_id: rhs_name, ty, flags: 0 });
                            CfgOp::Assign { lhs: lhs_id, rhs: rhs_id }
                        }
                        Stmt::Expr(e) => {
                            let eid = NameId(canon.name_intern.intern(e));
                            let ty = unit_ty(canon);
                            let loc = canon.push_node(CanonNodeKind::Local { name_id: eid, ty, flags: 0 });
                            CfgOp::Expr(loc)
                        }
                        Stmt::Call { func, args, dest } => {
                            let ty = unit_ty(canon);
                            let func_name = NameId(canon.name_intern.intern(func));
                            let func_id = canon.push_node(CanonNodeKind::Local { name_id: func_name, ty, flags: 0 });
                            let args: Vec<CanonId> = args
                                .iter()
                                .map(|arg| {
                                    let arg_name = NameId(canon.name_intern.intern(arg));
                                    canon.push_node(CanonNodeKind::Local { name_id: arg_name, ty, flags: 0 })
                                })
                                .collect();
                            let dest = dest.as_deref().map(|name| {
                                let name_id = NameId(canon.name_intern.intern(name));
                                canon.push_node(CanonNodeKind::Local { name_id, ty, flags: 0 })
                            });
                            CfgOp::Call { func: func_id, args, dest }
                        }
                        Stmt::FieldAccess { base, field, dest } => {
                            let ty = unit_ty(canon);
                            let base_name = NameId(canon.name_intern.intern(base));
                            let base_id = canon.push_node(CanonNodeKind::Local {
                                name_id: base_name,
                                ty,
                                flags: 0,
                            });
                            let field_id = NameId(canon.name_intern.intern(field));
                            let dest_id = dest.as_deref().map(|name| {
                                let name_id = NameId(canon.name_intern.intern(name));
                                canon.push_node(CanonNodeKind::Local {
                                    name_id,
                                    ty,
                                    flags: 0,
                                })
                            });
                            CfgOp::FieldAccess { base: base_id, field: field_id, dest: dest_id }
                        }
                        Stmt::MethodCall {
                            receiver,
                            method,
                            args,
                            dest,
                        } => {
                            let ty = unit_ty(canon);
                            let receiver_name = NameId(canon.name_intern.intern(receiver));
                            let receiver_id = canon.push_node(CanonNodeKind::Local {
                                name_id: receiver_name,
                                ty,
                                flags: 0,
                            });
                            let arg_ids: Vec<CanonId> = args
                                .iter()
                                .map(|arg| {
                                    let arg_name = NameId(canon.name_intern.intern(arg));
                                    canon.push_node(CanonNodeKind::Local {
                                        name_id: arg_name,
                                        ty,
                                        flags: 0,
                                    })
                                })
                                .collect();
                            let method_id = NameId(canon.name_intern.intern(method));
                            let dest_id = dest.as_deref().map(|name| {
                                let name_id = NameId(canon.name_intern.intern(name));
                                canon.push_node(CanonNodeKind::Local {
                                    name_id,
                                    ty,
                                    flags: 0,
                                })
                            });
                            CfgOp::MethodCall {
                                receiver: receiver_id,
                                method: method_id,
                                args: arg_ids,
                                dest: dest_id,
                            }
                        }
                        Stmt::StructLit { ty, fields, dest } => {
                            let ty_id = intern_ty_expr(canon, ty);
                            let value_ty = unit_ty(canon);
                            let lowered_fields: Vec<(NameId, CanonId)> = fields
                                .iter()
                                .map(|(field, value)| {
                                    let field_name = NameId(canon.name_intern.intern(field));
                                    let value_name = NameId(canon.name_intern.intern(value));
                                    (
                                        field_name,
                                        canon.push_node(CanonNodeKind::Local {
                                            name_id: value_name,
                                            ty: value_ty,
                                            flags: 0,
                                        }),
                                    )
                                })
                                .collect();
                            let dest_id = dest.as_deref().map(|name| {
                                let name_id = NameId(canon.name_intern.intern(name));
                                canon.push_node(CanonNodeKind::Local {
                                    name_id,
                                    ty: value_ty,
                                    flags: 0,
                                })
                            });
                            CfgOp::StructLit {
                                ty: ty_id,
                                fields: lowered_fields,
                                dest: dest_id,
                            }
                        }
                        Stmt::Return(val) => {
                            let v = val.as_deref().map(|e| {
                                let eid = NameId(canon.name_intern.intern(e));
                                let ty = unit_ty(canon);
                                canon.push_node(CanonNodeKind::Local { name_id: eid, ty, flags: 0 })
                            });
                            CfgOp::Return(v)
                        }
                    };
                    ops.push(op);
                }
                use crate::types::Terminator;
                match &bb.terminator {
                    Terminator::Goto(t) => ops.push(CfgOp::Goto(*t)),
                    Terminator::Branch { cond, true_bb, false_bb } => {
                        let cid = NameId(canon.name_intern.intern(cond));
                        let ty = bool_ty(canon);
                        let cloc = canon.push_node(CanonNodeKind::Local { name_id: cid, ty, flags: 0 });
                        ops.push(CfgOp::Branch { cond: cloc, true_bb: *true_bb, false_bb: *false_bb });
                    }
                    Terminator::Return => {}
                    Terminator::None => {}
                }
                block_ids.push(canon.push_node(CanonNodeKind::BasicBlock { ops, next: None }));
            }
            Some(canon.push_node(CanonNodeKind::Body { blocks: block_ids }))
        }
    }
}

fn assemble_model_like(tcx: TyCtxt<'_>, index: &Index, parts: Vec<Partial>) -> ModelLike {
    let mut model_like = ModelLike::default();

    let crate_name = tcx.crate_name(LOCAL_CRATE).to_string();
    let edition = format!("{}", tcx.sess.edition());
    let crate_node = Node { id: NodeId(0), kind: NodeKind::Crate { name: crate_name, edition }, span: None };

    let mut nodes: Vec<Node> = vec![crate_node];
    for part in parts {
        for mut node in part.nodes {
            node.id = NodeId(node.id.0 + 1);
            nodes.push(node);
        }
        for mut hint in part.edge_hints {
            hint.src += 1;
            hint.dst += 1;
            model_like.edge_hints.push(hint);
        }
    }

    model_like.nodes = nodes;

    let valid_ids: HashSet<u32> = model_like.nodes.iter().map(|n| n.id.0).collect();
    model_like.edge_hints.retain(|h| valid_ids.contains(&h.src) && valid_ids.contains(&h.dst));

    let mut remap: HashMap<u32, u32> = HashMap::new();
    for (new_id, node) in model_like.nodes.iter_mut().enumerate() {
        remap.insert(node.id.0, new_id as u32);
        node.id = NodeId(new_id as u32);
    }

    for h in &mut model_like.edge_hints {
        if let (Some(src), Some(dst)) = (remap.get(&h.src), remap.get(&h.dst)) {
            h.src = *src;
            h.dst = *dst;
        }
    }

    for (def_id, &node_id) in &index.def_to_node {
        if tcx.opt_parent(*def_id).map_or(true, |p| !index.def_to_node.contains_key(&p)) {
            let shifted = node_id.0 + 1;
            if let Some(mapped) = remap.get(&shifted) {
                model_like.edge_hints.push(EdgeHint { src: 0, dst: *mapped, kind: ModelEdgeKind::Contains });
            }
        }
    }

    model_like
}

pub fn canon_assemble(tcx: TyCtxt<'_>, index: &Index, parts: Vec<Partial>) -> CanonIR {
    let model_like = assemble_model_like(tcx, index, parts);
    let mut canon = CanonIR::new();

    let mut id_map: Vec<CanonId> = Vec::with_capacity(model_like.nodes.len());
    let mut pending_vis_paths: Vec<(CanonId, String)> = Vec::new();

    for node in &model_like.nodes {
        let canon_kind = match &node.kind {
            NodeKind::Crate { name, edition } => {
                let name_id = NameId(canon.name_intern.intern(name));
                let cargo_name = if name.contains('_') {
                    Some(NameId(canon.name_intern.intern(&name.replace('_', "-"))))
                } else {
                    None
                };
                let ed: u32 = edition.parse().unwrap_or(2021);
                let declared_dependencies = load_declared_dependency_specs(&mut canon);
                CanonNodeKind::Crate {
                    name_id,
                    cargo_name,
                    edition: ed,
                    dependencies: vec![],
                    dependency_packages: vec![],
                    declared_dependencies,
                }
            }
            NodeKind::Module { path, vis, inline, .. } => {
                let path_id = canon.intern_path(path);
                let mut f = vis_flags(vis);
                if *inline {
                    f |= flags::INLINE;
                }
                CanonNodeKind::Module { path_id, flags: f }
            }
            NodeKind::Struct { name, vis, struct_kind, .. } => {
                let name_id = NameId(canon.name_intern.intern(name));
                let sk = match struct_kind {
                    StructKind::Named => 0,
                    StructKind::Tuple => 1,
                    StructKind::Unit => 2,
                };
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
                let for_ty = intern_ty_expr(&mut canon, for_struct);
                let trait_ty = for_trait.as_ref().map(|t| intern_ty_expr(&mut canon, t));
                let mut f = 0u32;
                if *unsafe_ {
                    f |= flags::UNSAFE;
                }
                CanonNodeKind::Impl { for_ty, for_trait: trait_ty, generics: vec![], attrs: vec![], flags: f }
            }
            NodeKind::Function { name, vis, params, ret, body, unsafe_, async_, .. } | NodeKind::Method { name, vis, params, ret, body, unsafe_, async_, .. } => {
                let name_id = NameId(canon.name_intern.intern(name));
                let ret_id = intern_ty_expr(&mut canon, ret);
                let param_ids: Vec<CanonId> = params
                    .iter()
                    .map(|p| {
                        let pname = NameId(canon.name_intern.intern(&p.name));
                        let pty = intern_ty_expr(&mut canon, &p.ty);
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
            NodeKind::AssocType { name, vis, default_ty, .. } => {
                let name_id = NameId(canon.name_intern.intern(name));
                let default_ty = default_ty.as_ref().map(|t| intern_ty_expr(&mut canon, t));
                CanonNodeKind::AssocType { name_id, generics: vec![], default_ty, flags: vis_flags(vis) }
            }
            NodeKind::AssocConst { name, vis, ty, default_value, .. } => {
                let name_id = NameId(canon.name_intern.intern(name));
                let ty_id = intern_ty_expr(&mut canon, ty);
                let default_value = default_value.as_deref().map(|v| NameId(canon.name_intern.intern(v)));
                CanonNodeKind::AssocConst { name_id, ty: ty_id, default_value, flags: vis_flags(vis) }
            }
            NodeKind::Const { name, vis, ty, value, .. } => {
                let name_id = NameId(canon.name_intern.intern(name));
                let ty_id = intern_ty_expr(&mut canon, ty);
                let value_id = NameId(canon.name_intern.intern(value));
                CanonNodeKind::Const { name_id, ty: ty_id, value_id, attrs: vec![], flags: vis_flags(vis) }
            }
            NodeKind::Static { name, vis, ty, value, mutable, .. } => {
                let name_id = NameId(canon.name_intern.intern(name));
                let ty_id = intern_ty_expr(&mut canon, ty);
                let value_id = NameId(canon.name_intern.intern(value));
                let mut f = vis_flags(vis);
                if *mutable {
                    f |= flags::MUT;
                }
                CanonNodeKind::Static { name_id, ty: ty_id, value_id, attrs: vec![], flags: f }
            }
            NodeKind::Use { vis, path, alias, glob } => {
                let path_id = canon.intern_path(path);
                let alias_id = alias.as_deref().map(|a| NameId(canon.name_intern.intern(a)));
                let mut f = vis_flags(vis);
                if *glob {
                    f |= flags::GLOB;
                }
                CanonNodeKind::Use { path_id, alias: alias_id, flags: f, target: None }
            }
            NodeKind::ExternCrate { name, alias, vis } => {
                let name_id = NameId(canon.name_intern.intern(name));
                let alias_id = alias.as_deref().map(|a| NameId(canon.name_intern.intern(a)));
                CanonNodeKind::ExternCrate { name_id, alias: alias_id, flags: vis_flags(vis) }
            }
            NodeKind::TypeAlias { name, vis, ty, .. } => {
                let name_id = NameId(canon.name_intern.intern(name));
                let ty_id = intern_ty_expr(&mut canon, ty);
                CanonNodeKind::TypeAlias { name_id, generics: vec![], ty: ty_id, attrs: vec![], flags: vis_flags(vis) }
            }
            NodeKind::TypeRef { name } => CanonNodeKind::TypeRef { name_id: NameId(canon.name_intern.intern(name)) },
            NodeKind::Lifetime { name } => CanonNodeKind::Lifetime { name_id: NameId(canon.name_intern.intern(name)) },
            NodeKind::MacroCall { path, tokens } => {
                let path_id = canon.intern_path(path);
                let tokens_id = canon.intern_body(tokens);
                CanonNodeKind::MacroCall { path_id, tokens_id }
            }
            NodeKind::PathRef { path } => {
                let path_id = canon.intern_path(path);
                CanonNodeKind::PathRef { path_id }
            }
        };

        let cid = canon.push_node(canon_kind);
        match &node.kind {
            NodeKind::Module { vis: Visibility::PubIn(path), .. }
            | NodeKind::Struct { vis: Visibility::PubIn(path), .. }
            | NodeKind::Enum { vis: Visibility::PubIn(path), .. }
            | NodeKind::Trait { vis: Visibility::PubIn(path), .. }
            | NodeKind::Function { vis: Visibility::PubIn(path), .. }
            | NodeKind::Method { vis: Visibility::PubIn(path), .. }
            | NodeKind::AssocType { vis: Visibility::PubIn(path), .. }
            | NodeKind::AssocConst { vis: Visibility::PubIn(path), .. }
            | NodeKind::Const { vis: Visibility::PubIn(path), .. }
            | NodeKind::Static { vis: Visibility::PubIn(path), .. }
            | NodeKind::Use { vis: Visibility::PubIn(path), .. }
            | NodeKind::ExternCrate { vis: Visibility::PubIn(path), .. }
            | NodeKind::TypeAlias { vis: Visibility::PubIn(path), .. } => {
                pending_vis_paths.push((cid, path.clone()));
            }
            _ => {}
        }
        id_map.push(cid);
    }

    let mut vispath_edges: Vec<(CanonId, CanonId, CanonEdgeKind)> = Vec::new();
    for (owner, path) in pending_vis_paths {
        let path_id = canon.intern_path(&path);
        let vis_id = canon.push_node(CanonNodeKind::VisPath { flags: flags::PUB_IN, path_id });
        vispath_edges.push((owner, vis_id, CanonEdgeKind::Contains));
    }

    for (i, node) in model_like.nodes.iter().enumerate() {
        let cid = id_map[i];
        match &node.kind {
            NodeKind::Struct { generics, fields, derives, .. } => {
                let gen_ids: Vec<CanonId> = generics.iter().map(|g| seal_generic_param(&mut canon, g)).collect();
                let field_ids: Vec<CanonId> = fields.iter().map(|f| seal_field(&mut canon, f)).collect();
                let derive_ids: Vec<CanonId> = derives
                    .iter()
                    .map(|d| {
                        let name_id = NameId(canon.name_intern.intern(d));
                        canon.push_node(CanonNodeKind::TypeRef { name_id })
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
                        let name_id = NameId(canon.name_intern.intern(d));
                        canon.push_node(CanonNodeKind::TypeRef { name_id })
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
            NodeKind::AssocType { generics, .. } => {
                let gen_ids: Vec<CanonId> = generics.iter().map(|g| seal_generic_param(&mut canon, g)).collect();
                if let CanonNodeKind::AssocType { generics, .. } = &mut canon.nodes[cid.0 as usize].kind {
                    *generics = gen_ids;
                }
            }
            _ => {}
        }
    }

    canon.emit_order = model_like.emit_order.iter().map(|nid| id_map[nid.index()]).collect();

    let mut name_edges = vec![];
    let mut type_edges = vec![];
    let mut call_edges = vec![];
    let mut module_edges = vec![];
    let mut cfg_edges = vec![];
    let mut region_edges = vec![];
    let mut value_edges = vec![];
    let mut macro_edges = vec![];

    for hint in &model_like.edge_hints {
        let src = id_map[hint.src as usize];
        let dst = id_map[hint.dst as usize];
        let k = map_edge_kind(&hint.kind);
        match &hint.kind {
            ModelEdgeKind::Renames
            | ModelEdgeKind::Resolves => name_edges.push((src, dst, k)),
            ModelEdgeKind::TypeOf | ModelEdgeKind::TypeUnifies | ModelEdgeKind::ImplTrait | ModelEdgeKind::DynTrait | ModelEdgeKind::ImplRef | ModelEdgeKind::Instantiates => type_edges.push((src, dst, k)),
            ModelEdgeKind::Calls => call_edges.push((src, dst, k)),
            ModelEdgeKind::Contains | ModelEdgeKind::ImplFor | ModelEdgeKind::AssocItem => module_edges.push((src, dst, k)),
            ModelEdgeKind::CfgEdge | ModelEdgeKind::CfgBranch { .. } => cfg_edges.push((src, dst, k)),
            ModelEdgeKind::Outlives => region_edges.push((src, dst, k)),
            ModelEdgeKind::ConstDep => value_edges.push((src, dst, k)),
            ModelEdgeKind::Expands => macro_edges.push((src, dst, k)),
            ModelEdgeKind::Reexports => name_edges.push((src, dst, k)),
        }
    }
    module_edges.extend(vispath_edges);

    let n = canon.nodes.len();
    let node_data: Vec<CanonId> = (0..n as u32).map(CanonId).collect();
    let to_raw = |v: Vec<(CanonId, CanonId, CanonEdgeKind)>| -> Vec<(u32, u32, CanonEdgeKind)> { v.into_iter().map(|(s, d, k)| (s.0, d.0, k)).collect() };

    canon.name_graph = CsrGraph::from_edges(node_data.clone(), to_raw(name_edges));
    canon.type_graph = CsrGraph::from_edges(node_data.clone(), to_raw(type_edges));
    canon.call_graph = CsrGraph::from_edges(node_data.clone(), to_raw(call_edges));
    canon.module_graph = CsrGraph::from_edges(node_data.clone(), to_raw(module_edges));
    canon.cfg_graph = CsrGraph::from_edges(node_data.clone(), to_raw(cfg_edges));
    canon.region_graph = CsrGraph::from_edges(node_data.clone(), to_raw(region_edges));
    canon.value_graph = CsrGraph::from_edges(node_data.clone(), to_raw(value_edges));
    canon.macro_graph = CsrGraph::from_edges(node_data, to_raw(macro_edges));

    // Canonicalize Impl payload links from structural edges:
    // - Impl.for_ty from module_graph ImplFor
    // - Impl.for_trait from type_graph ImplRef
    let mut impl_payload_updates: Vec<(usize, Option<CanonId>, Option<CanonId>)> = Vec::new();
    for idx in 0..canon.nodes.len() {
        if !matches!(canon.nodes[idx].kind, CanonNodeKind::Impl { .. }) {
            continue;
        }
        let mut for_ty: Option<CanonId> = None;
        for (dst, edge) in canon.module_graph.neighbours(canon::id::NodeId(idx as u32)) {
            if *edge == CanonEdgeKind::ImplFor {
                for_ty = Some(CanonId(dst.0));
                break;
            }
        }
        let mut for_trait: Option<CanonId> = None;
        for (dst, edge) in canon.type_graph.neighbours(canon::id::NodeId(idx as u32)) {
            if *edge == CanonEdgeKind::ImplRef {
                for_trait = Some(CanonId(dst.0));
                break;
            }
        }
        impl_payload_updates.push((idx, for_ty, for_trait));
    }
    for (idx, for_ty, for_trait) in impl_payload_updates {
        if let CanonNodeKind::Impl { for_ty: node_for_ty, for_trait: node_for_trait, .. } = &mut canon.nodes[idx].kind {
            if let Some(for_ty) = for_ty {
                *node_for_ty = for_ty;
            }
            *node_for_trait = for_trait;
        }
    }

    // Relink local type paths to structural ADT references.
    let mut local_type_by_path: HashMap<String, CanonId> = HashMap::new();
    for src_idx in 0..canon.module_graph.vertex_count() {
        let Some(CanonNodeKind::Module { path_id, .. }) = canon.nodes.get(src_idx).map(|n| &n.kind) else {
            continue;
        };
        let module_path = canon.lookup_path(*path_id).to_string();
        for (dst_id, edge) in canon.module_graph.neighbours(canon::id::NodeId(src_idx as u32)) {
            if *edge != CanonEdgeKind::Contains {
                continue;
            }
            let Some(kind) = canon.nodes.get(dst_id.index()).map(|n| &n.kind) else {
                continue;
            };
            let Some(name_id) = (match kind {
                CanonNodeKind::Struct { name_id, .. }
                | CanonNodeKind::Enum { name_id, .. }
                | CanonNodeKind::Trait { name_id, .. }
                | CanonNodeKind::TypeAlias { name_id, .. } => Some(*name_id),
                _ => None,
            }) else {
                continue;
            };
            let full = format!("{module_path}::{}", canon.lookup_name(name_id));
            local_type_by_path.entry(full).or_insert(CanonId(dst_id.0));
        }
    }

    let mut local_type_updates: Vec<(usize, CanonId)> = Vec::new();
    for (idx, node) in canon.nodes.iter().enumerate() {
        let path_id = match &node.kind {
            CanonNodeKind::Type { kind: TypeKind::Extern(path_id) } => Some(*path_id),
            CanonNodeKind::Type { kind: TypeKind::Unresolved(path_id) } => Some(*path_id),
            _ => None,
        };
        let Some(path_id) = path_id else {
            continue;
        };
        let path = canon.lookup_path(path_id);
        if let Some(&target) = local_type_by_path.get(path) {
            local_type_updates.push((idx, target));
        }
    }
    for (idx, target) in local_type_updates {
        if let CanonNodeKind::Type { kind } = &mut canon.nodes[idx].kind {
            *kind = TypeKind::Adt(target);
        }
    }

    canon
}
