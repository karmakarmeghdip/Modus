use super::diagnostics::{load_module_interface, resolve_import};
use super::document::{Document, DocumentStore};
use std::collections::HashSet;
use tower_lsp::lsp_types::{
    CompletionItem, CompletionItemKind, CompletionList, CompletionResponse, InsertTextFormat,
    Position,
};

/// Computes completion items at the cursor position without store.
pub fn completions_at(doc: &Document, position: Position) -> Option<CompletionResponse> {
    completions_with_store(doc, position, None)
}

/// Computes completion items at the cursor position, supporting import completions from other files.
pub fn completions_with_store(
    doc: &Document,
    position: Position,
    store: Option<&DocumentStore>,
) -> Option<CompletionResponse> {
    // 0. Check if typing inside an import clause: `import { ... } from "source"`
    let _offset = doc.line_index.position_to_offset(position);
    let line_start = doc.line_index.position_to_offset(Position {
        line: position.line,
        character: 0,
    });
    let line_end = doc.line_index.position_to_offset(Position {
        line: position.line + 1,
        character: 0,
    });
    let line_text = if line_start < line_end && line_end <= doc.text.len() {
        &doc.text[line_start..line_end]
    } else {
        ""
    };

    if line_text.contains("import")
        && line_text.contains("from")
        && let Some(source_str) = extract_source_from_import_line(line_text)
    {
        let current_file_path = doc.uri.to_file_path().ok();
        if let Ok(resolved) = resolve_import(&source_str, current_file_path.as_deref(), store) {
            let mut visiting = HashSet::new();
            if let Ok(iface) = load_module_interface(&resolved, store, &mut visiting) {
                let mut import_items = Vec::new();
                for (fn_name, sig) in &iface.exported_functions {
                    import_items.push(CompletionItem {
                        label: fn_name.clone(),
                        kind: Some(CompletionItemKind::FUNCTION),
                        detail: Some(format!(
                            "function {}{}: {}",
                            fn_name,
                            if sig.type_params.is_empty() {
                                ""
                            } else {
                                "(...)"
                            },
                            sig.return_type
                        )),
                        ..Default::default()
                    });
                }
                for type_name in iface.exported_types.keys() {
                    import_items.push(CompletionItem {
                        label: type_name.clone(),
                        kind: Some(CompletionItemKind::CLASS),
                        detail: Some("Exported type".to_string()),
                        ..Default::default()
                    });
                }
                for trait_name in iface.exported_traits.keys() {
                    import_items.push(CompletionItem {
                        label: trait_name.clone(),
                        kind: Some(CompletionItemKind::INTERFACE),
                        detail: Some("Exported trait".to_string()),
                        ..Default::default()
                    });
                }
                if !import_items.is_empty() {
                    return Some(CompletionResponse::List(CompletionList {
                        is_incomplete: false,
                        items: import_items,
                    }));
                }
            }
        }
    }

    let mut items = Vec::new();

    // 1. Language Keywords
    let keywords = [
        ("function", "Function declaration"),
        ("type", "Type declaration"),
        ("let", "Immutable variable binding"),
        ("if", "Conditional expression"),
        ("else", "Else branch"),
        ("match", "Pattern matching expression"),
        ("perform", "Unwrap IO effect"),
        ("check", "Propagate Result error"),
        ("return", "Return statement"),
        ("trait", "Trait declaration"),
        ("impl", "Trait implementation"),
        ("import", "Module import"),
        ("export", "Module export"),
        ("library", "Library header declaration"),
        ("extern", "External C FFI binding"),
    ];

    for (kw, doc_text) in keywords {
        items.push(CompletionItem {
            label: kw.to_string(),
            kind: Some(CompletionItemKind::KEYWORD),
            detail: Some(doc_text.to_string()),
            ..Default::default()
        });
    }

    // 2. Primitive & Standard Types
    let types = [
        "i32", "i64", "i16", "i8", "u32", "u64", "u16", "u8", "f32", "f64", "bool", "String",
        "void", "IO", "Option", "Result",
    ];

    for ty in types {
        items.push(CompletionItem {
            label: ty.to_string(),
            kind: Some(CompletionItemKind::CLASS),
            detail: Some("Built-in type".to_string()),
            ..Default::default()
        });
    }

    // 3. Built-in Constructors
    let constructors = [
        ("Some", "Option::Some(val)"),
        ("None", "Option::None"),
        ("Ok", "Result::Ok(val)"),
        ("Err", "Result::Err(err)"),
    ];

    for (ctor, doc_text) in constructors {
        items.push(CompletionItem {
            label: ctor.to_string(),
            kind: Some(CompletionItemKind::CONSTRUCTOR),
            detail: Some(doc_text.to_string()),
            ..Default::default()
        });
    }

    // 4. Code Snippets
    items.push(CompletionItem {
        label: "fn-def".to_string(),
        kind: Some(CompletionItemKind::SNIPPET),
        detail: Some("function name(params): Ret { ... }".to_string()),
        insert_text: Some(
            "function ${1:name}(${2:params}): ${3:i32} {\n    ${0:return 0;}\n}".to_string(),
        ),
        insert_text_format: Some(InsertTextFormat::SNIPPET),
        ..Default::default()
    });

    items.push(CompletionItem {
        label: "fn-expr".to_string(),
        kind: Some(CompletionItemKind::SNIPPET),
        detail: Some("function name(params): Ret => expr;".to_string()),
        insert_text: Some("function ${1:name}(${2:params}): ${3:i32} => ${0:expr};".to_string()),
        insert_text_format: Some(InsertTextFormat::SNIPPET),
        ..Default::default()
    });

    items.push(CompletionItem {
        label: "type-record".to_string(),
        kind: Some(CompletionItemKind::SNIPPET),
        detail: Some("type Name = { field: Type };".to_string()),
        insert_text: Some("type ${1:Name} = {\n    ${2:field}: ${3:i32},\n};".to_string()),
        insert_text_format: Some(InsertTextFormat::SNIPPET),
        ..Default::default()
    });

    items.push(CompletionItem {
        label: "type-union".to_string(),
        kind: Some(CompletionItemKind::SNIPPET),
        detail: Some("type Option(T) = Some(T) | None;".to_string()),
        insert_text: Some("type ${1:Option}(${2:T}) = ${3:Some}(${2:T}) | ${4:None};".to_string()),
        insert_text_format: Some(InsertTextFormat::SNIPPET),
        ..Default::default()
    });

    items.push(CompletionItem {
        label: "match-expr".to_string(),
        kind: Some(CompletionItemKind::SNIPPET),
        detail: Some("match expr { pat => val, ... }".to_string()),
        insert_text: Some("match (${1:expr}) {\n    ${2:Some}(${3:v}) => ${4:v},\n    ${5:None} => ${0:fallback},\n}".to_string()),
        insert_text_format: Some(InsertTextFormat::SNIPPET),
        ..Default::default()
    });

    // 5. In-scope functions & types from Environment
    if let Some(env) = &doc.env {
        for (fn_name, sig) in &env.functions {
            items.push(CompletionItem {
                label: fn_name.clone(),
                kind: Some(CompletionItemKind::FUNCTION),
                detail: Some(format!("{}", sig.return_type)),
                ..Default::default()
            });
        }

        for type_name in env.types.keys() {
            items.push(CompletionItem {
                label: type_name.clone(),
                kind: Some(CompletionItemKind::CLASS),
                detail: Some("Declared type".to_string()),
                ..Default::default()
            });
        }
    }

    Some(CompletionResponse::List(CompletionList {
        is_incomplete: false,
        items,
    }))
}

fn extract_source_from_import_line(line: &str) -> Option<String> {
    let first_quote = line.find('"')?;
    let remainder = &line[first_quote + 1..];
    let second_quote = remainder.find('"')?;
    Some(remainder[..second_quote].to_string())
}
