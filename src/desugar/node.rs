//! Desugared AST definitions for Modus.
//!
//! The desugared AST eliminates syntactic sugar:
//! 1. `check` expressions are desugared into explicit Result match branching with early return.
//! 2. Record functional updates `{ ...base, field: val }` are lowered to full record literals.
//! 3. Function expression bodies `=> expr` are canonicalized to statement blocks.
//! 4. Unary operations no longer contain `Check`.
//! 5. Every expression carries its resolved semantic `Type`.

use crate::ast::{BinaryOp, Literal, Span, TraitDecl, TypeDecl, TypeParam};
use crate::typechecker::Type;

#[derive(Debug, Clone, PartialEq)]
pub struct DesugaredProgram {
    pub declarations: Vec<DesugaredDecl>,
}

#[derive(Debug, Clone, PartialEq)]
pub enum DesugaredDecl {
    Function(DesugaredFunction),
    Type(TypeDecl),
    Trait(TraitDecl),
    Impl(DesugaredImpl),
}

#[derive(Debug, Clone, PartialEq)]
pub struct DesugaredFunction {
    pub name: String,
    pub type_params: Vec<TypeParam>,
    pub params: Vec<(String, Type)>,
    pub return_type: Type,
    pub body: Vec<DesugaredStmt>,
    pub is_effectful: bool,
    pub span: Span,
}

#[derive(Debug, Clone, PartialEq)]
pub struct DesugaredImpl {
    pub trait_name: String,
    pub target_type: Type,
    pub methods: Vec<DesugaredFunction>,
    pub span: Span,
}

#[derive(Debug, Clone, PartialEq)]
pub enum DesugaredStmt {
    Let {
        name: String,
        ty: Type,
        initializer: DesugaredExpr,
        span: Span,
    },
    Expr(DesugaredExpr),
    Return(Option<DesugaredExpr>, Span),
}

#[derive(Debug, Clone, PartialEq)]
pub struct DesugaredExpr {
    pub kind: DesugaredExprKind,
    pub ty: Type,
    pub span: Span,
}

impl DesugaredExpr {
    pub fn new(kind: DesugaredExprKind, ty: Type, span: Span) -> Self {
        Self { kind, ty, span }
    }
}

#[derive(Debug, Clone, PartialEq)]
pub enum DesugaredExprKind {
    Literal(Literal),
    Ident(String),
    Binary {
        lhs: Box<DesugaredExpr>,
        op: BinaryOp,
        rhs: Box<DesugaredExpr>,
    },
    Unary {
        op: DesugaredUnaryOp,
        expr: Box<DesugaredExpr>,
    },
    Call {
        callee: Box<DesugaredExpr>,
        args: Vec<DesugaredExpr>,
    },
    MethodCall {
        receiver: Box<DesugaredExpr>,
        method: String,
        args: Vec<DesugaredExpr>,
    },
    FieldAccess {
        receiver: Box<DesugaredExpr>,
        field: String,
    },
    Index {
        receiver: Box<DesugaredExpr>,
        index: Box<DesugaredExpr>,
    },
    /// Canonical structural record literal (all RecordUpdate lowered here)
    Record(Vec<(String, DesugaredExpr)>),
    Array(Vec<DesugaredExpr>),
    Closure {
        params: Vec<(String, Type)>,
        return_type: Type,
        body: Vec<DesugaredStmt>,
    },
    If {
        condition: Box<DesugaredExpr>,
        then_branch: Vec<DesugaredStmt>,
        else_branch: Option<Vec<DesugaredStmt>>,
    },
    Match {
        expr: Box<DesugaredExpr>,
        arms: Vec<DesugaredMatchArm>,
    },
    Block(Vec<DesugaredStmt>),
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum DesugaredUnaryOp {
    Not,
    Neg,
    Perform,
}

#[derive(Debug, Clone, PartialEq)]
pub struct DesugaredMatchArm {
    pub pattern: DesugaredPattern,
    pub body: Vec<DesugaredStmt>,
}

#[derive(Debug, Clone, PartialEq)]
pub enum DesugaredPattern {
    Wildcard,
    Ident(String),
    Literal(Literal),
    Variant {
        type_name: Option<String>,
        variant: String,
        patterns: Vec<DesugaredPattern>,
    },
    Record(Vec<(String, Option<DesugaredPattern>)>),
    Tuple(Vec<DesugaredPattern>),
}
