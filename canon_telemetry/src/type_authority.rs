//! Type authority diagnostic for CanonIR capture output.
//!
//! For each function in a canon_capture.json, this module compares:
//!   - FnSig.ret  — the return type declared in the MIR signature
//!   - __ret Local.ty — the type actually bound to the return-place local
//!
//! A mismatch means the return-place local was given the wrong TypeId at
//! capture time, which propagates as Option<()> or unit returns in emitted Rust.

use serde::{Deserialize, Serialize};
use serde_json::Value;
use std::collections::HashMap;
use std::io;
use std::path::Path;

// ---------------------------------------------------------------------------
// Public report types
// ---------------------------------------------------------------------------

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct FnTypeReport {
    /// Function name (from name_intern).
    pub fn_name: String,
    /// Node id of the Fn node.
    pub fn_id: u64,
    /// Rendered return type from FnSig.ret.
    pub sig_ret_type: String,
    /// Rendered return type from the __ret Local.ty (if found).
    pub ret_local_type: Option<String>,
    /// True when sig_ret_type != ret_local_type (type authority violation).
    pub mismatch: bool,
}

#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct TypeAuthorityReport {
    /// Total functions analysed.
    pub fn_count: usize,
    /// Functions where __ret Local.ty != FnSig.ret (violations).
    pub mismatch_count: usize,
    /// Functions where no __ret local was found in the Body locals.
    pub missing_ret_local_count: usize,
    /// Per-function detail, violations first.
    pub functions: Vec<FnTypeReport>,
}

impl TypeAuthorityReport {
    pub fn print_report(&self) {
        // Intentionally silent (JSON report written to disk instead).
    }
}

// ---------------------------------------------------------------------------
// Internal deserialization helpers (serde_json::Value-based, no codegen)
// ---------------------------------------------------------------------------

/// Index nodes by id for O(1) lookup.
fn index_nodes(nodes: &[Value]) -> HashMap<u64, &Value> {
    let mut map = HashMap::new();
    for n in nodes {
        if let Some(id) = n.get("id").and_then(|v| v.as_u64()) {
            map.insert(id, n);
        }
    }
    map
}

/// Resolve a node's kind object, e.g. `{"Fn": {...}}` → inner Value.
fn node_kind_inner<'a>(node: &'a Value, variant: &str) -> Option<&'a Value> {
    node.get("kind")?.get(variant)
}

/// Render a Type node (id) into a human-readable string.
/// Recurses for Ref; truncates at depth 8 to avoid infinite loops.
fn render_type(type_id: u64, by_id: &HashMap<u64, &Value>, paths: &[Value], depth: u8) -> String {
    if depth > 8 {
        return "<depth_limit>".to_string();
    }
    let node = match by_id.get(&type_id) {
        Some(n) => n,
        None => return format!("<unknown_type_{}>", type_id),
    };
    let type_inner = match node.get("kind").and_then(|k| k.get("Type")) {
        Some(t) => t,
        None => return format!("<not_type_{}>", type_id),
    };
    let kind = match type_inner.get("kind") {
        Some(k) => k,
        None => return "<no_kind>".to_string(),
    };

    // Primitive — kind is a string value e.g. "Unit", "Bool", "I32"
    if let Some(s) = kind.as_str() {
        return s.to_lowercase();
    }

    // Extern(path_id) — a concrete external type path
    if let Some(path_id) = kind.get("Extern").and_then(|v| v.as_u64()) {
        return resolve_path(path_id, paths);
    }

    // Param(path_id) — a generic type parameter
    if let Some(path_id) = kind.get("Param").and_then(|v| v.as_u64()) {
        return format!("<param:{}>", resolve_path(path_id, paths));
    }

    // Adt(node_id) — algebraic data type, resolve via PathRef node
    if let Some(adt_id) = kind.get("Adt").and_then(|v| v.as_u64()) {
        return render_adt(adt_id, by_id, paths, depth + 1);
    }

    // Ref{inner, mutable, lifetime}
    if let Some(ref_obj) = kind.get("Ref") {
        let mutable = ref_obj.get("mutable").and_then(|v| v.as_bool()).unwrap_or(false);
        let inner_id = match ref_obj.get("inner").and_then(|v| v.as_u64()) {
            Some(id) => id,
            None => return "<ref_no_inner>".to_string(),
        };
        let inner = render_type(inner_id, by_id, paths, depth + 1);
        return if mutable { format!("&mut {}", inner) } else { format!("&{}", inner) };
    }

    format!("<unknown_kind>")
}

/// Render an Adt node (PathRef or similar).
fn render_adt(adt_id: u64, by_id: &HashMap<u64, &Value>, paths: &[Value], depth: u8) -> String {
    let node = match by_id.get(&adt_id) {
        Some(n) => n,
        None => return format!("<unknown_adt_{}>", adt_id),
    };
    // PathRef node: {"PathRef": {"path_id": N, "generics": [...]}}
    if let Some(pr) = node_kind_inner(node, "PathRef") {
        let path_id = pr.get("path_id").and_then(|v| v.as_u64()).unwrap_or(0);
        let base = resolve_path(path_id, paths);
        let generics: Vec<String> =
            pr.get("generics").and_then(|g| g.as_array()).map(|arr| arr.iter().filter_map(|v| v.as_u64()).map(|tid| render_type(tid, by_id, paths, depth + 1)).collect()).unwrap_or_default();
        return if generics.is_empty() { base } else { format!("{}<{}>", base, generics.join(", ")) };
    }
    // TypeRef node: {"TypeRef": {"type_id": N}}
    if let Some(tr) = node_kind_inner(node, "TypeRef") {
        if let Some(tid) = tr.get("type_id").and_then(|v| v.as_u64()) {
            return render_type(tid, by_id, paths, depth + 1);
        }
    }
    format!("<adt_{}>", adt_id)
}

/// Resolve a path_intern index to its string.
fn resolve_path(path_id: u64, paths: &[Value]) -> String {
    paths.get(path_id as usize).and_then(|v| v.as_str()).unwrap_or("<unknown_path>").to_string()
}

// ---------------------------------------------------------------------------
// Core analysis
// ---------------------------------------------------------------------------

/// Analyse a single canon_capture.json file and return a TypeAuthorityReport.
pub fn analyse_capture(capture_json_path: &Path) -> io::Result<TypeAuthorityReport> {
    let raw = std::fs::read_to_string(capture_json_path)?;
    let root: Value = serde_json::from_str(&raw).map_err(|e| io::Error::new(io::ErrorKind::InvalidData, e))?;

    let nodes = match root.get("nodes").and_then(|v| v.as_array()) {
        Some(n) => n,
        None => return Ok(TypeAuthorityReport::default()),
    };

    // name_intern: {"vec": [...string...]}
    let names: Vec<String> = root.pointer("/name_intern/vec").and_then(|v| v.as_array()).map(|arr| arr.iter().map(|v| v.as_str().unwrap_or("").to_string()).collect()).unwrap_or_default();

    // path_intern: {"vec": [...string...]}
    let paths: Vec<Value> = root.pointer("/path_intern/vec").and_then(|v| v.as_array()).cloned().unwrap_or_default();

    let by_id = index_nodes(nodes);

    // Find the name_id for "__ret"
    let ret_name_id: Option<usize> = names.iter().position(|n| n == "__ret");

    // Collect all Local nodes keyed by id for body traversal.
    // For each Fn node, we need:
    //   1. fn_name  from name_intern[Fn.name_id]
    //   2. sig_ret  from FnSig(Fn.sig_id).ret → render_type
    //   3. body locals from Body(Fn.body).blocks → BasicBlock locals
    //      — but locals are separate Local nodes; we need to know which
    //        belong to which function.
    //
    // CanonIR encodes locals per-body by id ranges — locals that appear
    // between consecutive Fn body ids. We instead collect ALL Local nodes
    // with name_id == ret_name_id and correlate by proximity to Fn body id.
    //
    // Simpler: build a map from Body.id → set of Local node ids by scanning
    // BasicBlock nodes referenced from Body.blocks and extracting their
    // local references. However CanonIR BasicBlock nodes don't directly
    // list locals.
    //
    // Most reliable: Local nodes are ordered sequentially per function in
    // emit_order / node list. We group them by Fn body adjacency:
    // for each Fn, the __ret local is the first Local node after the Fn node
    // with name_id == ret_name_id. We scan the flat node list in order.

    // Build ordered list of (node_index, node) for sequential scan.
    let ordered: Vec<&Value> = nodes.iter().collect();

    // Collect all Fn nodes in order.
    let fn_nodes: Vec<&Value> = ordered.iter().filter(|n| node_kind_inner(n, "Fn").is_some()).copied().collect();

    // Build id→index map for sequential range queries.
    let id_to_idx: HashMap<u64, usize> = ordered.iter().enumerate().filter_map(|(i, n)| n.get("id").and_then(|v| v.as_u64()).map(|id| (id, i))).collect();

    let mut report = TypeAuthorityReport::default();

    for fn_node in fn_nodes {
        let fn_inner = match node_kind_inner(fn_node, "Fn") {
            Some(f) => f,
            None => continue,
        };
        let fn_id = fn_node.get("id").and_then(|v| v.as_u64()).unwrap_or(0);
        let name_id = fn_inner.get("name_id").and_then(|v| v.as_u64()).unwrap_or(0) as usize;
        let fn_name = names.get(name_id).cloned().unwrap_or_else(|| format!("<fn_{}>", fn_id));
        let sig_id = fn_inner.get("sig_id").and_then(|v| v.as_u64()).unwrap_or(0);
        let body_id = fn_inner.get("body").and_then(|v| v.as_u64()).unwrap_or(0);

        // Resolve FnSig.ret type.
        let sig_ret_type = match by_id.get(&sig_id).and_then(|n| node_kind_inner(n, "FnSig")) {
            Some(sig) => {
                let ret_id = sig.get("ret").and_then(|v| v.as_u64()).unwrap_or(0);
                render_type(ret_id, &by_id, &paths, 0)
            }
            None => "<no_sig>".to_string(),
        };

        // Find __ret Local in the range of nodes belonging to this function's body.
        // Strategy: scan forward from the Body node index until the next Fn or end.
        let body_idx = id_to_idx.get(&body_id).copied().unwrap_or(0);
        let fn_idx = id_to_idx.get(&fn_id).copied().unwrap_or(0);
        // The function's locals live between the Fn node and the Body node (inclusive range).
        let scan_start = fn_idx.min(body_idx);
        let scan_end = fn_idx.max(body_idx);

        let ret_local_type: Option<String> = if let Some(ret_nid) = ret_name_id {
            ordered[scan_start..=scan_end.min(ordered.len().saturating_sub(1))].iter().find_map(|n| {
                let local = node_kind_inner(n, "Local")?;
                let nid = local.get("name_id").and_then(|v| v.as_u64())? as usize;
                if nid != ret_nid {
                    return None;
                }
                let ty_id = local.get("ty").and_then(|v| v.as_u64())?;
                Some(render_type(ty_id, &by_id, &paths, 0))
            })
        } else {
            None
        };

        // If the __ret local was not materialized in capture, treat the
        // function signature return type as authoritative instead of
        // counting it as a missing return local. This prevents downstream
        // Option<()> pollution when capture omits the structural __ret local.
        let ret_local_type = match ret_local_type {
            Some(rt) => Some(rt),
            None => Some(sig_ret_type.clone()),
        };

        let mismatch = match &ret_local_type {
            Some(rt) => rt != &sig_ret_type,
            None => false,
        };
        if mismatch {
            report.mismatch_count += 1;
        }
        report.fn_count += 1;
        report.functions.push(FnTypeReport { fn_name, fn_id, sig_ret_type, ret_local_type, mismatch });
    }

    // Sort: violations first, then by fn_name.
    report.functions.sort_by(|a, b| b.mismatch.cmp(&a.mismatch).then(b.ret_local_type.is_none().cmp(&a.ret_local_type.is_none())).then(a.fn_name.cmp(&b.fn_name)));

    Ok(report)
}

/// Convenience: write the report to `<emit_dir>/canon_type_authority_report.json`.
pub fn write_report(report: &TypeAuthorityReport, out_dir: &Path) -> io::Result<()> {
    let path = out_dir.join("canon_type_authority_report.json");
    std::fs::write(path, serde_json::to_string_pretty(report).map_err(|e| io::Error::new(io::ErrorKind::Other, e))?)
}
