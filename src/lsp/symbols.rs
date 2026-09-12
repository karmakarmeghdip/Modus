//! Document symbol hierarchy and outline extraction for Modus programs.

use super::document::LineIndex;
use crate::ast::{Declaration, FunctionDecl, ImplDecl, Program, TraitDecl, TypeDecl, TypeDef};
use tower_lsp::lsp_types::{DocumentSymbol, DocumentSymbolResponse, SymbolKind};

/// Extracts the hierarchical document symbol outline for an AST Program.
pub fn document_symbols(
    program: &Program,
    line_index: &LineIndex,
) -> Option<DocumentSymbolResponse> {
    let mut symbols = Vec::new();

    for decl in &program.declarations {
        match &decl.node {
            Declaration::Function(f) => {
                symbols.push(function_symbol(f, decl.span, line_index));
            }
            Declaration::Type(t) => {
                symbols.push(type_symbol(t, decl.span, line_index));
            }
            Declaration::Trait(tr) => {
                symbols.push(trait_symbol(tr, decl.span, line_index));
            }
            Declaration::Impl(i) => {
                symbols.push(impl_symbol(i, decl.span, line_index));
            }
            Declaration::Extern(ext) => {
                for f in &ext.functions {
                    symbols.push(function_symbol(&f.node, f.span, line_index));
                }
            }
        }
    }

    Some(DocumentSymbolResponse::Nested(symbols))
}

fn function_symbol(
    f: &FunctionDecl,
    span: crate::ast::Span,
    line_index: &LineIndex,
) -> DocumentSymbol {
    let range = line_index.span_to_range(span);
    let selection_range = range; // Name span approximation

    let params: Vec<String> = f
        .params
        .iter()
        .map(|p| format!("{}: {}", p.name, format_type(&p.ty.node)))
        .collect();

    let detail = match &f.return_type {
        Some(ret) => format!("({}): {}", params.join(", "), format_type(&ret.node)),
        None => format!("({})", params.join(", ")),
    };

    #[allow(deprecated)]
    DocumentSymbol {
        name: f.name.clone(),
        detail: Some(detail),
        kind: SymbolKind::FUNCTION,
        tags: None,
        deprecated: None,
        range,
        selection_range,
        children: None,
    }
}

fn type_symbol(t: &TypeDecl, span: crate::ast::Span, line_index: &LineIndex) -> DocumentSymbol {
    let range = line_index.span_to_range(span);
    let selection_range = range;

    let (kind, children) = match &t.definition.node {
        TypeDef::Union(variants) => {
            let var_symbols = variants
                .iter()
                .map(|v| {
                    #[allow(deprecated)]
                    DocumentSymbol {
                        name: v.name.clone(),
                        detail: None,
                        kind: SymbolKind::ENUM_MEMBER,
                        tags: None,
                        deprecated: None,
                        range,
                        selection_range,
                        children: None,
                    }
                })
                .collect();
            (SymbolKind::ENUM, Some(var_symbols))
        }
        TypeDef::Alias(_) => (SymbolKind::STRUCT, None),
    };

    #[allow(deprecated)]
    DocumentSymbol {
        name: t.name.clone(),
        detail: Some("type".to_string()),
        kind,
        tags: None,
        deprecated: None,
        range,
        selection_range,
        children,
    }
}

fn trait_symbol(tr: &TraitDecl, span: crate::ast::Span, line_index: &LineIndex) -> DocumentSymbol {
    let range = line_index.span_to_range(span);
    let selection_range = range;

    let children: Vec<DocumentSymbol> = tr
        .members
        .iter()
        .map(|m| {
            let m_range = line_index.span_to_range(m.span);
            let params: Vec<String> = m
                .node
                .params
                .iter()
                .map(|p| format!("{}: {}", p.name, format_type(&p.ty.node)))
                .collect();
            let detail = format!(
                "({}): {}",
                params.join(", "),
                format_type(&m.node.return_type.node)
            );

            #[allow(deprecated)]
            DocumentSymbol {
                name: m.node.name.clone(),
                detail: Some(detail),
                kind: SymbolKind::METHOD,
                tags: None,
                deprecated: None,
                range: m_range,
                selection_range: m_range,
                children: None,
            }
        })
        .collect();

    #[allow(deprecated)]
    DocumentSymbol {
        name: tr.name.clone(),
        detail: Some("trait".to_string()),
        kind: SymbolKind::INTERFACE,
        tags: None,
        deprecated: None,
        range,
        selection_range,
        children: Some(children),
    }
}

fn impl_symbol(i: &ImplDecl, span: crate::ast::Span, line_index: &LineIndex) -> DocumentSymbol {
    let range = line_index.span_to_range(span);
    let selection_range = range;

    let target_name = format_type(&i.target_type.node);
    let name = format!("impl {} for {}", i.trait_name, target_name);

    let children: Vec<DocumentSymbol> = i
        .methods
        .iter()
        .map(|m| {
            let mut s = function_symbol(&m.node, m.span, line_index);
            s.kind = SymbolKind::METHOD;
            s
        })
        .collect();

    #[allow(deprecated)]
    DocumentSymbol {
        name,
        detail: None,
        kind: SymbolKind::CLASS,
        tags: None,
        deprecated: None,
        range,
        selection_range,
        children: Some(children),
    }
}

fn format_type(ty: &crate::ast::Type) -> String {
    match ty {
        crate::ast::Type::Primitive(p) => format!("{p:?}").to_lowercase(),
        crate::ast::Type::Generic { name, type_args } => {
            let args: Vec<String> = type_args.iter().map(|a| format_type(&a.node)).collect();
            format!("{name}({})", args.join(", "))
        }
        crate::ast::Type::Path(parts) => parts.join("."),
        crate::ast::Type::Array(inner) => format!("[{}]", format_type(&inner.node)),
        crate::ast::Type::Record(fields) => {
            let fs: Vec<String> = fields
                .iter()
                .map(|(n, t)| format!("{n}: {}", format_type(&t.node)))
                .collect();
            format!("{{ {} }}", fs.join(", "))
        }
        crate::ast::Type::Tuple(types) => {
            let ts: Vec<String> = types.iter().map(|t| format_type(&t.node)).collect();
            format!("({})", ts.join(", "))
        }
        crate::ast::Type::Function {
            param_types,
            return_type,
        } => {
            let pts: Vec<String> = param_types.iter().map(|t| format_type(&t.node)).collect();
            format!("({}) => {}", pts.join(", "), format_type(&return_type.node))
        }
        crate::ast::Type::Unit => "()".to_string(),
    }
}
