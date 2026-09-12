pub mod grammar;
pub mod lexer;
pub mod token;

use crate::ast::{Declaration, Expr, Program, Spanned, Stmt, Type};
use chumsky::prelude::*;
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

macro_rules! parse_source {
    ($source:expr, $parser:expr) => {{
        let source = $source;
        let (tokens, lex_errs) = lexer::lexer().parse(source).into_output_errors();
        if !lex_errs.is_empty() {
            return Err(lex_errs
                .into_iter()
                .map(|e| ParseError {
                    message: e.to_string(),
                    span: crate::ast::Span::new(e.span().start, e.span().end),
                })
                .collect());
        }
        let tokens = tokens.unwrap_or_default();
        let eoi = source.len();
        let token_input = tokens.as_slice().map((eoi..eoi).into(), |(t, s)| (t, s));
        let (output, parse_errs) = $parser
            .then_ignore(end())
            .parse(token_input)
            .into_output_errors();
        if !parse_errs.is_empty() || output.is_none() {
            return Err(parse_errs
                .into_iter()
                .map(|e| ParseError {
                    message: e.to_string(),
                    span: crate::ast::Span::new(e.span().start, e.span().end),
                })
                .collect());
        }
        Ok(output.unwrap())
    }};
}

/// Parse a full Modus program from source code.
pub fn parse_program(source: &str) -> Result<Program, Vec<ParseError>> {
    parse_source!(source, grammar::program_parser())
}

/// Parse a single Modus expression from source code.
pub fn parse_expr(source: &str) -> Result<Spanned<Expr>, Vec<ParseError>> {
    parse_source!(source, grammar::expr_parser())
}

/// Parse a single Modus statement from source code.
pub fn parse_stmt(source: &str) -> Result<Spanned<Stmt>, Vec<ParseError>> {
    parse_source!(source, grammar::stmt_parser())
}

/// Parse a single Modus type expression from source code.
pub fn parse_type(source: &str) -> Result<Spanned<Type>, Vec<ParseError>> {
    parse_source!(source, grammar::type_parser())
}

/// Parse a single Modus declaration from source code.
pub fn parse_decl(source: &str) -> Result<Spanned<Declaration>, Vec<ParseError>> {
    parse_source!(source, grammar::decl_parser())
}
