use model::ir::{
    edge::EdgeKind,
    model_ir::ModelIR,
    node::{
        Body, EnumVariant, Field, GenericParam, NodeId, NodeKind, Param, TraitMethod, Visibility,
    },
};

use crate::emit::body::{emit_blocks, indent_raw};
use crate::emit::fmt::{fmt_field, fmt_generics, fmt_params, fmt_trait_method};

// ═══════════════════════════════════════════════════════════════════════════
// Core trait
// ═══════════════════════════════════════════════════════════════════════════

/// Every NodeKind has a corresponding emitter that implements this trait.
///
/// Equation:
///   Emit::emit(self, ir, pad) -> String
trait Emit {
    fn emit(&self, ir: &ModelIR, pad: &str) -> String;
}

// ═══════════════════════════════════════════════════════════════════════════
// Public entry
// ═══════════════════════════════════════════════════════════════════════════

/// Emit all Module nodes as (file_path, source) pairs.
pub fn emit_files(ir: &ModelIR) -> Vec<(String, String)> {
    ir.nodes
        .iter()
        .filter_map(|n| {
            if let NodeKind::Module { file, .. } = &n.kind {
                Some((file.clone(), ModuleEmitter(n.id).emit(ir, "")))
            } else {
                None
            }
        })
        .collect()
}

/// Emit a single node by id.
pub fn emit_node(ir: &ModelIR, id: NodeId) -> String {
    dispatch(ir, id, "")
}

// ═══════════════════════════════════════════════════════════════════════════
// Dispatch
// ═══════════════════════════════════════════════════════════════════════════

fn dispatch(ir: &ModelIR, id: NodeId, pad: &str) -> String {
    let node = ir.node(id);
    match &node.kind {
        NodeKind::Module { .. } => ModuleEmitter(id).emit(ir, pad),
        NodeKind::Struct { name, vis, generics, fields, derives, attrs, where_clauses } => {
            StructEmitter { name, vis, generics, fields, derives, attrs, where_clauses }.emit(ir, pad)
        }
        NodeKind::Enum { name, vis, generics, variants, derives, attrs, where_clauses } => {
            EnumEmitter { name, vis, generics, variants, derives, attrs, where_clauses }.emit(ir, pad)
        }
        NodeKind::Trait { name, vis, generics, methods, attrs, where_clauses, unsafe_ } => {
            TraitEmitter { name, vis, generics, methods, attrs, where_clauses, unsafe_: *unsafe_ }.emit(ir, pad)
        }
        NodeKind::Impl { for_struct, for_trait, generics, attrs, where_clauses, unsafe_ } => {
            ImplEmitter { id, for_struct, for_trait, generics, attrs, where_clauses, unsafe_: *unsafe_ }.emit(ir, pad)
        }
        NodeKind::Function { name, vis, generics, params, ret, body, attrs, where_clauses, unsafe_, async_ } => {
            FnEmitter { name, vis, generics, params, ret, body, attrs, where_clauses, unsafe_: *unsafe_, async_: *async_ }.emit(ir, pad)
        }
        NodeKind::Method { name, vis, generics, params, ret, body, attrs, where_clauses, unsafe_, async_ } => {
            FnEmitter { name, vis, generics, params, ret, body, attrs, where_clauses, unsafe_: *unsafe_, async_: *async_ }.emit(ir, pad)
        }
        NodeKind::Use { path, alias } => {
            UseEmitter { path, alias }.emit(ir, pad)
        }
        NodeKind::TypeRef { name } => TypeRefEmitter { name }.emit(ir, pad),
        NodeKind::Const { name, vis, ty, value, attrs } => {
            ConstEmitter { name, vis, ty, value, attrs }.emit(ir, pad)
        }
        NodeKind::Static { name, vis, ty, value, mutable, attrs } => {
            StaticEmitter { name, vis, ty, value, mutable: *mutable, attrs }.emit(ir, pad)
        }
        NodeKind::TypeAlias { name, vis, generics, ty, attrs, where_clauses } => {
            TypeAliasEmitter { name, vis, generics, ty, attrs, where_clauses }.emit(ir, pad)
        }
        NodeKind::MacroCall { path, tokens } => {
            MacroCallEmitter { path, tokens }.emit(ir, pad)
        }
        NodeKind::Crate { .. } => String::new(),
    }
}

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
    /// Equation:
    ///   emit(TypeAlias) = vis "type" name generics "=" ty ";"
    fn emit(&self, _ir: &ModelIR, pad: &str) -> String {
        let s = fmt_attrs(self.attrs, pad);
        let wc = fmt_where(self.where_clauses);
        format!(
            "{}{}{}type {}{} = {}{};\n",
            s,
            pad,
            self.vis.to_token(),
            self.name,
            fmt_generics(self.generics),
            self.ty,
            wc,
        )
    }
}

// ═══════════════════════════════════════════════════════════════════════════
// ModuleEmitter
// ═══════════════════════════════════════════════════════════════════════════

struct ModuleEmitter(NodeId);

impl Emit for ModuleEmitter {
    /// Equation:
    ///   emit(Module m) =
    ///     for (child, Contains) in module_graph.neighbours(m):
    ///       if child is Use     -> emit(Use)          // use declarations first
    ///       if child is Module  -> "pub mod <name>;"
    ///       else                -> emit(child)
    fn emit(&self, ir: &ModelIR, pad: &str) -> String {
        let mut out = String::new();
        let id = self.0;
        if id.index() >= ir.module_graph.vertex_count() {
            return out;
        }

        // Collect children, partitioned: Use nodes first, then the rest.
        let mut use_nodes: Vec<NodeId> = Vec::new();
        let mut other_nodes: Vec<NodeId> = Vec::new();

        for (child_id, edge) in ir.module_graph.neighbours(id) {
            if *edge != EdgeKind::Contains {
                continue;
            }
            let child = ir.node(child_id);
            if matches!(&child.kind, NodeKind::Use { .. }) {
                use_nodes.push(child_id);
            } else {
                other_nodes.push(child_id);
            }
        }

        // Emit use declarations at the top of the file.
        for uid in &use_nodes {
            out.push_str(&dispatch(ir, *uid, pad));
        }
        if !use_nodes.is_empty() {
            out.push('\n');
        }

        // Emit all other children.
        for child_id in &other_nodes {
            let child = ir.node(*child_id);
            match &child.kind {
                NodeKind::Module { path, .. } => {
                    let name = path.rsplit("::").next().unwrap_or(path.as_str());
                    out.push_str(&format!("{}pub mod {};\n", pad, name));
                }
                _ => {
                    let src = dispatch(ir, *child_id, pad);
                    out.push_str(&src);
                    out.push('\n');
                }
            }
        }
        out
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
}

impl Emit for StructEmitter<'_> {
    /// Equation:
    ///   emit(Struct) = [#[derive(...)]] vis "struct" name generics "{" fields "}"
    fn emit(&self, _ir: &ModelIR, pad: &str) -> String {
        let mut s = fmt_attrs(self.attrs, pad);
        if !self.derives.is_empty() {
            s.push_str(&format!("{}#[derive({})]\n", pad, self.derives.join(", ")));
        }
        let wc = fmt_where(self.where_clauses);
        s.push_str(&format!(
            "{}{}struct {}{}{} {{\n",
            pad,
            self.vis.to_token(),
            self.name,
            fmt_generics(self.generics),
            wc,
        ));
        for f in self.fields {
            s.push_str(&fmt_field(f, &format!("{}    ", pad)));
        }
        s.push_str(&format!("{}}}\n", pad));
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
    /// Equation:
    ///   emit(Trait) = vis "trait" name generics "{" methods "}"
    fn emit(&self, ir: &ModelIR, pad: &str) -> String {
        let inner = format!("{}    ", pad);
        let mut s = fmt_attrs(self.attrs, pad);
        let unsafe_kw = if self.unsafe_ { "unsafe " } else { "" };
        let wc = fmt_where(self.where_clauses);
        s.push_str(&format!(
            "{}{}{}trait {}{}{} {{\n",
            pad,
            self.vis.to_token(),
            unsafe_kw,
            self.name,
            fmt_generics(self.generics),
            wc,
        ));
        for m in self.methods {
            s.push_str(&fmt_trait_method(m, ir, &inner));
        }
        s.push_str(&format!("{}}}\n", pad));
        s
    }
}

// ═══════════════════════════════════════════════════════════════════════════
// ImplEmitter
// ═══════════════════════════════════════════════════════════════════════════

struct ImplEmitter<'a> {
    id: NodeId,
    for_struct: &'a str,
    for_trait: &'a Option<String>,
    generics: &'a [GenericParam],
    attrs: &'a [String],
    where_clauses: &'a [String],
    unsafe_: bool,
}

impl Emit for ImplEmitter<'_> {
    /// Equation:
    ///   emit(Impl) = "impl" generics [trait "for"] struct "{" methods "}"
    ///   methods    = children via module_graph Contains edges
    fn emit(&self, ir: &ModelIR, pad: &str) -> String {
        let mut s = fmt_attrs(self.attrs, pad);
        let unsafe_kw = if self.unsafe_ { "unsafe " } else { "" };
        let wc = fmt_where(self.where_clauses);
        let header = match self.for_trait {
            Some(tr) => format!(
                "{}{}impl{} {} for {}{} {{\n",
                pad,
                unsafe_kw,
                fmt_generics(self.generics),
                tr,
                self.for_struct,
                wc,
            ),
            None => format!(
                "{}{}impl{} {}{} {{\n",
                pad,
                unsafe_kw,
                fmt_generics(self.generics),
                self.for_struct,
                wc,
            ),
        };
        s.push_str(&header);
        let inner = format!("{}    ", pad);
        if self.id.index() < ir.module_graph.vertex_count() {
            for (child_id, edge) in ir.module_graph.neighbours(self.id) {
                if *edge != EdgeKind::Contains {
                    continue;
                }
                // Trait impl methods must not carry a visibility qualifier.
                // Equation: vis(method in trait impl) = ε
                let src = if self.for_trait.is_some() {
                    match &ir.node(child_id).kind {
                        NodeKind::Method { name, generics, params, ret, body, attrs, where_clauses, unsafe_, async_, .. } => {
                            FnEmitter {
                                name,
                                vis: &Visibility::Private,
                                generics,
                                params,
                                ret,
                                body,
                                attrs,
                                where_clauses,
                                unsafe_: *unsafe_,
                                async_: *async_,
                            }
                            .emit(ir, &inner)
                        }
                        _ => dispatch(ir, child_id, &inner),
                    }
                } else {
                    dispatch(ir, child_id, &inner)
                };
                s.push_str(&src);
            }
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
    /// Equation:
    ///   emit(Fn) = vis "fn" name generics "(" params ")" ["->" ret] "{" body "}"
    fn emit(&self, _ir: &ModelIR, pad: &str) -> String {
        let ret_part = if self.ret == "()" {
            String::new()
        } else {
            format!(" -> {}", self.ret)
        };
        let unsafe_kw = if self.unsafe_ { "unsafe " } else { "" };
        let async_kw  = if self.async_  { "async "  } else { "" };
        let wc = fmt_where(self.where_clauses);
        let mut s = fmt_attrs(self.attrs, pad);
        let sig = format!(
            "{}{}{}{}fn {}{}{}{}{}",
            pad,
            self.vis.to_token(),
            async_kw,
            unsafe_kw,
            self.name,
            fmt_generics(self.generics),
            fmt_params(self.params),
            ret_part,
            wc,
        );
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
    path: &'a str,
    alias: &'a Option<String>,
}

impl Emit for UseEmitter<'_> {
    /// Equation:
    ///   emit(Use) = "use" path ["as" alias] ";"
    fn emit(&self, _ir: &ModelIR, pad: &str) -> String {
        match self.alias {
            Some(a) => format!("{}use {} as {};\n", pad, self.path, a),
            None    => format!("{}use {};\n", pad, self.path),
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
    /// TypeRef with no expansion emits a placeholder comment.
    fn emit(&self, _ir: &ModelIR, pad: &str) -> String {
        format!("{}// type alias: {}\n", pad, self.name)
    }
}

// ═══════════════════════════════════════════════════════════════════════════
// EnumEmitter  (E6)
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
    /// Equation:
    ///   emit(Enum) = attrs [#[derive(...)]] vis "enum" name generics wc "{" variants "}"
    fn emit(&self, _ir: &ModelIR, pad: &str) -> String {
        let mut s = fmt_attrs(self.attrs, pad);
        if !self.derives.is_empty() {
            s.push_str(&format!("{}#[derive({})]\n", pad, self.derives.join(", ")));
        }
        let wc = fmt_where(self.where_clauses);
        s.push_str(&format!(
            "{}{}enum {}{}{} {{\n",
            pad,
            self.vis.to_token(),
            self.name,
            fmt_generics(self.generics),
            wc,
        ));
        let inner = format!("{}    ", pad);
        for v in self.variants {
            if v.fields.is_empty() {
                // Unit variant: Foo,
                s.push_str(&format!("{}{},\n", inner, v.name));
            } else if v.fields.iter().all(|f| f.name.is_none()) {
                // Tuple variant: Foo(T, U),
                let tys: Vec<&str> = v.fields.iter().map(|f| f.ty.as_str()).collect();
                s.push_str(&format!("{}{}({}),\n", inner, v.name, tys.join(", ")));
            } else {
                // Named-field variant: Foo { x: T, y: U },
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
// ConstEmitter  (E5)
// ═══════════════════════════════════════════════════════════════════════════

struct ConstEmitter<'a> {
    name: &'a str,
    vis: &'a Visibility,
    ty: &'a str,
    value: &'a str,
    attrs: &'a [String],
}

impl Emit for ConstEmitter<'_> {
    /// Equation:
    ///   emit(Const) = attrs vis "const" name ":" ty "=" value ";"
    fn emit(&self, _ir: &ModelIR, pad: &str) -> String {
        let mut s = fmt_attrs(self.attrs, pad);
        s.push_str(&format!(
            "{}{}const {}: {} = {};\n",
            pad, self.vis.to_token(), self.name, self.ty, self.value,
        ));
        s
    }
}

// ═══════════════════════════════════════════════════════════════════════════
// StaticEmitter  (E5)
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
    /// Equation:
    ///   emit(Static) = attrs vis ["mut"] "static" name ":" ty "=" value ";"
    fn emit(&self, _ir: &ModelIR, pad: &str) -> String {
        let mut s = fmt_attrs(self.attrs, pad);
        let mut_kw = if self.mutable { "mut " } else { "" };
        s.push_str(&format!(
            "{}{}static {}{}: {} = {};\n",
            pad, self.vis.to_token(), mut_kw, self.name, self.ty, self.value,
        ));
        s
    }
}

// ═══════════════════════════════════════════════════════════════════════════
// MacroCallEmitter  (E14)
// ═══════════════════════════════════════════════════════════════════════════

struct MacroCallEmitter<'a> {
    path: &'a str,
    tokens: &'a str,
}

impl Emit for MacroCallEmitter<'_> {
    /// Equation:
    ///   emit(MacroCall) = path "!(" tokens ");"
    fn emit(&self, _ir: &ModelIR, pad: &str) -> String {
        format!("{}{}!({});\n", pad, self.path, self.tokens)
    }
}

// ═══════════════════════════════════════════════════════════════════════════
// Shared helpers
// ═══════════════════════════════════════════════════════════════════════════

/// Emit `#[attr]` lines for an attrs list.
/// Equation: fmt_attrs(A, pad) = ∑_{a ∈ A} pad "#[" a "]\n"
fn fmt_attrs(attrs: &[String], pad: &str) -> String {
    attrs.iter().map(|a| format!("{}#[{}]\n", pad, a)).collect()
}

/// Emit a `where` clause block if non-empty.
/// Equation: fmt_where(W) = "" if W=∅, else "\nwhere " W.join(", ")
fn fmt_where(wc: &[String]) -> String {
    if wc.is_empty() {
        String::new()
    } else {
        format!("\nwhere\n    {}", wc.join(",\n    "))
    }
}
