//! Go to Definition provider for variables, functions, types, traits, and modules.
//!
//! Fully supports navigating into imported symbols from external files,
//! re-exported symbols, and standard library modules.

use super::diagnostics::{ResolvedImport, resolve_import};
use super::document::{Document, DocumentStore, LineIndex};
use crate::ast::{Declaration, ImportClause, Program, TypeDef};
use crate::parser::parse_program;
use tower_lsp::lsp_types::{GotoDefinitionResponse, Location, Position, Range, Url};

/// Finds the definition location for the symbol at the given position without store.
pub fn definition_at(doc: &Document, position: Position) -> Option<GotoDefinitionResponse> {
    definition_with_store(doc, position, None)
}

/// Finds the definition location for the symbol at the given position,
/// resolving cross-file imports using DocumentStore and disk.
pub fn definition_with_store(
    doc: &Document,
    position: Position,
    store: Option<&DocumentStore>,
) -> Option<GotoDefinitionResponse> {
    let offset = doc.line_index.position_to_offset(position);
    let word = get_word_at_offset(&doc.text, offset)?;

    // 1. Check if hovering/clicking on an import statement
    if let Some(program) = &doc.program {
        for import_decl in &program.imports {
            if import_decl.span.start <= offset && offset <= import_decl.span.end {
                match &import_decl.node.clause {
                    ImportClause::Named(specifiers) => {
                        for spec in specifiers {
                            let local_name = spec.alias.as_ref().unwrap_or(&spec.name);
                            if &word == local_name || word == spec.name {
                                return goto_symbol_in_imported_module(
                                    &spec.name,
                                    &import_decl.node.source,
                                    doc,
                                    store,
                                );
                            }
                        }
                    }
                    ImportClause::Namespace(alias) => {
                        if &word == alias {
                            return goto_imported_module_file(&import_decl.node.source, doc, store);
                        }
                    }
                    ImportClause::SideEffect => {}
                }
                // Fallback: cursor is on source string or keyword
                return goto_imported_module_file(&import_decl.node.source, doc, store);
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

    // 6. Check if symbol was imported from an external module
    if let Some(program) = &doc.program {
        for import_decl in &program.imports {
            match &import_decl.node.clause {
                ImportClause::Named(specifiers) => {
                    for spec in specifiers {
                        let local_name = spec.alias.as_ref().unwrap_or(&spec.name);
                        if local_name == &word {
                            return goto_symbol_in_imported_module(
                                &spec.name,
                                &import_decl.node.source,
                                doc,
                                store,
                            );
                        }
                    }
                }
                ImportClause::Namespace(alias) => {
                    if is_preceded_by_namespace(&doc.text, offset, alias) {
                        return goto_symbol_in_imported_module(
                            &word,
                            &import_decl.node.source,
                            doc,
                            store,
                        );
                    }
                }
                ImportClause::SideEffect => {}
            }
        }
    }

    // 7. Check function in Environment (local fallback)
    if let Some(env) = &doc.env
        && let Some(sig) = env.lookup_function(&word)
        && sig.span.end <= doc.text.len()
    {
        let range = doc.line_index.span_to_range(sig.span);
        return Some(GotoDefinitionResponse::Scalar(Location {
            uri: doc.uri.clone(),
            range,
        }));
    }

    None
}

/// Navigates to the definition of a specific symbol inside an imported module.
fn goto_symbol_in_imported_module(
    symbol: &str,
    source: &str,
    doc: &Document,
    store: Option<&DocumentStore>,
) -> Option<GotoDefinitionResponse> {
    let current_file_path = doc.uri.to_file_path().ok();
    let resolved = resolve_import(source, current_file_path.as_deref(), store).ok()?;

    match resolved {
        ResolvedImport::Std(std_name) => {
            let (source_text, uri_str) = if std_name == "std:io" {
                (crate::modules::stdlib::STD_IO_SOURCE, "modus://std/io.mds")
            } else if std_name == "std:fs" {
                (crate::modules::stdlib::STD_FS_SOURCE, "modus://std/fs.mds")
            } else {
                return None;
            };
            let line_index = LineIndex::new(source_text);
            let prog = parse_program(source_text).ok()?;
            let range = find_symbol_range_in_program(&prog, symbol, &line_index)?;
            let uri = Url::parse(uri_str).unwrap();
            Some(GotoDefinitionResponse::Scalar(Location { uri, range }))
        }
        ResolvedImport::File { path, url } => {
            let (text, line_index, program) = if let Some(store) = store
                && let Some(target_doc) = store.get(&url)
            {
                let prog = if let Some(p) = &target_doc.program {
                    p.clone()
                } else {
                    parse_program(&target_doc.text).ok()?
                };
                (target_doc.text.clone(), target_doc.line_index.clone(), prog)
            } else {
                let text = std::fs::read_to_string(&path).ok()?;
                let line_index = LineIndex::new(&text);
                let prog = parse_program(&text).ok()?;
                (text, line_index, prog)
            };

            if let Some(range) = find_symbol_range_in_program(&program, symbol, &line_index) {
                Some(GotoDefinitionResponse::Scalar(Location { uri: url, range }))
            } else {
                // Check if target file re-exports symbol from another module
                for export_decl in &program.exports {
                    if let crate::ast::ExportDecl::Named {
                        specifiers,
                        source: Some(reexp_source),
                    } = &export_decl.node
                    {
                        for spec in specifiers {
                            let exp_name = spec.alias.as_ref().unwrap_or(&spec.name);
                            if exp_name == symbol {
                                let dummy_doc = Document::new(url.clone(), 0, text.clone());
                                return goto_symbol_in_imported_module(
                                    &spec.name,
                                    reexp_source,
                                    &dummy_doc,
                                    store,
                                );
                            }
                        }
                    }
                }

                // Fallback to beginning of target module
                Some(GotoDefinitionResponse::Scalar(Location {
                    uri: url,
                    range: Range::default(),
                }))
            }
        }
    }
}

/// Navigates to the file of an imported module.
fn goto_imported_module_file(
    source: &str,
    doc: &Document,
    store: Option<&DocumentStore>,
) -> Option<GotoDefinitionResponse> {
    let current_file_path = doc.uri.to_file_path().ok();
    let resolved = resolve_import(source, current_file_path.as_deref(), store).ok()?;
    match resolved {
        ResolvedImport::Std(_) => {
            let uri = Url::parse("modus://std/io.mds").unwrap();
            Some(GotoDefinitionResponse::Scalar(Location {
                uri,
                range: Range::default(),
            }))
        }
        ResolvedImport::File { url, .. } => Some(GotoDefinitionResponse::Scalar(Location {
            uri: url,
            range: Range::default(),
        })),
    }
}

/// Finds the declaration range of a symbol name in an AST Program.
fn find_symbol_range_in_program(
    program: &Program,
    symbol: &str,
    line_index: &LineIndex,
) -> Option<Range> {
    for decl in &program.declarations {
        match &decl.node {
            Declaration::Function(f) if f.name == symbol => {
                return Some(line_index.span_to_range(decl.span));
            }
            Declaration::Type(t) if t.name == symbol => {
                return Some(line_index.span_to_range(decl.span));
            }
            Declaration::Trait(tr) if tr.name == symbol => {
                return Some(line_index.span_to_range(decl.span));
            }
            Declaration::Extern(ext) => {
                for f in &ext.functions {
                    if f.node.name == symbol {
                        return Some(line_index.span_to_range(f.span));
                    }
                }
            }
            Declaration::Type(t) => {
                if let TypeDef::Union(variants) = &t.definition.node {
                    for v in variants {
                        if v.name == symbol {
                            return Some(line_index.span_to_range(decl.span));
                        }
                    }
                }
            }
            _ => {}
        }
    }
    None
}

/// Checks if an identifier at offset is preceded by `alias.`
fn is_preceded_by_namespace(text: &str, offset: usize, alias: &str) -> bool {
    let bytes = text.as_bytes();
    let mut i = offset.min(text.len().saturating_sub(1));
    while i > 0 && is_ident_char(bytes[i]) {
        i -= 1;
    }
    while i > 0 && bytes[i].is_ascii_whitespace() {
        i -= 1;
    }
    if bytes[i] != b'.' {
        return false;
    }
    if i == 0 {
        return false;
    }
    i -= 1;
    while i > 0 && bytes[i].is_ascii_whitespace() {
        i -= 1;
    }
    let end = i + 1;
    while i > 0 && is_ident_char(bytes[i - 1]) {
        i -= 1;
    }
    &text[i..end] == alias
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
