use canon::csr_graph::CsrGraph;
use canon::edge::EdgeKind;
use canon::node::{CanonId, CanonNodeKind};
use canon::CanonIR;
use std::collections::HashMap;

pub struct NameGraphBuilder {
    v: usize,
    edges: Vec<(u32, u32, EdgeKind)>,
}

impl NameGraphBuilder {
    pub fn new(v: usize) -> Self {
        Self { v, edges: Vec::new() }
    }

    pub fn add_rename(&mut self, src: CanonId, dst: CanonId) {
        self.edges.push((src.0, dst.0, EdgeKind::Renames));
    }

    pub fn add_resolves(&mut self, src: CanonId, dst: CanonId) {
        self.edges.push((src.0, dst.0, EdgeKind::Resolves));
    }

    pub fn derive_from_ir(&mut self, ir: &CanonIR) {
        let resolver = Resolver::new(ir);

        for n in &ir.nodes {
            match &n.kind {
                CanonNodeKind::Use { path_id, alias, .. } => {
                    let path = ir.lookup_path(*path_id);
                    if let Some(target) = resolver.resolve_use(n.id, path) {
                        self.add_resolves(n.id, target);
                        if alias.is_some() {
                            self.add_rename(n.id, target);
                        }
                    }
                }
                CanonNodeKind::ExternCrate { name_id, alias: Some(_), .. } => {
                    let original = ir.lookup_name(*name_id);
                    if let Some(target) = resolver.resolve_unique_name(n.id, original) {
                        self.add_rename(n.id, target);
                    }
                }
                _ => {}
            }
        }
    }

    pub fn edges(&self) -> &[(u32, u32, EdgeKind)] {
        &self.edges
    }

    pub fn build(self) -> CsrGraph<CanonId, EdgeKind> {
        let node_ids: Vec<CanonId> = (0..self.v as u32).map(CanonId).collect();
        CsrGraph::from_edges(node_ids, self.edges)
    }
}

struct Resolver {
    by_mod_and_name: HashMap<(String, String), Vec<CanonId>>,
    by_name: HashMap<String, Vec<CanonId>>,
    module_path_of: Vec<Option<String>>,
    parent_of: Vec<Option<usize>>,
}

impl Resolver {
    fn new(ir: &CanonIR) -> Self {
        let mut module_path_of = vec![None; ir.nodes.len()];
        for n in &ir.nodes {
            if let CanonNodeKind::Module { path_id, .. } = &n.kind {
                module_path_of[n.id.0 as usize] = Some(ir.lookup_path(*path_id).to_string());
            }
        }

        let mut parent_of = vec![None; ir.nodes.len()];
        for src in 0..ir.module_graph.vertex_count() {
            let src_id = canon::id::NodeId(src as u32);
            for (dst, edge) in ir.module_graph.neighbours(src_id) {
                if *edge == EdgeKind::Contains && dst.index() < parent_of.len() {
                    parent_of[dst.index()] = Some(src);
                }
            }
        }

        let containing_module = |start: usize| -> Option<usize> {
            let mut cur = Some(start);
            while let Some(i) = cur {
                if matches!(ir.nodes.get(i).map(|n| &n.kind), Some(CanonNodeKind::Module { .. })) {
                    return Some(i);
                }
                cur = parent_of.get(i).and_then(|x| *x);
            }
            None
        };

        let mut by_mod_and_name: HashMap<(String, String), Vec<CanonId>> = HashMap::new();
        let mut by_name: HashMap<String, Vec<CanonId>> = HashMap::new();
        for n in &ir.nodes {
            let Some(name) = node_name(ir, &n.kind) else {
                continue;
            };
            by_name.entry(name.to_string()).or_default().push(n.id);

            if let Some(midx) = containing_module(n.id.0 as usize) {
                if let Some(Some(mod_path)) = module_path_of.get(midx) {
                    by_mod_and_name.entry((mod_path.clone(), name.to_string())).or_default().push(n.id);
                }
            }
        }

        Self { by_mod_and_name, by_name, module_path_of, parent_of }
    }

    fn resolve_unique_name(&self, skip: CanonId, name: &str) -> Option<CanonId> {
        let matches: Vec<CanonId> = self.by_name.get(name)?.iter().copied().filter(|id| *id != skip).collect();
        if matches.len() == 1 {
            Some(matches[0])
        } else {
            None
        }
    }

    fn resolve_use(&self, use_id: CanonId, path: &str) -> Option<CanonId> {
        if path.ends_with("::*") {
            return None;
        }

        let segments: Vec<&str> = path.split("::").filter(|s| !s.is_empty()).collect();
        if segments.is_empty() {
            return None;
        }

        let (base_mod, start_idx) = match segments.first().copied() {
            Some("crate") => (Some("crate".to_string()), 1usize),
            Some("self") => (self.containing_module_path(use_id), 1usize),
            Some("super") => (self.parent_module_path(use_id), 1usize),
            _ => (self.containing_module_path(use_id), 0usize),
        };

        let parts = &segments[start_idx..];
        if parts.is_empty() {
            return None;
        }

        let mut candidates: Vec<CanonId> = Vec::new();
        if parts.len() >= 2 {
            let target_name = parts[parts.len() - 1];
            let mod_suffix = parts[..parts.len() - 1].join("::");
            for module in self.candidate_module_paths(base_mod.as_deref(), &mod_suffix) {
                if let Some(ids) = self.by_mod_and_name.get(&(module, target_name.to_string())) {
                    candidates.extend(ids.iter().copied().filter(|id| *id != use_id));
                }
            }
        } else {
            let target_name = parts[0];
            if let Some(site_mod) = self.containing_module_path(use_id) {
                if let Some(ids) = self.by_mod_and_name.get(&(site_mod, target_name.to_string())) {
                    candidates.extend(ids.iter().copied().filter(|id| *id != use_id));
                }
            }
            if candidates.is_empty() {
                if let Some(ids) = self.by_mod_and_name.get(&("crate".to_string(), target_name.to_string())) {
                    candidates.extend(ids.iter().copied().filter(|id| *id != use_id));
                }
            }
        }

        dedup_ids(&mut candidates);
        if candidates.len() == 1 {
            return Some(candidates[0]);
        }

        // Strict fallback: only accept unique global symbol by exact name.
        let target_name = parts[parts.len() - 1];
        self.resolve_unique_name(use_id, target_name)
    }

    fn containing_module_path(&self, id: CanonId) -> Option<String> {
        let mut cur = Some(id.0 as usize);
        while let Some(i) = cur {
            if let Some(Some(path)) = self.module_path_of.get(i) {
                return Some(path.clone());
            }
            cur = self.parent_of.get(i).and_then(|x| *x);
        }
        None
    }

    fn parent_module_path(&self, id: CanonId) -> Option<String> {
        let here = self.containing_module_path(id)?;
        here.rsplit_once("::").map(|(p, _)| p.to_string())
    }

    fn candidate_module_paths(&self, base: Option<&str>, suffix: &str) -> Vec<String> {
        let Some(base) = base else {
            return vec![];
        };
        if suffix.is_empty() {
            return vec![base.to_string()];
        }
        if base == "crate" {
            vec![format!("crate::{}", suffix)]
        } else {
            vec![format!("{}::{}", base, suffix), format!("crate::{}", suffix)]
        }
    }
}

fn node_name<'a>(ir: &'a CanonIR, kind: &'a CanonNodeKind) -> Option<&'a str> {
    match kind {
        CanonNodeKind::Fn { name_id, .. }
        | CanonNodeKind::Struct { name_id, .. }
        | CanonNodeKind::Enum { name_id, .. }
        | CanonNodeKind::Trait { name_id, .. }
        | CanonNodeKind::TypeAlias { name_id, .. }
        | CanonNodeKind::TypeRef { name_id }
        | CanonNodeKind::Const { name_id, .. }
        | CanonNodeKind::Static { name_id, .. }
        | CanonNodeKind::ExternCrate { name_id, .. }
        | CanonNodeKind::Lifetime { name_id }
        | CanonNodeKind::GenericParam { name_id, .. }
        | CanonNodeKind::Param { name_id, .. }
        | CanonNodeKind::Variant { name_id, .. } => Some(ir.lookup_name(*name_id)),
        CanonNodeKind::Use { alias: Some(alias), .. } => Some(ir.lookup_name(*alias)),
        CanonNodeKind::Use { path_id, alias: None, .. } => Some(ir.lookup_path(*path_id)),
        _ => None,
    }
}

fn dedup_ids(ids: &mut Vec<CanonId>) {
    ids.sort_by_key(|id| id.0);
    ids.dedup_by_key(|id| id.0);
}
