//! Context-aware completion items and code snippet provider for Modus.

use super::document::Document;
use tower_lsp::lsp_types::{
    CompletionItem, CompletionItemKind, CompletionList, CompletionResponse, InsertTextFormat,
    Position,
};

/// Computes completion items at the cursor position.
pub fn completions_at(doc: &Document, _position: Position) -> Option<CompletionResponse> {
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
