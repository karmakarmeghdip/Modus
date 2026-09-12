//! Diagnostics computation for syntax, purity, and semantic type errors.

use super::document::LineIndex;
use crate::ast::Program;
use crate::parser::parse_program;
use crate::typechecker::{Environment, check_program_with_env};
use tower_lsp::lsp_types::{Diagnostic, DiagnosticSeverity, Range};

/// Computes both syntactic (parser) and semantic (typechecker) diagnostics for Modus source code.
/// Returns the parsed Program (if parsing succeeded), the Environment (if typechecking succeeded),
/// and the list of LSP diagnostics.
pub fn compute_diagnostics(
    text: &str,
    line_index: &LineIndex,
) -> (Option<Program>, Option<Environment>, Vec<Diagnostic>) {
    let mut diagnostics = Vec::new();

    // 1. Syntactic Parsing Phase (Chumsky lexer + parser)
    match parse_program(text) {
        Ok(program) => {
            // 2. Semantic Analysis & Typechecking Phase
            let mut env = Environment::new();
            match check_program_with_env(&program, &mut env) {
                Ok(()) => (Some(program), Some(env), diagnostics),
                Err(type_error) => {
                    let range = match type_error.span {
                        Some(span) => line_index.span_to_range(span),
                        None => Range::default(),
                    };

                    let diagnostic = Diagnostic {
                        range,
                        severity: Some(DiagnosticSeverity::ERROR),
                        code: None,
                        code_description: None,
                        source: Some("modus".to_string()),
                        message: type_error.kind.to_string(),
                        related_information: None,
                        tags: None,
                        data: None,
                    };
                    diagnostics.push(diagnostic);

                    (Some(program), Some(env), diagnostics)
                }
            }
        }
        Err(parse_errors) => {
            for err in parse_errors {
                let range = line_index.span_to_range(err.span);
                let message = format_parse_error(&err.message);

                let diagnostic = Diagnostic {
                    range,
                    severity: Some(DiagnosticSeverity::ERROR),
                    code: None,
                    code_description: None,
                    source: Some("modus".to_string()),
                    message,
                    related_information: None,
                    tags: None,
                    data: None,
                };
                diagnostics.push(diagnostic);
            }

            (None, None, diagnostics)
        }
    }
}

/// Formats raw chumsky/parser errors into clean, readable messages for LSP clients.
fn format_parse_error(raw_msg: &str) -> String {
    // Chumsky errors often read like "found 'fn' expected 'function'..."
    // Clean up or return directly
    raw_msg.to_string()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_valid_program_diagnostics() {
        let text = "function add(a: i32, b: i32): i32 { return a + b; }";
        let line_index = LineIndex::new(text);
        let (prog, env, diags) = compute_diagnostics(text, &line_index);
        assert!(prog.is_some());
        assert!(env.is_some());
        assert!(diags.is_empty());
    }

    #[test]
    fn test_syntax_error_diagnostics() {
        let text = "fn add(a: i32): i32 { return a; }"; // 'fn' is forbidden in Modus
        let line_index = LineIndex::new(text);
        let (prog, _env, diags) = compute_diagnostics(text, &line_index);
        assert!(prog.is_none());
        assert!(!diags.is_empty());
        assert_eq!(diags[0].severity, Some(DiagnosticSeverity::ERROR));
    }

    #[test]
    fn test_semantic_error_diagnostics() {
        // Pure function returning void is a semantic error in Modus
        let text = "function dead(): void { return; }";
        let line_index = LineIndex::new(text);
        let (prog, _env, diags) = compute_diagnostics(text, &line_index);
        assert!(prog.is_some());
        assert!(!diags.is_empty());
        assert!(diags[0].message.contains("dead computation") || diags[0].message.contains("void"));
    }
}
