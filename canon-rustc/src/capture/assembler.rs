use crate::capture::types::{Body, EdgeHint, EdgeKind as ModelEdgeKind, EnumVariant, Field, GenericParam, Node, NodeId, NodeKind, PrimType, StructKind, TraitMethod, TypeExpr, Visibility};
use crate::capture::{index::Index, Partial};
use canon_ir::{
    csr_graph::CsrGraph,
    edge::EdgeKind as CanonEdgeKind,
    PathId,
    intern::NameId,
    ir::CanonIR,
    node::{flags, CanonId, CanonNodeKind, CfgOp, DependencySpec, PatternKind, PrimTy, TypeKind, WherePredKind},
};
use rustc_middle::ty::TyCtxt;
use rustc_span::def_id::LOCAL_CRATE;
use std::collections::{HashMap, HashSet};
use std::path::PathBuf;
use crate::log::append_panic_record;

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
        let crate_root = match intern_path(canon, &crate_root) {
            Ok(id) => id,
            Err(_) => continue,
        };
        let package_name = match package_name.as_deref().filter(|pkg| !pkg.is_empty()).map(|pkg| intern_name(canon, pkg)).transpose() {
            Ok(id) => id,
            Err(_) => continue,
        };
        out.push(DependencySpec { crate_root, package_name });
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

pub fn render_type_expr(expr: &TypeExpr) -> String {
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

fn is_alloc_artifact(name: &str) -> bool {
    name.starts_with("__rust_")
        || name.starts_with("__rdl_")
        || name.starts_with("alloc::")
        || name.contains('\0')
        || name.contains("{{")
        || name.contains("{alloc")
        || name.contains("promoted[")
}

fn intern_name(canon: &mut CanonIR, name: &str) -> Result<NameId, anyhow::Error> {
    if is_alloc_artifact(name) {
        return Err(anyhow::anyhow!("canon-capture invariant violation: alloc/debug artifact leaked into name interner name={name:?}").context(format!("{:?}", std::backtrace::Backtrace::capture())));
    }
    Ok(NameId(canon.name_intern.intern(name)))
}

fn intern_ty_expr(canon: &mut CanonIR, ty: &TypeExpr) -> Result<CanonId, anyhow::Error> {
    let kind = match ty {
        TypeExpr::Primitive(prim) => TypeKind::Primitive(prim_to_canon(prim)),
        TypeExpr::Ref { lifetime, inner, mutable } => {
            let lifetime = lifetime.as_ref().map(|lt| -> Result<CanonId, anyhow::Error> {
                let name_id = intern_name(canon, lt)?;
                Ok(canon.push_node(CanonNodeKind::Lifetime { name_id }))
            }).transpose()?;
            let inner = intern_ty_expr(canon, inner)?;
            TypeKind::Ref { lifetime, inner, mutable: *mutable }
        }
        TypeExpr::RawPtr { inner, mutable } => {
            let inner = intern_ty_expr(canon, inner)?;
            TypeKind::RawPtr { inner, mutable: *mutable }
        }
        TypeExpr::Array { inner, len } => {
            let inner = intern_ty_expr(canon, inner)?;
            if let Some(len) = len {
                TypeKind::Array { inner, len: *len }
            } else {
                // Arrays without a concrete length must not introduce
                // TypeKind::Unresolved, as analysis requires a fully
                // resolved type graph. Fall back to a slice instead,
                // which preserves element type without fabricating an
                // unresolved path.
                TypeKind::Slice(inner)
            }
        }
        TypeExpr::Slice(inner) => TypeKind::Slice(intern_ty_expr(canon, inner)?),
        TypeExpr::Tuple(items) => {
            let elems = items.iter().map(|item| intern_ty_expr(canon, item)).collect::<Result<Vec<_>, _>>()?;
            TypeKind::Tuple(elems)
        }
        TypeExpr::FnPtr { params, ret } => {
            let params: Vec<CanonId> = params
                .iter()
                .enumerate()
                .map(|(i, p)| -> Result<CanonId, anyhow::Error> {
                    let name_id = intern_name(canon, &format!("__fnptr_arg{i}"))?;
                    let ty = intern_ty_expr(canon, p)?;
                    Ok(canon.push_node(CanonNodeKind::Param { name_id, ty, flags: 0 }))
                })
                .collect::<Result<Vec<_>, _>>()?;
            let ret = intern_ty_expr(canon, ret)?;
            let sig_id = canon.push_node(CanonNodeKind::FnSig { generics: vec![], params, ret, where_clauses: vec![] });
            TypeKind::FnPtr(sig_id)
        }
        TypeExpr::Param(name) => TypeKind::Param(intern_name(canon, name)?),
        TypeExpr::DynTrait(path) => {
            let name_id = intern_name(canon, path)?;
            let trait_id = canon.push_node(CanonNodeKind::TypeRef { name_id });
            TypeKind::DynTrait(trait_id)
        }
        TypeExpr::ImplTrait(path) => {
            let name_id = intern_name(canon, path)?;
            let trait_id = canon.push_node(CanonNodeKind::TypeRef { name_id });
            TypeKind::ImplTrait(trait_id)
        }
        TypeExpr::AppliedPath { base, args } => {
            let base_path = canonicalize_path_fragment(base)?;
            let base_path_id = intern_path(canon, &base_path)?;
            let base_ty = canon.intern_type(TypeKind::Extern(base_path_id));
            // Filter allocator type arguments like `std::alloc::Global`
            // which are implicit in many std types (e.g., Vec<T, A>) but
            // should not surface in projected Rust.
            let filtered_args: Vec<CanonId> = args
                .iter()
                .filter(|arg| match arg {
                    TypeExpr::Path(p) => !(p.contains("std::alloc::Global") || p.contains("alloc::Global")),
                    _ => true,
                })
                .map(|arg| intern_ty_expr(canon, arg))
                .collect::<Result<Vec<_>, _>>()?;

            TypeKind::Applied { base: base_ty, args: filtered_args }
        }
        TypeExpr::Path(path) => {
            let path = canonicalize_path_fragment(path)?;
            TypeKind::Extern(intern_path(canon, &path)?)
        }
    };
    Ok(canon.intern_type(kind))
}

fn canonicalize_path_fragment(raw: &str) -> Result<String, anyhow::Error> {
    let trimmed = raw.trim();
    let mut errors: Vec<String> = Vec::new();
    if trimmed.is_empty() {
        errors.push("empty path fragment".to_string());
    }
    let mut out = if let Some(rest) = trimmed.strip_prefix("$crate::") {
        format!("crate::{rest}")
    } else {
        trimmed.to_string()
    };
    out = match strip_generic_args(&out) {
        Ok(v) => v,
        Err(err) => {
            errors.push(format!("strip_generic_args: {err}"));
            strip_generic_args_lossy(&out)
        }
    };
    let mut segments: Vec<&str> = Vec::new();
    for seg in out.split("::") {
        if seg.is_empty() {
            errors.push(format!("empty path segment raw={raw:?}"));
            continue;
        }
        segments.push(seg);
    }
    out = segments.join("::");
    if out.is_empty() {
        errors.push(format!("empty path after normalization raw={raw:?}"));
    }
    let invalid = out.contains('$')
        || out.contains('{')
        || out.contains('}')
        || out.contains("=>")
        || out.contains('!')
        || out.contains('(')
        || out.contains(')')
        || out.starts_with(':')
        || out.ends_with(':');
    if invalid {
        errors.push(format!(
            "invalid path fragment raw={raw:?} normalized={out:?}"
        ));
    }
    if errors.is_empty() {
        return Ok(out);
    }
    let sanitized = sanitize_path_fragment(raw);
    append_panic_record(
        "path_fragment",
        &format!(
            "canon-capture tolerance: {}; raw={raw:?} normalized={out:?} sanitized={sanitized:?}",
            errors.join(" | ")
        ),
    );
    Ok(sanitized)
}

fn strip_generic_args(raw: &str) -> Result<String, anyhow::Error> {
    let mut out = String::with_capacity(raw.len());
    let mut depth = 0usize;
    for ch in raw.chars() {
        match ch {
            '<' => {
                depth += 1;
            }
            '>' => {
                if depth == 0 {
                    return Err(anyhow::anyhow!("canon-capture invariant violation: unmatched generic close in path raw={raw:?}").context(format!("{:?}", std::backtrace::Backtrace::capture())));
                }
                depth -= 1;
            }
            _ => {
                if depth == 0 {
                    out.push(ch);
                }
            }
        }
    }
    if depth != 0 {
        return Err(anyhow::anyhow!("canon-capture invariant violation: unterminated generics in path raw={raw:?}").context(format!("{:?}", std::backtrace::Backtrace::capture())));
    }
    Ok(out)
}

fn strip_generic_args_lossy(raw: &str) -> String {
    let mut out = String::with_capacity(raw.len());
    let mut depth = 0usize;
    for ch in raw.chars() {
        match ch {
            '<' => {
                depth = depth.saturating_add(1);
            }
            '>' => {
                if depth > 0 {
                    depth -= 1;
                }
            }
            _ => {
                if depth == 0 {
                    out.push(ch);
                }
            }
        }
    }
    out
}

fn sanitize_path_fragment(raw: &str) -> String {
    let trimmed = raw.trim();
    if trimmed.is_empty() {
        return "invalid_path".to_string();
    }
    let mut out = if let Some(rest) = trimmed.strip_prefix("$crate::") {
        format!("crate::{rest}")
    } else {
        trimmed.to_string()
    };
    out = strip_generic_args_lossy(&out);
    let mut cleaned_segments: Vec<String> = Vec::new();
    for seg in out.split("::") {
        let mut cleaned = String::with_capacity(seg.len());
        for ch in seg.chars() {
            if ch.is_ascii_alphanumeric() || ch == '_' {
                cleaned.push(ch);
            } else {
                cleaned.push('_');
            }
        }
        if !cleaned.is_empty() {
            cleaned_segments.push(cleaned);
        }
    }
    let cleaned = cleaned_segments.join("::");
    if cleaned.is_empty() {
        "invalid_path".to_string()
    } else {
        cleaned
    }
}

fn intern_path(canon: &mut CanonIR, raw: &str) -> Result<PathId, anyhow::Error> {
    let path = canonicalize_path_fragment(raw)?;
    match canon.intern_path(&path) {
        Ok(id) => Ok(id),
        Err(err) => {
            let sanitized = sanitize_path_fragment(raw);
            if sanitized != path {
                if let Ok(id) = canon.intern_path(&sanitized) {
                    append_panic_record(
                        "path_intern",
                        &format!(
                            "canon-capture tolerance: intern_path failed; raw={raw:?} normalized={path:?} sanitized={sanitized:?} err={err}"
                        ),
                    );
                    return Ok(id);
                }
            }
            append_panic_record(
                "path_intern",
                &format!(
                    "canon-capture tolerance: intern_path failed; raw={raw:?} normalized={path:?} sanitized={sanitized:?} err={err}"
                ),
            );
            canon.intern_path("invalid_path")
                .map_err(|e| anyhow::anyhow!("{e}"))
        }
    }
}

pub fn unit_ty(canon: &mut CanonIR) -> CanonId {
    canon.intern_type(TypeKind::Primitive(PrimTy::Unit))
}

fn bool_ty(canon: &mut CanonIR) -> CanonId {
    canon.intern_type(TypeKind::Primitive(PrimTy::Bool))
}

fn seal_generic_param(canon: &mut CanonIR, gp: &GenericParam) -> Result<CanonId, anyhow::Error> {
    let name_id = intern_name(canon, &gp.name)?;
    let bounds: Vec<CanonId> = gp
        .bounds
        .iter()
        .map(|b| -> Result<CanonId, anyhow::Error> {
            let bid = intern_name(canon, b)?;
            Ok(canon.push_node(CanonNodeKind::TypeRef { name_id: bid }))
        })
        .collect::<Result<Vec<_>, _>>()?;
    let default_ty = gp.default_ty.as_ref().map(|t| intern_ty_expr(canon, t)).transpose()?;
    Ok(canon.push_node(CanonNodeKind::GenericParam { name_id, bounds, is_lifetime: gp.is_lifetime, default_ty }))
}

fn seal_field(canon: &mut CanonIR, f: &Field) -> Result<CanonId, anyhow::Error> {
    let name_id = f.name.as_deref().map(|n| intern_name(canon, n)).transpose()?;
    let ty = intern_ty_expr(canon, &f.ty)?;
    Ok(canon.push_node(CanonNodeKind::Field { name_id, ty, flags: vis_flags(&f.vis) }))
}

fn seal_variant(canon: &mut CanonIR, v: &EnumVariant) -> Result<CanonId, anyhow::Error> {
    let name_id = intern_name(canon, &v.name)?;
    let fields = v.fields.iter().map(|f| seal_field(canon, f)).collect::<Result<Vec<_>, _>>()?;
    Ok(canon.push_node(CanonNodeKind::Variant { name_id, fields }))
}

fn seal_trait_method(canon: &mut CanonIR, m: &TraitMethod) -> Result<CanonId, anyhow::Error> {
    let name_id = intern_name(canon, &m.name)?;
    let ret_id = intern_ty_expr(canon, &m.ret)?;
    let param_ids: Vec<CanonId> = m
        .params
        .iter()
        .map(|p| -> Result<CanonId, anyhow::Error> {
            let pname = intern_name(canon, &p.name)?;
            let pty = intern_ty_expr(canon, &p.ty)?;
            let mut pf = 0u32;
            if p.mutable {
                pf |= flags::MUT;
            }
            Ok(canon.push_node(CanonNodeKind::Param { name_id: pname, ty: pty, flags: pf }))
        })
        .collect::<Result<Vec<_>, _>>()?;
    let generics = m.generics.iter().map(|g| seal_generic_param(canon, g)).collect::<Result<Vec<_>, _>>()?;
    let sig_id = canon.push_node(CanonNodeKind::FnSig { generics, params: param_ids, ret: ret_id, where_clauses: vec![] });
    let body = seal_body(canon, &m.body)?;
    let mut f = vis_flags(&m.vis);
    if m.unsafe_ {
        f |= flags::UNSAFE;
    }
    if m.async_ {
        f |= flags::ASYNC;
    }
    Ok(canon.push_node(CanonNodeKind::Fn { name_id, sig_id, body, attrs: vec![], flags: f }))
}

fn seal_body(canon: &mut CanonIR, body: &Body) -> Result<Option<CanonId>, anyhow::Error> {
    match body {
        Body::None => Ok(None),
        Body::Blocks(blocks) => {
            let mut block_ids = Vec::with_capacity(blocks.len());
            let mut locals: std::collections::HashMap<String, CanonId> = std::collections::HashMap::new();
            fn get_or_create_local(
                canon: &mut CanonIR,
                locals: &mut std::collections::HashMap<String, CanonId>,
                name: &str,
                ty: CanonId,
            ) -> Result<CanonId, anyhow::Error> {
                if let Some(id) = locals.get(name) {
                    return Ok(*id);
                }
                let name_id = intern_name(canon, name)?;
                let id = canon.push_node(CanonNodeKind::Local { name_id, ty, flags: 0 });
                locals.insert(name.to_string(), id);
                Ok(id)
            }
            for bb in blocks {
                let mut ops = Vec::new();
                // Centralized unresolved placeholder type to avoid
                // repeatedly fabricating distinct "unknown" path ids.
                // This keeps all late-link placeholders canonical.
                // TEMPORARY: use a single canonical unresolved type per body
                // instead of fabricating per-use unit types. This avoids
                // unit pollution while we refactor toward a proper
                // authoritative local registry.
                // Must not start with '_' (validator rejects private/helper segments)
                // Temporary canonical fallback type for locals that lack an explicit
                // annotation in the lowered Stmt surface. This must be a
                // validator-safe path segment and is expected to be resolved
                // by analysis before projection.
                // Use a concrete, analyzer-resolvable fallback instead of Unresolved.
                // This prevents TypeKind::Unresolved from surviving analysis while
                // still providing a deterministic placeholder type.
                // Reintroduce a canonical unresolved placeholder for unannotated locals.
                // Using unit here causes pervasive `():` pollution in projection
                // (E0308/E0599/E0609). Now that orchestration no longer hard-aborts
                // on unresolved types, prefer a distinct placeholder that can be
                // detected and properly resolved by analysis instead of silently
                // collapsing everything to `()`.
                // Use a distinct unresolved placeholder instead of unit `()`
                // for unannotated locals in the lowered surface.
                //
                // Using unit here pollutes CanonIR with `()`-typed locals
                // (notably `__ret`), which then project as
                // `let mut __ret = ();` even for non-unit-returning
                // functions. By emitting an Unresolved type, we defer
                // concretization to analysis/type authority instead of
                // collapsing everything to unit prematurely.
                // Reintroduce a canonical unresolved placeholder type for
                // unannotated locals. TypeKind::Unresolved requires a PathId,
                // so fabricate a single, validator-safe synthetic path and
                // intern it once per body as the fallback anchor.
                // Use a validator-safe synthetic segment (no leading underscore)
                // to avoid invariant violations about private/helper paths.
                // Use unit type as the temporary fallback for unannotated locals.
                // Unresolved types must not escape capture into analysis/projection.
                // This restores the invariant that TypeKind::Unresolved never
                // reaches projection.
                let fallback_ty = canon.intern_type(TypeKind::Tuple(vec![]));

                fn unknown_ty(fallback_ty: CanonId) -> CanonId {
                    fallback_ty
                }

                for stmt in &bb.stmts {
                    use crate::capture::types::Stmt;
                    let op = match stmt {
                        Stmt::Let { pat, ty, init: _ } => {
                            let ty_id = if let Some(t) = ty.as_ref() { intern_ty_expr(canon, t)? } else { unknown_ty(fallback_ty) };
                            // Do not synthesize a separate Local for initializer here.
                            // Initializations are modeled by subsequent Assign/Call ops.
                            // Emitting a fresh Local for `init` here breaks dataflow and
                            // causes widespread unit-typed pollution in the emit phase.
                            let rhs = None;
                            let lhs = get_or_create_local(canon, &mut locals, pat, ty_id)?;
                            CfgOp::Let { lhs, ty: ty_id, rhs }
                        }
                        Stmt::Assign { lhs, rhs } => {
                            // Use unresolved fallback for assignment operands as well.
                            // Forcing unit here pollutes CanonIR with `()`-typed locals
                            // (e.g., temporaries feeding __ret), which then project as
                            // `():` or cause E0308/E0599 mismatches. Defer concretization
                            // to analysis/type authority instead of collapsing to unit.
                            if lhs == "__ret" && rhs.trim() == "()" {
                                // Ignore synthetic unit writes into the return place.
                                // Non-unit return values are lowered by later assignments,
                                // and unit initialization here causes `let mut __ret = ();`
                                // to leak into projection.
                                continue;
                            }
                            let ty = unknown_ty(fallback_ty);
                            let lhs_name = lhs.as_str();
                            let lhs_id = get_or_create_local(canon, &mut locals, lhs_name, ty)?;
                            let rhs_id = get_or_create_local(canon, &mut locals, rhs, ty)?;
                            CfgOp::Assign { lhs: lhs_id, rhs: rhs_id }
                        }
                        Stmt::Expr(e) => {
                            // Avoid forcing expression temporaries to unit type.
                            // Use unresolved placeholder to prevent `()` pollution downstream.
                            // Use unit type instead of manufacturing an "unknown" unresolved type.
                            // Unresolved placeholders must not survive into analysis.
                            let ty = unknown_ty(fallback_ty);
                            let loc = get_or_create_local(canon, &mut locals, e, ty)?;
                            CfgOp::Expr(loc)
                        }
                        Stmt::Call { func, args, dest } => {
                            // Avoid interning malformed helper path segments like "_"
                            let ty = unknown_ty(fallback_ty);
                            let func_id = get_or_create_local(canon, &mut locals, func, ty)?;
                            let args: Vec<CanonId> = args
                                .iter()
                                .map(|arg| {
                                    get_or_create_local(canon, &mut locals, arg, ty)
                                })
                                .collect::<Result<Vec<_>, _>>()?;
                            let dest = dest.as_deref().map(|name| get_or_create_local(canon, &mut locals, name, ty)).transpose()?;
                            CfgOp::Call { func: func_id, args, dest }
                        }
                        Stmt::FieldAccess { base, field, dest } => {
                            // Avoid interning malformed helper path segments like "_"
                            let ty = unknown_ty(fallback_ty);
                            let base_id = get_or_create_local(canon, &mut locals, base, ty)?;
                            let field_id = intern_name(canon, field)?;
                            let dest_id = dest.as_deref().map(|name| get_or_create_local(canon, &mut locals, name, ty)).transpose()?;
                            CfgOp::FieldAccess { base: base_id, field: field_id, dest: dest_id }
                        }
                        Stmt::MethodCall { receiver, method, args, dest } => {
                            // Avoid interning malformed helper path segments like "_"
                            let ty = unknown_ty(fallback_ty);
                            let receiver_id = get_or_create_local(canon, &mut locals, receiver, ty)?;
                            let arg_ids: Vec<CanonId> = args
                                .iter()
                                .map(|arg| {
                                    get_or_create_local(canon, &mut locals, arg, ty)
                                })
                                .collect::<Result<Vec<_>, _>>()?;
                            let method_id = intern_name(canon, method)?;
                            let dest_id = dest.as_deref().map(|name| get_or_create_local(canon, &mut locals, name, ty)).transpose()?;
                            CfgOp::MethodCall { receiver: receiver_id, method: method_id, args: arg_ids, dest: dest_id }
                        }
                        Stmt::StructLit { ty, fields, dest } => {
                            let ty_id = intern_ty_expr(canon, ty)?;
                            // Do not force struct literal field temporaries to unit.
                            let value_ty = unknown_ty(fallback_ty);
                            let lowered_fields: Vec<(NameId, CanonId)> = fields
                                .iter()
                                .map(|(field, value)| -> Result<(NameId, CanonId), anyhow::Error> {
                                    let field_name = intern_name(canon, field)?;
                                    Ok((field_name, get_or_create_local(canon, &mut locals, value, value_ty)?))
                                })
                                .collect::<Result<Vec<_>, _>>()?;
                            let dest_id = dest.as_deref().map(|name| get_or_create_local(canon, &mut locals, name, value_ty)).transpose()?;
                            CfgOp::StructLit { ty: ty_id, fields: lowered_fields, dest: dest_id }
                        }
                        Stmt::Match { dest } => {
                            // Avoid unit-typing match destinations; preserve unresolved type.
                            let ty = unknown_ty(fallback_ty);
                            let dest = dest.as_deref().map(|name| get_or_create_local(canon, &mut locals, name, ty)).transpose()?;
                            CfgOp::Match { dest }
                        }
                        Stmt::Return(val) => {
                            // Do not force return temporaries to unit type.
                            // Use an unresolved placeholder type to avoid
                            // polluting downstream emit with `()`-typed locals.
                            let v = val.as_deref().map(|e| -> Result<CanonId, anyhow::Error> {
                                // Normalize borrow at Rust return boundary: if returning
                                // Do not inject a dereference for `__ret`.
                                // The capture layer must ensure `__ret`
                                // already matches the authoritative return type.
                                let rendered = e;
                                let ty = unknown_ty(fallback_ty);
                                get_or_create_local(canon, &mut locals, rendered, ty)
                            }).transpose()?;
                            CfgOp::Return(v)
                        }
                    };
                    ops.push(op);
                }
                use crate::capture::types::Terminator;
                match &bb.terminator {
                    Terminator::Goto(t) => ops.push(CfgOp::Goto(*t)),
                    Terminator::Branch { cond, true_bb, false_bb } => {
                        let ty = bool_ty(canon);
                        let cloc = get_or_create_local(canon, &mut locals, cond, ty)?;
                        ops.push(CfgOp::Branch { cond: cloc, true_bb: *true_bb, false_bb: *false_bb });
                    }
                    Terminator::Switch { discr, targets, otherwise } => {
                        let ty = unknown_ty(fallback_ty);
                        let discr_id = get_or_create_local(canon, &mut locals, discr, ty)?;
                        let mut lowered: Vec<(CanonId, u32)> = Vec::with_capacity(targets.len());
                        for (value, bb) in targets {
                            let value_id = get_or_create_local(canon, &mut locals, value, ty)?;
                            lowered.push((value_id, *bb));
                        }
                        ops.push(CfgOp::Switch { discr: discr_id, targets: lowered, otherwise: *otherwise });
                    }
                    Terminator::Return => {}
                    Terminator::Unreachable => ops.push(CfgOp::Unreachable),
                    Terminator::None => {}
                }
                block_ids.push(canon.push_node(CanonNodeKind::BasicBlock { ops, next: None }));
            }
            Ok(Some(canon.push_node(CanonNodeKind::Body { blocks: block_ids })))
        }
    }
}

fn collect_contains_edges(canon: &CanonIR, out: &mut Vec<(CanonId, CanonId, CanonEdgeKind)>) {
    use CanonEdgeKind::Contains;
    for (idx, node) in canon.nodes.iter().enumerate() {
        let src = CanonId(idx as u32);
        match &node.kind {
            CanonNodeKind::Crate { .. } => {}
            CanonNodeKind::Module { .. } => {}
            CanonNodeKind::Struct { generics, fields, derives, attrs, .. } => {
                push_contains_ids(out, src, generics);
                push_contains_ids(out, src, fields);
                push_contains_ids(out, src, derives);
                push_contains_ids(out, src, attrs);
            }
            CanonNodeKind::Enum { generics, variants, derives, attrs, .. } => {
                push_contains_ids(out, src, generics);
                push_contains_ids(out, src, variants);
                push_contains_ids(out, src, derives);
                push_contains_ids(out, src, attrs);
            }
            CanonNodeKind::Trait { generics, methods, attrs, .. } => {
                push_contains_ids(out, src, generics);
                push_contains_ids(out, src, methods);
                push_contains_ids(out, src, attrs);
            }
            CanonNodeKind::AssocType { generics, default_ty, .. } => {
                push_contains_ids(out, src, generics);
                if let Some(id) = default_ty {
                    out.push((src, *id, Contains));
                }
            }
            CanonNodeKind::AssocConst { ty, .. } => {
                out.push((src, *ty, Contains));
            }
            CanonNodeKind::Impl { generics, attrs, .. } => {
                push_contains_ids(out, src, generics);
                push_contains_ids(out, src, attrs);
            }
            CanonNodeKind::Fn { sig_id, body, attrs, .. } => {
                out.push((src, *sig_id, Contains));
                if let Some(id) = body {
                    out.push((src, *id, Contains));
                }
                push_contains_ids(out, src, attrs);
            }
            CanonNodeKind::FnSig { generics, params, ret, where_clauses } => {
                push_contains_ids(out, src, generics);
                push_contains_ids(out, src, params);
                out.push((src, *ret, Contains));
                push_contains_ids(out, src, where_clauses);
            }
            CanonNodeKind::Type { kind } => {
                collect_type_contains(kind, src, out);
            }
            CanonNodeKind::Field { ty, .. } => {
                out.push((src, *ty, Contains));
            }
            CanonNodeKind::Param { ty, .. } => {
                out.push((src, *ty, Contains));
            }
            CanonNodeKind::GenericParam { bounds, default_ty, .. } => {
                push_contains_ids(out, src, bounds);
                if let Some(id) = default_ty {
                    out.push((src, *id, Contains));
                }
            }
            CanonNodeKind::WherePred { kind } => {
                match kind {
                    WherePredKind::TypeBound { ty, bounds } => {
                        out.push((src, *ty, Contains));
                        push_contains_ids(out, src, bounds);
                    }
                    WherePredKind::LifetimeBound { lifetime, bounds } => {
                        out.push((src, *lifetime, Contains));
                        push_contains_ids(out, src, bounds);
                    }
                }
            }
            CanonNodeKind::Variant { fields, .. } => {
                push_contains_ids(out, src, fields);
            }
            CanonNodeKind::Attr { .. } => {}
            CanonNodeKind::Lifetime { .. } => {}
            CanonNodeKind::Const { ty, attrs, .. } => {
                out.push((src, *ty, Contains));
                push_contains_ids(out, src, attrs);
            }
            CanonNodeKind::Static { ty, attrs, .. } => {
                out.push((src, *ty, Contains));
                push_contains_ids(out, src, attrs);
            }
            CanonNodeKind::Use { target, .. } => {
                if let Some(id) = target {
                    out.push((src, *id, Contains));
                }
            }
            CanonNodeKind::ExternCrate { .. } => {}
            CanonNodeKind::TypeAlias { generics, ty, attrs, .. } => {
                push_contains_ids(out, src, generics);
                out.push((src, *ty, Contains));
                push_contains_ids(out, src, attrs);
            }
            CanonNodeKind::TypeRef { .. } => {}
            CanonNodeKind::MacroCall { .. } => {}
            CanonNodeKind::PathRef { .. } => {}
            CanonNodeKind::Body { blocks } => {
                push_contains_ids(out, src, blocks);
            }
            CanonNodeKind::BasicBlock { ops, .. } => {
                for op in ops {
                    collect_cfgop_contains(op, src, out);
                }
            }
            CanonNodeKind::MatchArm { pattern, guard, body } => {
                out.push((src, *pattern, Contains));
                if let Some(id) = guard {
                    out.push((src, *id, Contains));
                }
                out.push((src, *body, Contains));
            }
            CanonNodeKind::Pattern { kind } => {
                collect_pattern_contains(kind, src, out);
            }
            CanonNodeKind::VisPath { .. } => {}
            CanonNodeKind::Local { ty, .. } => {
                out.push((src, *ty, Contains));
            }
        }
    }
}

fn push_contains_ids(out: &mut Vec<(CanonId, CanonId, CanonEdgeKind)>, src: CanonId, ids: &[CanonId]) {
    for id in ids {
        out.push((src, *id, CanonEdgeKind::Contains));
    }
}

fn collect_type_contains(kind: &TypeKind, src: CanonId, out: &mut Vec<(CanonId, CanonId, CanonEdgeKind)>) {
    use TypeKind::*;
    match kind {
        Primitive(_) => {}
        Adt(id) => out.push((src, *id, CanonEdgeKind::Contains)),
        Ref { lifetime, inner, .. } => {
            out.push((src, *inner, CanonEdgeKind::Contains));
            if let Some(id) = lifetime {
                out.push((src, *id, CanonEdgeKind::Contains));
            }
        }
        RawPtr { inner, .. } => out.push((src, *inner, CanonEdgeKind::Contains)),
        Array { inner, .. } => out.push((src, *inner, CanonEdgeKind::Contains)),
        Slice(inner) => out.push((src, *inner, CanonEdgeKind::Contains)),
        Tuple(items) => push_contains_ids(out, src, items),
        FnPtr(sig) => out.push((src, *sig, CanonEdgeKind::Contains)),
        ImplTrait(id) => out.push((src, *id, CanonEdgeKind::Contains)),
        DynTrait(id) => out.push((src, *id, CanonEdgeKind::Contains)),
        Param(_) => {}
        Applied { base, args } => {
            out.push((src, *base, CanonEdgeKind::Contains));
            push_contains_ids(out, src, args);
        }
        Extern(_) => {}
        Unresolved(_) => {}
        TypeRef { .. } => {}
    }
}

fn collect_cfgop_contains(op: &CfgOp, src: CanonId, out: &mut Vec<(CanonId, CanonId, CanonEdgeKind)>) {
    use CfgOp::*;
    match op {
        Let { lhs, ty, rhs } => {
            out.push((src, *lhs, CanonEdgeKind::Contains));
            out.push((src, *ty, CanonEdgeKind::Contains));
            if let Some(id) = rhs {
                out.push((src, *id, CanonEdgeKind::Contains));
            }
        }
        Assign { lhs, rhs } => {
            out.push((src, *lhs, CanonEdgeKind::Contains));
            out.push((src, *rhs, CanonEdgeKind::Contains));
        }
        Return(ret) => {
            if let Some(id) = ret {
                out.push((src, *id, CanonEdgeKind::Contains));
            }
        }
        Call { func, args, dest } => {
            out.push((src, *func, CanonEdgeKind::Contains));
            push_contains_ids(out, src, args);
            if let Some(id) = dest {
                out.push((src, *id, CanonEdgeKind::Contains));
            }
        }
        FieldAccess { base, dest, .. } => {
            out.push((src, *base, CanonEdgeKind::Contains));
            if let Some(id) = dest {
                out.push((src, *id, CanonEdgeKind::Contains));
            }
        }
        MethodCall { receiver, args, dest, .. } => {
            out.push((src, *receiver, CanonEdgeKind::Contains));
            push_contains_ids(out, src, args);
            if let Some(id) = dest {
                out.push((src, *id, CanonEdgeKind::Contains));
            }
        }
        Index { base, idx, dest } => {
            out.push((src, *base, CanonEdgeKind::Contains));
            out.push((src, *idx, CanonEdgeKind::Contains));
            if let Some(id) = dest {
                out.push((src, *id, CanonEdgeKind::Contains));
            }
        }
        Closure { sig_id, body_id } => {
            out.push((src, *sig_id, CanonEdgeKind::Contains));
            out.push((src, *body_id, CanonEdgeKind::Contains));
        }
        InvalidUnitSentinel => {}
        StructLit { ty, fields, dest } => {
            out.push((src, *ty, CanonEdgeKind::Contains));
            for (_, val) in fields {
                out.push((src, *val, CanonEdgeKind::Contains));
            }
            if let Some(id) = dest {
                out.push((src, *id, CanonEdgeKind::Contains));
            }
        }
        Match { dest } => {
            if let Some(id) = dest {
                out.push((src, *id, CanonEdgeKind::Contains));
            }
        }
        Branch { cond, .. } => {
            out.push((src, *cond, CanonEdgeKind::Contains));
        }
        Goto(_) => {}
        Switch { discr, targets, .. } => {
            out.push((src, *discr, CanonEdgeKind::Contains));
            for (val, _) in targets {
                out.push((src, *val, CanonEdgeKind::Contains));
            }
        }
        Unreachable => {}
        SizeOf { ty, dest } => {
            out.push((src, *ty, CanonEdgeKind::Contains));
            out.push((src, *dest, CanonEdgeKind::Contains));
        }
        AlignOf { ty, dest } => {
            out.push((src, *ty, CanonEdgeKind::Contains));
            out.push((src, *dest, CanonEdgeKind::Contains));
        }
        OffsetOf { ty, dest, .. } => {
            out.push((src, *ty, CanonEdgeKind::Contains));
            out.push((src, *dest, CanonEdgeKind::Contains));
        }
        Transmute { operand, src_ty, dst_ty, dest } => {
            out.push((src, *operand, CanonEdgeKind::Contains));
            out.push((src, *src_ty, CanonEdgeKind::Contains));
            out.push((src, *dst_ty, CanonEdgeKind::Contains));
            out.push((src, *dest, CanonEdgeKind::Contains));
        }
        Expr(id) => {
            out.push((src, *id, CanonEdgeKind::Contains));
        }
    }
}

fn collect_pattern_contains(kind: &PatternKind, src: CanonId, out: &mut Vec<(CanonId, CanonId, CanonEdgeKind)>) {
    match kind {
        PatternKind::Wildcard => {}
        PatternKind::Binding { .. } => {}
        PatternKind::Tuple(items) => {
            push_contains_ids(out, src, items);
        }
        PatternKind::Struct { fields, .. } => {
            for (_, id) in fields {
                out.push((src, *id, CanonEdgeKind::Contains));
            }
        }
        PatternKind::TupleStruct { fields, .. } => {
            push_contains_ids(out, src, fields);
        }
        PatternKind::Literal(_) => {}
        PatternKind::Or(items) => {
            push_contains_ids(out, src, items);
        }
    }
}

fn assemble_model_like(tcx: TyCtxt<'_>, index: &Index, parts: Vec<Partial>) -> Result<ModelLike, anyhow::Error> {
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
    let mut invalid_edges: Vec<EdgeHint> = Vec::new();
    for h in &model_like.edge_hints {
        if !valid_ids.contains(&h.src) || !valid_ids.contains(&h.dst) {
            invalid_edges.push(h.clone());
        }
    }
    if !invalid_edges.is_empty() {
        return Err(anyhow::anyhow!(
            "canon-capture invariant violation: edge_hints reference missing nodes (count={})",
            invalid_edges.len()
        ).context(format!("{:?}", std::backtrace::Backtrace::capture())));
    }

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

    let mut module_ids: Vec<u32> = Vec::new();
    for node in &model_like.nodes {
        if matches!(node.kind, NodeKind::Module { .. }) {
            module_ids.push(node.id.0);
        }
    }
    let mut module_has_parent: HashSet<u32> = HashSet::new();
    for h in &model_like.edge_hints {
        if h.kind == ModelEdgeKind::Contains && module_ids.contains(&h.dst) {
            module_has_parent.insert(h.dst);
        }
    }
    for module_id in module_ids {
        if !module_has_parent.contains(&module_id) {
            model_like.edge_hints.push(EdgeHint { src: 0, dst: module_id, kind: ModelEdgeKind::Contains });
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

    Ok(model_like)
}

pub fn canon_assemble(tcx: TyCtxt<'_>, index: &Index, parts: Vec<Partial>) -> Result<CanonIR, anyhow::Error> {
    let model_like = assemble_model_like(tcx, index, parts)?;
    let mut canon = CanonIR::new();

    let mut id_map: Vec<CanonId> = Vec::with_capacity(model_like.nodes.len());
    let mut pending_vis_paths: Vec<(CanonId, String)> = Vec::new();

    for node in &model_like.nodes {
        let canon_kind = match &node.kind {
            NodeKind::Crate { name, edition } => {
                let name_id = intern_name(&mut canon, name)?;
                let cargo_name = if name.contains('_') { Some(intern_name(&mut canon, &name.replace('_', "-"))?) } else { None };
                let ed: u32 = edition.parse().unwrap_or(2021);
                let declared_dependencies = load_declared_dependency_specs(&mut canon);
                CanonNodeKind::Crate { name_id, cargo_name, edition: ed, dependencies: vec![], dependency_packages: vec![], declared_dependencies }
            }
            NodeKind::Module { path, vis, inline, .. } => {
                let path_id = intern_path(&mut canon, path)?;
                let mut f = vis_flags(vis);
                if *inline {
                    f |= flags::INLINE;
                }
                CanonNodeKind::Module { path_id, flags: f }
            }
            NodeKind::Struct { name, vis, struct_kind, .. } => {
                let name_id = intern_name(&mut canon, name)?;
                let sk = match struct_kind {
                    StructKind::Named => 0,
                    StructKind::Tuple => 1,
                    StructKind::Unit => 2,
                };
                CanonNodeKind::Struct { name_id, generics: vec![], fields: vec![], derives: vec![], attrs: vec![], flags: vis_flags(vis), struct_kind: sk }
            }
            NodeKind::Enum { name, vis, .. } => {
                let name_id = intern_name(&mut canon, name)?;
                CanonNodeKind::Enum { name_id, generics: vec![], variants: vec![], derives: vec![], attrs: vec![], flags: vis_flags(vis) }
            }
            NodeKind::Trait { name, vis, unsafe_, .. } => {
                let name_id = intern_name(&mut canon, name)?;
                let mut f = vis_flags(vis);
                if *unsafe_ {
                    f |= flags::UNSAFE;
                }
                CanonNodeKind::Trait { name_id, generics: vec![], methods: vec![], attrs: vec![], flags: f }
            }
            NodeKind::Impl { for_struct, for_trait, unsafe_, .. } => {
                let for_ty = intern_ty_expr(&mut canon, for_struct)?;
                let trait_ty = for_trait.as_ref().map(|t| intern_ty_expr(&mut canon, t)).transpose()?;
                let mut f = 0u32;
                if *unsafe_ {
                    f |= flags::UNSAFE;
                }
                CanonNodeKind::Impl { for_ty, for_trait: trait_ty, generics: vec![], attrs: vec![], flags: f }
            }
            NodeKind::Function { name, vis, params, ret, body, unsafe_, async_, .. } | NodeKind::Method { name, vis, params, ret, body, unsafe_, async_, .. } => {
                let name_id = intern_name(&mut canon, name)?;
                let ret_id = intern_ty_expr(&mut canon, ret)?;
                let param_ids: Vec<CanonId> = params
                    .iter()
                    .map(|p| -> Result<CanonId, anyhow::Error> {
                        let pname = intern_name(&mut canon, &p.name)?;
                        let pty = intern_ty_expr(&mut canon, &p.ty)?;
                        let mut pf = 0u32;
                        if p.mutable {
                            pf |= flags::MUT;
                        }
                        Ok(canon.push_node(CanonNodeKind::Param { name_id: pname, ty: pty, flags: pf }))
                    })
                    .collect::<Result<Vec<_>, _>>()?;
                let sig_id = canon.push_node(CanonNodeKind::FnSig { generics: vec![], params: param_ids, ret: ret_id, where_clauses: vec![] });
                let body_id = seal_body(&mut canon, body)?;
                let mut f = vis_flags(vis);
                if *unsafe_ {
                    f |= flags::UNSAFE;
                }
                if *async_ {
                    f |= flags::ASYNC;
                }
                if body_id.is_none() {
                    f |= flags::EXTERN;
                }
                CanonNodeKind::Fn { name_id, sig_id, body: body_id, attrs: vec![], flags: f }
            }
            NodeKind::AssocType { name, vis, default_ty, .. } => {
                let name_id = intern_name(&mut canon, name)?;
                let default_ty = default_ty.as_ref().map(|t| intern_ty_expr(&mut canon, t)).transpose()?;
                CanonNodeKind::AssocType { name_id, generics: vec![], default_ty, flags: vis_flags(vis) }
            }
            NodeKind::AssocConst { name, vis, ty, default_value, .. } => {
                let name_id = intern_name(&mut canon, name)?;
                let ty_id = intern_ty_expr(&mut canon, ty)?;
                let default_value = default_value.as_deref().map(|v| intern_name(&mut canon, v)).transpose()?;
                CanonNodeKind::AssocConst { name_id, ty: ty_id, default_value, flags: vis_flags(vis) }
            }
            NodeKind::Const { name, vis, ty, value, .. } => {
                let name_id = intern_name(&mut canon, name)?;
                let ty_id = intern_ty_expr(&mut canon, ty)?;
                let value_id = intern_name(&mut canon, value)?;
                CanonNodeKind::Const { name_id, ty: ty_id, value_id, attrs: vec![], flags: vis_flags(vis) }
            }
            NodeKind::Static { name, vis, ty, value, mutable, .. } => {
                let name_id = intern_name(&mut canon, name)?;
                let ty_id = intern_ty_expr(&mut canon, ty)?;
                let value_id = intern_name(&mut canon, value)?;
                let mut f = vis_flags(vis);
                if *mutable {
                    f |= flags::MUT;
                }
                CanonNodeKind::Static { name_id, ty: ty_id, value_id, attrs: vec![], flags: f }
            }
            NodeKind::Use { vis, path, alias, glob } => {
                let path_id = intern_path(&mut canon, path)?;
                let alias_id = alias.as_deref().map(|a| intern_name(&mut canon, a)).transpose()?;
                let mut f = vis_flags(vis);
                if *glob {
                    f |= flags::GLOB;
                }
                CanonNodeKind::Use { path_id, alias: alias_id, flags: f, target: None }
            }
            NodeKind::ExternCrate { name, alias, vis } => {
                let name_id = intern_name(&mut canon, name)?;
                let alias_id = alias.as_deref().map(|a| intern_name(&mut canon, a)).transpose()?;
                CanonNodeKind::ExternCrate { name_id, alias: alias_id, flags: vis_flags(vis) }
            }
            NodeKind::TypeAlias { name, vis, ty, .. } => {
                let name_id = intern_name(&mut canon, name)?;
                let ty_id = intern_ty_expr(&mut canon, ty)?;
                CanonNodeKind::TypeAlias { name_id, generics: vec![], ty: ty_id, attrs: vec![], flags: vis_flags(vis) }
            }
            NodeKind::TypeRef { name } => CanonNodeKind::TypeRef { name_id: intern_name(&mut canon, name)? },
            NodeKind::Lifetime { name } => CanonNodeKind::Lifetime { name_id: intern_name(&mut canon, name)? },
            NodeKind::MacroCall { path, tokens } => {
                let path_id = intern_path(&mut canon, path)?;
                let tokens_id = canon.intern_body(tokens);
                CanonNodeKind::MacroCall { path_id, tokens_id }
            }
            NodeKind::PathRef { path } => {
                let path_id = intern_path(&mut canon, path)?;
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
        let path_id = intern_path(&mut canon, &path)?;
        let vis_id = canon.push_node(CanonNodeKind::VisPath { flags: flags::PUB_IN, path_id });
        vispath_edges.push((owner, vis_id, CanonEdgeKind::Contains));
    }

    for (i, node) in model_like.nodes.iter().enumerate() {
        let cid = id_map[i];
        match &node.kind {
            NodeKind::Struct { generics, fields, derives, .. } => {
                let gen_ids: Vec<CanonId> = generics.iter().map(|g| seal_generic_param(&mut canon, g)).collect::<Result<Vec<_>, _>>()?;
                let field_ids: Vec<CanonId> = fields.iter().map(|f| seal_field(&mut canon, f)).collect::<Result<Vec<_>, _>>()?;
                let derive_ids: Vec<CanonId> = derives
                    .iter()
                    .map(|d| -> Result<CanonId, anyhow::Error> {
                        let name_id = intern_name(&mut canon, d)?;
                        Ok(canon.push_node(CanonNodeKind::TypeRef { name_id }))
                    })
                    .collect::<Result<Vec<_>, _>>()?;
                if let CanonNodeKind::Struct { generics, fields, derives, .. } = &mut canon.nodes[cid.0 as usize].kind {
                    *generics = gen_ids;
                    *fields = field_ids;
                    *derives = derive_ids;
                }
            }
            NodeKind::Enum { generics, variants, derives, .. } => {
                let gen_ids: Vec<CanonId> = generics.iter().map(|g| seal_generic_param(&mut canon, g)).collect::<Result<Vec<_>, _>>()?;
                let variant_ids: Vec<CanonId> = variants.iter().map(|v| seal_variant(&mut canon, v)).collect::<Result<Vec<_>, _>>()?;
                let derive_ids: Vec<CanonId> = derives
                    .iter()
                    .map(|d| -> Result<CanonId, anyhow::Error> {
                        let name_id = intern_name(&mut canon, d)?;
                        Ok(canon.push_node(CanonNodeKind::TypeRef { name_id }))
                    })
                    .collect::<Result<Vec<_>, _>>()?;
                if let CanonNodeKind::Enum { generics, variants, derives, .. } = &mut canon.nodes[cid.0 as usize].kind {
                    *generics = gen_ids;
                    *variants = variant_ids;
                    *derives = derive_ids;
                }
            }
            NodeKind::Trait { generics, methods, .. } => {
                let gen_ids: Vec<CanonId> = generics.iter().map(|g| seal_generic_param(&mut canon, g)).collect::<Result<Vec<_>, _>>()?;
                let method_ids: Vec<CanonId> = methods.iter().map(|m| seal_trait_method(&mut canon, m)).collect::<Result<Vec<_>, _>>()?;
                if let CanonNodeKind::Trait { generics, methods, .. } = &mut canon.nodes[cid.0 as usize].kind {
                    *generics = gen_ids;
                    *methods = method_ids;
                }
            }
            NodeKind::Impl { generics, .. } => {
                let gen_ids: Vec<CanonId> = generics.iter().map(|g| seal_generic_param(&mut canon, g)).collect::<Result<Vec<_>, _>>()?;
                if let CanonNodeKind::Impl { generics, .. } = &mut canon.nodes[cid.0 as usize].kind {
                    *generics = gen_ids;
                }
            }
            NodeKind::Function { generics, .. } | NodeKind::Method { generics, .. } => {
                let gen_ids: Vec<CanonId> = generics.iter().map(|g| seal_generic_param(&mut canon, g)).collect::<Result<Vec<_>, _>>()?;
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
                let gen_ids: Vec<CanonId> = generics.iter().map(|g| seal_generic_param(&mut canon, g)).collect::<Result<Vec<_>, _>>()?;
                if let CanonNodeKind::TypeAlias { generics, .. } = &mut canon.nodes[cid.0 as usize].kind {
                    *generics = gen_ids;
                }
            }
            NodeKind::AssocType { generics, .. } => {
                let gen_ids: Vec<CanonId> = generics.iter().map(|g| seal_generic_param(&mut canon, g)).collect::<Result<Vec<_>, _>>()?;
                if let CanonNodeKind::AssocType { generics, .. } = &mut canon.nodes[cid.0 as usize].kind {
                    *generics = gen_ids;
                }
            }
            _ => {}
        }
    }

    canon.emit_order = model_like.emit_order.iter().map(|nid| id_map[nid.index()]).collect();

    let mut contains_edges: Vec<(CanonId, CanonId, CanonEdgeKind)> = Vec::new();
    collect_contains_edges(&canon, &mut contains_edges);

    let mut name_edges = vec![];
    let mut type_edges = vec![];
    let mut call_edges = vec![];
    let mut module_edges = vec![];
    let mut cfg_edges = vec![];
    let mut region_edges = vec![];
    let mut value_edges = vec![];
    let mut macro_edges = vec![];
    let mut all_edges = vec![];

    for hint in &model_like.edge_hints {
        let src = id_map[hint.src as usize];
        let dst = id_map[hint.dst as usize];
        let k = map_edge_kind(&hint.kind);
        all_edges.push((src, dst, k.clone()));
        match &hint.kind {
            ModelEdgeKind::Renames | ModelEdgeKind::Resolves => name_edges.push((src, dst, k)),
            ModelEdgeKind::TypeOf | ModelEdgeKind::TypeUnifies | ModelEdgeKind::ImplTrait | ModelEdgeKind::DynTrait | ModelEdgeKind::ImplRef | ModelEdgeKind::Instantiates => {
                type_edges.push((src, dst, k))
            }
            ModelEdgeKind::Calls => call_edges.push((src, dst, k)),
            ModelEdgeKind::Contains | ModelEdgeKind::ImplFor | ModelEdgeKind::AssocItem => module_edges.push((src, dst, k)),
            ModelEdgeKind::CfgEdge | ModelEdgeKind::CfgBranch { .. } => cfg_edges.push((src, dst, k)),
            ModelEdgeKind::Outlives => region_edges.push((src, dst, k)),
            ModelEdgeKind::ConstDep => value_edges.push((src, dst, k)),
            ModelEdgeKind::Expands => macro_edges.push((src, dst, k)),
            ModelEdgeKind::Reexports => name_edges.push((src, dst, k)),
        }
    }
    for (src, dst, kind) in &vispath_edges {
        all_edges.push((*src, *dst, kind.clone()));
    }
    module_edges.extend(vispath_edges);
    for (src, dst, kind) in &contains_edges {
        all_edges.push((*src, *dst, kind.clone()));
    }
    module_edges.extend(contains_edges);

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
    let all_raw: Vec<(u32, u32, CanonEdgeKind)> = all_edges.into_iter().map(|(s, d, k)| (s.0, d.0, k)).collect();
    canon.rebuild_global_csr_from_edges(&all_raw);

    // Canonicalize Impl payload links from structural edges:
    // - Impl.for_ty from module_graph ImplFor
    // - Impl.for_trait from type_graph ImplRef
    let mut impl_payload_updates: Vec<(usize, Option<CanonId>, Option<CanonId>)> = Vec::new();
    for idx in 0..canon.nodes.len() {
        if !matches!(canon.nodes[idx].kind, CanonNodeKind::Impl { .. }) {
            continue;
        }
        let mut for_ty: Option<CanonId> = None;
        for (dst, edge) in canon.module_graph.neighbours(canon_ir::id::NodeId(idx as u32)) {
            if *edge == CanonEdgeKind::ImplFor {
                for_ty = Some(CanonId(dst.0));
                break;
            }
        }
        let mut for_trait: Option<CanonId> = None;
        for (dst, edge) in canon.type_graph.neighbours(canon_ir::id::NodeId(idx as u32)) {
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
        for (dst_id, edge) in canon.module_graph.neighbours(canon_ir::id::NodeId(src_idx as u32)) {
            if *edge != CanonEdgeKind::Contains {
                continue;
            }
            let Some(kind) = canon.nodes.get(dst_id.index()).map(|n| &n.kind) else {
                continue;
            };
            let Some(name_id) = (match kind {
                CanonNodeKind::Struct { name_id, .. } | CanonNodeKind::Enum { name_id, .. } | CanonNodeKind::Trait { name_id, .. } | CanonNodeKind::TypeAlias { name_id, .. } => Some(*name_id),
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

    Ok(canon)
}
