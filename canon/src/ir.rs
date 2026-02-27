//! CanonIR — the canonical intermediate representation.
//!
//! Variables:
//!   nodes        : Vec<CanonNode>              — flat node arena, index = CanonId
//!   name_intern  : Interner                    — NameId  → String
//!   path_intern  : Interner                    — PathId  → String
//!   type_index   : HashMap<TypeKey, CanonId>   — content-addressed type dedup (runtime only)
//!   name_graph   : CsrGraph<CanonId, EdgeKind> — G_name
//!   type_graph   : CsrGraph<CanonId, EdgeKind> — G_type
//!   call_graph   : CsrGraph<CanonId, EdgeKind> — G_call
//!   module_graph : CsrGraph<CanonId, EdgeKind> — G_module
//!   cfg_graph    : CsrGraph<CanonId, EdgeKind> — G_cfg
//!   region_graph : CsrGraph<CanonId, EdgeKind> — G_region
//!   value_graph  : CsrGraph<CanonId, EdgeKind> — G_value
//!   macro_graph  : CsrGraph<CanonId, EdgeKind> — G_macro
//!
//! Invariants:
//!   nodes[CanonId] is the only source of truth — no string refs outside intern tables
//!   type_index enforces structural dedup: identical TypeKind → same CanonId
//!   graphs are sealed (immutable CsrGraph) after the seal pass
//!
//! Pipeline:
//!   ModelIR  ->  seal_pass()  ->  CanonIR  ->  analyze()  ->  emit()

use std::collections::HashMap;

use serde::{Deserialize, Serialize};

use crate::{
    csr_graph::CsrGraph,
    edge::EdgeKind,
    intern::{Interner, NameId, PathId},
    node::{CanonId, CanonNodeKind, TypeKind},
};

/// A node in the canon arena.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct CanonNode {
    pub id: CanonId,
    pub kind: CanonNodeKind,
}

/// Key used for content-addressed type deduplication.
/// Wraps TypeKind — we rely on PartialEq + Hash for identity.
/// Not serialized; rebuilt by restore_type_index().
#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub struct TypeKey(pub TypeKind);

// TypeKind must be Eq + Hash for content-addressing.
// We derive PartialEq on TypeKind already; add Eq + Hash here.
impl Eq for TypeKind {}

impl std::hash::Hash for TypeKind {
    fn hash<H: std::hash::Hasher>(&self, state: &mut H) {
        std::mem::discriminant(self).hash(state);
        match self {
            TypeKind::Primitive(p) => p.hash(state),
            TypeKind::Adt(id) => id.hash(state),
            TypeKind::Ref { lifetime, inner, mutable } => {
                lifetime.hash(state);
                inner.hash(state);
                mutable.hash(state);
            }
            TypeKind::RawPtr { inner, mutable } => {
                inner.hash(state);
                mutable.hash(state);
            }
            TypeKind::Array { inner, len } => {
                inner.hash(state);
                len.hash(state);
            }
            TypeKind::Slice(id) => id.hash(state),
            TypeKind::Tuple(ids) => ids.hash(state),
            TypeKind::FnPtr(id) => id.hash(state),
            TypeKind::ImplTrait(id) => id.hash(state),
            TypeKind::DynTrait(id) => id.hash(state),
            TypeKind::Param(name_id) => name_id.hash(state),
            TypeKind::Applied { base, args } => {
                base.hash(state);
                args.hash(state);
            }
            TypeKind::Extern(path_id) => path_id.hash(state),
            TypeKind::Unresolved(path_id) => path_id.hash(state),
            TypeKind::TypeRef { name_id } => name_id.hash(state),
        }
    }
}

/// The full canonical intermediate representation.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct CanonIR {
    pub version: String,

    // ── Node arena ───────────────────────────────────────────────────────────
    /// All nodes.  CanonId(i) indexes nodes[i].
    pub nodes: Vec<CanonNode>,

    /// Topological emit order (CanonIds), produced by module_solver on CanonIR.
    pub emit_order: Vec<CanonId>,

    // ── Intern tables ────────────────────────────────────────────────────────
    /// Identifiers: fn names, field names, local names, attr names, lifetime names.
    pub name_intern: Interner,
    /// Qualified paths: module paths, use paths, extern crate paths, macro paths.
    pub path_intern: Interner,
    /// Raw source snippets and macro token strings.
    #[serde(default)]
    pub body_intern: Interner,

    // ── Type dedup index (runtime only, not serialized) ───────────────────────
    /// Maps TypeKind → CanonId of the canonical Type node.
    /// Rebuilt by restore_type_index() after deserialization.
    #[serde(skip)]
    pub type_index: HashMap<TypeKey, CanonId>,

    // ── 8 CSR graphs ─────────────────────────────────────────────────────────
    /// G_name  — rename / name-resolution constraints.
    pub name_graph: CsrGraph<CanonId, EdgeKind>,
    /// G_type  — type inference / unification / impl / dyn edges.
    pub type_graph: CsrGraph<CanonId, EdgeKind>,
    /// G_call  — caller → callee.
    pub call_graph: CsrGraph<CanonId, EdgeKind>,
    /// G_module — containment: module → item.
    pub module_graph: CsrGraph<CanonId, EdgeKind>,
    /// G_cfg   — control-flow edges within bodies.
    pub cfg_graph: CsrGraph<CanonId, EdgeKind>,
    /// G_region — lifetime outlives constraints.
    #[serde(default)]
    pub region_graph: CsrGraph<CanonId, EdgeKind>,
    /// G_value  — const/static dependency edges.
    #[serde(default)]
    pub value_graph: CsrGraph<CanonId, EdgeKind>,
    /// G_macro  — macro expansion edges.
    #[serde(default)]
    pub macro_graph: CsrGraph<CanonId, EdgeKind>,
}

impl CanonIR {
    pub fn new() -> Self {
        Self {
            version: "canon-1.0".into(),
            nodes: Vec::new(),
            emit_order: Vec::new(),
            name_intern: Interner::new(),
            path_intern: Interner::new(),
            body_intern: Interner::new(),
            type_index: HashMap::new(),
            name_graph: CsrGraph::empty(),
            type_graph: CsrGraph::empty(),
            call_graph: CsrGraph::empty(),
            module_graph: CsrGraph::empty(),
            cfg_graph: CsrGraph::empty(),
            region_graph: CsrGraph::empty(),
            value_graph: CsrGraph::empty(),
            macro_graph: CsrGraph::empty(),
        }
    }

    // ── Arena ─────────────────────────────────────────────────────────────────

    /// Push a node into the arena, returning its CanonId.
    pub fn push_node(&mut self, kind: CanonNodeKind) -> CanonId {
        let id = CanonId(self.nodes.len() as u32);
        self.nodes.push(CanonNode { id, kind });
        id
    }

    /// Look up a node by id.  Panics on out-of-range.
    #[inline]
    pub fn node(&self, id: CanonId) -> &CanonNode {
        &self.nodes[id.0 as usize]
    }

    // ── Intern helpers ────────────────────────────────────────────────────────

    /// Intern an identifier string, returning a NameId.
    #[inline]
    pub fn intern_name(&mut self, s: &str) -> NameId {
        NameId(self.name_intern.intern(s))
    }

    /// Intern a qualified path string, returning a PathId.
    #[inline]
    pub fn intern_path(&mut self, s: &str) -> PathId {
        let normalized = canonical_path_form(s);
        PathId(self.path_intern.intern(normalized.as_ref()))
    }

    /// Intern raw source/token text, returning its stable index.
    #[inline]
    pub fn intern_body(&mut self, s: &str) -> NameId {
        NameId(self.body_intern.intern(s))
    }

    /// Look up a name by NameId.
    #[inline]
    pub fn lookup_name(&self, id: NameId) -> &str {
        self.name_intern.lookup(id.0)
    }

    /// Look up a path by PathId.
    #[inline]
    pub fn lookup_path(&self, id: PathId) -> &str {
        self.path_intern.lookup(id.0)
    }

    /// Look up raw body/token text.
    #[inline]
    pub fn lookup_body(&self, id: NameId) -> &str {
        self.body_intern.lookup(id.0)
    }

    // ── Type dedup ────────────────────────────────────────────────────────────

    /// Intern a Type node by structural content.
    /// If an identical TypeKind already exists in the arena, returns its existing CanonId.
    /// Otherwise allocates a new Type node and indexes it.
    pub fn intern_type(&mut self, kind: TypeKind) -> CanonId {
        let key = TypeKey(kind.clone());
        if let Some(&existing) = self.type_index.get(&key) {
            return existing;
        }
        let id = self.push_node(CanonNodeKind::Type { kind });
        self.type_index.insert(key, id);
        id
    }

    // ── Post-deserialize restore ───────────────────────────────────────────────

    /// Rebuild all runtime-only indices after loading from JSON.
    /// Must be called once after serde_json::from_str / from_reader.
    pub fn restore(&mut self) {
        self.name_intern.restore_index();
        self.path_intern.restore_index();
        self.body_intern.restore_index();
        self.restore_type_index();
    }

    fn restore_type_index(&mut self) {
        self.type_index.clear();
        for node in &self.nodes {
            if let CanonNodeKind::Type { kind } = &node.kind {
                let key = TypeKey(kind.clone());
                self.type_index.entry(key).or_insert(node.id);
            }
        }
    }
}

fn canonical_path_form(s: &str) -> std::borrow::Cow<'_, str> {
    let trimmed = s.trim();
    let stripped = if let Some(rest) = trimmed.strip_prefix("::") { rest } else { trimmed };
    let normalized = stripped.to_string();

    let invalid = normalized.is_empty()
        || normalized.contains('{')
        || normalized.contains('}')
        || normalized.contains("=>")
        || normalized.contains('!')
        || normalized.starts_with(':')
        || normalized.ends_with(':');
    if invalid {
        panic!("invalid path for path_intern: {s}");
    }

    std::borrow::Cow::Owned(normalized)
}

impl Default for CanonIR {
    fn default() -> Self {
        Self::new()
    }
}
