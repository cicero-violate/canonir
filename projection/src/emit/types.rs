// CONTRACT:
// - No sorting
// - No graph traversal
// - No mutation
// - Pure string rendering of Plan

use crate::emit::fmt::{fmt_field, fmt_generics, normalize_ty};
use crate::emit::helpers::{fmt_attrs, fmt_where, Emit};
use model::ir::node::{EnumVariant, Field, GenericParam, StructKind, Visibility};

pub struct StructEmitter<'a> {
    pub name: &'a str,
    pub vis: &'a Visibility,
    pub generics: &'a [GenericParam],
    pub fields: &'a [Field],
    pub derives: &'a [String],
    pub attrs: &'a [String],
    pub where_clauses: &'a [String],
    pub struct_kind: &'a StructKind,
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
                let tys: Vec<String> = self.fields.iter().map(|f| format!("{}{}", f.vis.to_token(), normalize_ty(&f.ty))).collect();
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

pub struct EnumEmitter<'a> {
    pub name: &'a str,
    pub vis: &'a Visibility,
    pub generics: &'a [GenericParam],
    pub variants: &'a [EnumVariant],
    pub derives: &'a [String],
    pub attrs: &'a [String],
    pub where_clauses: &'a [String],
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
                let tys: Vec<String> = v.fields.iter().map(|f| normalize_ty(&f.ty)).collect();
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

pub struct TypeAliasEmitter<'a> {
    pub name: &'a str,
    pub vis: &'a Visibility,
    pub generics: &'a [GenericParam],
    pub ty: &'a str,
    pub attrs: &'a [String],
    pub where_clauses: &'a [String],
}

impl Emit for TypeAliasEmitter<'_> {
    fn emit(&self, pad: &str) -> String {
        let s = fmt_attrs(self.attrs, pad);
        let wc = fmt_where(self.where_clauses);
        format!("{}{}{}type {}{} = {}{};\n", s, pad, self.vis.to_token(), self.name, fmt_generics(self.generics), normalize_ty(self.ty), wc,)
    }
}

pub struct TypeRefEmitter<'a> {
    pub name: &'a str,
}

impl Emit for TypeRefEmitter<'_> {
    fn emit(&self, pad: &str) -> String {
        format!("{}// type alias: {}\n", pad, self.name)
    }
}

pub struct ExternCrateEmitter<'a> {
    pub name: &'a str,
    pub alias: &'a Option<String>,
    pub vis: &'a Visibility,
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
