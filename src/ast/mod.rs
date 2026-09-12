//! Modus Abstract Syntax Tree (AST) definitions.
//!
//! Canonical syntax notes (from docs/SPEC.md & AGENTS.md):
//! - `function`, never `fn`
//! - `if (cond)` - parens always required
//! - Generics parenthesized, never `<>`: `Result(T, E)`
//! - Strictly immutable variables: `let x: T = expr;` (no `mut` keyword)
//! - Pure functional control flow: no imperative loops (`while`, `for`); iteration is via tail recursion, closures, pattern matching
//! - Pure by default: effects use `perform`, errors use `check`

use std::fmt;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Default)]
pub struct Span {
    pub start: usize,
    pub end: usize,
}

impl Span {
    pub fn new(start: usize, end: usize) -> Self {
        Self { start, end }
    }
}

#[derive(Debug, Clone, PartialEq)]
pub struct Spanned<T> {
    pub node: T,
    pub span: Span,
}

impl<T> Spanned<T> {
    pub fn new(node: T, span: Span) -> Self {
        Self { node, span }
    }
}

#[derive(Debug, Clone, PartialEq, Default)]
pub struct Program {
    pub library: Option<Spanned<String>>,
    pub imports: Vec<Spanned<ImportDecl>>,
    pub exports: Vec<Spanned<ExportDecl>>,
    pub declarations: Vec<Spanned<Declaration>>,
}

#[derive(Debug, Clone, PartialEq)]
pub struct ImportSpecifier {
    pub name: String,
    pub alias: Option<String>,
}

#[derive(Debug, Clone, PartialEq)]
pub enum ImportClause {
    Named(Vec<ImportSpecifier>),
    Namespace(String),
    SideEffect,
}

#[derive(Debug, Clone, PartialEq)]
pub struct ImportDecl {
    pub clause: ImportClause,
    pub source: String,
}

#[derive(Debug, Clone, PartialEq)]
pub struct ExportSpecifier {
    pub name: String,
    pub alias: Option<String>,
}

#[derive(Debug, Clone, PartialEq)]
pub enum ExportDecl {
    Declaration(Spanned<Declaration>),
    Named {
        specifiers: Vec<ExportSpecifier>,
        source: Option<String>,
    },
    All {
        alias: Option<String>,
        source: String,
    },
}

#[derive(Debug, Clone, PartialEq)]
pub struct ExternBlock {
    pub abi: Option<String>,
    pub functions: Vec<Spanned<FunctionDecl>>,
}

#[derive(Debug, Clone, PartialEq)]
pub enum Declaration {
    Function(FunctionDecl),
    Type(TypeDecl),
    Trait(TraitDecl),
    Impl(ImplDecl),
    Extern(ExternBlock),
}

impl Declaration {
    pub fn is_exported(&self) -> bool {
        match self {
            Declaration::Function(f) => f.is_exported,
            Declaration::Type(t) => t.is_exported,
            Declaration::Trait(tr) => tr.is_exported,
            Declaration::Impl(_) => false,
            Declaration::Extern(ext) => ext.functions.iter().any(|f| f.node.is_exported),
        }
    }
}

#[derive(Debug, Clone, PartialEq)]
pub struct TypeParam {
    pub name: String,
    pub bound: Option<Spanned<Type>>,
}

#[derive(Debug, Clone, PartialEq)]
pub struct Param {
    pub name: String,
    pub ty: Spanned<Type>,
}

#[derive(Debug, Clone, PartialEq)]
pub enum FunctionBody {
    Block(Vec<Spanned<Stmt>>),
    Expr(Box<Spanned<Expr>>),
}

#[derive(Debug, Clone, PartialEq)]
pub struct FunctionDecl {
    pub name: String,
    pub type_params: Vec<TypeParam>,
    pub params: Vec<Param>,
    pub return_type: Option<Spanned<Type>>,
    pub body: Option<FunctionBody>,
    pub is_exported: bool,
}

#[derive(Debug, Clone, PartialEq)]
pub struct TypeDecl {
    pub name: String,
    pub type_params: Vec<TypeParam>,
    pub definition: Spanned<TypeDef>,
    pub is_exported: bool,
}

#[derive(Debug, Clone, PartialEq)]
pub enum TypeDef {
    Alias(Type),
    Union(Vec<VariantDecl>),
}

#[derive(Debug, Clone, PartialEq)]
pub struct VariantDecl {
    pub name: String,
    pub fields: Vec<Spanned<Type>>,
}

#[derive(Debug, Clone, PartialEq)]
pub struct TraitMember {
    pub name: String,
    pub params: Vec<Param>,
    pub return_type: Spanned<Type>,
}

#[derive(Debug, Clone, PartialEq)]
pub struct TraitDecl {
    pub name: String,
    pub type_params: Vec<TypeParam>,
    pub members: Vec<Spanned<TraitMember>>,
    pub is_exported: bool,
}

#[derive(Debug, Clone, PartialEq)]
pub struct ImplDecl {
    pub trait_name: String,
    pub target_type: Spanned<Type>,
    pub methods: Vec<Spanned<FunctionDecl>>,
}

#[derive(Debug, Clone, PartialEq)]
pub enum Stmt {
    Let {
        name: String,
        ty: Option<Spanned<Type>>,
        initializer: Spanned<Expr>,
    },
    Expr(Spanned<Expr>),
    Return(Option<Spanned<Expr>>),
}

#[derive(Debug, Clone, PartialEq)]
pub enum Expr {
    Literal(Literal),
    Ident(String),
    Binary {
        lhs: Box<Spanned<Expr>>,
        op: BinaryOp,
        rhs: Box<Spanned<Expr>>,
    },
    Unary {
        op: UnaryOp,
        expr: Box<Spanned<Expr>>,
    },
    Call {
        callee: Box<Spanned<Expr>>,
        args: Vec<Spanned<Expr>>,
    },
    MethodCall {
        receiver: Box<Spanned<Expr>>,
        method: String,
        args: Vec<Spanned<Expr>>,
    },
    FieldAccess {
        receiver: Box<Spanned<Expr>>,
        field: String,
    },
    Index {
        receiver: Box<Spanned<Expr>>,
        index: Box<Spanned<Expr>>,
    },
    Record(Vec<(String, Spanned<Expr>)>),
    RecordUpdate {
        base: Box<Spanned<Expr>>,
        fields: Vec<(String, Spanned<Expr>)>,
    },
    Array(Vec<Spanned<Expr>>),
    Closure {
        params: Vec<Param>,
        return_type: Option<Spanned<Type>>,
        body: FunctionBody,
    },
    If {
        condition: Box<Spanned<Expr>>,
        then_branch: Vec<Spanned<Stmt>>,
        else_branch: Option<ElseBranch>,
    },
    Match {
        expr: Box<Spanned<Expr>>,
        arms: Vec<MatchArm>,
    },
    Block(Vec<Spanned<Stmt>>),
}

#[derive(Debug, Clone, PartialEq)]
pub enum ElseBranch {
    Block(Vec<Spanned<Stmt>>),
    If(Box<Spanned<Expr>>),
}

#[derive(Debug, Clone, PartialEq)]
pub struct MatchArm {
    pub pattern: Spanned<Pattern>,
    pub body: MatchArmBody,
}

#[derive(Debug, Clone, PartialEq)]
pub enum MatchArmBody {
    Expr(Spanned<Expr>),
    Block(Vec<Spanned<Stmt>>),
}

#[derive(Debug, Clone, PartialEq)]
pub enum Literal {
    Int(i64),
    UInt(u64),
    Float(f64),
    String(String),
    Bool(bool),
    Unit,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum UnaryOp {
    Not,
    Neg,
    Perform,
    Check,
}

impl fmt::Display for UnaryOp {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Not => write!(f, "!"),
            Self::Neg => write!(f, "-"),
            Self::Perform => write!(f, "perform "),
            Self::Check => write!(f, "check "),
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum BinaryOp {
    Add,
    Sub,
    Mul,
    Div,
    Rem,
    Eq,
    NotEq,
    Lt,
    LtEq,
    Gt,
    GtEq,
    And,
    Or,
}

#[derive(Debug, Clone, PartialEq)]
pub enum Pattern {
    Wildcard,
    Ident(String),
    Literal(Literal),
    Variant {
        type_name: Option<String>,
        variant: String,
        patterns: Vec<Spanned<Pattern>>,
    },
    Record(Vec<(String, Option<Spanned<Pattern>>)>),
    Tuple(Vec<Spanned<Pattern>>),
}

#[derive(Debug, Clone, PartialEq)]
pub enum Type {
    Primitive(PrimitiveType),
    Generic {
        name: String,
        type_args: Vec<Spanned<Type>>,
    },
    Path(Vec<String>),
    Array(Box<Spanned<Type>>),
    Function {
        param_types: Vec<Spanned<Type>>,
        return_type: Box<Spanned<Type>>,
    },
    Record(Vec<(String, Spanned<Type>)>),
    Tuple(Vec<Spanned<Type>>),
    Unit,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum PrimitiveType {
    U8,
    U16,
    U32,
    U64,
    I8,
    I16,
    I32,
    I64,
    F32,
    F64,
    Bool,
    String,
    Void,
}
