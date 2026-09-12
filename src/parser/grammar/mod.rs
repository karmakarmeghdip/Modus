//! Modus Grammar Parsers using Chumsky.
//!
//! Organizes Modus parsing rules into focused submodules:
//! - [`common`]: shared parser types and identifier parsing
//! - [`types`]: type expressions
//! - [`patterns`]: pattern matching expressions
//! - [`stmts`]: statements and code blocks
//! - [`exprs`]: expressions, operators, and Pratt climbing
//! - [`decls`]: declarations (functions, types, traits, impls) and programs

pub mod common;
pub mod decls;
pub mod exprs;
pub mod patterns;
pub mod stmts;
pub mod types;

pub use common::{Span, ident_parser, to_ast_span};
pub use decls::{decl_parser, function_decl_internal, program_parser};
pub use exprs::expr_parser;
pub use patterns::pattern_parser;
pub use stmts::{block_parser, block_parser_internal, stmt_parser, stmt_parser_internal};
pub use types::type_parser;
