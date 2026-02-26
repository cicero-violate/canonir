// CONTRACT:
// - No sorting
// - No graph traversal
// - No mutation
// - Pure string rendering of Plan

use crate::emit::body::{emit_blocks, indent_raw};
use crate::emit::fmt::{fmt_generics, fmt_params, fmt_trait_method};
use crate::emit::helpers::{fmt_attrs, fmt_where, Emit};
use model::ir::node::{Body, GenericParam, Param, TraitMethod, Visibility};

pub struct TraitEmitter<'a> {
    pub name: &'a str,
    pub vis: &'a Visibility,
    pub generics: &'a [GenericParam],
    pub methods: &'a [TraitMethod],
    pub attrs: &'a [String],
    pub where_clauses: &'a [String],
    pub unsafe_: bool,
}

impl Emit for TraitEmitter<'_> {
    fn emit(&self, pad: &str) -> String {
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
            s.push_str(&fmt_trait_method(m, &inner));
        }
        s.push_str(&format!("{}}}\n", pad));
        s
    }
}

pub struct FnEmitter<'a> {
    pub name: &'a str,
    pub vis: &'a Visibility,
    pub generics: &'a [GenericParam],
    pub params: &'a [Param],
    pub ret: &'a str,
    pub body: &'a Body,
    pub attrs: &'a [String],
    pub where_clauses: &'a [String],
    pub unsafe_: bool,
    pub async_: bool,
}

impl Emit for FnEmitter<'_> {
    fn emit(&self, pad: &str) -> String {
        let ret_part = if self.ret == "()" {
            String::new()
        } else {
            format!(" -> {}", self.ret)
        };
        let unsafe_kw = if self.unsafe_ { "unsafe " } else { "" };
        let async_kw = if self.async_ { "async " } else { "" };
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
