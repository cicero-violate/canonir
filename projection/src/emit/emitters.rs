use crate::emit::body::{emit_blocks, indent_raw};
use crate::emit::cargo::emit_cargo_toml;
use crate::emit::fmt::{fmt_field, fmt_generics, fmt_params, fmt_trait_method};
use crate::layout::{FilePlan, ImplPlan, ItemPlan, ModuleDeclPlan, Plan};
use model::ir::node::{Body, EnumVariant, Field, GenericParam, NodeKind, Param, StructKind, TraitMethod, Visibility};
use std::path::PathBuf;

/// Emit the full plan into `(path, source)` pairs.
pub fn emit_plan(plan: &Plan) -> Vec<(PathBuf, String)> {
    plan.files.iter().map(|f| (f.path.clone(), emit_file(f))).collect()
}

fn emit_file(file: &FilePlan) -> String {
    let mut out = String::new();
    for item in &file.items {
        out.push_str(&dispatch_item(item, ""));
        if !out.ends_with('\n') {
            out.push('\n');
        }
        // Separate top-level items with a newline for readability.
        out.push('\n');
    }
    // Trim trailing whitespace newline added after the last item.
    while out.ends_with('\n') {
        out.pop();
        if !out.ends_with('\n') {
            break;
        }
    }
    out
}

fn dispatch_item(item: &ItemPlan, pad: &str) -> String {
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
        NodeKind::Enum { name, vis, generics, variants, derives, attrs, where_clauses } => EnumEmitter { name, vis, generics, variants, derives, attrs, where_clauses }.emit(pad),
        NodeKind::Trait { name, vis, generics, methods, attrs, where_clauses, unsafe_ } => TraitEmitter { name, vis, generics, methods, attrs, where_clauses, unsafe_: *unsafe_ }.emit(pad),
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
        NodeKind::TypeAlias { name, vis, generics, ty, attrs, where_clauses } => TypeAliasEmitter { name, vis, generics, ty, attrs, where_clauses }.emit(pad),
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
// StructEmitter
// ═══════════════════════════════════════════════════════════════════════════

struct StructEmitter<'a> {
    name: &'a str,
    vis: &'a Visibility,
    generics: &'a [GenericParam],
    fields: &'a [Field],
    derives: &'a [String],
    attrs: &'a [String],
    where_clauses: &'a [String],
    struct_kind: &'a StructKind,
}

impl Emit for StructEmitter<'_> {
    fn emit(&self, pad: &str) -> String {
        let mut s = fmt_attrs(self.attrs, pad);
        if !self.derives.is_empty() {
            s.push_str(&format!("{}#[derive({})]\n", pad, self.derives.join(", ")));
        }
        let wc = fmt_where(self.where_clauses);
        match self.struct_kind {
            StructKind::Unit => {
                s.push_str(&format!("{}{}struct {}{};\n", pad, self.vis.to_token(), self.name, wc,));
            }
            StructKind::Tuple => {
                let tys: Vec<String> = self.fields.iter().map(|f| format!("{}{}", f.vis.to_token(), f.ty)).collect();
                s.push_str(&format!("{}{}struct {}{}({}){};\n", pad, self.vis.to_token(), self.name, fmt_generics(self.generics), tys.join(", "), wc,));
            }
            StructKind::Named => {
                s.push_str(&format!("{}{}struct {}{}{} {{\n", pad, self.vis.to_token(), self.name, fmt_generics(self.generics), wc,));
                for f in self.fields {
                    s.push_str(&fmt_field(f, &format!("{}    ", pad)));
                }
                s.push_str(&format!("{}}}\n", pad));
            }
        }
        s
    }
}

// ═══════════════════════════════════════════════════════════════════════════
// TraitEmitter
// ═══════════════════════════════════════════════════════════════════════════

struct TraitEmitter<'a> {
    name: &'a str,
    vis: &'a Visibility,
    generics: &'a [GenericParam],
    methods: &'a [TraitMethod],
    attrs: &'a [String],
    where_clauses: &'a [String],
    unsafe_: bool,
}

impl Emit for TraitEmitter<'_> {
    fn emit(&self, pad: &str) -> String {
        let inner = format!("{}    ", pad);
        let mut s = fmt_attrs(self.attrs, pad);
        let unsafe_kw = if self.unsafe_ { "unsafe " } else { "" };
        let wc = fmt_where(self.where_clauses);
        s.push_str(&format!("{}{}{}trait {}{}{} {{\n", pad, self.vis.to_token(), unsafe_kw, self.name, fmt_generics(self.generics), wc,));
        for m in self.methods {
            s.push_str(&fmt_trait_method(m, &inner));
        }
        s.push_str(&format!("{}}}\n", pad));
        s
    }
}

// ═══════════════════════════════════════════════════════════════════════════
// ImplEmitter
// ═══════════════════════════════════════════════════════════════════════════

struct ImplEmitter<'a> {
    imp: &'a ImplPlan,
}

impl ImplEmitter<'_> {
    fn emit(&self, pad: &str) -> String {
        let mut s = fmt_attrs(&self.imp.attrs, pad);
        let unsafe_kw = if self.imp.unsafe_ { "unsafe " } else { "" };
        let wc = fmt_where(&self.imp.where_clauses);
        let header = match &self.imp.for_trait {
            Some(tr) => format!("{}{}impl{} {} for {}{} {{\n", pad, unsafe_kw, fmt_generics(&self.imp.generics), tr, self.imp.for_struct, wc,),
            None => format!("{}{}impl{} {}{} {{\n", pad, unsafe_kw, fmt_generics(&self.imp.generics), self.imp.for_struct, wc,),
        };
        s.push_str(&header);
        let inner = format!("{}    ", pad);
        for m in &self.imp.methods {
            s.push_str(&dispatch_kind(m, &inner));
        }
        s.push_str(&format!("{}}}\n", pad));
        s
    }
}

// ═══════════════════════════════════════════════════════════════════════════
// FnEmitter  (Function + Method share the same layout)
// ═══════════════════════════════════════════════════════════════════════════

struct FnEmitter<'a> {
    name: &'a str,
    vis: &'a Visibility,
    generics: &'a [GenericParam],
    params: &'a [Param],
    ret: &'a str,
    body: &'a Body,
    attrs: &'a [String],
    where_clauses: &'a [String],
    unsafe_: bool,
    async_: bool,
}

impl Emit for FnEmitter<'_> {
    fn emit(&self, pad: &str) -> String {
        let ret_part = if self.ret == "()" { String::new() } else { format!(" -> {}", self.ret) };
        let unsafe_kw = if self.unsafe_ { "unsafe " } else { "" };
        let async_kw = if self.async_ { "async " } else { "" };
        let wc = fmt_where(self.where_clauses);
        let mut s = fmt_attrs(self.attrs, pad);
        let sig = format!("{}{}{}{}fn {}{}{}{}{}", pad, self.vis.to_token(), async_kw, unsafe_kw, self.name, fmt_generics(self.generics), fmt_params(self.params), ret_part, wc,);
        let inner = format!("{}    ", pad);
        s.push_str(&match self.body {
            Body::None => format!("{};\n", sig),
            Body::Blocks(bb) => format!("{} {{\n{}{}}}\n", sig, emit_blocks(bb, &inner), pad),
            Body::Raw(src) => format!("{} {{\n{}{}}}\n", sig, indent_raw(src, &inner), pad),
        });
        s
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
// ExternCrateEmitter
// ═══════════════════════════════════════════════════════════════════════════

struct ExternCrateEmitter<'a> {
    name: &'a str,
    alias: &'a Option<String>,
    vis: &'a Visibility,
}

impl Emit for ExternCrateEmitter<'_> {
    fn emit(&self, pad: &str) -> String {
        let v = self.vis.to_token();
        match self.alias {
            Some(a) => format!("{}{}extern crate {} as {};\n", pad, v, self.name, a),
            None => format!("{}{}extern crate {};\n", pad, v, self.name),
        }
    }
}

// ═══════════════════════════════════════════════════════════════════════════
// TypeRefEmitter
// ═══════════════════════════════════════════════════════════════════════════

struct TypeRefEmitter<'a> {
    name: &'a str,
}

impl Emit for TypeRefEmitter<'_> {
    fn emit(&self, pad: &str) -> String {
        format!("{}// type alias: {}\n", pad, self.name)
    }
}

// ═══════════════════════════════════════════════════════════════════════════
// EnumEmitter
// ═══════════════════════════════════════════════════════════════════════════

struct EnumEmitter<'a> {
    name: &'a str,
    vis: &'a Visibility,
    generics: &'a [GenericParam],
    variants: &'a [EnumVariant],
    derives: &'a [String],
    attrs: &'a [String],
    where_clauses: &'a [String],
}

impl Emit for EnumEmitter<'_> {
    fn emit(&self, pad: &str) -> String {
        let mut s = fmt_attrs(self.attrs, pad);
        if !self.derives.is_empty() {
            s.push_str(&format!("{}#[derive({})]\n", pad, self.derives.join(", ")));
        }
        let wc = fmt_where(self.where_clauses);
        s.push_str(&format!("{}{}enum {}{}{} {{\n", pad, self.vis.to_token(), self.name, fmt_generics(self.generics), wc,));
        let inner = format!("{}    ", pad);
        for v in self.variants {
            if v.fields.is_empty() {
                s.push_str(&format!("{}{},\n", inner, v.name));
            } else if v.fields.iter().all(|f| f.name.is_none()) {
                let tys: Vec<&str> = v.fields.iter().map(|f| f.ty.as_str()).collect();
                s.push_str(&format!("{}{}({}),\n", inner, v.name, tys.join(", ")));
            } else {
                s.push_str(&format!("{}{} {{\n", inner, v.name));
                let inner2 = format!("{}    ", inner);
                for f in &v.fields {
                    s.push_str(&fmt_field(f, &inner2));
                }
                s.push_str(&format!("{}}},\n", inner));
            }
        }
        s.push_str(&format!("{}}}\n", pad));
        s
    }
}

// ═══════════════════════════════════════════════════════════════════════════
// Trait Method helper uses fmt_trait_method in fmt.rs
// ═══════════════════════════════════════════════════════════════════════════

// ═══════════════════════════════════════════════════════════════════════════
// TypeAliasEmitter
// ═══════════════════════════════════════════════════════════════════════════

struct TypeAliasEmitter<'a> {
    name: &'a str,
    vis: &'a Visibility,
    generics: &'a [GenericParam],
    ty: &'a str,
    attrs: &'a [String],
    where_clauses: &'a [String],
}

impl Emit for TypeAliasEmitter<'_> {
    fn emit(&self, pad: &str) -> String {
        let s = fmt_attrs(self.attrs, pad);
        let wc = fmt_where(self.where_clauses);
        format!("{}{}{}type {}{} = {}{};\n", s, pad, self.vis.to_token(), self.name, fmt_generics(self.generics), self.ty, wc,)
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
        let s = fmt_attrs(self.attrs, pad);
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
        let s = fmt_attrs(self.attrs, pad);
        let mut_kw = if self.mutable { "mut " } else { "" };
        format!("{}{}{}static {}{}: {} = {};\n", s, pad, self.vis.to_token(), mut_kw, self.name, self.ty, self.value,)
    }
}

// ═══════════════════════════════════════════════════════════════════════════
// MacroCallEmitter
// ═══════════════════════════════════════════════════════════════════════════

struct MacroCallEmitter<'a> {
    path: &'a str,
    tokens: &'a str,
}

impl Emit for MacroCallEmitter<'_> {
    fn emit(&self, pad: &str) -> String {
        let helper = format!("__macro_call_{}", self.path.replace("::", "_"));
        format!("{pad}#[allow(dead_code)]\n{pad}fn {helper}() {{\n{pad}    {path}!({tokens});\n{pad}}}\n", pad = pad, helper = helper, path = self.path, tokens = self.tokens)
    }
}

// ═══════════════════════════════════════════════════════════════════════════
// Shared helpers
// ═══════════════════════════════════════════════════════════════════════════

trait Emit {
    fn emit(&self, pad: &str) -> String;
}

/// Emit `#[attr]` lines for an attrs list.
fn fmt_attrs(attrs: &[String], pad: &str) -> String {
    attrs.iter().map(|a| format!("{}#[{}]\n", pad, a)).collect()
}

/// Emit a `where` clause block if non-empty.
fn fmt_where(wc: &[String]) -> String {
    if wc.is_empty() {
        String::new()
    } else {
        format!("\nwhere\n    {}", wc.join(",\n    "))
    }
}
