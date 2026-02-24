//! Node identity and kind for ModelIR.
//!
//! Variables:
//!   NodeId       = dense u32 index into the node arena
//!   NodeKind     = discriminated union of all IR item types
//!   GenericParam = (name, bounds, is_lifetime)
//!   Field        = (name, ty, vis)
//!   EnumVariant  = (name, fields)
//!   BodyNode     = one statement/expression in a function body CFG
//!   Terminator   = how a basic block exits
//!
//! Equations:
//!   node_arena[NodeId] -> NodeKind
//!   attrs(v)          -> Vec<String>      (#[attr] list, E1)
//!   where_clauses(v)  -> Vec<String>      (E2)
//!   unsafe_(v)        -> bool             (E12)
//!   async_(v)         -> bool             (E13)

use serde::{Deserialize, Serialize};

/// Opaque dense node index. All graphs share the same NodeId space.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub struct NodeId(pub u32);

impl NodeId {
    #[inline]
    pub fn index(self) -> usize {
        self.0 as usize
    }
}

/// `pub`, `pub(crate)`, `pub(super)`, or private.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub enum Visibility {
    Public,
    PubCrate,
    PubSuper,
    PubIn(String),
    Private,
}

impl Visibility {
    pub fn to_token(&self) -> &str {
        match self {
            Visibility::Public    => "pub ",
            Visibility::PubCrate  => "pub(crate) ",
            Visibility::PubSuper  => "pub(super) ",
            Visibility::PubIn(_)  => "pub(in ...) ",
            Visibility::Private   => "",
        }
    }
}

/// A generic parameter: `T`, `T: Clone + Debug`, `'a`.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct GenericParam {
    pub name: String,
    pub bounds: Vec<String>,
    pub is_lifetime: bool,
}

/// A struct or tuple field.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Field {
    pub name: Option<String>, // None = tuple field
    pub ty: String,
    pub vis: Visibility,
}

/// A function parameter.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Param {
    pub name: String,
    pub ty: String,
    pub is_self: bool,
    pub mutable: bool,
}

/// One variant of an enum  (E6).
///   name   : identifier, e.g. "Ok", "Err", "Point"
///   fields : empty = unit variant; Some(name)=None = tuple; Some(name)=Some = named
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct EnumVariant {
    pub name: String,
    pub fields: Vec<Field>,
}

/// A single statement or expression inside a function body.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub enum Stmt {
    /// `let <pat>: <ty> = <expr>;`
    Let { pat: String, ty: Option<String>, init: Option<String> },
    /// A bare expression statement.
    Expr(String),
    /// A return expression.  `return <expr>;`
    Return(Option<String>),
    /// Raw source verbatim (escape hatch for complex bodies).
    Raw(String),
}

/// How a basic block exits.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub enum Terminator {
    /// Falls through to block index `target`.
    Goto(u32),
    /// `if <cond> { goto true_bb } else { goto false_bb }`
    Branch { cond: String, true_bb: u32, false_bb: u32 },
    /// Function returns (value already in last Stmt::Return).
    Return,
    /// Unreachable / not yet assigned.
    None,
}

/// A basic block: list of statements + terminator.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct BasicBlock {
    pub stmts: Vec<Stmt>,
    pub terminator: Terminator,
}

/// Inline body for a function or method.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub enum Body {
    /// No body (trait declaration, extern).
    None,
    /// Structured basic-block CFG.
    Blocks(Vec<BasicBlock>),
    /// Raw source verbatim (escape hatch).
    Raw(String),
}

/// A trait method signature (with optional default body).
///
/// E1  : attrs          — #[attr] list on the method declaration
/// E2  : where_clauses  — where T: Foo bounds
/// E12 : unsafe_        — unsafe fn
/// E13 : async_         — async fn
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct TraitMethod {
    pub name: String,
    pub vis: Visibility,
    pub generics: Vec<GenericParam>,
    pub params: Vec<Param>,
    pub ret: String,
    pub body: Body,
    /// E1 — #[attr] annotations on the method.
    #[serde(default)]
    pub attrs: Vec<String>,
    /// E2 — where clauses, e.g. ["T: Clone", "U: Debug"].
    #[serde(default)]
    pub where_clauses: Vec<String>,
    /// E12 — unsafe fn.
    #[serde(default)]
    pub unsafe_: bool,
    /// E13 — async fn.
    #[serde(default)]
    pub async_: bool,
}

/// Every item in the IR is one of these kinds.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub enum NodeKind {
    // ── Crate / Module ──────────────────────────────────────────────────────
    Crate { name: String, edition: String },

    /// E10: inline flag not yet added (future).
    Module { path: String, file: String },

    // ── Type definitions ────────────────────────────────────────────────────

    /// E1: attrs; E2: where_clauses; E7: StructKind not yet split.
    Struct {
        name: String,
        vis: Visibility,
        generics: Vec<GenericParam>,
        fields: Vec<Field>,
        derives: Vec<String>,
        /// E1 — #[attr] list.
        #[serde(default)]
        attrs: Vec<String>,
        /// E2 — where clauses.
        #[serde(default)]
        where_clauses: Vec<String>,
    },

    /// E6 — enum with variants.  Unblocks S13 (exhaustiveness_solver).
    Enum {
        name: String,
        vis: Visibility,
        generics: Vec<GenericParam>,
        variants: Vec<EnumVariant>,
        derives: Vec<String>,
        /// E1 — #[attr] list.
        #[serde(default)]
        attrs: Vec<String>,
        /// E2 — where clauses.
        #[serde(default)]
        where_clauses: Vec<String>,
    },

    // ── Trait / Impl ────────────────────────────────────────────────────────

    Trait {
        name: String,
        vis: Visibility,
        generics: Vec<GenericParam>,
        methods: Vec<TraitMethod>,
        /// E1
        #[serde(default)]
        attrs: Vec<String>,
        /// E2
        #[serde(default)]
        where_clauses: Vec<String>,
        /// E12 — unsafe trait.
        #[serde(default)]
        unsafe_: bool,
    },

    Impl {
        for_struct: String,
        for_trait: Option<String>,
        generics: Vec<GenericParam>,
        /// E1
        #[serde(default)]
        attrs: Vec<String>,
        /// E2
        #[serde(default)]
        where_clauses: Vec<String>,
        /// E12 — unsafe impl.
        #[serde(default)]
        unsafe_: bool,
    },

    // ── Functions / Methods ─────────────────────────────────────────────────

    Function {
        name: String,
        vis: Visibility,
        generics: Vec<GenericParam>,
        params: Vec<Param>,
        ret: String,
        body: Body,
        /// E1
        #[serde(default)]
        attrs: Vec<String>,
        /// E2
        #[serde(default)]
        where_clauses: Vec<String>,
        /// E12 — unsafe fn.
        #[serde(default)]
        unsafe_: bool,
        /// E13 — async fn.
        #[serde(default)]
        async_: bool,
    },

    Method {
        name: String,
        vis: Visibility,
        generics: Vec<GenericParam>,
        params: Vec<Param>,
        ret: String,
        body: Body,
        /// E1
        #[serde(default)]
        attrs: Vec<String>,
        /// E2
        #[serde(default)]
        where_clauses: Vec<String>,
        /// E12 — unsafe fn.
        #[serde(default)]
        unsafe_: bool,
        /// E13 — async fn.
        #[serde(default)]
        async_: bool,
    },

    // ── Values ──────────────────────────────────────────────────────────────

    /// E5 — const item.  Unblocks S11 (const_solver).
    ///   Equation: emit = attrs vis "const" name ":" ty "=" value ";"
    Const {
        name: String,
        vis: Visibility,
        ty: String,
        value: String,
        /// E1
        #[serde(default)]
        attrs: Vec<String>,
    },

    /// E5 — static item.  Unblocks S11 (const_solver).
    ///   Equation: emit = attrs vis ["mut"] "static" name ":" ty "=" value ";"
    Static {
        name: String,
        vis: Visibility,
        ty: String,
        value: String,
        /// true -> `static mut`
        #[serde(default)]
        mutable: bool,
        /// E1
        #[serde(default)]
        attrs: Vec<String>,
    },

    // ── Use / Type helpers ──────────────────────────────────────────────────

    /// A synthetic `use` declaration injected by use_solver.
    Use { path: String, alias: Option<String> },

    TypeRef { name: String },

    TypeAlias {
        name: String,
        vis: Visibility,
        generics: Vec<GenericParam>,
        ty: String,
        /// E1
        #[serde(default)]
        attrs: Vec<String>,
        /// E2
        #[serde(default)]
        where_clauses: Vec<String>,
    },

    // ── Macros ──────────────────────────────────────────────────────────────

    /// E14 — macro invocation.  Unblocks S12 (macro_solver).
    ///   Equation: emit = path "!(" tokens ")"
    MacroCall {
        path: String,
        tokens: String,
    },
}

/// A node in the arena: identity + kind + source span (optional).
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Node {
    pub id: NodeId,
    pub kind: NodeKind,
    /// Original source span for round-trip emit, e.g. "src/lib.rs:12:5".
    pub span: Option<String>,
}
