use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub struct NodeId(pub u32);

impl NodeId {
    pub fn index(&self) -> usize {
        self.0 as usize
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Node {
    pub id: NodeId,
    pub kind: NodeKind,
    pub span: Option<String>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub enum Visibility {
    Public,
    PubCrate,
    PubSuper,
    PubIn(String),
    Private,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum StructKind {
    Named,
    Tuple,
    Unit,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum PrimType {
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
    Unit,
    Never,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub enum TypeExpr {
    Primitive(PrimType),
    Ref {
        lifetime: Option<String>,
        inner: Box<TypeExpr>,
        mutable: bool,
    },
    RawPtr {
        inner: Box<TypeExpr>,
        mutable: bool,
    },
    Array {
        inner: Box<TypeExpr>,
        len: Option<u64>,
    },
    Slice(Box<TypeExpr>),
    Tuple(Vec<TypeExpr>),
    FnPtr {
        params: Vec<TypeExpr>,
        ret: Box<TypeExpr>,
    },
    Param(String),
    DynTrait(String),
    ImplTrait(String),
    AppliedPath {
        base: String,
        args: Vec<TypeExpr>,
    },
    Path(String),
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct GenericParam {
    pub name: String,
    pub bounds: Vec<String>,
    pub is_lifetime: bool,
    pub default_ty: Option<TypeExpr>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Param {
    pub name: String,
    pub ty: TypeExpr,
    pub is_self: bool,
    pub mutable: bool,
    pub lifetime: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Field {
    pub name: Option<String>,
    pub ty: TypeExpr,
    pub vis: Visibility,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct EnumVariant {
    pub name: String,
    pub fields: Vec<Field>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct TraitMethod {
    pub name: String,
    pub vis: Visibility,
    pub generics: Vec<GenericParam>,
    pub params: Vec<Param>,
    pub ret: TypeExpr,
    pub body: Body,
    pub attrs: Vec<String>,
    pub where_clauses: Vec<String>,
    pub unsafe_: bool,
    pub async_: bool,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub enum NodeKind {
    Crate {
        name: String,
        edition: String,
    },
    Module {
        path: String,
        file: String,
        vis: Visibility,
        inline: bool,
    },
    Struct {
        name: String,
        vis: Visibility,
        generics: Vec<GenericParam>,
        fields: Vec<Field>,
        derives: Vec<String>,
        attrs: Vec<String>,
        where_clauses: Vec<String>,
        struct_kind: StructKind,
    },
    Enum {
        name: String,
        vis: Visibility,
        generics: Vec<GenericParam>,
        variants: Vec<EnumVariant>,
        derives: Vec<String>,
        attrs: Vec<String>,
        where_clauses: Vec<String>,
    },
    Trait {
        name: String,
        vis: Visibility,
        generics: Vec<GenericParam>,
        methods: Vec<TraitMethod>,
        attrs: Vec<String>,
        where_clauses: Vec<String>,
        unsafe_: bool,
    },
    Impl {
        for_struct: TypeExpr,
        for_trait: Option<TypeExpr>,
        generics: Vec<GenericParam>,
        attrs: Vec<String>,
        where_clauses: Vec<String>,
        unsafe_: bool,
    },
    AssocType {
        name: String,
        vis: Visibility,
        generics: Vec<GenericParam>,
        default_ty: Option<TypeExpr>,
        attrs: Vec<String>,
        where_clauses: Vec<String>,
    },
    AssocConst {
        name: String,
        vis: Visibility,
        ty: TypeExpr,
        default_value: Option<String>,
        attrs: Vec<String>,
    },
    Function {
        name: String,
        vis: Visibility,
        generics: Vec<GenericParam>,
        params: Vec<Param>,
        ret: TypeExpr,
        body: Body,
        attrs: Vec<String>,
        where_clauses: Vec<String>,
        unsafe_: bool,
        async_: bool,
    },
    Method {
        name: String,
        vis: Visibility,
        generics: Vec<GenericParam>,
        params: Vec<Param>,
        ret: TypeExpr,
        body: Body,
        attrs: Vec<String>,
        where_clauses: Vec<String>,
        unsafe_: bool,
        async_: bool,
    },
    Const {
        name: String,
        vis: Visibility,
        ty: TypeExpr,
        value: String,
        attrs: Vec<String>,
    },
    Static {
        name: String,
        vis: Visibility,
        ty: TypeExpr,
        value: String,
        mutable: bool,
        attrs: Vec<String>,
    },
    Use {
        vis: Visibility,
        path: String,
        alias: Option<String>,
        glob: bool,
    },
    ExternCrate {
        name: String,
        alias: Option<String>,
        vis: Visibility,
    },
    TypeAlias {
        name: String,
        vis: Visibility,
        generics: Vec<GenericParam>,
        ty: TypeExpr,
        attrs: Vec<String>,
        where_clauses: Vec<String>,
    },
    TypeRef {
        name: String,
    },
    Lifetime {
        name: String,
    },
    MacroCall {
        path: String,
        tokens: String,
    },
    PathRef {
        path: String,
    },
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub enum EdgeKind {
    Renames,
    Resolves,
    ImplRef,
    TypeOf,
    TypeUnifies,
    ImplTrait,
    DynTrait,
    Calls,
    Contains,
    ImplFor,
    CfgEdge,
    CfgBranch { label: String },
    Outlives,
    ConstDep,
    Expands,
    AssocItem,
    Instantiates,
    Reexports,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct EdgeHint {
    pub src: u32,
    pub dst: u32,
    pub kind: EdgeKind,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub enum Stmt {
    Let {
        pat: String,
        ty: Option<TypeExpr>,
        init: Option<String>,
    },
    Assign {
        lhs: String,
        rhs: String,
    },
    Expr(String),
    Call {
        func: String,
        args: Vec<String>,
        dest: Option<String>,
    },
    FieldAccess {
        base: String,
        field: String,
        dest: Option<String>,
    },
    MethodCall {
        receiver: String,
        method: String,
        args: Vec<String>,
        dest: Option<String>,
    },
    StructLit {
        ty: TypeExpr,
        fields: Vec<(String, String)>,
        dest: Option<String>,
    },
    Match {
        dest: Option<String>,
    },
    Return(Option<String>),
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub enum Terminator {
    Goto(u32),
    Branch { cond: String, true_bb: u32, false_bb: u32 },
    Switch { discr: String, targets: Vec<(String, u32)>, otherwise: Option<u32> },
    Return,
    Unreachable,
    None,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct BasicBlock {
    pub stmts: Vec<Stmt>,
    pub terminator: Terminator,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub enum Body {
    None,
    Blocks(Vec<BasicBlock>),
}
