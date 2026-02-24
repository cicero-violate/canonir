//! semantic: domain=tooling
//! Model-aware diff for self-host invariant

use serde_json::{Map, Value};
use crate::ir::model_ir::ModelIR;

#[derive(Debug)]
pub struct ModelDiff {
    pub added: Vec<String>,
    pub removed: Vec<String>,
    pub changed: Vec<String>,
}

impl ModelDiff {
    pub fn is_empty(&self) -> bool {
        self.added.is_empty() && self.removed.is_empty() && self.changed.is_empty()
    }

    pub fn diff_semantic(a: &ModelIR, b: &ModelIR) -> Self {
        let mut changed = Vec::new();
        if a.nodes        != b.nodes        { changed.push("nodes".to_string()); }
        if a.edge_hints   != b.edge_hints   { changed.push("edge_hints".to_string()); }
        if a.emit_order   != b.emit_order   { changed.push("emit_order".to_string()); }
        // graph vertex counts as a cheap proxy for structural change
        if a.name_graph  .vertex_count() != b.name_graph  .vertex_count() { changed.push("name_graph".to_string()); }
        if a.type_graph  .vertex_count() != b.type_graph  .vertex_count() { changed.push("type_graph".to_string()); }
        if a.call_graph  .vertex_count() != b.call_graph  .vertex_count() { changed.push("call_graph".to_string()); }
        if a.module_graph.vertex_count() != b.module_graph.vertex_count() { changed.push("module_graph".to_string()); }
        if a.cfg_graph   .vertex_count() != b.cfg_graph   .vertex_count() { changed.push("cfg_graph".to_string()); }
        if a.region_graph.vertex_count() != b.region_graph.vertex_count() { changed.push("region_graph".to_string()); }
        if a.value_graph .vertex_count() != b.value_graph .vertex_count() { changed.push("value_graph".to_string()); }
        if a.macro_graph .vertex_count() != b.macro_graph .vertex_count() { changed.push("macro_graph".to_string()); }
        ModelDiff { added: vec![], removed: vec![], changed }
    }
}

pub fn diff_models(a: &Value, b: &Value) -> ModelDiff {
    let a = normalize_model(a);
    let b = normalize_model(b);

    let mut added = Vec::new();
    let mut removed = Vec::new();
    let mut changed = Vec::new();

    for key in ["modules", "structs", "traits", "impls"] {
        let ma = index_by_id(&a[key]);
        let mb = index_by_id(&b[key]);

        for k in ma.keys() {
            if !mb.contains_key(k) {
                removed.push(format!("{}:{}", key, k));
            }
        }
        for k in mb.keys() {
            if !ma.contains_key(k) {
                added.push(format!("{}:{}", key, k));
            }
        }
        for k in ma.keys() {
            if let (Some(va), Some(vb)) = (ma.get(k), mb.get(k)) {
                if va != vb {
                    changed.push(format!("{}:{}", key, k));
                }
            }
        }
    }

    if a["rules"] != b["rules"] {
        changed.push("rules".into());
    }
    if a["limits"] != b["limits"] {
        changed.push("limits".into());
    }

    ModelDiff { added, removed, changed }
}

fn normalize_model(v: &Value) -> Value {
    let mut v = v.clone();
    for key in ["modules", "structs", "traits", "impls"] {
        if let Some(arr) = v.get_mut(key).and_then(|v| v.as_array_mut()) {
            arr.sort_by(|a, b| {
                let a_id = a.get("id").and_then(|v| v.as_str()).unwrap_or("");
                let b_id = b.get("id").and_then(|v| v.as_str()).unwrap_or("");
                a_id.cmp(b_id)
            });
        }
    }
    if let Some(ord) = v.get_mut("ordering").and_then(|v| v.as_object_mut()) {
        for (_, v) in ord.iter_mut() {
            if let Some(arr) = v.as_array_mut() {
                arr.sort_by(|a, b| {
                    let a_s = a.as_str().unwrap_or("");
                    let b_s = b.as_str().unwrap_or("");
                    a_s.cmp(b_s)
                });
            }
        }
    }
    v
}

fn index_by_id(v: &Value) -> Map<String, Value> {
    let mut m = Map::new();
    if let Some(arr) = v.as_array() {
        for item in arr {
            if let Some(id) = item.get("id").and_then(|v| v.as_str()) {
                m.insert(id.to_string(), item.clone());
            }
        }
    }
    m
}
