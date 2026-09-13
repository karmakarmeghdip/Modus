//! Typechecker and semantic analysis error definitions.

use crate::ast::Span;
use std::fmt;

#[derive(Debug, Clone, PartialEq)]
pub struct TypeError {
    pub kind: TypeErrorKind,
    pub span: Option<Span>,
}

impl TypeError {
    pub fn new(kind: TypeErrorKind, span: Option<Span>) -> Self {
        Self { kind, span }
    }

    pub fn without_span(kind: TypeErrorKind) -> Self {
        Self { kind, span: None }
    }
}

#[derive(Debug, Clone, PartialEq)]
pub enum TypeErrorKind {
    /// Effectful operation (perform or calling an IO function) inside a pure function
    PurityViolation {
        function_name: String,
        reason: String,
    },
    /// A pure function returning void is a dead computation violation
    DeadComputation { function_name: String },
    /// General type mismatch
    TypeMismatch { expected: String, found: String },
    /// Missing a field in a record literal
    MissingRecordField { field: String, record_type: String },
    /// Extraneous field provided in a record literal
    ExtraneousRecordField { field: String, record_type: String },
    /// Use of an undeclared variable
    UndeclaredVariable(String),
    /// Variable shadowing or duplicate definition in the same lexical scope
    DuplicateVariable(String),
    /// Use of an undeclared function
    UndeclaredFunction(String),
    /// Duplicate function declaration in the same scope
    DuplicateFunction(String),
    /// Use of an undeclared type
    UndeclaredType(String),
    /// Duplicate type declaration in the same scope
    DuplicateType(String),
    /// A type does not implement the required trait
    TraitNotImplemented { ty: String, trait_name: String },
    /// Calling a method that is not defined on the type or its implemented traits
    MethodNotFound { ty: String, method: String },
    /// Check operator applied to a non-Result type
    CheckOnNonResult(String),
    /// Perform operator applied to a non-IO type
    PerformOnNonIO(String),
    /// Wrong number of arguments in a function or method call
    ArgCountMismatch { expected: usize, found: usize },
    /// Occurs check failed during type variable unification (recursive type)
    OccursCheckFailed,
    /// Type inference failure
    CannotInfer(String),
    /// Invalid pattern match
    InvalidPattern(String),
    /// Function declared without a body in a non-library file
    MissingFunctionBody { function_name: String },
    /// Function declared with a body in a library export map file
    UnexpectedFunctionBodyInHeader { function_name: String },
    /// Invalid type cast
    InvalidCast { from: String, to: String },
    /// General semantic error
    General(String),
}

impl fmt::Display for TypeError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        if let Some(span) = self.span {
            write!(
                f,
                "Semantic error at [{}, {}]: {}",
                span.start, span.end, self.kind
            )
        } else {
            write!(f, "Semantic error: {}", self.kind)
        }
    }
}

impl fmt::Display for TypeErrorKind {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::PurityViolation {
                function_name,
                reason,
            } => {
                write!(
                    f,
                    "Purity violation in function '{function_name}': {reason}"
                )
            }
            Self::DeadComputation { function_name } => {
                write!(
                    f,
                    "Dead computation violation: pure function '{function_name}' cannot return void. Pure functions must return a value, or use IO(void) if performing side effects."
                )
            }
            Self::TypeMismatch { expected, found } => {
                write!(f, "Type mismatch: expected '{expected}', found '{found}'")
            }
            Self::MissingRecordField { field, record_type } => {
                write!(
                    f,
                    "Missing field '{field}' in record of type '{record_type}'"
                )
            }
            Self::ExtraneousRecordField { field, record_type } => {
                write!(
                    f,
                    "Extraneous field '{field}' not found in record type '{record_type}'"
                )
            }
            Self::UndeclaredVariable(name) => {
                write!(f, "Undeclared variable '{name}'")
            }
            Self::DuplicateVariable(name) => {
                write!(
                    f,
                    "Duplicate variable declaration '{name}' in the same scope"
                )
            }
            Self::UndeclaredFunction(name) => {
                write!(f, "Undeclared function '{name}'")
            }
            Self::DuplicateFunction(name) => {
                write!(f, "Duplicate function '{name}'")
            }
            Self::UndeclaredType(name) => {
                write!(f, "Undeclared type '{name}'")
            }
            Self::DuplicateType(name) => {
                write!(f, "Duplicate type '{name}'")
            }
            Self::TraitNotImplemented { ty, trait_name } => {
                write!(f, "Type '{ty}' does not implement trait '{trait_name}'")
            }
            Self::MethodNotFound { ty, method } => {
                write!(f, "Method '{method}' not found on type '{ty}'")
            }
            Self::CheckOnNonResult(ty) => {
                write!(
                    f,
                    "'check' expression must be applied to a Result type, but found '{ty}'"
                )
            }
            Self::PerformOnNonIO(ty) => {
                write!(
                    f,
                    "'perform' expression must be applied to an IO type, but found '{ty}'"
                )
            }
            Self::ArgCountMismatch { expected, found } => {
                write!(f, "Expected {expected} arguments, but found {found}")
            }
            Self::OccursCheckFailed => {
                write!(f, "Occurs check failed: cannot construct infinite type")
            }
            Self::CannotInfer(msg) => {
                write!(f, "Cannot infer type: {msg}")
            }
            Self::InvalidPattern(msg) => {
                write!(f, "Invalid pattern: {msg}")
            }
            Self::MissingFunctionBody { function_name } => {
                write!(
                    f,
                    "Function '{function_name}' must have an implementation body. Empty function prototypes are only permitted in export map files containing a 'library' directive."
                )
            }
            Self::UnexpectedFunctionBodyInHeader { function_name } => {
                write!(
                    f,
                    "Function '{function_name}' cannot define a body in a library export map. Remove the body and terminate the signature with ';' to declare the exported symbol."
                )
            }
            Self::InvalidCast { from, to } => {
                write!(f, "Invalid cast from '{from}' to '{to}'")
            }
            Self::General(msg) => {
                write!(f, "{msg}")
            }
        }
    }
}

impl std::error::Error for TypeError {}
