//! Code actions and automated quick-fixes for canonical Modus syntax.

use super::document::Document;
use std::collections::HashMap;
use tower_lsp::lsp_types::{
    CodeAction, CodeActionContext, CodeActionKind, CodeActionOrCommand, CodeActionResponse, Range,
    TextEdit, WorkspaceEdit,
};

/// Computes code actions and quick fixes for diagnostics within the requested range.
pub fn code_actions_at(
    doc: &Document,
    _range: Range,
    context: &CodeActionContext,
) -> Option<CodeActionResponse> {
    let mut actions = Vec::new();

    for diag in &context.diagnostics {
        let span = doc.line_index.range_to_span(diag.range);
        let start = span.start.min(doc.text.len());
        let end = span.end.min(doc.text.len());
        let error_slice = &doc.text[start..end];

        // 1. Quick fix: 'fn' -> 'function'
        if error_slice.contains("fn") || diag.message.contains("fn") {
            let edit = TextEdit {
                range: diag.range,
                new_text: "function".to_string(),
            };
            let mut changes = HashMap::new();
            changes.insert(doc.uri.clone(), vec![edit]);

            actions.push(CodeActionOrCommand::CodeAction(CodeAction {
                title: "Replace 'fn' with 'function'".to_string(),
                kind: Some(CodeActionKind::QUICKFIX),
                diagnostics: Some(vec![diag.clone()]),
                edit: Some(WorkspaceEdit {
                    changes: Some(changes),
                    ..Default::default()
                }),
                is_preferred: Some(true),
                ..Default::default()
            }));
        }

        // 2. Quick fix: remove 'mut'
        if error_slice.contains("mut") || diag.message.contains("mut") {
            let edit = TextEdit {
                range: diag.range,
                new_text: String::new(),
            };
            let mut changes = HashMap::new();
            changes.insert(doc.uri.clone(), vec![edit]);

            actions.push(CodeActionOrCommand::CodeAction(CodeAction {
                title: "Remove 'mut' (variables are strictly immutable)".to_string(),
                kind: Some(CodeActionKind::QUICKFIX),
                diagnostics: Some(vec![diag.clone()]),
                edit: Some(WorkspaceEdit {
                    changes: Some(changes),
                    ..Default::default()
                }),
                is_preferred: Some(true),
                ..Default::default()
            }));
        }

        // 3. Quick fix: dead computation pure void -> IO(void)
        if diag.message.contains("dead computation") || diag.message.contains("void") {
            let edit = TextEdit {
                range: diag.range,
                new_text: ": IO(void)".to_string(),
            };
            let mut changes = HashMap::new();
            changes.insert(doc.uri.clone(), vec![edit]);

            actions.push(CodeActionOrCommand::CodeAction(CodeAction {
                title: "Change return type to IO(void)".to_string(),
                kind: Some(CodeActionKind::QUICKFIX),
                diagnostics: Some(vec![diag.clone()]),
                edit: Some(WorkspaceEdit {
                    changes: Some(changes),
                    ..Default::default()
                }),
                is_preferred: Some(true),
                ..Default::default()
            }));
        }
    }

    Some(actions)
}
