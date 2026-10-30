//! Syntax tree of a .c.c file.

/// Byte range in the source file.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct Span {
    pub lo: usize,
    pub hi: usize,
}

impl Span {
    pub fn to(self, other: Span) -> Span {
        Span { lo: self.lo, hi: other.hi }
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Prim {
    I32,
    I64,
    U8,
    U16,
    U32,
    U64,
    F64,
    Bool,
    String,
}

impl Prim {
    pub fn from_name(name: &str) -> Option<Prim> {
        Some(match name {
            "i32" => Prim::I32,
            "i64" | "int" => Prim::I64,
            "u8" => Prim::U8,
            "u16" => Prim::U16,
            "u32" => Prim::U32,
            "u64" => Prim::U64,
            "f64" => Prim::F64,
            "bool" => Prim::Bool,
            "string" | "str" => Prim::String,
            _ => return None,
        })
    }

    /// Types usable as a C++ non-type template parameter, so literals can be checked at compile time.
    pub fn is_integral(self) -> bool {
        !matches!(self, Prim::F64 | Prim::String)
    }
}

#[derive(Clone, Debug, PartialEq)]
pub enum TypeKind {
    Prim(Prim),
    Json,
    Named(String),
    Vec(Box<Type>),
    Raw(Box<Type>),
    Optional(Box<Type>),
}

#[derive(Clone, Debug)]
pub struct Type {
    pub kind: TypeKind,
    pub span: Span,
}

impl PartialEq for Type {
    fn eq(&self, other: &Type) -> bool {
        self.kind == other.kind
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum UnOp {
    Neg,
    Not,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum BinOp {
    Add,
    Sub,
    Mul,
    Div,
    Rem,
    And,
    Or,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum CmpOp {
    Eq,
    Ne,
    Lt,
    Le,
    Gt,
    Ge,
}

#[derive(Clone, Debug)]
pub enum ExprKind {
    Int(String),
    Float(String),
    /// `lit` is the C++ spelling of the literal (escapes kept as written).
    Str {
        lit: String,
    },
    Bool(bool),
    None,
    List(Vec<Expr>),
    Ident(String),
    SelfValue,
    /// `Type::member`
    Path(String, String),
    Field(Box<Expr>, String),
    Index(Box<Expr>, Box<Expr>),
    /// Callee is an `Ident` (function), `Path` (static member) or `Field` (method).
    Call(Box<Expr>, Vec<Expr>),
    Unary(UnOp, Box<Expr>),
    Binary(BinOp, Box<Expr>, Box<Expr>),
    /// `a < b <= c` keeps every operand once; emitted as `a < b && b <= c`.
    Compare(Box<Expr>, Vec<(CmpOp, Expr)>),
    Old(Box<Expr>),
    Try(Box<Expr>),
}

#[derive(Clone, Debug)]
pub struct Expr {
    pub kind: ExprKind,
    pub span: Span,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum AssignOp {
    Set,
    Add,
    Sub,
    Mul,
    Div,
    Rem,
}

#[derive(Clone, Debug)]
pub enum ForIter {
    Range(Expr, Expr),
    Each(Expr),
}

#[derive(Clone, Debug)]
pub enum StmtKind {
    Let { name: String, ty: Option<Type>, init: Expr },
    Assign { target: Expr, op: AssignOp, value: Expr },
    Expr(Expr),
    Return(Option<Expr>),
    If { cond: Expr, then: Block, els: Option<Block> },
    For { var: String, iter: ForIter, body: Block },
    While { cond: Expr, body: Block },
    Block(Block),
    Cpp(String),
}

#[derive(Clone, Debug)]
pub struct Stmt {
    pub kind: StmtKind,
    pub span: Span,
}

#[derive(Clone, Debug)]
pub struct Block {
    pub stmts: Vec<Stmt>,
    pub span: Span,
}

#[derive(Clone, Debug)]
pub struct Name {
    pub text: String,
    pub span: Span,
}

/// `type Port = u16 where ... matches ...;`
#[derive(Clone, Debug)]
pub struct TypeDecl {
    pub name: Name,
    pub base: Type,
    pub cond: Option<Expr>,
    pub matches: Option<Name>,
    pub span: Span,
}

#[derive(Clone, Debug)]
pub struct Field {
    pub name: Name,
    pub ty: Type,
    pub cond: Option<Expr>,
    pub matches: Option<Name>,
    pub transform: Option<Name>,
    pub default: Option<Expr>,
    pub span: Span,
}

/// `pre(...)`, `invariant(...)`. `label` is the decoded string.
#[derive(Clone, Debug)]
pub struct Clause {
    pub cond: Expr,
    pub label: Option<String>,
    pub span: Span,
}

#[derive(Clone, Debug)]
pub struct PostClause {
    pub binder: Option<Name>,
    pub cond: Expr,
    pub label: Option<String>,
    pub span: Span,
}

#[derive(Clone, Debug)]
pub struct SchemaDecl {
    pub name: Name,
    pub fields: Vec<Field>,
    pub invariants: Vec<Clause>,
    pub span: Span,
}

#[derive(Clone, Debug)]
pub struct Param {
    pub ty: Type,
    pub by_ref: bool,
    pub name: Name,
}

#[derive(Clone, Debug)]
pub struct FnDecl {
    pub name: Name,
    pub is_contract: bool,
    pub params: Vec<Param>,
    pub ret: Option<Type>,
    /// `None` when `effects(...)` is omitted.
    pub effects: Option<Vec<Name>>,
    pub pres: Vec<Clause>,
    pub posts: Vec<PostClause>,
    /// `None` for a declaration of a function implemented in C++.
    pub body: Option<Block>,
    pub span: Span,
}

#[derive(Clone, Debug)]
pub enum Item {
    Type(TypeDecl),
    Schema(SchemaDecl),
    Fn(FnDecl),
    Cpp { code: String, span: Span },
}

#[derive(Clone, Debug)]
pub struct Program {
    pub items: Vec<Item>,
}
