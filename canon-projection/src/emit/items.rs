use canon::ir::CanonIR;
use canon::node::{flags, CanonId, CanonNodeKind};

use crate::emit::cargo::emit_cargo_toml;
use crate::emit::fmt::{normalize_use_path, vis_token};
use crate::emit::functions::{emit_fn, emit_trait};
use crate::emit::impls::emit_impl;
use crate::emit::macros::emit_macro_call;
use crate::emit::types::{emit_enum, emit_struct, render_generic_decl, render_type_id};
use crate::layout::ItemPlan;

pub fn dispatch_item(ir: &CanonIR, item: &ItemPlan, pad: &str) -> String {
    match item {
        ItemPlan::CargoToml { name, edition, has_binary, dependencies } => emit_cargo_toml(name, edition, *has_binary, dependencies),
        ItemPlan::Node(id) => emit_node(ir, *id, pad),
    }
}

pub fn emit_node(ir: &CanonIR, id: CanonId, pad: &str) -> String {
    match &ir.node(id).kind {
        CanonNodeKind::Module { path_id, flags: f } => emit_module(ir, id, *path_id, *f, pad),
        CanonNodeKind::Use { path_id, alias, flags } => {
            let vis = vis_token(*flags);
            let glob = if (*flags & flags::GLOB) != 0 { "::*" } else { "" };
            let alias = alias.map(|a| format!(" as {}", ir.lookup_name(a))).unwrap_or_default();
            let raw_path = ir.lookup_path(*path_id);
            let path = normalize_use_path(raw_path, ir);
            if !path.contains("::") && path.chars().next().is_some_and(|c| c.is_ascii_uppercase()) {
                return String::new();
            }
            format!("{}{}use {}{}{};\n", pad, vis, path, glob, alias)
        }
        CanonNodeKind::ExternCrate { name_id, alias, flags } => {
            let vis = vis_token(*flags);
            let alias = alias.map(|a| format!(" as {}", ir.lookup_name(a))).unwrap_or_default();
            format!("{}{}extern crate {}{};\n", pad, vis, ir.lookup_name(*name_id), alias)
        }
        CanonNodeKind::Struct { name_id, generics, fields, derives, flags, struct_kind, .. } => emit_struct(ir, *name_id, generics, fields, derives, *flags, *struct_kind, pad),
        CanonNodeKind::Enum { name_id, generics, variants, derives, flags, .. } => emit_enum(ir, *name_id, generics, variants, derives, *flags, pad),
        CanonNodeKind::Trait { name_id, methods, flags, .. } => emit_trait(ir, *name_id, methods, *flags, pad),
        CanonNodeKind::Impl { for_ty, for_trait, flags, .. } => emit_impl(ir, id, *for_ty, *for_trait, *flags, pad),
        CanonNodeKind::Fn { name_id, sig_id, body, flags, .. } => emit_fn(ir, *name_id, *sig_id, *body, *flags, pad),
        CanonNodeKind::Const { name_id, ty, value_id, flags, .. } => {
            let vis = vis_token(*flags);
            format!("{}{}const {}: {} = {};\n", pad, vis, ir.lookup_name(*name_id), render_type_id(ir, *ty), ir.lookup_name(*value_id))
        }
        CanonNodeKind::Static { name_id, ty, value_id, flags, .. } => {
            let vis = vis_token(*flags);
            let mut_kw = if (*flags & flags::MUT) != 0 { "mut " } else { "" };
            format!("{}{}static {}{}: {} = {};\n", pad, vis, mut_kw, ir.lookup_name(*name_id), render_type_id(ir, *ty), ir.lookup_name(*value_id))
        }
        CanonNodeKind::TypeAlias { name_id, generics, ty, flags, .. } => {
            let vis = vis_token(*flags);
            let gs = render_generic_decl(ir, generics);
            format!("{}{}type {}{} = {};\n", pad, vis, ir.lookup_name(*name_id), gs, render_type_id(ir, *ty))
        }
        CanonNodeKind::MacroCall { path_id, tokens_id } => emit_macro_call(ir, *path_id, *tokens_id, pad),
        _ => String::new(),
    }
}

fn emit_module(ir: &CanonIR, module_id: CanonId, path_id: canon::node::PathId, f: u32, pad: &str) -> String {
    let name = ir.lookup_path(path_id).rsplit("::").next().unwrap_or(ir.lookup_path(path_id));
    let vis = if (f & (flags::PUB | flags::PUB_CRATE | flags::PUB_SUPER)) == 0 { "pub " } else { vis_token(f) };

    if (f & flags::INLINE) != 0 {
        let inner_pad = format!("{}    ", pad);
        let mut inner = String::new();
        for (dst, edge) in ir.module_graph.neighbours(canon::id::NodeId(module_id.0)) {
            if matches!(edge, canon::edge::EdgeKind::Contains) {
                inner.push_str(&emit_node(ir, CanonId(dst.0), &inner_pad));
            }
        }
        format!("{}{}mod {} {{\n{}{}}}\n", pad, vis, name, inner, pad)
    } else {
        format!("{}{}mod {};\n", pad, vis, name)
    }
}
