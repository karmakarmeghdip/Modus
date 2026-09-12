//! Go to Definition provider for variables, functions, types, traits, and modules.

use super::document::Document;
use crate::ast::Declaration;
use crate::modules::resolver::resolve_module_path;
use tower_lsp::lsp_types::{GotoDefinitionResponse, Location, Position, Range, Url};

/// Finds the definition location for the symbol at the given position.
pub fn definition_at(doc: &Document, position: Position) -> Option<GotoDefinitionResponse> {
    let offset = doc.line_index.position_to_offset(position);
    let word = get_word_at_offset(&doc.text, offset)?;

    // 1. Check if hovering/clicking on an import statement source path
    if let Some(program) = &doc.program {
        for import_decl in &program.imports {
            if import_decl.span.start <= offset && offset <= import_decl.span.end {
                let current_file_path = doc.uri.to_file_path().ok()?;
                if let Ok(resolved_path) =
                    resolve_module_path(&import_decl.node.source, Some(&current_file_path))
                    && let Ok(target_url) = Url::from_file_path(&resolved_path)
                {
                    return Some(GotoDefinitionResponse::Scalar(Location {
                        uri: target_url,
                        range: Range::default(),
                    }));
                }
            }
        }
    }

    // 2. Check local variables in Environment
    if let Some(env) = &doc.env
        && let Some((_, var_span)) = env.lookup_var(&word)
    {
        let range = doc.line_index.span_to_range(*var_span);
        return Some(GotoDefinitionResponse::Scalar(Location {
            uri: doc.uri.clone(),
            range,
        }));
    }

    // 3. Check functions in AST declarations
    if let Some(program) = &doc.program {
        for decl in &program.declarations {
            if let Declaration::Function(f) = &decl.node
                && f.name == word
            {
                let range = doc.line_index.span_to_range(decl.span);
                return Some(GotoDefinitionResponse::Scalar(Location {
                    uri: doc.uri.clone(),
                    range,
                }));
            }
        }
    }

    // 4. Check types in AST declarations
    if let Some(program) = &doc.program {
        for decl in &program.declarations {
            if let Declaration::Type(t) = &decl.node
                && t.name == word
            {
                let range = doc.line_index.span_to_range(decl.span);
                return Some(GotoDefinitionResponse::Scalar(Location {
                    uri: doc.uri.clone(),
                    range,
                }));
            }
        }
    }

    // 5. Check traits in AST declarations
    if let Some(program) = &doc.program {
        for decl in &program.declarations {
            if let Declaration::Trait(tr) = &decl.node
                && tr.name == word
            {
                let range = doc.line_index.span_to_range(decl.span);
                return Some(GotoDefinitionResponse::Scalar(Location {
                    uri: doc.uri.clone(),
                    range,
                }));
            }
        }
    }

    // 6. Check function in Environment (fallback)
    if let Some(env) = &doc.env
        && let Some(sig) = env.lookup_function(&word)
    {
        let range = doc.line_index.span_to_range(sig.span);
        return Some(GotoDefinitionResponse::Scalar(Location {
            uri: doc.uri.clone(),
            range,
        }));
    }

    None
}

fn get_word_at_offset(text: &str, offset: usize) -> Option<String> {
    if text.is_empty() || offset > text.len() {
        return None;
    }

    let bytes = text.as_bytes();
    let mut start = offset.min(text.len().saturating_sub(1));

    if start > 0 && (start == text.len() || !is_ident_char(bytes[start])) {
        if is_ident_char(bytes[start - 1]) {
            start -= 1;
        } else {
            return None;
        }
    }

    while start > 0 && is_ident_char(bytes[start - 1]) {
        start -= 1;
    }

    let mut end = offset;
    while end < text.len() && is_ident_char(bytes[end]) {
        end += 1;
    }

    if start < end {
        Some(text[start..end].to_string())
    } else {
        None
    }
}

fn is_ident_char(b: u8) -> bool {
    b.is_ascii_alphanumeric() || b == b'_'
}
