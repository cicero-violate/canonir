use crate::norm;
use crate::types::{Body, EdgeHint, EdgeKind as ModelEdgeKind, EnumVariant, Field, GenericParam, Node, NodeId, NodeKind, StructKind, TraitMethod, Visibility};
use crate::{index::Index, Partial};
use canon::{
    csr_graph::CsrGraph,
    edge::EdgeKind as CanonEdgeKind,
    intern::{NameId, PathId},
    ir::CanonIR,
    node::{flags, CanonId, CanonNodeKind, CfgOp, PrimTy, TypeKind},
};
use rustc_middle::ty::TyCtxt;
use rustc_span::def_id::LOCAL_CRATE;
use std::collections::{HashMap, HashSet};

#[derive(Default)]
struct ModelLike {
    nodes: Vec<Node>,
    edge_hints: Vec<EdgeHint>,
    emit_order: Vec<NodeId>,
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
        Visibility::PubIn(_) => flags::PUB_CRATE,
        Visibility::Private => 0,
    }
}

fn split_top_level<'a>(s: &'a str, delim: char) -> Vec<&'a str> {
    let mut out: Vec<&str> = Vec::new();
    let mut start = 0usize;
    let mut angle = 0i32;
    let mut paren = 0i32;
    let mut bracket = 0i32;

    for (idx, ch) in s.char_indices() {
        match ch {
            '<' => angle += 1,
            '>' if angle > 0 => angle -= 1,
            '(' => paren += 1,
            ')' if paren > 0 => paren -= 1,
            '[' => bracket += 1,
            ']' if bracket > 0 => bracket -= 1,
            _ => {}
        }
        if ch == delim && angle == 0 && paren == 0 && bracket == 0 {
            out.push(s[start..idx].trim());
            start = idx + ch.len_utf8();
        }
    }
    out.push(s[start..].trim());
    out
}

fn wrapped_by(s: &str, open: u8, close: u8) -> bool {
    let bytes = s.as_bytes();
    if bytes.len() < 2 || bytes[0] != open || bytes[bytes.len() - 1] != close {
        return false;
    }
    let mut depth = 0i32;
    for (i, b) in bytes.iter().copied().enumerate() {
        if b == open {
            depth += 1;
        } else if b == close {
            depth -= 1;
            if depth == 0 && i != bytes.len() - 1 {
                return false;
            }
            if depth < 0 {
                return false;
            }
        }
    }
    depth == 0
}

fn split_generic_args(s: &str) -> Option<(&str, &str)> {
    let bytes = s.as_bytes();
    let mut start = None;
    let mut depth = 0i32;
    let mut end = None;
    for (i, b) in bytes.iter().copied().enumerate() {
        if b == b'<' {
            if depth == 0 {
                start = Some(i);
            }
            depth += 1;
        } else if b == b'>' {
            if depth == 0 {
                return None;
            }
            depth -= 1;
            if depth == 0 {
                end = Some(i);
                break;
            }
        }
    }
    let (start, end) = (start?, end?);
    if end != bytes.len() - 1 {
        return None;
    }
    let root = s[..start].trim();
    let args = s[start + 1..end].trim();
    if root.is_empty() {
        None
    } else {
        Some((root, args))
    }
}

fn parse_array_len(raw: &str) -> Option<u64> {
    let token = raw.trim();
    if token.is_empty() {
        return None;
    }
    let numeric = token.trim_end_matches(|c: char| c.is_ascii_alphabetic());
    let numeric = numeric.replace('_', "");
    numeric.parse::<u64>().ok()
}

fn normalize_type_text(raw: &str) -> String {
    let normalize_leaf = |s: &str| norm::norm_path(&norm::ty(s));
    let s = raw.trim();
    if s.is_empty() {
        return String::new();
    }

    if let Some(rest) = s.strip_prefix("&mut ") {
        return format!("&mut {}", normalize_type_text(rest));
    }
    if let Some(rest) = s.strip_prefix('&') {
        return format!("&{}", normalize_type_text(rest));
    }
    if let Some(rest) = s.strip_prefix("*const ") {
        return format!("*const {}", normalize_type_text(rest));
    }
    if let Some(rest) = s.strip_prefix("*mut ") {
        return format!("*mut {}", normalize_type_text(rest));
    }
    if let Some(rest) = s.strip_prefix("dyn ") {
        let bounds: Vec<String> = split_top_level(rest, '+')
            .into_iter()
            .filter(|b| !b.is_empty())
            .map(normalize_type_text)
            .collect();
        return if bounds.is_empty() { "dyn".to_string() } else { format!("dyn {}", bounds.join(" + ")) };
    }
    if let Some(rest) = s.strip_prefix("impl ") {
        let bounds: Vec<String> = split_top_level(rest, '+')
            .into_iter()
            .filter(|b| !b.is_empty())
            .map(normalize_type_text)
            .collect();
        return if bounds.is_empty() { "impl".to_string() } else { format!("impl {}", bounds.join(" + ")) };
    }
    if wrapped_by(s, b'(', b')') && s != "()" {
        let inner = &s[1..s.len() - 1];
        let parts = split_top_level(inner, ',');
        let trailing = inner.trim_end().ends_with(',');
        if parts.len() == 1 && !trailing {
            return normalize_type_text(parts[0]);
        }
        let elems: Vec<String> = parts
            .into_iter()
            .filter(|p| !p.is_empty())
            .map(normalize_type_text)
            .collect();
        if elems.len() == 1 {
            return format!("({},)", elems[0]);
        }
        return format!("({})", elems.join(", "));
    }
    if wrapped_by(s, b'[', b']') {
        let inner = &s[1..s.len() - 1];
        let parts = split_top_level(inner, ';');
        if parts.len() == 2 {
            return format!("[{}; {}]", normalize_type_text(parts[0]), parts[1].trim());
        }
        return format!("[{}]", normalize_type_text(inner));
    }
    if let Some((root, args)) = split_generic_args(s) {
        let args: Vec<String> = split_top_level(args, ',')
            .into_iter()
            .filter(|a| !a.is_empty())
            .map(normalize_type_text)
            .collect();
        let root = normalize_leaf(root);
        return format!("{}<{}>", root, args.join(", "));
    }
    normalize_leaf(s)
}

fn parse_fn_ptr(canon: &mut CanonIR, trimmed: &str) -> Option<TypeKind> {
    let rest = trimmed.strip_prefix("fn(")?;
    let mut depth = 1i32;
    let mut close_idx = None;
    for (i, ch) in rest.char_indices() {
        if ch == '(' {
            depth += 1;
        } else if ch == ')' {
            depth -= 1;
            if depth == 0 {
                close_idx = Some(i);
                break;
            }
        }
    }
    let close_idx = close_idx?;
    let params_src = &rest[..close_idx];
    let tail = rest[close_idx + 1..].trim();

    let params: Vec<CanonId> = split_top_level(params_src, ',')
        .into_iter()
        .filter(|p| !p.is_empty())
        .enumerate()
        .map(|(i, p)| {
            let name_id = NameId(canon.name_intern.intern(&format!("__fnptr_arg{}", i)));
            let ty = intern_ty(canon, p);
            canon.push_node(CanonNodeKind::Param { name_id, ty, flags: 0 })
        })
        .collect();

    let ret = if let Some(ret_ty) = tail.strip_prefix("->") {
        intern_ty(canon, ret_ty.trim())
    } else {
        unit_ty(canon)
    };
    let sig_id = canon.push_node(CanonNodeKind::FnSig { generics: vec![], params, ret, where_clauses: vec![] });
    Some(TypeKind::FnPtr(sig_id))
}

fn str_to_type_kind(canon: &mut CanonIR, ty: &str) -> TypeKind {
    let trimmed = ty.trim();
    if let Some(kind) = parse_fn_ptr(canon, trimmed) {
        return kind;
    }
    if let Some(inner) = trimmed.strip_prefix("*mut ") {
        let inner_id = intern_ty(canon, inner);
        return TypeKind::RawPtr { inner: inner_id, mutable: true };
    }
    if let Some(inner) = trimmed.strip_prefix("*const ") {
        let inner_id = intern_ty(canon, inner);
        return TypeKind::RawPtr { inner: inner_id, mutable: false };
    }
    if wrapped_by(trimmed, b'(', b')') && trimmed != "()" {
        let inner = &trimmed[1..trimmed.len() - 1];
        let parts = split_top_level(inner, ',');
        let trailing = inner.trim_end().ends_with(',');
        if parts.len() == 1 && !trailing {
            return str_to_type_kind(canon, parts[0]);
        }
        let elems: Vec<CanonId> = parts.into_iter().filter(|p| !p.is_empty()).map(|p| intern_ty(canon, p)).collect();
        return TypeKind::Tuple(elems);
    }
    if wrapped_by(trimmed, b'[', b']') {
        let inner = &trimmed[1..trimmed.len() - 1];
        let parts = split_top_level(inner, ';');
        if parts.len() == 2 {
            let inner_id = intern_ty(canon, parts[0]);
            if let Some(len) = parse_array_len(parts[1]) {
                return TypeKind::Array { inner: inner_id, len };
            }
            let normalized = normalize_type_text(trimmed);
            let pid = canon.intern_path(&normalized);
            return TypeKind::Extern(pid);
        }
        let inner_id = intern_ty(canon, inner);
        return TypeKind::Slice(inner_id);
    }
    // Structured parsing for reference types before primitive/extern fallthrough.
    if let Some(after_ref) = trimmed.strip_prefix('&') {
        let mut rest = after_ref.trim_start();
        let mut lifetime = None;
        if rest.starts_with('\'') {
            let mut split = rest.splitn(2, char::is_whitespace);
            if let Some(lt) = split.next() {
                let lt_id = NameId(canon.name_intern.intern(lt));
                lifetime = Some(canon.push_node(CanonNodeKind::Lifetime { name_id: lt_id }));
            }
            rest = split.next().map(str::trim_start).unwrap_or("");
        }
        let (mutable, inner) = if let Some(inner) = rest.strip_prefix("mut ") { (true, inner) } else { (false, rest) };
        let inner_id = intern_ty(canon, inner);
        return TypeKind::Ref { lifetime, inner: inner_id, mutable };
    }
    if let Some(inner) = trimmed.strip_prefix("dyn ") {
        let normalized = normalize_type_text(inner);
        let name_id = NameId(canon.name_intern.intern(&normalized));
        let trait_id = canon.push_node(CanonNodeKind::TypeRef { name_id });
        return TypeKind::DynTrait(trait_id);
    }
    if let Some(inner) = trimmed.strip_prefix("impl ") {
        let normalized = normalize_type_text(inner);
        let name_id = NameId(canon.name_intern.intern(&normalized));
        let trait_id = canon.push_node(CanonNodeKind::TypeRef { name_id });
        return TypeKind::ImplTrait(trait_id);
    }
    match trimmed {
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
            let normalized = normalize_type_text(other);
            let pid = canon.intern_path(&normalized);
            TypeKind::Extern(pid)
        }
    }
}

fn intern_ty(canon: &mut CanonIR, ty: &str) -> CanonId {
    let kind = str_to_type_kind(canon, ty);
    canon.intern_type(kind)
}

fn unit_ty(canon: &mut CanonIR) -> CanonId {
    canon.intern_type(TypeKind::Primitive(PrimTy::Unit))
}

fn bool_ty(canon: &mut CanonIR) -> CanonId {
    canon.intern_type(TypeKind::Primitive(PrimTy::Bool))
}

fn synth_local(canon: &mut CanonIR, raw: &str) -> CanonId {
    let name_id = NameId(canon.name_intern.intern(raw.trim()));
    let ty = unit_ty(canon);
    canon.push_node(CanonNodeKind::Local { name_id, ty, flags: 0 })
}

fn split_statements(src: &str) -> Vec<&str> {
    src.split(';').map(str::trim).filter(|s| !s.is_empty()).collect()
}

fn parse_method_call(canon: &mut CanonIR, stmt: &str) -> Option<CfgOp> {
    if stmt.starts_with("let ") || !stmt.contains('.') || !stmt.contains('(') || !stmt.ends_with(')') {
        return None;
    }
    let dot = stmt.find('.')?;
    let open = stmt[dot + 1..].find('(')? + dot + 1;
    let close = stmt.rfind(')')?;
    if open >= close {
        return None;
    }
    let receiver = stmt[..dot].trim();
    let method_raw = stmt[dot + 1..open].trim();
    let method = method_raw.split("::").next().unwrap_or(method_raw).trim();
    if receiver.is_empty() || method.is_empty() {
        return None;
    }
    let args_src = &stmt[open + 1..close];
    let args = split_top_level(args_src, ',')
        .into_iter()
        .filter(|a| !a.trim().is_empty())
        .map(|a| synth_local(canon, a))
        .collect();
    Some(CfgOp::MethodCall {
        receiver: synth_local(canon, receiver),
        method: NameId(canon.name_intern.intern(method)),
        args,
        dest: None,
    })
}

fn parse_field_access(canon: &mut CanonIR, stmt: &str) -> Option<CfgOp> {
    if stmt.starts_with("let ") || stmt.contains('(') || !stmt.contains('.') {
        return None;
    }
    let dot = stmt.find('.')?;
    let base = stmt[..dot].trim();
    let field = stmt[dot + 1..].trim();
    if base.is_empty() || field.is_empty() || !field.chars().all(|c| c.is_ascii_alphanumeric() || c == '_') {
        return None;
    }
    Some(CfgOp::FieldAccess {
        base: synth_local(canon, base),
        field: NameId(canon.name_intern.intern(field)),
        dest: None,
    })
}

fn parse_index(canon: &mut CanonIR, stmt: &str) -> Option<CfgOp> {
    if stmt.starts_with("let ") || !stmt.contains('[') || !stmt.ends_with(']') {
        return None;
    }
    let open = stmt.find('[')?;
    let close = stmt.rfind(']')?;
    if open >= close {
        return None;
    }
    let base = stmt[..open].trim();
    let idx = stmt[open + 1..close].trim();
    if base.is_empty() || idx.is_empty() {
        return None;
    }
    Some(CfgOp::Index {
        base: synth_local(canon, base),
        idx: synth_local(canon, idx),
        dest: None,
    })
}

fn parse_struct_lit(canon: &mut CanonIR, stmt: &str) -> Option<CfgOp> {
    if stmt.starts_with("let ") || !stmt.contains('{') || !stmt.contains('}') {
        return None;
    }
    let open = stmt.find('{')?;
    let close = stmt.rfind('}')?;
    if open >= close {
        return None;
    }
    let ty_src = stmt[..open].trim();
    if ty_src.is_empty() {
        return None;
    }
    let fields_src = &stmt[open + 1..close];
    let mut fields = Vec::new();
    for part in split_top_level(fields_src, ',') {
        if part.trim().is_empty() {
            continue;
        }
        let (name, value) = if let Some((n, v)) = part.split_once(':') { (n.trim(), v.trim()) } else { (part.trim(), part.trim()) };
        if name.is_empty() || value.is_empty() {
            continue;
        }
        fields.push((NameId(canon.name_intern.intern(name)), synth_local(canon, value)));
    }
    if fields.is_empty() {
        return None;
    }
    Some(CfgOp::StructLit {
        ty: intern_ty(canon, ty_src),
        fields,
        dest: None,
    })
}

fn lower_raw_stmt(canon: &mut CanonIR, stmt: &str) -> CfgOp {
    parse_method_call(canon, stmt)
        .or_else(|| parse_struct_lit(canon, stmt))
        .or_else(|| parse_index(canon, stmt))
        .or_else(|| parse_field_access(canon, stmt))
        .unwrap_or_else(|| CfgOp::Raw(NameId(canon.name_intern.intern(stmt))))
}

fn lower_raw_body(canon: &mut CanonIR, src: &str) -> Vec<CfgOp> {
    let mut ops: Vec<CfgOp> = split_statements(src).into_iter().map(|stmt| lower_raw_stmt(canon, stmt)).collect();
    if ops.is_empty() {
        ops.push(CfgOp::Raw(NameId(canon.name_intern.intern(src))));
    }
    ops
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

fn seal_body(canon: &mut CanonIR, body: &Body) -> Option<CanonId> {
    match body {
        Body::None => None,
        Body::Raw(src) => {
            let ops = lower_raw_body(canon, src);
            let bb_id = canon.push_node(CanonNodeKind::BasicBlock { ops, next: None });
            Some(canon.push_node(CanonNodeKind::Body { blocks: vec![bb_id] }))
        }
        Body::Blocks(blocks) => {
            let mut block_ids = Vec::with_capacity(blocks.len());
            for bb in blocks {
                let mut ops = Vec::new();
                for stmt in &bb.stmts {
                    use crate::types::Stmt;
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
                        Stmt::Raw(src) => lower_raw_stmt(canon, src),
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
                    Terminator::Return => ops.push(CfgOp::Return(None)),
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

    // Synthesize PathRef nodes from raw function/method bodies so downstream
    // solvers can consume structural external path references without scanning
    // raw interned text.
    let mut local_roots: HashSet<String> = HashSet::new();
    for n in &model_like.nodes {
        if let NodeKind::Module { path, .. } = &n.kind {
            if let Some(rest) = path.strip_prefix("crate::") {
                if let Some(root) = rest.split("::").next() {
                    if !root.is_empty() {
                        local_roots.insert(root.to_string());
                    }
                }
            }
        }
    }
    let crate_name = tcx.crate_name(LOCAL_CRATE).to_string();
    let mut seen_paths: HashSet<String> = HashSet::new();
    let mut next_id = model_like.nodes.iter().map(|n| n.id.0).max().map(|x| x + 1).unwrap_or(0);
    let mut extra_nodes: Vec<Node> = Vec::new();
    for n in &model_like.nodes {
        let body_src = match &n.kind {
            NodeKind::Function { body: Body::Raw(src), .. } | NodeKind::Method { body: Body::Raw(src), .. } => Some(src.as_str()),
            _ => None,
        };
        let Some(src) = body_src else {
            continue;
        };
        for path in extract_external_paths(src, &crate_name, &local_roots) {
            if seen_paths.insert(path.clone()) {
                extra_nodes.push(Node { id: NodeId(next_id), kind: NodeKind::PathRef { path }, span: n.span.clone() });
                next_id += 1;
            }
        }
    }
    model_like.nodes.extend(extra_nodes);

    model_like
}

fn extract_external_paths(src: &str, crate_name: &str, local_roots: &HashSet<String>) -> Vec<String> {
    const BUILTIN_ROOTS: &[&str] = &["std", "core", "alloc", "proc_macro", "crate", "self", "super"];
    let mut out: Vec<String> = Vec::new();
    for token in src.split(|c: char| !(c.is_ascii_alphanumeric() || c == '_' || c == ':' || c == '<' || c == '>')) {
        if token.is_empty() || !token.contains("::") {
            continue;
        }
        let Some((root, rest)) = token.split_once("::") else {
            continue;
        };
        if root.is_empty() || root == crate_name || BUILTIN_ROOTS.contains(&root) || local_roots.contains(root) {
            continue;
        }
        let Some(first_rest) = rest.chars().next() else {
            continue;
        };
        if !(first_rest.is_ascii_alphabetic() || first_rest == '_') {
            continue;
        }
        if !out.iter().any(|p| p == token) {
            out.push(token.to_string());
        }
    }
    out
}

pub fn canon_assemble(tcx: TyCtxt<'_>, index: &Index, parts: Vec<Partial>) -> CanonIR {
    let model_like = assemble_model_like(tcx, index, parts);
    let local_crate = tcx.crate_name(LOCAL_CRATE).to_string();
    let mut canon = CanonIR::new();

    let mut id_map: Vec<CanonId> = Vec::with_capacity(model_like.nodes.len());

    for node in &model_like.nodes {
        let canon_kind = match &node.kind {
            NodeKind::Crate { name, edition } => {
                let name_id = NameId(canon.name_intern.intern(name));
                let ed: u32 = edition.parse().unwrap_or(2021);
                CanonNodeKind::Crate { name_id, edition: ed, dependencies: vec![] }
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
                let for_ty = intern_ty(&mut canon, for_struct);
                let trait_ty = for_trait.as_deref().map(|t| intern_ty(&mut canon, t));
                let mut f = 0u32;
                if *unsafe_ {
                    f |= flags::UNSAFE;
                }
                CanonNodeKind::Impl { for_ty, for_trait: trait_ty, generics: vec![], attrs: vec![], flags: f }
            }
            NodeKind::Function { name, vis, params, ret, body, unsafe_, async_, .. } | NodeKind::Method { name, vis, params, ret, body, unsafe_, async_, .. } => {
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
            NodeKind::AssocType { name, vis, default_ty, .. } => {
                let name_id = NameId(canon.name_intern.intern(name));
                let default_ty = default_ty.as_deref().map(|t| intern_ty(&mut canon, t));
                CanonNodeKind::AssocType { name_id, generics: vec![], default_ty, flags: vis_flags(vis) }
            }
            NodeKind::AssocConst { name, vis, ty, default_value, .. } => {
                let name_id = NameId(canon.name_intern.intern(name));
                let ty_id = intern_ty(&mut canon, ty);
                let default_value = default_value.as_deref().map(|v| NameId(canon.name_intern.intern(v)));
                CanonNodeKind::AssocConst { name_id, ty: ty_id, default_value, flags: vis_flags(vis) }
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
                let ty_id = intern_ty(&mut canon, ty);
                CanonNodeKind::TypeAlias { name_id, generics: vec![], ty: ty_id, attrs: vec![], flags: vis_flags(vis) }
            }
            NodeKind::TypeRef { name } => CanonNodeKind::TypeRef { name_id: NameId(canon.name_intern.intern(name)) },
            NodeKind::Lifetime { name } => CanonNodeKind::Lifetime { name_id: NameId(canon.name_intern.intern(name)) },
            NodeKind::MacroCall { path, tokens } => {
                let path_id = canon.intern_path(path);
                let tokens_id = NameId(canon.name_intern.intern(tokens));
                CanonNodeKind::MacroCall { path_id, tokens_id }
            }
            NodeKind::PathRef { path } => {
                let path_id = canon.intern_path(path);
                CanonNodeKind::PathRef { path_id }
            }
        };

        id_map.push(canon.push_node(canon_kind));
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

    // Ensure every Extern type path is normalized before projection.
    let mut extern_type_updates: Vec<(usize, PathId)> = Vec::new();
    for idx in 0..canon.nodes.len() {
        let path_id = match &canon.nodes[idx].kind {
            CanonNodeKind::Type { kind: TypeKind::Extern(path_id) } => Some(*path_id),
            _ => None,
        };
        if let Some(path_id) = path_id {
            let raw = canon.lookup_path(path_id).to_string();
            let normalized = norm::norm_path(&raw);
            if normalized != raw {
                let normalized_id = canon.intern_path(&normalized);
                extern_type_updates.push((idx, normalized_id));
            }
        }
    }
    for (idx, normalized_id) in extern_type_updates {
        if let CanonNodeKind::Type { kind: TypeKind::Extern(path_id) } = &mut canon.nodes[idx].kind {
            *path_id = normalized_id;
        }
    }

    let mut local_module_roots: HashSet<String> = HashSet::new();
    for node in &canon.nodes {
        if let CanonNodeKind::Module { path_id, .. } = &node.kind {
            if let Some(rest) = canon.lookup_path(*path_id).strip_prefix("crate::") {
                if let Some(root) = rest.split("::").next() {
                    if !root.is_empty() {
                        local_module_roots.insert(root.to_string());
                    }
                }
            }
        }
    }

    // Normalize local crate-qualified paths once at assemble time.
    for path in &mut canon.path_intern.vec {
        let mut normalized = norm::local_crate_path(path, &local_crate);
        for root in &local_module_roots {
            let root_prefix = format!("{root}::");
            if normalized.starts_with(&root_prefix) {
                normalized = format!("crate::{normalized}");
            }
            normalized = normalized.replace(&format!("<{root}::"), &format!("<crate::{root}::"));
            normalized = normalized.replace(&format!("dyn {root}::"), &format!("dyn crate::{root}::"));
            normalized = normalized.replace(&format!("&{root}::"), &format!("&crate::{root}::"));
            normalized = normalized.replace(&format!("Box<dyn {root}::"), &format!("Box<dyn crate::{root}::"));
            normalized = normalized.replace(&format!(", {root}::"), &format!(", crate::{root}::"));
            normalized = normalized.replace(&format!("({root}::"), &format!("(crate::{root}::"));
        }
        *path = normalized;
    }
    canon.path_intern.restore_index();

    canon
}

#[cfg(test)]
mod tests {
    use super::str_to_type_kind;
    use canon::node::{PrimTy, TypeKind};
    use canon::CanonIR;

    #[test]
    fn parses_tuple_array_slice_and_grouped_types() {
        let mut ir = CanonIR::new();

        let tuple = str_to_type_kind(&mut ir, "(u8, &str)");
        assert!(matches!(tuple, TypeKind::Tuple(items) if items.len() == 2));

        let array = str_to_type_kind(&mut ir, "[u8; 32]");
        assert!(matches!(array, TypeKind::Array { len: 32, .. }));

        let slice = str_to_type_kind(&mut ir, "[u8]");
        assert!(matches!(slice, TypeKind::Slice(_)));

        let grouped = str_to_type_kind(&mut ir, "(u8)");
        assert!(matches!(grouped, TypeKind::Primitive(PrimTy::U8)));
    }

    #[test]
    fn parses_fn_ptr_and_raw_ptr() {
        let mut ir = CanonIR::new();

        let fn_ptr = str_to_type_kind(&mut ir, "fn(u8, &str) -> bool");
        assert!(matches!(fn_ptr, TypeKind::FnPtr(_)));

        let raw_ptr = str_to_type_kind(&mut ir, "*const u8");
        assert!(matches!(raw_ptr, TypeKind::RawPtr { mutable: false, .. }));
    }

    #[test]
    fn normalizes_generic_extern_text() {
        let mut ir = CanonIR::new();
        let ty = str_to_type_kind(&mut ir, "std::vec::Vec<std::option::Option<u8>>");
        match ty {
            TypeKind::Extern(path_id) => {
                assert_eq!(ir.lookup_path(path_id), "Vec<Option<u8>>");
            }
            _ => panic!("expected extern type"),
        }
    }
}
