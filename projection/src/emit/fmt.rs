use model::ir::{
    model_ir::ModelIR,
    node::{Body, Field, GenericParam, Param, TraitMethod},
};

use crate::emit::body::{emit_blocks, indent_raw};

pub fn fmt_generics(gs: &[GenericParam]) -> String {
    if gs.is_empty() {
        return String::new();
    }
    let inner: Vec<String> = gs
        .iter()
        .map(|g| {
            if g.is_lifetime {
                format!("'{}", g.name)
            } else if g.bounds.is_empty() {
                g.name.clone()
            } else {
                format!("{}: {}", g.name, g.bounds.join(" + "))
            }
        })
        .collect();
    format!("<{}>", inner.join(", "))
}

pub fn fmt_params(params: &[Param]) -> String {
    let inner: Vec<String> = params
        .iter()
        .map(|p| {
            if p.is_self {
                if p.mutable { "&mut self".into() } else { "&self".into() }
            } else if p.mutable {
                format!("mut {}: {}", p.name, p.ty)
            } else {
                format!("{}: {}", p.name, p.ty)
            }
        })
        .collect();
    format!("({})", inner.join(", "))
}

pub fn fmt_field(f: &Field, pad: &str) -> String {
    match &f.name {
        Some(n) => format!("{}{}{}: {},\n", pad, f.vis.to_token(), n, f.ty),
        None => format!("{}{}{},\n", pad, f.vis.to_token(), f.ty),
    }
}

/// Trait method helper (not a NodeKind — lives inside Trait node directly)
pub fn fmt_trait_method(m: &TraitMethod, _ir: &ModelIR, pad: &str) -> String {
    let ret_part = if m.ret == "()" { String::new() } else { format!(" -> {}", m.ret) };
    let unsafe_kw = if m.unsafe_ { "unsafe " } else { "" };
    let async_kw  = if m.async_  { "async "  } else { "" };
    let wc = if m.where_clauses.is_empty() {
        String::new()
    } else {
        format!("\nwhere\n    {}", m.where_clauses.join(",\n    "))
    };
    let mut s: String = m.attrs.iter().map(|a| format!("{}#[{}]\n", pad, a)).collect();
    let sig = format!(
        "{}{}{}fn {}{}{}{}{}",
        pad,
        async_kw,
        unsafe_kw,
        m.name,
        fmt_generics(&m.generics),
        fmt_params(&m.params),
        ret_part,
        wc,
    );
    let inner = format!("{}    ", pad);
    s.push_str(&match &m.body {
        Body::None => format!("{};\n", sig),
        Body::Blocks(bb) => format!("{} {{\n{}{}}}\n", sig, emit_blocks(bb, &inner), pad),
        Body::Raw(src) => format!("{} {{\n{}{}}}\n", sig, indent_raw(src, &inner), pad),
    });
    s
}
