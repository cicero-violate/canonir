use rustc_hir::def::DefKind;
use rustc_span::def_id::DefId;

use crate::types::{Body, EdgeHint, Node};

/// Canon fragment produced by rule-driven lowering.
/// This is a forward-compatible container for engine-based lowering.
#[derive(Debug, Default, Clone)]
pub struct CanonFragment {
    pub nodes: Vec<Node>,
    pub edge_hints: Vec<EdgeHint>,
    pub body: Option<Body>,
}

/// Def-level facts computed once and consumed by matching rules.
#[derive(Debug, Clone)]
pub struct DefMeta {
    pub def_id: DefId,
    pub def_kind: DefKind,
    pub has_body: bool,
    pub is_trait_item: bool,
    pub is_assoc_item: bool,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum RuleDefKind {
    Mod,
    Struct,
    Union,
    Enum,
    Trait,
    Impl,
    Fn,
    AssocFn,
    AssocTy,
    AssocConst,
    Const,
    StaticAny,
    TyAlias,
    Use,
}

/// Predicate used by a rule table entry.
#[derive(Debug, Clone, Default)]
pub struct RulePred {
    pub def_kind: Option<RuleDefKind>,
    pub has_body: Option<bool>,
    pub is_trait_item: Option<bool>,
    pub is_assoc_item: Option<bool>,
}

impl RulePred {
    pub fn matches(&self, meta: &DefMeta) -> bool {
        if let Some(k) = self.def_kind {
            let kind_ok = match k {
                RuleDefKind::Mod => matches!(meta.def_kind, DefKind::Mod),
                RuleDefKind::Struct => matches!(meta.def_kind, DefKind::Struct),
                RuleDefKind::Union => matches!(meta.def_kind, DefKind::Union),
                RuleDefKind::Enum => matches!(meta.def_kind, DefKind::Enum),
                RuleDefKind::Trait => matches!(meta.def_kind, DefKind::Trait),
                RuleDefKind::Impl => matches!(meta.def_kind, DefKind::Impl { .. }),
                RuleDefKind::Fn => matches!(meta.def_kind, DefKind::Fn),
                RuleDefKind::AssocFn => matches!(meta.def_kind, DefKind::AssocFn),
                RuleDefKind::AssocTy => matches!(meta.def_kind, DefKind::AssocTy),
                RuleDefKind::AssocConst => matches!(meta.def_kind, DefKind::AssocConst),
                RuleDefKind::Const => matches!(meta.def_kind, DefKind::Const),
                RuleDefKind::StaticAny => matches!(meta.def_kind, DefKind::Static { .. }),
                RuleDefKind::TyAlias => matches!(meta.def_kind, DefKind::TyAlias),
                RuleDefKind::Use => matches!(meta.def_kind, DefKind::Use),
            };
            if !kind_ok {
                return false;
            }
        }
        if let Some(v) = self.has_body {
            if meta.has_body != v {
                return false;
            }
        }
        if let Some(v) = self.is_trait_item {
            if meta.is_trait_item != v {
                return false;
            }
        }
        if let Some(v) = self.is_assoc_item {
            if meta.is_assoc_item != v {
                return false;
            }
        }
        true
    }
}

/// Edge template kind reserved for rule-table migration.
#[derive(Debug, Clone)]
pub enum RuleEdge {
    Contains,
    TypeOf,
    ImplFor,
    Calls,
    Custom(&'static str),
}

/// Emit mode for a rule entry.
#[derive(Debug, Clone)]
pub enum RuleEmit {
    /// Rule is currently handled by an explicit hook.
    Hook(&'static str),
    /// Reserved named template.
    Template(&'static str),
}

/// Rule table row.
#[derive(Debug, Clone)]
pub struct RuleSpec {
    pub name: &'static str,
    pub pred: RulePred,
    pub emit: RuleEmit,
    pub edges: &'static [RuleEdge],
}

/// Initial scaffold table.
/// Phase 3 starts with a low-risk authoritative subset.
pub static RULES: &[RuleSpec] = &[
    RuleSpec {
        name: "mod_item",
        pred: RulePred {
            def_kind: Some(RuleDefKind::Mod),
            has_body: None,
            is_trait_item: None,
            is_assoc_item: None,
        },
        emit: RuleEmit::Template("mod_item"),
        edges: &[],
    },
    RuleSpec {
        name: "struct_item",
        pred: RulePred {
            def_kind: Some(RuleDefKind::Struct),
            has_body: None,
            is_trait_item: None,
            is_assoc_item: None,
        },
        emit: RuleEmit::Template("struct_like_item"),
        edges: &[],
    },
    RuleSpec {
        name: "union_item",
        pred: RulePred {
            def_kind: Some(RuleDefKind::Union),
            has_body: None,
            is_trait_item: None,
            is_assoc_item: None,
        },
        emit: RuleEmit::Template("struct_like_item"),
        edges: &[],
    },
    RuleSpec {
        name: "enum_item",
        pred: RulePred {
            def_kind: Some(RuleDefKind::Enum),
            has_body: None,
            is_trait_item: None,
            is_assoc_item: None,
        },
        emit: RuleEmit::Template("enum_item"),
        edges: &[],
    },
    RuleSpec {
        name: "trait_item",
        pred: RulePred {
            def_kind: Some(RuleDefKind::Trait),
            has_body: None,
            is_trait_item: None,
            is_assoc_item: None,
        },
        emit: RuleEmit::Template("trait_item"),
        edges: &[],
    },
    RuleSpec {
        name: "impl_item",
        pred: RulePred {
            def_kind: Some(RuleDefKind::Impl),
            has_body: None,
            is_trait_item: None,
            is_assoc_item: None,
        },
        emit: RuleEmit::Template("impl_item"),
        edges: &[],
    },
    RuleSpec {
        name: "assoc_ty_item",
        pred: RulePred {
            def_kind: Some(RuleDefKind::AssocTy),
            has_body: None,
            is_trait_item: None,
            is_assoc_item: None,
        },
        emit: RuleEmit::Template("assoc_ty_item"),
        edges: &[],
    },
    RuleSpec {
        name: "assoc_const_item",
        pred: RulePred {
            def_kind: Some(RuleDefKind::AssocConst),
            has_body: None,
            is_trait_item: None,
            is_assoc_item: None,
        },
        emit: RuleEmit::Template("assoc_const_item"),
        edges: &[],
    },
    RuleSpec {
        name: "fn_item",
        pred: RulePred {
            def_kind: Some(RuleDefKind::Fn),
            has_body: None,
            is_trait_item: None,
            is_assoc_item: None,
        },
        emit: RuleEmit::Template("fn_item"),
        edges: &[],
    },
    RuleSpec {
        name: "assoc_fn_item",
        pred: RulePred {
            def_kind: Some(RuleDefKind::AssocFn),
            has_body: None,
            is_trait_item: None,
            is_assoc_item: None,
        },
        emit: RuleEmit::Template("assoc_fn_item"),
        edges: &[],
    },
    RuleSpec {
        name: "const_item",
        pred: RulePred {
            def_kind: Some(RuleDefKind::Const),
            has_body: None,
            is_trait_item: None,
            is_assoc_item: None,
        },
        emit: RuleEmit::Template("const_item"),
        edges: &[],
    },
    RuleSpec {
        name: "static_item",
        pred: RulePred {
            def_kind: Some(RuleDefKind::StaticAny),
            has_body: None,
            is_trait_item: None,
            is_assoc_item: None,
        },
        emit: RuleEmit::Template("static_item"),
        edges: &[],
    },
    RuleSpec {
        name: "ty_alias_item",
        pred: RulePred {
            def_kind: Some(RuleDefKind::TyAlias),
            has_body: None,
            is_trait_item: None,
            is_assoc_item: None,
        },
        emit: RuleEmit::Template("ty_alias_item"),
        edges: &[],
    },
    RuleSpec {
        name: "use_item",
        pred: RulePred {
            def_kind: Some(RuleDefKind::Use),
            has_body: None,
            is_trait_item: None,
            is_assoc_item: None,
        },
        emit: RuleEmit::Template("use_item"),
        edges: &[],
    },
];
