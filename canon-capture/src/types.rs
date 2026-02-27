use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub struct NodeId(pub u32);

impl NodeId {
    #[inline]
    pub fn index(self) -> usize {
        self.0 as usize
    }
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, Default)]
pub enum Visibility {
    Public,
    PubCrate,
    PubSuper,
    PubIn(String),
    #[default]
    Private,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct GenericParam {
    pub name: String,
    pub bounds: Vec<String>,
    pub is_lifetime: bool,
    pub default_ty: Option<TypeExpr>,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
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

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Field {
    pub name: Option<String>,
    pub ty: TypeExpr,
    pub vis: Visibility,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Param {
    pub name: String,
    pub ty: TypeExpr,
    pub is_self: bool,
    pub mutable: bool,
    pub lifetime: Option<String>,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct EnumVariant {
    pub name: String,
    pub fields: Vec<Field>,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub enum Stmt {
    Let { pat: String, ty: Option<TypeExpr>, init: Option<String> },
    Assign { lhs: String, rhs: String },
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

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub enum Terminator {
    Goto(u32),
    Branch { cond: String, true_bb: u32, false_bb: u32 },
    Return,
    Unreachable,
    None,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct BasicBlock {
    pub stmts: Vec<Stmt>,
    pub terminator: Terminator,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub enum Body {
    None,
    Blocks(Vec<BasicBlock>),
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct TraitMethod {
    pub name: String,
    pub vis: Visibility,
    pub generics: Vec<GenericParam>,
    pub params: Vec<Param>,
    pub ret: TypeExpr,
    pub body: Body,
    #[serde(default)]
    pub attrs: Vec<String>,
    #[serde(default)]
    pub where_clauses: Vec<String>,
    #[serde(default)]
    pub unsafe_: bool,
    #[serde(default)]
    pub async_: bool,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, Default)]
pub enum StructKind {
    #[default]
    Named,
    Tuple,
    Unit,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub enum NodeKind {
    Crate {
        name: String,
        edition: String,
    },
    Module {
        path: String,
        file: String,
        #[serde(default)]
        vis: Visibility,
        #[serde(default)]
        inline: bool,
    },
    Struct {
        name: String,
        vis: Visibility,
        generics: Vec<GenericParam>,
        fields: Vec<Field>,
        derives: Vec<String>,
        #[serde(default)]
        attrs: Vec<String>,
        #[serde(default)]
        where_clauses: Vec<String>,
        #[serde(default)]
        struct_kind: StructKind,
    },
    Enum {
        name: String,
        vis: Visibility,
        generics: Vec<GenericParam>,
        variants: Vec<EnumVariant>,
        derives: Vec<String>,
        #[serde(default)]
        attrs: Vec<String>,
        #[serde(default)]
        where_clauses: Vec<String>,
    },
    Trait {
        name: String,
        vis: Visibility,
        generics: Vec<GenericParam>,
        methods: Vec<TraitMethod>,
        #[serde(default)]
        attrs: Vec<String>,
        #[serde(default)]
        where_clauses: Vec<String>,
        #[serde(default)]
        unsafe_: bool,
    },
    Impl {
        for_struct: TypeExpr,
        for_trait: Option<TypeExpr>,
        generics: Vec<GenericParam>,
        #[serde(default)]
        attrs: Vec<String>,
        #[serde(default)]
        where_clauses: Vec<String>,
        #[serde(default)]
        unsafe_: bool,
    },
    Function {
        name: String,
        vis: Visibility,
        generics: Vec<GenericParam>,
        params: Vec<Param>,
        ret: TypeExpr,
        body: Body,
        #[serde(default)]
        attrs: Vec<String>,
        #[serde(default)]
        where_clauses: Vec<String>,
        #[serde(default)]
        unsafe_: bool,
        #[serde(default)]
        async_: bool,
    },
    Method {
        name: String,
        vis: Visibility,
        generics: Vec<GenericParam>,
        params: Vec<Param>,
        ret: TypeExpr,
        body: Body,
        #[serde(default)]
        attrs: Vec<String>,
        #[serde(default)]
        where_clauses: Vec<String>,
        #[serde(default)]
        unsafe_: bool,
        #[serde(default)]
        async_: bool,
    },
    AssocType {
        name: String,
        vis: Visibility,
        generics: Vec<GenericParam>,
        default_ty: Option<TypeExpr>,
        #[serde(default)]
        attrs: Vec<String>,
        #[serde(default)]
        where_clauses: Vec<String>,
    },
    AssocConst {
        name: String,
        vis: Visibility,
        ty: TypeExpr,
        default_value: Option<String>,
        #[serde(default)]
        attrs: Vec<String>,
    },
    Const {
        name: String,
        vis: Visibility,
        ty: TypeExpr,
        value: String,
        #[serde(default)]
        attrs: Vec<String>,
    },
    Static {
        name: String,
        vis: Visibility,
        ty: TypeExpr,
        value: String,
        #[serde(default)]
        mutable: bool,
        #[serde(default)]
        attrs: Vec<String>,
    },
    Use {
        #[serde(default)]
        vis: Visibility,
        path: String,
        alias: Option<String>,
        #[serde(default)]
        glob: bool,
    },
    TypeRef {
        name: String,
    },
    TypeAlias {
        name: String,
        vis: Visibility,
        generics: Vec<GenericParam>,
        ty: TypeExpr,
        #[serde(default)]
        attrs: Vec<String>,
        #[serde(default)]
        where_clauses: Vec<String>,
    },
    Lifetime {
        name: String,
    },
    ExternCrate {
        name: String,
        alias: Option<String>,
        #[serde(default)]
        vis: Visibility,
    },
    MacroCall {
        path: String,
        tokens: String,
    },
    PathRef {
        path: String,
    },
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Node {
    pub id: NodeId,
    pub kind: NodeKind,
    pub span: Option<String>,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
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

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct EdgeHint {
    pub src: u32,
    pub dst: u32,
    pub kind: EdgeKind,
}
