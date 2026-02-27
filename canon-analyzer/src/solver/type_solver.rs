use crate::solver::csr_to_adj;
use algorithms::graph::scc::kosaraju_scc;
use anyhow::Result;
use canon::edge::EdgeKind;
use canon::id::NodeId;
use canon::node::{CanonId, CanonNodeKind, PrimTy, TypeKind};
use canon::CanonIR;
use std::collections::{HashMap, HashSet};

pub fn solve(ir: &mut CanonIR) -> Result<()> {
    let v = ir.type_graph.vertex_count();
    if v == 0 {
        return Ok(());
    }

    let adj = csr_to_adj(&ir.type_graph);
    let sccs = kosaraju_scc(&adj);
    for scc in sccs.iter().filter(|s| s.len() > 1) {
        let _ = scc;
    }

    derive_instantiates_edges(ir);

    Ok(())
}

fn derive_instantiates_edges(ir: &mut CanonIR) {
    let mut def_by_name: HashMap<String, Vec<u32>> = HashMap::new();
    for n in &ir.nodes {
        match &n.kind {
            CanonNodeKind::Struct { name_id, .. }
            | CanonNodeKind::Enum { name_id, .. }
            | CanonNodeKind::Trait { name_id, .. }
            | CanonNodeKind::TypeAlias { name_id, .. } => {
                def_by_name.entry(ir.lookup_name(*name_id).to_string()).or_default().push(n.id.0);
            }
            _ => {}
        }
    }

    let mut type_nodes_by_text: HashMap<String, Vec<u32>> = HashMap::new();
    for n in &ir.nodes {
        if let CanonNodeKind::Type { kind } = &n.kind {
            if let Some(key) = type_kind_text_key(ir, kind) {
                type_nodes_by_text.entry(key).or_default().push(n.id.0);
            }
        }
    }

    let mut existing: HashSet<(u32, u32, &'static str)> = HashSet::new();
    for src in 0..ir.type_graph.vertex_count() {
        for (dst, edge) in ir.type_graph.neighbours(NodeId(src as u32)) {
            if *edge == EdgeKind::Instantiates {
                existing.insert((src as u32, dst.0, "inst"));
            }
        }
    }

    let mut new_edges: Vec<(u32, u32, EdgeKind)> = Vec::new();
    for n in &ir.nodes {
        let (path_id, _) = match &n.kind {
            CanonNodeKind::Type { kind: TypeKind::Extern(path_id) } => (path_id, "extern"),
            CanonNodeKind::Type { kind: TypeKind::Unresolved(path_id) } => (path_id, "unresolved"),
            _ => continue,
        };
        let path = ir.lookup_path(*path_id).trim();
        let Some((root, args)) = split_generic_path(path) else {
            continue;
        };

        let root_name = root.rsplit("::").next().unwrap_or(root);
        let Some(def_candidates) = def_by_name.get(root_name) else {
            continue;
        };
        if def_candidates.len() != 1 {
            continue;
        }
        let def_id = def_candidates[0];
        if existing.insert((n.id.0, def_id, "inst")) {
            new_edges.push((n.id.0, def_id, EdgeKind::Instantiates));
        }

        for arg in split_top_level(args, ',') {
            let arg = arg.trim();
            if arg.is_empty() {
                continue;
            }
            let normalized = normalize_type_text(arg);
            let Some(candidates) = type_nodes_by_text.get(&normalized) else {
                continue;
            };
            if candidates.len() != 1 {
                continue;
            }
            let arg_id = candidates[0];
            if existing.insert((n.id.0, arg_id, "inst")) {
                new_edges.push((n.id.0, arg_id, EdgeKind::Instantiates));
            }
        }
    }

    if new_edges.is_empty() {
        return;
    }

    let mut all_edges: Vec<(u32, u32, EdgeKind)> = Vec::new();
    for src in 0..ir.type_graph.vertex_count() {
        for (dst, edge) in ir.type_graph.neighbours(NodeId(src as u32)) {
            all_edges.push((src as u32, dst.0, edge.clone()));
        }
    }
    all_edges.extend(new_edges);

    let node_ids: Vec<CanonId> = (0..ir.nodes.len() as u32).map(CanonId).collect();
    ir.type_graph = canon::csr_graph::CsrGraph::from_edges(node_ids, all_edges);
}

fn split_generic_path(path: &str) -> Option<(&str, &str)> {
    let bytes = path.as_bytes();
    let mut start = None;
    let mut depth = 0i32;
    let mut end = None;
    for (i, b) in bytes.iter().copied().enumerate() {
        if b == b'<' {
            if depth == 0 {
                start = Some(i);
            }
            depth += 1;
        } else if b == b'>' {
            if depth == 0 {
                return None;
            }
            depth -= 1;
            if depth == 0 {
                end = Some(i);
                break;
            }
        }
    }
    let (start, end) = (start?, end?);
    if end != bytes.len() - 1 {
        return None;
    }
    Some((path[..start].trim(), path[start + 1..end].trim()))
}

fn split_top_level(s: &str, delim: char) -> Vec<&str> {
    let mut out: Vec<&str> = Vec::new();
    let mut start = 0usize;
    let mut angle = 0i32;
    let mut paren = 0i32;
    let mut bracket = 0i32;
    for (idx, ch) in s.char_indices() {
        match ch {
            '<' => angle += 1,
            '>' if angle > 0 => angle -= 1,
            '(' => paren += 1,
            ')' if paren > 0 => paren -= 1,
            '[' => bracket += 1,
            ']' if bracket > 0 => bracket -= 1,
            _ => {}
        }
        if ch == delim && angle == 0 && paren == 0 && bracket == 0 {
            out.push(s[start..idx].trim());
            start = idx + ch.len_utf8();
        }
    }
    out.push(s[start..].trim());
    out
}

fn normalize_type_text(raw: &str) -> String {
    raw.chars().filter(|c| !c.is_whitespace()).collect()
}

fn type_kind_text_key(ir: &CanonIR, kind: &TypeKind) -> Option<String> {
    match kind {
        TypeKind::Primitive(p) => Some(primitive_name(p).to_string()),
        TypeKind::Extern(path_id) | TypeKind::Unresolved(path_id) => Some(normalize_type_text(ir.lookup_path(*path_id))),
        _ => None,
    }
}

fn primitive_name(p: &PrimTy) -> &'static str {
    match p {
        PrimTy::Bool => "bool",
        PrimTy::Char => "char",
        PrimTy::Str => "str",
        PrimTy::U8 => "u8",
        PrimTy::U16 => "u16",
        PrimTy::U32 => "u32",
        PrimTy::U64 => "u64",
        PrimTy::U128 => "u128",
        PrimTy::Usize => "usize",
        PrimTy::I8 => "i8",
        PrimTy::I16 => "i16",
        PrimTy::I32 => "i32",
        PrimTy::I64 => "i64",
        PrimTy::I128 => "i128",
        PrimTy::Isize => "isize",
        PrimTy::F32 => "f32",
        PrimTy::F64 => "f64",
        PrimTy::Unit => "()",
        PrimTy::Never => "!",
    }
}
