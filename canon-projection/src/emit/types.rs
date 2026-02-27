use canon::ir::CanonIR;
use canon::node::{CanonId, CanonNodeKind, NameId, PrimTy, TypeKind};

use crate::emit::fmt::vis_token;

pub fn emit_struct(ir: &CanonIR, name_id: NameId, generics: &[CanonId], fields: &[CanonId], derives: &[CanonId], flags: u32, struct_kind: u8, pad: &str) -> String {
    let vis = vis_token(flags);
    let name = ir.lookup_name(name_id);
    let gs = render_generic_decl(ir, generics);
    let derives_attr = render_derives_attr(ir, derives, pad);
    match struct_kind {
        2 => format!("{}{}{}struct {}{};\n", derives_attr, pad, vis, name, gs),
        1 => {
            let tys = fields
                .iter()
                .filter_map(|f| match &ir.node(*f).kind {
                    CanonNodeKind::Field { ty, .. } => Some(render_type_id(ir, *ty)),
                    _ => None,
                })
                .collect::<Vec<_>>()
                .join(", ");
            format!("{}{}{}struct {}{}({});\n", derives_attr, pad, vis, name, gs, tys)
        }
        _ => {
            let mut out = format!("{}{}{}struct {}{} {{\n", derives_attr, pad, vis, name, gs);
            for f in fields {
                if let CanonNodeKind::Field { name_id, ty, flags } = &ir.node(*f).kind {
                    let fname = name_id.map(|id| ir.lookup_name(id).to_string()).unwrap_or_else(|| "field".to_string());
                    out.push_str(&format!("{}    {}{}: {},\n", pad, vis_token(*flags), fname, render_type_id(ir, *ty)));
                }
            }
            out.push_str(&format!("{}}}\n", pad));
            out
        }
    }
}

pub fn emit_enum(ir: &CanonIR, name_id: NameId, generics: &[CanonId], variants: &[CanonId], derives: &[CanonId], flags: u32, pad: &str) -> String {
    let vis = vis_token(flags);
    let gs = render_generic_decl(ir, generics);
    let derives_attr = render_derives_attr(ir, derives, pad);
    let mut out = format!("{}{}{}enum {}{} {{\n", derives_attr, pad, vis, ir.lookup_name(name_id), gs);
    for v in variants {
        if let CanonNodeKind::Variant { name_id, fields } = &ir.node(*v).kind {
            if fields.is_empty() {
                out.push_str(&format!("{}    {},\n", pad, ir.lookup_name(*name_id)));
            } else {
                let has_named = fields.iter().any(|f| matches!(&ir.node(*f).kind, CanonNodeKind::Field { name_id: Some(_), .. }));
                if has_named {
                    let members = fields
                        .iter()
                        .filter_map(|f| match &ir.node(*f).kind {
                            CanonNodeKind::Field { name_id: Some(nid), ty, .. } => Some(format!("{}: {}", ir.lookup_name(*nid), render_type_id(ir, *ty))),
                            _ => None,
                        })
                        .collect::<Vec<_>>()
                        .join(", ");
                    out.push_str(&format!("{}    {} {{ {} }},\n", pad, ir.lookup_name(*name_id), members));
                } else {
                    let tys = fields
                        .iter()
                        .filter_map(|f| match &ir.node(*f).kind {
                            CanonNodeKind::Field { ty, .. } => Some(render_type_id(ir, *ty)),
                            _ => None,
                        })
                        .collect::<Vec<_>>()
                        .join(", ");
                    out.push_str(&format!("{}    {}({}),\n", pad, ir.lookup_name(*name_id), tys));
                }
            }
        }
    }
    out.push_str(&format!("{}}}\n", pad));
    out
}

pub fn render_generic_decl(ir: &CanonIR, generics: &[CanonId]) -> String {
    if generics.is_empty() {
        return String::new();
    }
    let items: Vec<String> = generics
        .iter()
        .filter_map(|g| match &ir.node(*g).kind {
            CanonNodeKind::GenericParam { name_id, bounds, is_lifetime, .. } => {
                let mut n = ir.lookup_name(*name_id).to_string();
                if *is_lifetime && !n.starts_with('\'') {
                    n = format!("'{}", n);
                }
                if bounds.is_empty() {
                    Some(n)
                } else {
                    let bs = bounds
                        .iter()
                        .filter_map(|b| match &ir.node(*b).kind {
                            CanonNodeKind::TypeRef { name_id } => Some(ir.lookup_name(*name_id).to_string()),
                            CanonNodeKind::Trait { name_id, .. } => Some(ir.lookup_name(*name_id).to_string()),
                            _ => None,
                        })
                        .collect::<Vec<_>>()
                        .join(" + ");
                    if bs.is_empty() {
                        Some(n)
                    } else {
                        Some(format!("{}: {}", n, bs))
                    }
                }
            }
            _ => None,
        })
        .collect();
    if items.is_empty() {
        String::new()
    } else {
        format!("<{}>", items.join(", "))
    }
}

fn render_derives_attr(ir: &CanonIR, derives: &[CanonId], pad: &str) -> String {
    let names: Vec<String> = derives
        .iter()
        .filter_map(|d| match &ir.node(*d).kind {
            CanonNodeKind::TypeRef { name_id } => Some(ir.lookup_name(*name_id).to_string()),
            CanonNodeKind::Trait { name_id, .. } => Some(ir.lookup_name(*name_id).to_string()),
            _ => None,
        })
        .collect();
    if names.is_empty() {
        String::new()
    } else {
        format!("{}#[derive({})]\n", pad, names.join(", "))
    }
}

pub fn render_type_id(ir: &CanonIR, id: CanonId) -> String {
    match &ir.node(id).kind {
        CanonNodeKind::Type { kind } => render_type_kind(ir, kind),
        CanonNodeKind::TypeRef { name_id } => ir.lookup_name(*name_id).to_string(),
        CanonNodeKind::Struct { name_id, .. }
        | CanonNodeKind::Enum { name_id, .. }
        | CanonNodeKind::Trait { name_id, .. }
        | CanonNodeKind::TypeAlias { name_id, .. }
        | CanonNodeKind::Fn { name_id, .. } => ir.lookup_name(*name_id).to_string(),
        _ => "()".into(),
    }
}

fn render_type_kind(ir: &CanonIR, kind: &TypeKind) -> String {
    match kind {
        TypeKind::Primitive(p) => render_prim(p),
        TypeKind::Adt(id) => render_type_id(ir, *id),
        TypeKind::Ref { lifetime, inner, mutable } => {
            let lt = lifetime
                .and_then(|id| match &ir.node(id).kind {
                    CanonNodeKind::Lifetime { name_id } => Some(format!("{} ", ir.lookup_name(*name_id))),
                    _ => None,
                })
                .unwrap_or_default();
            let m = if *mutable { "mut " } else { "" };
            format!("&{}{}{}", lt, m, render_type_id(ir, *inner))
        }
        TypeKind::RawPtr { inner, mutable } => {
            let m = if *mutable { "mut" } else { "const" };
            format!("*{} {}", m, render_type_id(ir, *inner))
        }
        TypeKind::Array { inner, len } => format!("[{}; {}]", render_type_id(ir, *inner), len),
        TypeKind::Slice(inner) => format!("[{}]", render_type_id(ir, *inner)),
        TypeKind::Tuple(items) => format!("({})", items.iter().map(|id| render_type_id(ir, *id)).collect::<Vec<_>>().join(", ")),
        TypeKind::FnPtr(sig_id) => match &ir.node(*sig_id).kind {
            CanonNodeKind::FnSig { params, ret, .. } => {
                let params = params
                    .iter()
                    .filter_map(|p| match &ir.node(*p).kind {
                        CanonNodeKind::Param { ty, .. } => Some(render_type_id(ir, *ty)),
                        _ => None,
                    })
                    .collect::<Vec<_>>()
                    .join(", ");
                format!("fn({}) -> {}", params, render_type_id(ir, *ret))
            }
            _ => "fn()".into(),
        },
        TypeKind::ImplTrait(id) => format!("impl {}", render_type_id(ir, *id)),
        TypeKind::DynTrait(id) => format!("dyn {}", render_type_id(ir, *id)),
        TypeKind::Param(name_id) => ir.lookup_name(*name_id).to_string(),
        TypeKind::Extern(path_id) => ir.lookup_path(*path_id).to_string(),
        TypeKind::Unresolved(path_id) => panic!("unresolved type reached projection: {}", ir.lookup_path(*path_id)),
        TypeKind::TypeRef { name_id } => ir.lookup_name(*name_id).to_string(),
    }
}

fn render_prim(p: &PrimTy) -> String {
    match p {
        PrimTy::Unit => "()".into(),
        PrimTy::Never => "!".into(),
        PrimTy::Bool => "bool".into(),
        PrimTy::Char => "char".into(),
        PrimTy::Str => "str".into(),
        PrimTy::U8 => "u8".into(),
        PrimTy::U16 => "u16".into(),
        PrimTy::U32 => "u32".into(),
        PrimTy::U64 => "u64".into(),
        PrimTy::U128 => "u128".into(),
        PrimTy::Usize => "usize".into(),
        PrimTy::I8 => "i8".into(),
        PrimTy::I16 => "i16".into(),
        PrimTy::I32 => "i32".into(),
        PrimTy::I64 => "i64".into(),
        PrimTy::I128 => "i128".into(),
        PrimTy::Isize => "isize".into(),
        PrimTy::F32 => "f32".into(),
        PrimTy::F64 => "f64".into(),
    }
}
