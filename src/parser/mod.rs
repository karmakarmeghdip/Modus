//! Modus Parser interface and error definitions.

use crate::ast::{Declaration, Expr, Program, Spanned, Stmt, Type};
use std::fmt;

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ParseError {
    pub message: String,
    pub span: crate::ast::Span,
}

impl fmt::Display for ParseError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(
            f,
            "Parse error at {}..{}: {}",
            self.span.start, self.span.end, self.message
        )
    }
}

impl std::error::Error for ParseError {}

/// Parse a full Modus program from source code.
pub fn parse_program(source: &str) -> Result<Program, Vec<ParseError>> {
    // TDD Red phase: stub returning error until parser implementation
    Err(vec![ParseError {
        message: "Parser not yet implemented".to_string(),
        span: crate::ast::Span::new(0, source.len()),
    }])
}

/// Parse a single Modus expression from source code.
pub fn parse_expr(source: &str) -> Result<Spanned<Expr>, Vec<ParseError>> {
    Err(vec![ParseError {
        message: "Parser not yet implemented".to_string(),
        span: crate::ast::Span::new(0, source.len()),
    }])
}

/// Parse a single Modus statement from source code.
pub fn parse_stmt(source: &str) -> Result<Spanned<Stmt>, Vec<ParseError>> {
    Err(vec![ParseError {
        message: "Parser not yet implemented".to_string(),
        span: crate::ast::Span::new(0, source.len()),
    }])
}

/// Parse a single Modus type expression from source code.
pub fn parse_type(source: &str) -> Result<Spanned<Type>, Vec<ParseError>> {
    Err(vec![ParseError {
        message: "Parser not yet implemented".to_string(),
        span: crate::ast::Span::new(0, source.len()),
    }])
}

/// Parse a single Modus declaration from source code.
pub fn parse_decl(source: &str) -> Result<Spanned<Declaration>, Vec<ParseError>> {
    Err(vec![ParseError {
        message: "Parser not yet implemented".to_string(),
        span: crate::ast::Span::new(0, source.len()),
    }])
}
