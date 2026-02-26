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

/// Canonical type string: strip well-known stdlib path prefixes so that
/// emitted code uses short idiomatic names instead of fully-qualified ones.
/// "std::string::String"                  → "String"
/// "std::result::Result<T, E>"            → "Result<T, E>"
/// "std::vec::Vec<T>"                     → "Vec<T>"
/// "std::boxed::Box<T>"                   → "Box<T>"
/// "std::option::Option<T>"               → "Option<T>"
/// Cross-crate paths like "data::model::User" are left as-is; the emit
/// layer handles resolution context.
pub fn ty(raw: &str) -> String {
    // Replace longest prefixes first to avoid partial matches.
    const REPLACEMENTS: &[(&str, &str)] = &[
        ("std::string::String", "String"),
        ("std::result::Result", "Result"),
        ("std::option::Option", "Option"),
        ("std::vec::Vec", "Vec"),
        ("std::boxed::Box", "Box"),
        ("std::collections::HashMap", "HashMap"),
        ("std::collections::HashSet", "HashSet"),
        ("alloc::string::String", "String"),
        ("alloc::vec::Vec", "Vec"),
        ("alloc::boxed::Box", "Box"),
    ];
    let mut s = raw.to_string();
    for (full, short) in REPLACEMENTS {
        // Replace all occurrences (e.g. inside generics).
        while let Some(pos) = s.find(full) {
            s.replace_range(pos..pos + full.len(), short);
        }
    }
    s
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
        // Cross-crate or already-canonical path — return as-is.
        // Do NOT re-prefix: if it already starts with "crate::" we must not touch it.
        raw
    }
}

/// Canonical module path built from def_path segments (not def_path_str).
/// def_path_str can lose the crate prefix in some module/re-export edge cases.
pub fn module_path(tcx: TyCtxt<'_>, def_id: DefId) -> String {
    use rustc_hir::definitions::DefPathData;

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
/// For non-inline modules (pub mod foo;), def_span points to the declaration
/// line, not the module's own file. Use the inner span of the module body
/// to get the correct file (e.g. "src/data/model.rs" not "src/data/mod.rs").
/// Falls back to norm::file(def_span) for inline modules and the crate root.
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
        FileName::Real(rn) => rn.local_path().map(|p| p.to_string_lossy().into_owned()).unwrap_or_else(|| "<unknown>".to_string()),
        _ => "<macro>".to_string(),
    }
}
