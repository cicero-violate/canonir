// CONTRACT:
// - No sorting
// - No graph traversal
// - No mutation
// - Pure string rendering of Plan

use crate::emit::impls::ImplEmitter;
use crate::emit::macros::MacroCallEmitter;
use crate::emit::types::{EnumEmitter, ExternCrateEmitter, StructEmitter, TypeAliasEmitter, TypeRefEmitter};
use crate::emit::{cargo::emit_cargo_toml, functions::FnEmitter, functions::TraitEmitter, helpers::Emit};
use crate::layout::{ImplPlan, ItemPlan, ModuleDeclPlan};
use model::ir::node::{NodeKind, Visibility};

pub fn dispatch_item(item: &ItemPlan, pad: &str) -> String {
    match item {
        ItemPlan::Module(decl) => ModuleEmitter { decl }.emit(pad),
        ItemPlan::Impl(imp) => ImplEmitter { imp }.emit(pad),
        ItemPlan::CargoToml { name, edition, has_binary } => emit_cargo_toml(name, edition, *has_binary),
        ItemPlan::Leaf(kind) => dispatch_kind(kind, pad),
    }
}

fn dispatch_kind(kind: &NodeKind, pad: &str) -> String {
    match kind {
        NodeKind::Struct { name, vis, generics, fields, derives, attrs, where_clauses, struct_kind } => {
            StructEmitter { name, vis, generics, fields, derives, attrs, where_clauses, struct_kind }.emit(pad)
        }
        NodeKind::Enum { name, vis, generics, variants, derives, attrs, where_clauses } => {
            EnumEmitter { name, vis, generics, variants, derives, attrs, where_clauses }.emit(pad)
        }
        NodeKind::Trait { name, vis, generics, methods, attrs, where_clauses, unsafe_ } => {
            TraitEmitter { name, vis, generics, methods, attrs, where_clauses, unsafe_: *unsafe_ }.emit(pad)
        }
        NodeKind::Impl { .. } => String::new(), // handled by ItemPlan::Impl
        NodeKind::Function { name, vis, generics, params, ret, body, attrs, where_clauses, unsafe_, async_ } => {
            FnEmitter { name, vis, generics, params, ret, body, attrs, where_clauses, unsafe_: *unsafe_, async_: *async_ }.emit(pad)
        }
        NodeKind::Method { name, vis, generics, params, ret, body, attrs, where_clauses, unsafe_, async_ } => {
            FnEmitter { name, vis, generics, params, ret, body, attrs, where_clauses, unsafe_: *unsafe_, async_: *async_ }.emit(pad)
        }
        NodeKind::Use { vis, path, alias, glob } => UseEmitter { vis, path, alias, glob: *glob }.emit(pad),
        NodeKind::TypeRef { name } => TypeRefEmitter { name }.emit(pad),
        NodeKind::Const { name, vis, ty, value, attrs } => ConstEmitter { name, vis, ty, value, attrs }.emit(pad),
        NodeKind::Static { name, vis, ty, value, mutable, attrs } => StaticEmitter { name, vis, ty, value, mutable: *mutable, attrs }.emit(pad),
        NodeKind::TypeAlias { name, vis, generics, ty, attrs, where_clauses } => {
            TypeAliasEmitter { name, vis, generics, ty, attrs, where_clauses }.emit(pad)
        }
        NodeKind::MacroCall { path, tokens } => MacroCallEmitter { path, tokens }.emit(pad),
        NodeKind::ExternCrate { name, alias, vis } => ExternCrateEmitter { name, alias, vis }.emit(pad),
        NodeKind::Crate { .. } | NodeKind::Module { .. } | NodeKind::Lifetime { .. } => String::new(),
    }
}

// ═══════════════════════════════════════════════════════════════════════════
// ModuleEmitter
// ═══════════════════════════════════════════════════════════════════════════

struct ModuleEmitter<'a> {
    decl: &'a ModuleDeclPlan,
}

impl ModuleEmitter<'_> {
    fn emit(&self, pad: &str) -> String {
        if self.decl.inline {
            let inner_pad = format!("{}    ", pad);
            let mut body = String::new();
            for item in &self.decl.items {
                body.push_str(&dispatch_item(item, &inner_pad));
            }
            format!("{}pub mod {} {{\n{}{}}}\n", pad, self.decl.name, body, pad)
        } else {
            format!("{}pub mod {};\n", pad, self.decl.name)
        }
    }
}

// ═══════════════════════════════════════════════════════════════════════════
// UseEmitter
// ═══════════════════════════════════════════════════════════════════════════

struct UseEmitter<'a> {
    vis: &'a Visibility,
    path: &'a str,
    alias: &'a Option<String>,
    glob: bool,
}

impl Emit for UseEmitter<'_> {
    fn emit(&self, pad: &str) -> String {
        let v = self.vis.to_token();
        if self.glob {
            format!("{}{}use {}::*;\n", pad, v, self.path)
        } else {
            match self.alias {
                Some(a) => format!("{}{}use {} as {};\n", pad, v, self.path, a),
                None => format!("{}{}use {};\n", pad, v, self.path),
            }
        }
    }
}

// ═══════════════════════════════════════════════════════════════════════════
// ConstEmitter
// ═══════════════════════════════════════════════════════════════════════════

struct ConstEmitter<'a> {
    name: &'a str,
    vis: &'a Visibility,
    ty: &'a str,
    value: &'a str,
    attrs: &'a [String],
}

impl Emit for ConstEmitter<'_> {
    fn emit(&self, pad: &str) -> String {
        let s = crate::emit::helpers::fmt_attrs(self.attrs, pad);
        format!("{}{}{}const {}: {} = {};\n", s, pad, self.vis.to_token(), self.name, self.ty, self.value,)
    }
}

// ═══════════════════════════════════════════════════════════════════════════
// StaticEmitter
// ═══════════════════════════════════════════════════════════════════════════

struct StaticEmitter<'a> {
    name: &'a str,
    vis: &'a Visibility,
    ty: &'a str,
    value: &'a str,
    mutable: bool,
    attrs: &'a [String],
}

impl Emit for StaticEmitter<'_> {
    fn emit(&self, pad: &str) -> String {
        let s = crate::emit::helpers::fmt_attrs(self.attrs, pad);
        let mut_kw = if self.mutable { "mut " } else { "" };
        format!("{}{}{}static {}{}: {} = {};\n", s, pad, self.vis.to_token(), mut_kw, self.name, self.ty, self.value,)
    }
}
