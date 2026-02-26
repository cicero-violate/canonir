use model::ir::node::{Body, Field, GenericParam, Param, TraitMethod};

use crate::emit::body::{emit_blocks, indent_raw};

pub fn fmt_generics(gs: &[GenericParam]) -> String {
    if gs.is_empty() {
        return String::new();
    }
    // Equation:
    //   fmt_generic(g) = "'" name                  if is_lifetime
    //                  = name                      if bounds=∅, default=None
    //                  = name ": " bounds          if bounds≠∅
    //                  = <above> " = " default_ty  if default_ty=Some  (E11)
    let inner: Vec<String> = gs
        .iter()
        .map(|g| {
            let base = if g.is_lifetime {
                // Lifetime names in the IR may already include the leading `'`.
                // Avoid doubling it (e.g. "'a" should stay "'a", not "''a").
                if g.name.starts_with('\'') {
                    g.name.clone()
                } else {
                    format!("'{}", g.name)
                }
            } else if g.bounds.is_empty() {
                g.name.clone()
            } else {
                format!("{}: {}", g.name, g.bounds.join(" + "))
            };
            match &g.default_ty {
                Some(d) => format!("{} = {}", base, d),
                None => base,
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
                if p.mutable {
                    "&mut self".into()
                } else {
                    "&self".into()
                }
            } else if p.mutable {
                let ty = fmt_ref_ty(&p.lifetime, &p.ty);
                format!("mut {}: {}", p.name, ty)
            } else {
                let ty = fmt_ref_ty(&p.lifetime, &p.ty);
                format!("{}: {}", p.name, ty)
            }
        })
        .collect();
    format!("({})", inner.join(", "))
}

/// Prepend a lifetime to a reference type if present.
///
/// Equation:
///   fmt_ref_ty(Some('a), "&T")  = "&'a T"
///   fmt_ref_ty(Some('a), "&mut T") = "&'a mut T"
///   fmt_ref_ty(None, ty)        = ty   (pass-through)
fn fmt_ref_ty(lifetime: &Option<String>, ty: &str) -> String {
    let ty = normalize_ty(ty);
    match lifetime {
        None => ty,
        Some(lt) => {
            if let Some(rest) = ty.as_str().strip_prefix("&mut ") {
                format!("&'{}  mut {}", lt, rest)
            } else if let Some(rest) = ty.as_str().strip_prefix('&') {
                format!("&'{} {}", lt, rest)
            } else {
                // Non-reference type with lifetime annotation — pass through.
                ty
            }
        }
    }
}

pub fn fmt_field(f: &Field, pad: &str) -> String {
    match &f.name {
        Some(n) => format!("{}{}{}: {},\n", pad, f.vis.to_token(), n, normalize_ty(&f.ty)),
        None => format!("{}{}{},\n", pad, f.vis.to_token(), normalize_ty(&f.ty)),
    }
}

/// Trait method helper (not a NodeKind — lives inside Trait node directly)
pub fn fmt_trait_method(m: &TraitMethod, pad: &str) -> String {
    let ret_part = if m.ret == "()" { String::new() } else { format!(" -> {}", normalize_ty(&m.ret)) };
    let unsafe_kw = if m.unsafe_ { "unsafe " } else { "" };
    let async_kw = if m.async_ { "async " } else { "" };
    let wc = if m.where_clauses.is_empty() { String::new() } else { format!("\nwhere\n    {}", m.where_clauses.join(",\n    ")) };
    let mut s: String = m.attrs.iter().map(|a| format!("{}#[{}]\n", pad, a)).collect();
    let sig = format!("{}{}{}fn {}{}{}{}{}", pad, async_kw, unsafe_kw, m.name, fmt_generics(&m.generics), fmt_params(&m.params), ret_part, wc,);
    let inner = format!("{}    ", pad);
    s.push_str(&match &m.body {
        Body::None => format!("{};\n", sig),
        Body::Blocks(bb) => format!("{} {{\n{}{}}}\n", sig, emit_blocks(bb, &inner), pad),
        Body::Raw(src) => format!("{} {{\n{}{}}}\n", sig, indent_raw(src, &inner), pad),
    });
    s
}

pub fn normalize_ty(ty: &str) -> String {
    let mut s = ty
        .replace("std::PathBuf", "std::path::PathBuf")
        .replace("std::Path", "std::path::Path")
        .replace("std::Formatter", "std::fmt::Formatter")
        .replace("std::Error", "std::error::Error");
    for root in ["data", "traits", "results", "core", "consts"] {
        s = qualify_root(&s, root);
    }
    s
}

fn qualify_root(s: &str, root: &str) -> String {
    let pat = format!("{root}::");
    let bytes = s.as_bytes();
    let mut out = String::with_capacity(s.len() + 16);
    let mut i = 0usize;
    while i < bytes.len() {
        if s[i..].starts_with(&pat) {
            let prev = if i == 0 { b' ' } else { bytes[i - 1] };
            let boundary = matches!(prev, b' ' | b'<' | b'(' | b'[' | b',' | b'&' | b'=' | b'>');
            let already_qualified = i >= 2 && bytes[i - 1] == b':' && bytes[i - 2] == b':';
            if boundary && !already_qualified {
                out.push_str("crate::");
            }
            out.push_str(&pat);
            i += pat.len();
            continue;
        }
        out.push(bytes[i] as char);
        i += 1;
    }
    out
}
