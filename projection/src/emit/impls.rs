// CONTRACT:
// - No sorting
// - No graph traversal
// - No mutation
// - Pure string rendering of Plan

use crate::emit::fmt::fmt_generics;
use crate::emit::functions::FnEmitter;
use crate::emit::helpers::{fmt_attrs, fmt_where, Emit};
use crate::layout::ImplPlan;
use model::ir::node::NodeKind;

pub struct ImplEmitter<'a> {
    pub imp: &'a ImplPlan,
}

impl ImplEmitter<'_> {
    pub fn emit(&self, pad: &str) -> String {
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
            if let NodeKind::Method { name, vis, generics, params, ret, body, attrs, where_clauses, unsafe_, async_ } = m {
                s.push_str(&FnEmitter { name, vis, generics, params, ret, body, attrs, where_clauses, unsafe_: *unsafe_, async_: *async_ }.emit(&inner));
            }
        }
        s.push_str(&format!("{}}}\n", pad));
        s
    }
}
