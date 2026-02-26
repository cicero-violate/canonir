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
    // Strip stdlib noise that leaks through ty.to_string().
    const REPLACEMENTS2: &[(&str, &str)] = &[
        ("std::future::Future", "Future"),
        ("core::future::Future", "Future"),
        ("core::marker::Sized", "Sized"),
        ("std::marker::Sized", "Sized"),
        ("core::marker::Send", "Send"),
        ("core::marker::Sync", "Sync"),
        ("std::marker::Send", "Send"),
        ("std::marker::Sync", "Sync"),
        ("core::cmp::PartialOrd", "PartialOrd"),
        ("core::cmp::PartialEq", "PartialEq"),
        ("core::fmt::Debug", "Debug"),
        ("core::fmt::Display", "Display"),
        ("core::marker::Copy", "Copy"),
        ("core::clone::Clone", "Clone"),
    ];
    let mut s = raw.to_string();
    for (full, short) in REPLACEMENTS {
        // Replace all occurrences (e.g. inside generics).
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
    // e.g. "dyn Describable + 'static" -> "dyn Describable"
    //      "Box<dyn Describable + 'static>" -> "Box<dyn Describable>"
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
            // Find matching closing paren
            let rest = &s[pos + 1..];
            if let Some(close) = rest.find(')') {
                let inner = &rest[..close]; // "dyn Foo"
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
    // Strip inline `: Sized` bounds that appear in type alias generics rendered by ty.to_string().
    // e.g. "Result1231<T: Sized>" -> "Result1231<T>"
    loop {
        if let Some(pos) = s.find(": Sized") {
            // Only strip if followed by '>' or ',' (i.e. it's a generic param bound, not a trait object)
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

/// Strip crate-relative path prefixes from type strings produced by
/// ty.to_string(). Called after norm::ty so stdlib is already shortened.
/// "data::model::User"   → "User"
/// "traits::Describable" → "Describable"
/// "Vec<data::model::User>" → "Vec<User>"
/// Leaves external crate paths (no matching prefix) intact.
/// Takes the local crate name so it can also strip "my_crate::foo" forms.
pub fn ty_strip_local(s: &str, krate: &str) -> String {
    // Build a regex-free replacement: walk all occurrences of "::" and strip
    // any leading path component that is a known local module segment.
    // Strategy: replace "X::Y::Z" where X is not std/core/alloc with "Z".
    // We do this by splitting on angle brackets to avoid breaking generics,
    // then applying path stripping to each token.
    strip_path_prefixes(s, krate)
}

/// Strip "impl Foo: Bar + Baz" → "impl Foo" in type position.
/// rustc renders impl-trait params as "impl Trait: Bound1 + Bound2".
/// In ModelIR / emitted Rust, impl Trait params have no inline bounds.
pub fn ty_clean_impl(s: &str) -> String {
    // "impl Trait: Bound1 + Bound2" -> "impl Trait"
    // Only strip when:
    //   - after "impl " is a plain identifier (no "::")
    //   - followed by a single ':' (not "::")
    // This avoids mangling "impl future::Future<...>" which has paths but no bounds-colon.
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
            // Strip only if next is ':' (single) meaning bounds, not path.
            if i < bytes.len() && bytes[i] == b':' && (i + 1 >= bytes.len() || bytes[i + 1] != b':') {
                // Skip until delimiter at depth 0.
                i += 1; // skip ':'
                let mut depth = 0usize;
                while i < bytes.len() {
                    match bytes[i] {
                        b'<' => {
                            depth += 1;
                            i += 1;
                        }
                        b'>' if depth > 0 => {
                            depth -= 1;
                            i += 1;
                        }
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

fn strip_path_prefixes(s: &str, krate: &str) -> String {
    // Tokenize preserving structure: split on < > ( ) , space
    // and strip path prefixes from identifier-like tokens.
    // Simple approach: find all "X::Y" patterns where X is not a known
    // external crate and strip leading segments until only the last remains,
    // BUT only when the full path starts with the local crate name or is a
    // relative module path (no std/core/alloc/etc prefix).
    const KEEP_PREFIXES: &[&str] = &["std::", "core::", "alloc::", "futures::", "tokio::", "serde::", "anyhow::", "thiserror::"];
    // Replace "krate::foo::Bar" with "foo::Bar" first (crate-qualified form).
    let crate_prefix = format!("{}::", krate);
    let mut result = s.replace(&crate_prefix, "");
    // Now strip remaining "foo::Bar" → "Bar" for paths that don't start
    // with a known external crate prefix.
    // We do a pass: find "::" and walk back to find the start of the segment.
    loop {
        let mut changed = false;
        // Find any "word::word" pattern where the leading word is not a
        // kept prefix root.
        let bytes = result.as_bytes();
        let mut i = 0;
        let mut new_result = String::with_capacity(result.len());
        while i < bytes.len() {
            // Look for "::"
            if i + 1 < bytes.len() && bytes[i] == b':' && bytes[i + 1] == b':' {
                // Walk back to find start of the leading segment.
                let seg_end = i;
                let mut seg_start = seg_end;
                while seg_start > 0 {
                    let c = bytes[seg_start - 1];
                    if c.is_ascii_alphanumeric() || c == b'_' {
                        seg_start -= 1;
                    } else {
                        break;
                    }
                }
                let seg = &result[seg_start..seg_end];
                // Check if this segment is a kept external prefix root.
                let keep = KEEP_PREFIXES.iter().any(|p| p.starts_with(&format!("{}::", seg)));
                if !keep && !seg.is_empty() {
                    // Strip "seg::" — remove from seg_start to i+2
                    // Rewrite: everything before seg_start already pushed,
                    // skip seg and "::", continue from i+2.
                    // But we've been pushing char by char — restart with replace.
                    let before = &result[..seg_start];
                    let after = &result[i + 2..];
                    result = format!("{}{}", before, after);
                    changed = true;
                    break;
                }
            }
            new_result.push(bytes[i] as char);
            i += 1;
        }
        if !changed {
            break;
        }
    }
    result
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
        // If this is a local DefId but def_path_str omitted the crate name (can happen in
        // some re-export / module edge cases), rebuild from def_path segments.
        if def_id.is_local() && !raw.starts_with("crate::") && raw.contains("::") {
            return module_path(tcx, def_id);
        }
        // Cross-crate or already-canonical path — return as-is.
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
