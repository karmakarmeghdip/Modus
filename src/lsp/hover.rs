//! Hover tooltip provider displaying types, signatures, effects, and documentation.

use super::document::Document;
use tower_lsp::lsp_types::{Hover, HoverContents, MarkupContent, MarkupKind, Position};

/// Generates hover information for a position in a Modus document.
pub fn hover_at(doc: &Document, position: Position) -> Option<Hover> {
    let offset = doc.line_index.position_to_offset(position);
    let word = get_word_at_offset(&doc.text, offset)?;

    // 1. Keyword documentation
    if let Some(doc_str) = keyword_doc(&word) {
        return Some(Hover {
            contents: HoverContents::Markup(MarkupContent {
                kind: MarkupKind::Markdown,
                value: doc_str.to_string(),
            }),
            range: None,
        });
    }

    let env = doc.env.as_ref()?;

    // 2. Function lookup
    if let Some(sig) = env.lookup_function(&word) {
        let params: Vec<String> = sig
            .params
            .iter()
            .map(|(n, t)| format!("{n}: {t}"))
            .collect();

        let type_params = if sig.type_params.is_empty() {
            String::new()
        } else {
            let tps: Vec<String> = sig
                .type_params
                .iter()
                .map(|tp| match &tp.bound {
                    Some(b) => format!("{}: {:?}", tp.name, b.node),
                    None => tp.name.clone(),
                })
                .collect();
            format!("({})", tps.join(", "))
        };

        let is_effectful = format!("{}", sig.return_type).contains("IO");
        let effect_note = if is_effectful {
            "⚡ **Effectful** (returns `IO`)"
        } else {
            "🌿 **Pure** (no side effects)"
        };

        let signature = format!(
            "```modus\nfunction {}{}({}): {}\n```\n{}",
            sig.name,
            type_params,
            params.join(", "),
            sig.return_type,
            effect_note
        );

        return Some(Hover {
            contents: HoverContents::Markup(MarkupContent {
                kind: MarkupKind::Markdown,
                value: signature,
            }),
            range: None,
        });
    }

    // 3. Local variable lookup
    if let Some((ty, _)) = env.lookup_var(&word) {
        let value = format!("```modus\nlet {word}: {ty}\n```\n*Local variable (immutable)*");
        return Some(Hover {
            contents: HoverContents::Markup(MarkupContent {
                kind: MarkupKind::Markdown,
                value,
            }),
            range: None,
        });
    }

    // 4. Type definition lookup
    if let Some(type_info) = env.types.get(&word) {
        let name_str = match type_info {
            crate::typechecker::TypeDefInfo::Alias { name, .. } => name,
            crate::typechecker::TypeDefInfo::Union { name, .. } => name,
            crate::typechecker::TypeDefInfo::Builtin { name, .. } => name,
            crate::typechecker::TypeDefInfo::Primitive(p) => {
                return Some(Hover {
                    contents: HoverContents::Markup(MarkupContent {
                        kind: MarkupKind::Markdown,
                        value: format!("```modus\n{word}\n```\n*Primitive type `{p:?}`*"),
                    }),
                    range: None,
                });
            }
        };
        let value = format!("```modus\ntype {name_str}\n```\n*Type declaration*");
        return Some(Hover {
            contents: HoverContents::Markup(MarkupContent {
                kind: MarkupKind::Markdown,
                value,
            }),
            range: None,
        });
    }

    // 5. Trait lookup
    if let Some(trait_def) = env.traits.get(&word) {
        let members: Vec<String> = trait_def
            .methods
            .iter()
            .map(|(name, sig)| {
                let params: Vec<String> = sig
                    .params
                    .iter()
                    .map(|(n, t)| format!("{n}: {t}"))
                    .collect();
                format!(
                    "    function {name}({}): {};",
                    params.join(", "),
                    sig.return_type
                )
            })
            .collect();

        let value = format!(
            "```modus\ntrait {}(Self) {{\n{}\n}}\n```",
            trait_def.name,
            members.join("\n")
        );

        return Some(Hover {
            contents: HoverContents::Markup(MarkupContent {
                kind: MarkupKind::Markdown,
                value,
            }),
            range: None,
        });
    }

    // 6. Variant constructor lookup
    if let Some(ctor) = env.constructors.get(&word) {
        let value = match ctor {
            crate::typechecker::ConstructorInfo::Value { parent_type, .. } => {
                format!("```modus\n{word}\n```\n*Unit variant of `{parent_type}`*")
            }
            crate::typechecker::ConstructorInfo::Function {
                params,
                return_type,
                ..
            } => {
                let pts: Vec<String> = params.iter().map(|t| format!("{t}")).collect();
                format!(
                    "```modus\n{word}({}): {}\n```\n*Variant constructor of `{return_type}`*",
                    pts.join(", "),
                    return_type
                )
            }
        };

        return Some(Hover {
            contents: HoverContents::Markup(MarkupContent {
                kind: MarkupKind::Markdown,
                value,
            }),
            range: None,
        });
    }

    None
}

/// Extracts the identifier or keyword word at the given byte offset.
fn get_word_at_offset(text: &str, offset: usize) -> Option<String> {
    if text.is_empty() || offset > text.len() {
        return None;
    }

    let bytes = text.as_bytes();
    let mut start = offset.min(text.len().saturating_sub(1));

    // If offset is pointing to whitespace/delimiter, step back 1
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

fn keyword_doc(kw: &str) -> Option<&'static str> {
    match kw {
        "perform" => Some(
            "### `perform`\n\nModus effect operator. Unwraps an `IO(T)` computation.\n\n*Note: Calling `perform` inside a non-`IO` function is a compile-time type error.*",
        ),
        "check" => Some(
            "### `check`\n\nModus error propagation operator. Early-returns on `Err` via `FromResidual`.\n\n```modus\nlet val: T = check perform fetch_data();\n```",
        ),
        "function" => Some(
            "### `function`\n\nDeclares a named function in Modus. Modus functions are pure by default.\nFunctions performing side effects must return `IO(T)` or `IO(void)`.",
        ),
        "type" => Some(
            "### `type`\n\nDeclares a type alias, record type, or discriminated union:\n\n```modus\ntype Point = { x: i32, y: i32 };\ntype Option(T) = Some(T) | None;\n```",
        ),
        "let" => Some(
            "### `let`\n\nDeclares an immutable variable binding. All variables in Modus are strictly immutable (no `mut`).",
        ),
        "trait" => Some(
            "### `trait`\n\nDeclares an interface/trait in Modus with static bounds or dynamic fat-pointer dispatch.",
        ),
        "impl" => Some("### `impl`\n\nImplements a trait for a specific target type."),
        "match" => Some(
            "### `match`\n\nPattern matches on expressions, discriminated unions, records, tuples, and literals.",
        ),
        _ => None,
    }
}
