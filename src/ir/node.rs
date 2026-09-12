//! A-Normal Form (ANF) Intermediate Representation definitions for Modus.
//!
//! In ANF:
//! - All intermediate computations are bound to named variables (`let _tN = ...`).
//! - Function arguments, binary/unary operands, branch conditions, etc., are atomic values (`Atom`).
//! - Control flow branches (`If`, `Match`) evaluate blocks that terminate in atoms or returns.
//! - Tail calls (direct recursion) are explicitly tagged for LLVM `musttail`.
//! - Reference count operations (`inc_ref`, `dec_ref`) and FBIP reuse are natively represented.

use crate::ast::{BinaryOp, Literal, Span, TraitDecl, TypeDecl, TypeParam};
use crate::desugar::{DesugaredPattern, DesugaredUnaryOp};
use crate::typechecker::Type;
use std::collections::HashSet;

/// An atomic value in ANF: immediate operand requiring no evaluation.
#[derive(Debug, Clone, PartialEq)]
pub enum Atom {
    /// A reference to a variable name
    Var(String),
    /// A literal constant value
    Literal(Literal),
}

impl Atom {
    pub fn var(name: impl Into<String>) -> Self {
        Atom::Var(name.into())
    }

    pub fn as_var(&self) -> Option<&str> {
        match self {
            Atom::Var(v) => Some(v.as_str()),
            Atom::Literal(_) => None,
        }
    }

    pub fn is_var(&self) -> bool {
        matches!(self, Atom::Var(_))
    }
}

/// An ANF expression representing a computation that produces a value.
/// All sub-operands are atomic (`Atom`).
#[derive(Debug, Clone, PartialEq)]
pub enum AnfExpr {
    /// A trivial atomic value
    Atom(Atom),
    /// Binary operation on two atomic operands: `lhs op rhs`
    Binary { op: BinaryOp, lhs: Atom, rhs: Atom },
    /// Unary operation on an atomic operand: `op operand`
    Unary { op: DesugaredUnaryOp, operand: Atom },
    /// Function call where callee and all arguments are atoms
    Call { callee: Atom, args: Vec<Atom> },
    /// Method call: `receiver.method(args...)`
    MethodCall {
        receiver: Atom,
        method: String,
        args: Vec<Atom>,
    },
    /// Record field access: `receiver.field`
    FieldAccess { receiver: Atom, field: String },
    /// Tuple index access: `receiver.0`
    TupleAccess { receiver: Atom, index: usize },
    /// Array index access: `receiver[index]`
    Index { receiver: Atom, index: Atom },
    /// Structural record constructor: `{ field1: atom1, ... }`
    Record { fields: Vec<(String, Atom)> },
    /// Array constructor: `[atom1, atom2, ...]`
    Array { elements: Vec<Atom> },
    /// Tuple constructor: `(atom1, atom2, ...)`
    Tuple { elements: Vec<Atom> },
    /// Discriminated union variant constructor: `Variant(atom1, ...)`
    Variant {
        type_name: Option<String>,
        variant: String,
        args: Vec<Atom>,
    },
    /// Closure expression before closure conversion
    Closure {
        params: Vec<(String, Type)>,
        return_type: Type,
        body: AnfBlock,
    },
    /// Closure instantiation after closure conversion: `MakeClosure(fn_ptr, env_record)`
    MakeClosure { fn_name: String, env: Option<Atom> },
    /// Invocation of a closure object: `closure(args...)`
    CallClosure { closure: Atom, args: Vec<Atom> },
    /// Branch expression: `if (cond) { then } else { else }`
    If {
        cond: Atom,
        then_branch: Box<AnfBlock>,
        else_branch: Box<AnfBlock>,
    },
    /// Pattern match expression
    Match {
        scrutinee: Atom,
        arms: Vec<AnfMatchArm>,
    },
    /// Functional But In-Place (FBIP) record buffer reuse:
    /// Reuses `base` record buffer if unique (`rc == 1`), updating specified fields.
    ReuseRecord {
        base: Atom,
        fields: Vec<(String, Atom)>,
    },
    /// Reference count uniqueness check: `rc(base) == 1`
    IsUnique(Atom),
}

impl AnfExpr {
    /// Collects all variable occurrences (with duplicates) in this ANF expression.
    pub fn var_occurrences(&self) -> Vec<String> {
        let mut vars = Vec::new();
        match self {
            AnfExpr::Atom(a) => {
                if let Some(v) = a.as_var() {
                    vars.push(v.to_string());
                }
            }
            AnfExpr::Binary { lhs, rhs, .. } => {
                if let Some(v) = lhs.as_var() {
                    vars.push(v.to_string());
                }
                if let Some(v) = rhs.as_var() {
                    vars.push(v.to_string());
                }
            }
            AnfExpr::Unary { operand, .. } => {
                if let Some(v) = operand.as_var() {
                    vars.push(v.to_string());
                }
            }
            AnfExpr::Call { callee, args } => {
                if let Some(v) = callee.as_var() {
                    vars.push(v.to_string());
                }
                for arg in args {
                    if let Some(v) = arg.as_var() {
                        vars.push(v.to_string());
                    }
                }
            }
            AnfExpr::MethodCall { receiver, args, .. } => {
                if let Some(v) = receiver.as_var() {
                    vars.push(v.to_string());
                }
                for arg in args {
                    if let Some(v) = arg.as_var() {
                        vars.push(v.to_string());
                    }
                }
            }
            AnfExpr::FieldAccess { receiver, .. } => {
                if let Some(v) = receiver.as_var() {
                    vars.push(v.to_string());
                }
            }
            AnfExpr::TupleAccess { receiver, .. } => {
                if let Some(v) = receiver.as_var() {
                    vars.push(v.to_string());
                }
            }
            AnfExpr::Index { receiver, index } => {
                if let Some(v) = receiver.as_var() {
                    vars.push(v.to_string());
                }
                if let Some(v) = index.as_var() {
                    vars.push(v.to_string());
                }
            }
            AnfExpr::Record { fields } => {
                for (_, atom) in fields {
                    if let Some(v) = atom.as_var() {
                        vars.push(v.to_string());
                    }
                }
            }
            AnfExpr::Array { elements } | AnfExpr::Tuple { elements } => {
                for atom in elements {
                    if let Some(v) = atom.as_var() {
                        vars.push(v.to_string());
                    }
                }
            }
            AnfExpr::Variant { args, .. } => {
                for atom in args {
                    if let Some(v) = atom.as_var() {
                        vars.push(v.to_string());
                    }
                }
            }
            AnfExpr::Closure { body, .. } => {
                vars.extend(body.used_vars());
            }
            AnfExpr::MakeClosure { env, .. } => {
                if let Some(Atom::Var(v)) = env {
                    vars.push(v.clone());
                }
            }
            AnfExpr::CallClosure { closure, args } => {
                if let Some(v) = closure.as_var() {
                    vars.push(v.to_string());
                }
                for arg in args {
                    if let Some(v) = arg.as_var() {
                        vars.push(v.to_string());
                    }
                }
            }
            AnfExpr::If {
                cond,
                then_branch,
                else_branch,
            } => {
                if let Some(v) = cond.as_var() {
                    vars.push(v.to_string());
                }
                vars.extend(then_branch.used_vars());
                vars.extend(else_branch.used_vars());
            }
            AnfExpr::Match { scrutinee, arms } => {
                if let Some(v) = scrutinee.as_var() {
                    vars.push(v.to_string());
                }
                for arm in arms {
                    vars.extend(arm.body.used_vars());
                }
            }
            AnfExpr::ReuseRecord { base, fields } => {
                if let Some(v) = base.as_var() {
                    vars.push(v.to_string());
                }
                for (_, atom) in fields {
                    if let Some(v) = atom.as_var() {
                        vars.push(v.to_string());
                    }
                }
            }
            AnfExpr::IsUnique(atom) => {
                if let Some(v) = atom.as_var() {
                    vars.push(v.to_string());
                }
            }
        }
        vars
    }

    /// Collects all unique variable names read/used by this ANF expression.
    pub fn used_vars(&self) -> HashSet<String> {
        self.var_occurrences().into_iter().collect()
    }
}

/// An ANF Statement.
#[derive(Debug, Clone, PartialEq)]
pub enum AnfStmt {
    /// Let binding: `let var: ty = value;`
    Let {
        var: String,
        ty: Type,
        value: AnfExpr,
        span: Span,
    },
    /// Standalone expression statement
    Expr(AnfExpr),
    /// Perceus reference count increment: `inc_ref(var)`
    IncRef { var: String },
    /// Perceus reference count decrement: `dec_ref(var)`
    DecRef { var: String },
    /// In-place destructive field update for FBIP: `receiver.field = value`
    SetField {
        receiver: Atom,
        field: String,
        value: Atom,
    },
}

impl AnfStmt {
    /// Returns the variable defined by this statement, if any.
    pub fn defined_var(&self) -> Option<&str> {
        match self {
            AnfStmt::Let { var, .. } => Some(var.as_str()),
            _ => None,
        }
    }

    /// Returns variables read/used by this statement.
    pub fn used_vars(&self) -> HashSet<String> {
        let mut vars = HashSet::new();
        match self {
            AnfStmt::Let { value, .. } => {
                vars.extend(value.used_vars());
            }
            AnfStmt::Expr(expr) => {
                vars.extend(expr.used_vars());
            }
            AnfStmt::IncRef { var } => {
                vars.insert(var.clone());
            }
            AnfStmt::DecRef { var } => {
                vars.insert(var.clone());
            }
            AnfStmt::SetField {
                receiver, value, ..
            } => {
                if let Some(v) = receiver.as_var() {
                    vars.insert(v.to_string());
                }
                if let Some(v) = value.as_var() {
                    vars.insert(v.to_string());
                }
            }
        }
        vars
    }
}

/// Block terminator in ANF.
#[derive(Debug, Clone, PartialEq)]
pub enum AnfTail {
    /// Return statement: `return atom;` or `return;`
    Return(Option<Atom>),
    /// Tail call directly to a function (enables LLVM `musttail`): `tailcall f(args...)`
    TailCall { callee: Atom, args: Vec<Atom> },
    /// Tail if branch
    If {
        cond: Atom,
        then_branch: Box<AnfBlock>,
        else_branch: Option<Box<AnfBlock>>,
    },
    /// Tail match branch
    Match {
        scrutinee: Atom,
        arms: Vec<AnfMatchArm>,
    },
    /// Yield an atomic value from a block
    Atom(Atom),
}

impl AnfTail {
    /// Returns variables read/used by this block terminator.
    pub fn used_vars(&self) -> HashSet<String> {
        let mut vars = HashSet::new();
        match self {
            AnfTail::Return(Some(a)) => {
                if let Some(v) = a.as_var() {
                    vars.insert(v.to_string());
                }
            }
            AnfTail::Return(None) => {}
            AnfTail::TailCall { callee, args } => {
                if let Some(v) = callee.as_var() {
                    vars.insert(v.to_string());
                }
                for a in args {
                    if let Some(v) = a.as_var() {
                        vars.insert(v.to_string());
                    }
                }
            }
            AnfTail::If {
                cond,
                then_branch,
                else_branch,
            } => {
                if let Some(v) = cond.as_var() {
                    vars.insert(v.to_string());
                }
                vars.extend(then_branch.used_vars());
                if let Some(eb) = else_branch {
                    vars.extend(eb.used_vars());
                }
            }
            AnfTail::Match { scrutinee, arms } => {
                if let Some(v) = scrutinee.as_var() {
                    vars.insert(v.to_string());
                }
                for arm in arms {
                    vars.extend(arm.body.used_vars());
                }
            }
            AnfTail::Atom(a) => {
                if let Some(v) = a.as_var() {
                    vars.insert(v.to_string());
                }
            }
        }
        vars
    }
}

/// An ANF Block consists of linearized statements followed by a terminator.
#[derive(Debug, Clone, PartialEq)]
pub struct AnfBlock {
    pub stmts: Vec<AnfStmt>,
    pub tail: AnfTail,
    pub span: Span,
}

impl AnfBlock {
    pub fn new(stmts: Vec<AnfStmt>, tail: AnfTail, span: Span) -> Self {
        Self { stmts, tail, span }
    }

    /// Collects all variables used in this block (including tail).
    pub fn used_vars(&self) -> HashSet<String> {
        let mut vars = HashSet::new();
        let mut defined = HashSet::new();
        for stmt in &self.stmts {
            for u in stmt.used_vars() {
                if !defined.contains(&u) {
                    vars.insert(u);
                }
            }
            if let Some(d) = stmt.defined_var() {
                defined.insert(d.to_string());
            }
        }
        for u in self.tail.used_vars() {
            if !defined.contains(&u) {
                vars.insert(u);
            }
        }
        vars
    }

    /// Collects all variable names defined in this block.
    pub fn defined_vars(&self) -> HashSet<String> {
        let mut defined = HashSet::new();
        for stmt in &self.stmts {
            if let Some(d) = stmt.defined_var() {
                defined.insert(d.to_string());
            }
        }
        defined
    }
}

/// A pattern match arm in ANF.
#[derive(Debug, Clone, PartialEq)]
pub struct AnfMatchArm {
    pub pattern: DesugaredPattern,
    pub body: AnfBlock,
}

/// A function in ANF form.
#[derive(Debug, Clone, PartialEq)]
pub struct AnfFunction {
    pub name: String,
    pub type_params: Vec<TypeParam>,
    pub params: Vec<(String, Type)>,
    pub return_type: Type,
    pub body: AnfBlock,
    pub is_effectful: bool,
    pub span: Span,
}

/// Trait implementation in ANF form.
#[derive(Debug, Clone, PartialEq)]
pub struct AnfImpl {
    pub trait_name: String,
    pub target_type: Type,
    pub methods: Vec<AnfFunction>,
    pub span: Span,
}

/// A complete Modus program in ANF IR.
#[derive(Debug, Clone, PartialEq)]
pub struct AnfProgram {
    pub functions: Vec<AnfFunction>,
    pub types: Vec<TypeDecl>,
    pub traits: Vec<TraitDecl>,
    pub impls: Vec<AnfImpl>,
}
