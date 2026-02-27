// canon/src/node.rs

use serde::{Deserialize, Serialize};

/// Stable node index — same space as ModelIR NodeId, u32.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub struct CanonId(pub u32);

/// Interned string index into CanonIR::name_intern.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub struct NameId(pub u32);

/// Interned path index into CanonIR::path_intern.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub struct PathId(pub u32);

// ── Flags bitfield ───────────────────────────────────────────────────────────
pub mod flags {
    pub const ASYNC: u32 = 1 << 0; // async fn
    pub const UNSAFE: u32 = 1 << 1; // unsafe fn / trait / impl
    pub const MUT: u32 = 1 << 2; // mut self / static mut
    pub const PUB: u32 = 1 << 3; // pub visibility
    pub const GLOB: u32 = 1 << 4; // use foo::*
    pub const INLINE: u32 = 1 << 5; // inline module
    pub const PUB_CRATE: u32 = 1 << 6;
    pub const PUB_SUPER: u32 = 1 << 7;
    pub const PUB_IN: u32 = 1 << 8;
}

// ── Primitive type enum (no strings) ─────────────────────────────────────────
#[derive(Debug, Clone, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub enum PrimTy {
    Bool,
    Char,
    Str,
    U8,
    U16,
    U32,
    U64,
    U128,
    Usize,
    I8,
    I16,
    I32,
    I64,
    I128,
    Isize,
    F32,
    F64,
    Unit,  // ()
    Never, // !
}

// ── Type node kinds ───────────────────────────────────────────────────────────
// TypeKind lives inside CanonNodeKind::Type { kind }
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub enum TypeKind {
    /// Primitive — no node reference needed.
    Primitive(PrimTy),
    /// Named ADT — points to Struct/Enum/Trait node.
    Adt(CanonId),
    /// &T or &mut T.  lifetime is CanonId of a Lifetime node, or None.
    Ref { lifetime: Option<CanonId>, inner: CanonId, mutable: bool },
    /// *const T / *mut T
    RawPtr { inner: CanonId, mutable: bool },
    /// [T; N]
    Array { inner: CanonId, len: u64 },
    /// [T]
    Slice(CanonId),
    /// (T, U, ...)
    Tuple(Vec<CanonId>),
    /// fn(T) -> U — points to FnSig node
    FnPtr(CanonId),
    /// impl Trait — points to Trait node
    ImplTrait(CanonId),
    /// dyn Trait — points to Trait node
    DynTrait(CanonId),
    /// Generic param — e.g. `T`.  Stored as NameId.
    Param(NameId),
    /// Generic application root + concrete args.
    Applied { base: CanonId, args: Vec<CanonId> },
    /// External / unresolved type (escape hatch, e.g. `std::collections::HashMap`)
    /// PathId points into path_intern.
    Extern(PathId),
    /// Known by path but not yet linked to a local definition.
    Unresolved(PathId),

    /// Unresolved type reference by name (e.g. from TypeRef nodes in ModelIR).
    /// NameId points into name_intern.
    TypeRef { name_id: NameId },
}

// ── CFG op kinds (replaces Stmt + Terminator strings) ────────────────────────
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub enum CfgOp {
    /// let lhs: ty = rhs
    Let { lhs: CanonId, ty: CanonId, rhs: Option<CanonId> },
    /// lhs = rhs  (assign)
    Assign { lhs: CanonId, rhs: CanonId },
    /// return value
    Return(Option<CanonId>),
    /// fn_id(args) -> dest
    Call { func: CanonId, args: Vec<CanonId>, dest: Option<CanonId> },
    /// base.field
    FieldAccess { base: CanonId, field: NameId, dest: Option<CanonId> },
    /// receiver.method(args)
    MethodCall { receiver: CanonId, method: NameId, args: Vec<CanonId>, dest: Option<CanonId> },
    /// base[idx]
    Index { base: CanonId, idx: CanonId, dest: Option<CanonId> },
    /// closure literal
    Closure { sig_id: CanonId, body_id: CanonId },
    /// struct literal
    StructLit { ty: CanonId, fields: Vec<(NameId, CanonId)>, dest: Option<CanonId> },
    /// match expression
    Match { scrutinee: CanonId, arms: Vec<CanonId> }, // -> MatchArm nodes
    /// if cond goto true_bb else false_bb
    Branch { cond: CanonId, true_bb: u32, false_bb: u32 },
    /// goto bb
    Goto(u32),
    /// unreachable
    Unreachable,
    /// bare expression statement (interned source string, escape hatch)
    Expr(CanonId),
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub enum PatternKind {
    Wildcard,
    Binding {
        name_id: NameId,
        mutable: bool,
    },
    Tuple(Vec<CanonId>), // -> Pattern nodes
    Struct {
        ty: CanonId,
        fields: Vec<(NameId, CanonId)>,
    },
    TupleStruct {
        ty: CanonId,
        fields: Vec<CanonId>,
    },
    Literal(NameId),
    Or(Vec<CanonId>), // -> Pattern nodes
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub enum WherePredKind {
    TypeBound {
        ty: CanonId,
        bounds: Vec<CanonId>,
    },
    LifetimeBound {
        lifetime: CanonId,
        bounds: Vec<CanonId>,
    },
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct DependencySpec {
    pub crate_root: PathId,
    pub package_name: Option<NameId>,
}

// ── The canonical node kind ───────────────────────────────────────────────────
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub enum CanonNodeKind {
    // ── Crate ────────────────────────────────────────────────────────────────
    Crate {
        name_id: NameId,
        #[serde(default)]
        cargo_name: Option<NameId>,
        edition: u32, // 2015 | 2018 | 2021
        #[serde(default)]
        dependencies: Vec<PathId>,
        #[serde(default)]
        dependency_packages: Vec<Option<NameId>>,
        #[serde(default)]
        declared_dependencies: Vec<DependencySpec>,
    },

    // ── Module ───────────────────────────────────────────────────────────────
    Module {
        path_id: PathId,
        flags: u32, // INLINE | PUB | PUB_CRATE | PUB_SUPER
    },

    // ── Type definitions ─────────────────────────────────────────────────────
    Struct {
        name_id: NameId,
        generics: Vec<CanonId>, // → GenericParam nodes
        fields: Vec<CanonId>,   // → Field nodes
        derives: Vec<CanonId>,  // → Trait nodes
        attrs: Vec<CanonId>,    // → Attr nodes
        flags: u32,             // UNSAFE (for repr)
        struct_kind: u8,        // 0=Named 1=Tuple 2=Unit
    },

    Enum {
        name_id: NameId,
        generics: Vec<CanonId>,
        variants: Vec<CanonId>, // → Variant nodes
        derives: Vec<CanonId>,
        attrs: Vec<CanonId>,
        flags: u32,
    },

    // ── Trait / Impl ─────────────────────────────────────────────────────────
    Trait {
        name_id: NameId,
        generics: Vec<CanonId>,
        methods: Vec<CanonId>, // → Fn nodes (trait methods)
        attrs: Vec<CanonId>,
        flags: u32, // UNSAFE
    },

    AssocType {
        name_id: NameId,
        generics: Vec<CanonId>,
        default_ty: Option<CanonId>, // None = abstract, Some = default/provided
        flags: u32,
    },

    AssocConst {
        name_id: NameId,
        ty: CanonId,
        default_value: Option<NameId>, // interned literal, None = abstract
        flags: u32,
    },

    Impl {
        for_ty: CanonId,            // → Struct/Enum/TypeAlias node
        for_trait: Option<CanonId>, // → Trait node
        generics: Vec<CanonId>,
        attrs: Vec<CanonId>,
        flags: u32, // UNSAFE
    },

    // ── Functions ────────────────────────────────────────────────────────────
    Fn {
        name_id: NameId,
        sig_id: CanonId,       // → FnSig node
        body: Option<CanonId>, // → Body node, None = extern/trait decl
        attrs: Vec<CanonId>,
        flags: u32, // ASYNC | UNSAFE | PUB | ...
    },

    // ── Signature (deduplicated) ──────────────────────────────────────────────
    FnSig {
        generics: Vec<CanonId>,      // → GenericParam nodes
        params: Vec<CanonId>,        // → Param nodes
        ret: CanonId,                // → Type node
        where_clauses: Vec<CanonId>, // → WherePred nodes
    },

    // ── Type node (content-addressed, deduplicated) ───────────────────────────
    Type {
        kind: TypeKind,
    },

    // ── Sub-nodes ─────────────────────────────────────────────────────────────
    Field {
        name_id: Option<NameId>, // None = tuple field
        ty: CanonId,             // → Type node
        flags: u32,              // PUB | PUB_CRATE | ...
    },

    Param {
        name_id: NameId,
        ty: CanonId, // → Type node
        flags: u32,  // MUT | (is_self encoded as name_id == "self")
    },

    GenericParam {
        name_id: NameId,
        bounds: Vec<CanonId>, // → Trait nodes
        is_lifetime: bool,
        default_ty: Option<CanonId>, // → Type node  (E11)
    },

    WherePred {
        kind: WherePredKind,
    },

    Variant {
        name_id: NameId,
        fields: Vec<CanonId>, // → Field nodes
    },

    Attr {
        path_id: PathId,        // e.g. "derive", "cfg", "allow"
        tokens: Option<NameId>, // raw token string if needed (E1 escape)
    },

    Lifetime {
        name_id: NameId, // "'a", "'static"
    },

    // ── Values ───────────────────────────────────────────────────────────────
    Const {
        name_id: NameId,
        ty: CanonId,      // → Type node
        value_id: NameId, // interned literal string (only string allowed)
        attrs: Vec<CanonId>,
        flags: u32, // PUB | ...
    },

    Static {
        name_id: NameId,
        ty: CanonId,
        value_id: NameId,
        attrs: Vec<CanonId>,
        flags: u32, // MUT | PUB | ...
    },

    // ── Use / Extern / Alias ──────────────────────────────────────────────────
    Use {
        path_id: PathId,
        alias: Option<NameId>,
        flags: u32, // GLOB | PUB | ...
        #[serde(default)]
        target: Option<CanonId>,
    },

    ExternCrate {
        name_id: NameId,
        alias: Option<NameId>,
        flags: u32, // PUB
    },

    TypeAlias {
        name_id: NameId,
        generics: Vec<CanonId>,
        ty: CanonId, // → Type node
        attrs: Vec<CanonId>,
        flags: u32,
    },

    // ── Macro ────────────────────────────────────────────────────────────────
    /// Unresolved type reference by name (from ModelIR TypeRef nodes).
    TypeRef {
        name_id: NameId,
    },

    MacroCall {
        path_id: PathId,
        tokens_id: NameId, // interned token string (unavoidable escape)
    },

    PathRef {
        path_id: PathId, // fully qualified external path
    },

    // ── CFG body ─────────────────────────────────────────────────────────────
    Body {
        blocks: Vec<CanonId>, // → BasicBlock nodes, in order
    },

    BasicBlock {
        ops: Vec<CfgOp>, // structured ops, no Raw
        /// Index of next block (for Goto/fallthrough).
        /// Branch terminator encodes both successors inside CfgOp::Branch.
        next: Option<u32>,
    },

    MatchArm {
        pattern: CanonId, // -> Pattern node
        guard: Option<CanonId>,
        body: CanonId, // -> BasicBlock node
    },

    Pattern {
        kind: PatternKind,
    },

    VisPath {
        flags: u32, // PUB_IN
        path_id: PathId, // in crate::some::module
    },

    // ── Local variable (inside body) ──────────────────────────────────────────
    Local {
        name_id: NameId,
        ty: CanonId,
        flags: u32, // MUT
    },
}
