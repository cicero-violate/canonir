//! Normalization layer: HIR raw values → canonical ModelIR strings.
//!
//! Pipeline position:
//!   HIR Extractor (project/item.rs)
//!     ↓
//!   norm::* (THIS FILE)  ← canonicalize before IR construction
//!     ↓
//!   Node / NodeKind construction
//!
//! Rules:
//!   span  : "file:lo:col: hi:col (#N)" → "file:line:col"  (lo only)
//!   path  : "my_crate::foo::bar"       → "crate::foo::bar"
//!   file  : span source file only      → "src/lib.rs"

use rustc_middle::ty::TyCtxt;
use rustc_span::{
    def_id::{DefId, LOCAL_CRATE},
    FileName, Pos, Span,
};

/// Short identifier: last segment of a canonical path.
/// "crate::data::model::Score" → "Score"
/// "crate"                     → "crate"  (module root, no segment to strip)
pub fn short(full_path: &str) -> &str {
    full_path.rsplit("::").next().unwrap_or(full_path)
}

/// Canonical type string: strip well-known stdlib/core/alloc path prefixes.
///
/// Pass 1 (REPLACEMENTS) — common type paths:
///   "std::string::String"          → "String"
///   "std::result::Result"          → "Result"
///   "core::result::Result"         → "Result"
///   "alloc::result::Result"        → "Result"
///   etc.
///
/// Pass 2 (REPLACEMENTS2) — trait / marker paths that leak from ty.to_string():
///   "core::marker::Sized"          → "Sized"
///   "core::fmt::Debug"             → "Debug"
///   etc.
pub fn ty(raw: &str) -> String {
    // Pass 1: common type paths (std / core / alloc variants).
    const REPLACEMENTS: &[(&str, &str)] = &[
        ("std::Path",                 "std::path::Path"),
        ("std::PathBuf",              "std::path::PathBuf"),
        ("std::Formatter",            "std::fmt::Formatter"),
        ("std::Error",                "std::error::Error"),
        ("std::string::String",       "String"),
        ("alloc::string::String",     "String"),
        ("std::result::Result",       "Result"),
        ("core::result::Result",      "Result"),
        ("alloc::result::Result",     "Result"),
        ("std::option::Option",       "Option"),
        ("core::option::Option",      "Option"),
        ("std::vec::Vec",             "Vec"),
        ("alloc::vec::Vec",           "Vec"),
        ("core::vec::Vec",            "Vec"),
        ("std::boxed::Box",           "Box"),
        ("alloc::boxed::Box",         "Box"),
        ("std::collections::HashMap", "HashMap"),
        ("std::collections::HashSet", "HashSet"),
    ];
    // Pass 2: trait / marker paths that appear inside type strings.
    const REPLACEMENTS2: &[(&str, &str)] = &[
        ("std::future::Future",   "Future"),
        ("core::future::Future",  "Future"),
        ("core::marker::Sized",   "Sized"),
        ("std::marker::Sized",    "Sized"),
        ("core::marker::Send",    "Send"),
        ("core::marker::Sync",    "Sync"),
        ("std::marker::Send",     "Send"),
        ("std::marker::Sync",     "Sync"),
        ("core::cmp::PartialOrd", "PartialOrd"),
        ("core::cmp::PartialEq",  "PartialEq"),
        ("core::fmt::Debug",      "Debug"),
        ("core::fmt::Display",    "Display"),
        ("core::marker::Copy",    "Copy"),
        ("core::clone::Clone",    "Clone"),
    ];
    let mut s = raw.to_string();
    for (full, short) in REPLACEMENTS {
        while let Some(pos) = s.find(full) {
            s.replace_range(pos..pos + full.len(), short);
        }
    }
    for (full, short) in REPLACEMENTS2 {
        while let Some(pos) = s.find(full) {
            s.replace_range(pos..pos + full.len(), short);
        }
    }
    // Strip trailing `+ 'static` lifetime bounds from dyn/impl types.
    // "dyn Describable + 'static" -> "dyn Describable"
    loop {
        if let Some(pos) = s.find(" + 'static") {
            s.replace_range(pos..pos + " + 'static".len(), "");
        } else {
            break;
        }
    }
    // Strip spurious parens around dyn traits: "Box<(dyn Foo)>" -> "Box<dyn Foo>"
    loop {
        if let Some(pos) = s.find("(dyn ") {
            let rest = &s[pos + 1..];
            if let Some(close) = rest.find(')') {
                let inner = &rest[..close];
                let before = &s[..pos];
                let after = &rest[close + 1..];
                s = format!("{}{}{}", before, inner, after);
            } else {
                break;
            }
        } else {
            break;
        }
    }
    // Strip inline `: Sized` bounds in type alias generics from ty.to_string().
    // "Result1231<T: Sized>" -> "Result1231<T>"
    loop {
        if let Some(pos) = s.find(": Sized") {
            let after = s.as_bytes().get(pos + 7).copied();
            if matches!(after, Some(b'>') | Some(b',') | Some(b' ') | None) {
                s.replace_range(pos..pos + 7, "");
            } else {
                break;
            }
        } else {
            break;
        }
    }
    s
}

/// Strip crate-relative path prefixes from type strings produced by ty.to_string().
/// Called after norm::ty so stdlib is already shortened.
/// "<crate>::data::model::User" → "crate::data::model::User"
/// "std::path::PathBuf"         → "std::path::PathBuf"
pub fn ty_strip_local(s: &str, krate: &str) -> String {
    let crate_prefix = format!("{}::", krate);
    s.replace(&crate_prefix, "crate::")
}

/// Strip "impl Foo: Bar + Baz" → "impl Foo" in type position.
/// rustc renders impl-trait params as "impl Trait: Bound1 + Bound2".
pub fn ty_clean_impl(s: &str) -> String {
    let bytes = s.as_bytes();
    let mut out = String::with_capacity(s.len());
    let mut i = 0usize;
    while i < bytes.len() {
        if bytes[i..].starts_with(b"impl ") {
            out.push_str("impl ");
            i += 5;
            let name_start = i;
            while i < bytes.len() && (bytes[i].is_ascii_alphanumeric() || bytes[i] == b'_') {
                i += 1;
            }
            let name = &s[name_start..i];
            out.push_str(name);
            if i < bytes.len() && bytes[i] == b':' && (i + 1 >= bytes.len() || bytes[i + 1] != b':') {
                i += 1;
                let mut depth = 0usize;
                while i < bytes.len() {
                    match bytes[i] {
                        b'<' => { depth += 1; i += 1; }
                        b'>' if depth > 0 => { depth -= 1; i += 1; }
                        b',' | b')' if depth == 0 => break,
                        _ => i += 1,
                    }
                }
            }
        } else {
            out.push(bytes[i] as char);
            i += 1;
        }
    }
    out
}

/// Normalize static-lifetime references in type position.
/// "&'static str" → "&str", "&'static [u8]" → "&[u8]".
/// These appear in const item types from tcx.type_of.
pub fn ty_strip_static_lifetime(s: &str) -> String {
    s.replace("&'static str", "&str")
     .replace("&'static [u8]", "&[u8]")
}

/// Canonical span: "src/foo.rs:line:col" — lo position only, no hygiene.
pub fn span(tcx: TyCtxt<'_>, s: Span) -> String {
    let sm = tcx.sess.source_map();
    let loc = sm.lookup_char_pos(s.lo());
    let f = real_filename(&loc.file.name);
    format!("{}:{}:{}", f, loc.line, loc.col.to_usize() + 1)
}

/// Canonical file: "src/foo.rs" — no line/col. Used for Module.file.
pub fn file(tcx: TyCtxt<'_>, s: Span) -> String {
    let sm = tcx.sess.source_map();
    let loc = sm.lookup_char_pos(s.lo());
    real_filename(&loc.file.name)
}

/// Canonical path: crate name prefix → "crate".
/// "my_crate::core::engine" → "crate::core::engine"
/// "my_crate"               → "crate"
pub fn path(tcx: TyCtxt<'_>, def_id: DefId) -> String {
    let raw = tcx.def_path_str(def_id);
    let krate = tcx.crate_name(LOCAL_CRATE).to_string();
    if raw == krate {
        "crate".to_string()
    } else if let Some(rest) = raw.strip_prefix(&format!("{}::", krate)) {
        format!("crate::{}", rest)
    } else {
        if def_id.is_local() && !raw.starts_with("crate::") && raw.contains("::") {
            return module_path(tcx, def_id);
        }
        raw
    }
}

/// Canonical module path built from def_path segments (not def_path_str).
pub fn module_path(tcx: TyCtxt<'_>, def_id: DefId) -> String {
    let def_path = tcx.def_path(def_id);
    let mut segments: Vec<String> = Vec::new();
    for d in def_path.data.iter() {
        if let Some(sym) = d.data.get_opt_name() {
            segments.push(sym.to_string());
        }
    }
    if segments.is_empty() {
        "crate".to_string()
    } else {
        format!("crate::{}", segments.join("::"))
    }
}

/// Canonical file path for a Module node.
/// For non-inline modules, uses inner_span to get the module's own file.
pub fn module_file(tcx: TyCtxt<'_>, def_id: DefId) -> String {
    if let Some(local) = def_id.as_local() {
        if let rustc_hir::Node::Item(item) = tcx.hir_node_by_def_id(local) {
            if let rustc_hir::ItemKind::Mod(_, inner_mod) = item.kind {
                let inner_span = inner_mod.spans.inner_span;
                let candidate = file(tcx, inner_span);
                if candidate != "<macro>" && candidate != "<unknown>" {
                    return candidate;
                }
            }
        }
    }
    file(tcx, tcx.def_span(def_id))
}

fn real_filename(name: &FileName) -> String {
    match name {
        FileName::Real(rn) => rn.local_path()
            .map(|p| p.to_string_lossy().into_owned())
            .unwrap_or_else(|| "<unknown>".to_string()),
        _ => "<macro>".to_string(),
    }
}
