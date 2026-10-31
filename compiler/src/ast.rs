//! Syntax tree of the extensions in a .c.c file: `type`, `schema`, and the enums schemas use.
//! Everything else in the file is C++, which `ccc` does not parse.

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
            "i32" | "int32" | "int" => Prim::I32,
            "i64" | "int64" => Prim::I64,
            "u8" | "uint8" => Prim::U8,
            "u16" | "uint16" => Prim::U16,
            "u32" | "uint32" => Prim::U32,
            "u64" | "uint64" => Prim::U64,
            "f64" | "double" => Prim::F64,
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
    /// `a::b::member`, split at the last `::`.
    Path(String, String),
    Field(Box<Expr>, String),
    Index(Box<Expr>, Box<Expr>),
    /// Callee is an `Ident` (function), `Path` (qualified function) or `Field` (method).
    Call(Box<Expr>, Vec<Expr>),
    Unary(UnOp, Box<Expr>),
    Binary(BinOp, Box<Expr>, Box<Expr>),
    /// `a < b <= c` keeps every operand once; emitted as `a < b && b <= c`.
    Compare(Box<Expr>, Vec<(CmpOp, Expr)>),
}

#[derive(Clone, Debug)]
pub struct Expr {
    pub kind: ExprKind,
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
pub struct SchemaDecl {
    /// `open schema`: unknown JSON fields are ignored instead of rejected.
    pub open: bool,
    pub name: Name,
    pub fields: Vec<Field>,
    pub invariants: Vec<Clause>,
    pub span: Span,
}

/// `enum Role { admin, member }`. An inline `enum { ... }` field becomes one of
/// these named `<Schema>_<field>`, declared just before the schema.
#[derive(Clone, Debug)]
pub struct EnumDecl {
    pub name: Name,
    pub variants: Vec<Name>,
    pub span: Span,
}
