use canon_graph::artifacts_loader::CodeGraph;
use canon_graph::ingest::report_ingest::ReportFeatures;
use serde::Serialize;
use std::collections::{BTreeMap, HashMap};

#[derive(Debug, Clone, Serialize)]
pub struct InvariantResult {
    pub name: String,
    pub description: String,
    pub coverage: f64,
    pub violation_rate: f64,
    pub violations: Vec<u32>,
}

pub trait InvariantRule {
    fn name(&self) -> &'static str;
    fn description(&self) -> &'static str;
    fn evaluate(&self, graph: &CodeGraph, features: &ReportFeatures) -> InvariantResult;
}

pub fn discover_invariants(graph: &CodeGraph, features: &ReportFeatures) -> Vec<InvariantResult> {
    let rules: Vec<Box<dyn InvariantRule>> = vec![
        Box::new(ModuleOwnerRule),
        Box::new(CallEdgeRule),
        Box::new(CfgEntryRule),
        Box::new(HasBlockSrcFnRule),
        Box::new(HasBlockDstBlockRule),
        Box::new(HasParamSrcFnRule),
        Box::new(HasParamDstParamRule),
        Box::new(HasFieldSrcAdtRule),
        Box::new(HasFieldDstFieldRule),
        Box::new(ContainsSrcModuleRule),
    ];
    let mut out = Vec::new();
    for rule in rules {
        out.push(rule.evaluate(graph, features));
    }
    out
}

pub fn mine_candidates(graph: &CodeGraph, features: &ReportFeatures) -> Vec<InvariantResult> {
    let mut out = Vec::new();
    let invariants = discover_invariants(graph, features);
    for inv in invariants {
        if inv.coverage >= 0.99 && inv.violation_rate <= 0.01 {
            out.push(inv);
        }
    }
    let mined = mine_edge_kind_constraints(graph);
    for inv in mined {
        if inv.coverage >= 0.99 && inv.violation_rate <= 0.01 {
            out.push(inv);
        }
    }
    out
}

struct ModuleOwnerRule;
impl InvariantRule for ModuleOwnerRule {
    fn name(&self) -> &'static str {
        "must_have_module_owner"
    }
    fn description(&self) -> &'static str {
        "non-module nodes must have an incoming CONTAINS edge"
    }
    fn evaluate(&self, graph: &CodeGraph, _features: &ReportFeatures) -> InvariantResult {
        let mut incoming_contains: HashMap<u32, usize> = HashMap::new();
        for e in &graph.edges {
            if e.kind == "CONTAINS" {
                *incoming_contains.entry(e.dst).or_default() += 1;
            }
        }
        let mut total = 0usize;
        let mut violations = Vec::new();
        for n in &graph.nodes {
            if n.kind == "MODULE" {
                continue;
            }
            total += 1;
            if incoming_contains.get(&n.id).cloned().unwrap_or(0) == 0 {
                violations.push(n.id);
            }
        }
        let violated = violations.len();
        let coverage = if total == 0 { 1.0 } else { (total - violated) as f64 / total as f64 };
        let violation_rate = if total == 0 { 0.0 } else { violated as f64 / total as f64 };
        InvariantResult { name: self.name().to_string(), description: self.description().to_string(), coverage, violation_rate, violations }
    }
}

struct CallEdgeRule;
impl InvariantRule for CallEdgeRule {
    fn name(&self) -> &'static str {
        "call_edge_src_is_callsite_or_fn"
    }
    fn description(&self) -> &'static str {
        "CALL edge source must be CALL_SITE, FUNCTION, or METHOD"
    }
    fn evaluate(&self, graph: &CodeGraph, _features: &ReportFeatures) -> InvariantResult {
        let id_to_kind: HashMap<u32, &str> = graph.nodes.iter().map(|n| (n.id, n.kind.as_str())).collect();
        let mut total = 0usize;
        let mut violations = Vec::new();
        for e in &graph.edges {
            if e.kind != "CALL" {
                continue;
            }
            total += 1;
            let src_kind = id_to_kind.get(&e.src).copied().unwrap_or("");
            if src_kind != "CALL_SITE" && src_kind != "FUNCTION" && src_kind != "METHOD" {
                violations.push(e.src);
            }
        }
        let violated = violations.len();
        let coverage = if total == 0 { 1.0 } else { (total - violated) as f64 / total as f64 };
        let violation_rate = if total == 0 { 0.0 } else { violated as f64 / total as f64 };
        violations.sort();
        violations.dedup();
        InvariantResult { name: self.name().to_string(), description: self.description().to_string(), coverage, violation_rate, violations }
    }
}

struct CfgEntryRule;
impl InvariantRule for CfgEntryRule {
    fn name(&self) -> &'static str {
        "cfg_has_single_entry"
    }
    fn description(&self) -> &'static str {
        "each function has exactly one entry basic block"
    }
    fn evaluate(&self, graph: &CodeGraph, _features: &ReportFeatures) -> InvariantResult {
        let mut block_to_fn: BTreeMap<u32, u32> = BTreeMap::new();
        for e in &graph.edges {
            if e.kind == "HAS_BLOCK" {
                block_to_fn.insert(e.dst, e.src);
            }
        }
        let mut block_in: HashMap<u32, usize> = HashMap::new();
        for e in &graph.edges {
            if e.kind == "FLOW" || e.kind == "UNWIND" || e.kind == "RETURN" || e.kind == "BRANCH" {
                *block_in.entry(e.dst).or_default() += 1;
            }
        }
        let mut fn_entries: HashMap<u32, usize> = HashMap::new();
        for (block, fn_id) in &block_to_fn {
            if block_in.get(block).cloned().unwrap_or(0) == 0 {
                *fn_entries.entry(*fn_id).or_default() += 1;
            }
        }
        let mut total = 0usize;
        let mut violations = Vec::new();
        for n in &graph.nodes {
            if n.kind != "FUNCTION" && n.kind != "METHOD" {
                continue;
            }
            total += 1;
            if fn_entries.get(&n.id).cloned().unwrap_or(0) != 1 {
                violations.push(n.id);
            }
        }
        let violated = violations.len();
        let coverage = if total == 0 { 1.0 } else { (total - violated) as f64 / total as f64 };
        let violation_rate = if total == 0 { 0.0 } else { violated as f64 / total as f64 };
        InvariantResult { name: self.name().to_string(), description: self.description().to_string(), coverage, violation_rate, violations }
    }
}

struct HasBlockSrcFnRule;
impl InvariantRule for HasBlockSrcFnRule {
    fn name(&self) -> &'static str {
        "has_block_src_is_fn"
    }
    fn description(&self) -> &'static str {
        "HAS_BLOCK edge src must be FUNCTION or METHOD"
    }
    fn evaluate(&self, graph: &CodeGraph, _features: &ReportFeatures) -> InvariantResult {
        edge_kind_src_rule(graph, "HAS_BLOCK", &["FUNCTION", "METHOD"])
    }
}

struct HasBlockDstBlockRule;
impl InvariantRule for HasBlockDstBlockRule {
    fn name(&self) -> &'static str {
        "has_block_dst_is_basic_block"
    }
    fn description(&self) -> &'static str {
        "HAS_BLOCK edge dst must be BASIC_BLOCK"
    }
    fn evaluate(&self, graph: &CodeGraph, _features: &ReportFeatures) -> InvariantResult {
        edge_kind_dst_rule(graph, "HAS_BLOCK", &["BASIC_BLOCK"])
    }
}

struct HasParamSrcFnRule;
impl InvariantRule for HasParamSrcFnRule {
    fn name(&self) -> &'static str {
        "has_param_src_is_fn"
    }
    fn description(&self) -> &'static str {
        "HAS_PARAM edge src must be FUNCTION or METHOD"
    }
    fn evaluate(&self, graph: &CodeGraph, _features: &ReportFeatures) -> InvariantResult {
        edge_kind_src_rule(graph, "HAS_PARAM", &["FUNCTION", "METHOD"])
    }
}

struct HasParamDstParamRule;
impl InvariantRule for HasParamDstParamRule {
    fn name(&self) -> &'static str {
        "has_param_dst_is_param"
    }
    fn description(&self) -> &'static str {
        "HAS_PARAM edge dst must be PARAM"
    }
    fn evaluate(&self, graph: &CodeGraph, _features: &ReportFeatures) -> InvariantResult {
        edge_kind_dst_rule(graph, "HAS_PARAM", &["PARAM"])
    }
}

struct HasFieldSrcAdtRule;
impl InvariantRule for HasFieldSrcAdtRule {
    fn name(&self) -> &'static str {
        "has_field_src_is_adt"
    }
    fn description(&self) -> &'static str {
        "HAS_FIELD edge src must be STRUCT or ENUM"
    }
    fn evaluate(&self, graph: &CodeGraph, _features: &ReportFeatures) -> InvariantResult {
        edge_kind_src_rule(graph, "HAS_FIELD", &["STRUCT", "ENUM"])
    }
}

struct HasFieldDstFieldRule;
impl InvariantRule for HasFieldDstFieldRule {
    fn name(&self) -> &'static str {
        "has_field_dst_is_field"
    }
    fn description(&self) -> &'static str {
        "HAS_FIELD edge dst must be FIELD"
    }
    fn evaluate(&self, graph: &CodeGraph, _features: &ReportFeatures) -> InvariantResult {
        edge_kind_dst_rule(graph, "HAS_FIELD", &["FIELD"])
    }
}

struct ContainsSrcModuleRule;
impl InvariantRule for ContainsSrcModuleRule {
    fn name(&self) -> &'static str {
        "contains_src_is_module_or_crate"
    }
    fn description(&self) -> &'static str {
        "CONTAINS edge src must be MODULE or CRATE"
    }
    fn evaluate(&self, graph: &CodeGraph, _features: &ReportFeatures) -> InvariantResult {
        edge_kind_src_rule(graph, "CONTAINS", &["MODULE", "CRATE"])
    }
}

fn edge_kind_src_rule(graph: &CodeGraph, kind: &str, allowed: &[&str]) -> InvariantResult {
    let id_to_kind: HashMap<u32, &str> = graph.nodes.iter().map(|n| (n.id, n.kind.as_str())).collect();
    let mut total = 0usize;
    let mut violations = Vec::new();
    for e in &graph.edges {
        if e.kind != kind {
            continue;
        }
        total += 1;
        let src_kind = id_to_kind.get(&e.src).copied().unwrap_or("");
        if !allowed.iter().any(|k| *k == src_kind) {
            violations.push(e.src);
        }
    }
    let violated = violations.len();
    let coverage = if total == 0 { 1.0 } else { (total - violated) as f64 / total as f64 };
    let violation_rate = if total == 0 { 0.0 } else { violated as f64 / total as f64 };
    violations.sort();
    violations.dedup();
    InvariantResult { name: format!("{kind}_src_kind"), description: format!("{kind} edge src kind in {:?}", allowed), coverage, violation_rate, violations }
}

fn edge_kind_dst_rule(graph: &CodeGraph, kind: &str, allowed: &[&str]) -> InvariantResult {
    let id_to_kind: HashMap<u32, &str> = graph.nodes.iter().map(|n| (n.id, n.kind.as_str())).collect();
    let mut total = 0usize;
    let mut violations = Vec::new();
    for e in &graph.edges {
        if e.kind != kind {
            continue;
        }
        total += 1;
        let dst_kind = id_to_kind.get(&e.dst).copied().unwrap_or("");
        if !allowed.iter().any(|k| *k == dst_kind) {
            violations.push(e.dst);
        }
    }
    let violated = violations.len();
    let coverage = if total == 0 { 1.0 } else { (total - violated) as f64 / total as f64 };
    let violation_rate = if total == 0 { 0.0 } else { violated as f64 / total as f64 };
    violations.sort();
    violations.dedup();
    InvariantResult { name: format!("{kind}_dst_kind"), description: format!("{kind} edge dst kind in {:?}", allowed), coverage, violation_rate, violations }
}

fn mine_edge_kind_constraints(graph: &CodeGraph) -> Vec<InvariantResult> {
    let mut out = Vec::new();
    let candidates = vec![
        ("HAS_BLOCK", true, &["FUNCTION", "METHOD"][..]),
        ("HAS_BLOCK", false, &["BASIC_BLOCK"][..]),
        ("HAS_PARAM", true, &["FUNCTION", "METHOD"][..]),
        ("HAS_PARAM", false, &["PARAM"][..]),
        ("HAS_FIELD", true, &["STRUCT", "ENUM"][..]),
        ("HAS_FIELD", false, &["FIELD"][..]),
        ("CONTAINS", true, &["MODULE", "CRATE"][..]),
    ];
    for (kind, check_src, allowed) in candidates {
        let inv = if check_src { edge_kind_src_rule(graph, kind, allowed) } else { edge_kind_dst_rule(graph, kind, allowed) };
        out.push(inv);
    }
    out
}
