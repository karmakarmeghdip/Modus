//! Diagnostics computation for syntax, purity, and semantic type errors.
//!
//! Handles full cross-module import resolution, dependency interface loading,
//! and standard library integration (`std:io`).

use super::document::{DocumentStore, LineIndex};
use crate::ast::Program;
use crate::modules::ModuleInterface;
use crate::modules::graph::ModuleId;
use crate::modules::resolver::ResolveError;
use crate::modules::stdlib::is_std_module;
use crate::parser::parse_program;
use crate::typechecker::{Environment, check_program_with_env};
use std::collections::{HashMap, HashSet};
use std::path::{Path, PathBuf};
use std::sync::OnceLock;
use tower_lsp::lsp_types::{Diagnostic, DiagnosticSeverity, Range, Url};

/// Represents a resolved import target (either standard library or local file).
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ResolvedImport {
    Std(String),
    File { path: PathBuf, url: Url },
}

/// Resolves an import source string relative to the importing file and open documents.
pub fn resolve_import(
    source: &str,
    current_file: Option<&Path>,
    store: Option<&DocumentStore>,
) -> Result<ResolvedImport, ResolveError> {
    if source.starts_with("std:") {
        if is_std_module(source) {
            return Ok(ResolvedImport::Std(source.to_string()));
        } else {
            return Err(ResolveError::UnknownStdModule {
                module: source.to_string(),
            });
        }
    }

    let raw_path = PathBuf::from(source);
    let target = if raw_path.is_relative() {
        if let Some(parent) = current_file.and_then(|p| p.parent()) {
            parent.join(&raw_path)
        } else {
            raw_path
        }
    } else {
        raw_path
    };

    let target = if target.extension().is_none() {
        target.with_extension("mds")
    } else {
        target
    };

    let ext = target.extension().and_then(|e| e.to_str()).unwrap_or("");
    if ext != "mds" && ext != "mdsi" {
        return Err(ResolveError::InvalidExtension { path: target });
    }

    if let Ok(canon) = std::fs::canonicalize(&target)
        && let Ok(url) = Url::from_file_path(&canon)
    {
        return Ok(ResolvedImport::File { path: canon, url });
    }

    if let Some(store) = store
        && let Some(doc) = store.get_by_path(&target)
    {
        return Ok(ResolvedImport::File {
            path: target,
            url: doc.uri,
        });
    }

    Err(ResolveError::FileNotFound {
        path: target,
        imported_from: current_file.map(|p| p.to_path_buf()),
    })
}

static STD_IO_INTERFACE: OnceLock<Result<ModuleInterface, String>> = OnceLock::new();
static STD_FS_INTERFACE: OnceLock<Result<ModuleInterface, String>> = OnceLock::new();
static STD_ENV_INTERFACE: OnceLock<Result<ModuleInterface, String>> = OnceLock::new();
static STD_PROCESS_INTERFACE: OnceLock<Result<ModuleInterface, String>> = OnceLock::new();
static STD_PRELUDE_INTERFACE: OnceLock<Result<ModuleInterface, String>> = OnceLock::new();

/// Computes or retrieves the static ModuleInterface for `std:io`.
pub fn get_std_io_interface() -> Result<ModuleInterface, String> {
    STD_IO_INTERFACE
        .get_or_init(|| {
            let prog = parse_program(crate::modules::stdlib::STD_IO_SOURCE)
                .map_err(|e| format!("Failed to parse std:io: {e:?}"))?;
            let mut env = Environment::new();
            check_program_with_env(&prog, &mut env)
                .map_err(|e| format!("Failed to typecheck std:io: {e:?}"))?;
            ModuleInterface::extract(
                ModuleId::new(PathBuf::from("std:io")),
                &prog,
                &env,
                None,
                &HashMap::new(),
            )
            .map_err(|e| format!("Failed to extract std:io interface: {e:?}"))
        })
        .clone()
}

/// Computes or retrieves the static ModuleInterface for `std:fs`.
pub fn get_std_fs_interface() -> Result<ModuleInterface, String> {
    STD_FS_INTERFACE
        .get_or_init(|| {
            let prog = parse_program(crate::modules::stdlib::STD_FS_SOURCE)
                .map_err(|e| format!("Failed to parse std:fs: {e:?}"))?;
            let mut env = Environment::new();
            check_program_with_env(&prog, &mut env)
                .map_err(|e| format!("Failed to typecheck std:fs: {e:?}"))?;
            ModuleInterface::extract(
                ModuleId::new(PathBuf::from("std:fs")),
                &prog,
                &env,
                None,
                &HashMap::new(),
            )
            .map_err(|e| format!("Failed to extract std:fs interface: {e:?}"))
        })
        .clone()
}

/// Computes or retrieves the static ModuleInterface for `std:env`.
pub fn get_std_env_interface() -> Result<ModuleInterface, String> {
    STD_ENV_INTERFACE
        .get_or_init(|| {
            let prog = parse_program(crate::modules::stdlib::STD_ENV_SOURCE)
                .map_err(|e| format!("Failed to parse std:env: {e:?}"))?;
            let mut env = Environment::new();
            check_program_with_env(&prog, &mut env)
                .map_err(|e| format!("Failed to typecheck std:env: {e:?}"))?;
            ModuleInterface::extract(
                ModuleId::new(PathBuf::from("std:env")),
                &prog,
                &env,
                None,
                &HashMap::new(),
            )
            .map_err(|e| format!("Failed to extract std:env interface: {e:?}"))
        })
        .clone()
}

/// Computes or retrieves the static ModuleInterface for `std:prelude`.
/// Its exported trait impls (`Eq`/`Add` for `String`) are registered into
/// every diagnostic environment so operator strictness checking matches the
/// compiler.
pub fn get_std_prelude_interface() -> Result<ModuleInterface, String> {
    STD_PRELUDE_INTERFACE
        .get_or_init(|| {
            let graph = crate::modules::ModuleGraph::build_from_source(
                std::path::Path::new(crate::modules::stdlib::STD_PRELUDE),
                crate::modules::stdlib::STD_PRELUDE_SOURCE,
            )
            .map_err(|e| format!("Failed to build std:prelude graph: {e}"))?;
            let (interfaces, _) = crate::modules::check_module_graph_with_envs(&graph)
                .map_err(|e| format!("Failed to typecheck std:prelude: {e:?}"))?;
            interfaces
                .get(&ModuleId::new(PathBuf::from(
                    crate::modules::stdlib::STD_PRELUDE,
                )))
                .cloned()
                .ok_or_else(|| "Missing std:prelude interface".to_string())
        })
        .clone()
}

/// Registers the prelude's operator trait impls into an environment.
/// Best-effort: if the prelude fails to load, strictness checking falls back
/// to rejecting non-primitive operators (same as having no impls).
fn register_prelude_impls(env: &mut Environment) {
    if let Ok(iface) = get_std_prelude_interface() {
        for impl_def in &iface.exported_impls {
            env.register_impl(impl_def.clone());
        }
    }
}

/// Computes or retrieves the static ModuleInterface for `std:process`.
pub fn get_std_process_interface() -> Result<ModuleInterface, String> {
    STD_PROCESS_INTERFACE
        .get_or_init(|| {
            let prog = parse_program(crate::modules::stdlib::STD_PROCESS_SOURCE)
                .map_err(|e| format!("Failed to parse std:process: {e:?}"))?;
            let mut env = Environment::new();
            check_program_with_env(&prog, &mut env)
                .map_err(|e| format!("Failed to typecheck std:process: {e:?}"))?;
            ModuleInterface::extract(
                ModuleId::new(PathBuf::from("std:process")),
                &prog,
                &env,
                None,
                &HashMap::new(),
            )
            .map_err(|e| format!("Failed to extract std:process interface: {e:?}"))
        })
        .clone()
}

/// Loads or compiles the ModuleInterface for a resolved module.
pub fn load_module_interface(
    resolved: &ResolvedImport,
    store: Option<&DocumentStore>,
    visiting: &mut HashSet<PathBuf>,
) -> Result<ModuleInterface, String> {
    match resolved {
        ResolvedImport::Std(std_name) => {
            if std_name == "std:io" {
                get_std_io_interface()
            } else if std_name == "std:fs" {
                get_std_fs_interface()
            } else if std_name == "std:env" {
                get_std_env_interface()
            } else if std_name == "std:process" {
                get_std_process_interface()
            } else {
                Err(format!("Unknown standard library module '{std_name}'"))
            }
        }
        ResolvedImport::File { path, url } => {
            if visiting.contains(path) {
                return Err(format!(
                    "Circular dependency detected involving '{}'",
                    path.display()
                ));
            }

            if let Some(store) = store {
                if let Some(iface) = store.get_cached_interface(path) {
                    return Ok(iface);
                }
                if let Some(doc) = store.get(url)
                    && let Some(iface) = &doc.interface
                {
                    return Ok(iface.clone());
                }
            }

            let source = if let Some(store) = store
                && let Some(text) = store.get_text_by_path(path)
            {
                text
            } else {
                std::fs::read_to_string(path)
                    .map_err(|e| format!("Cannot read module file '{}': {e}", path.display()))?
            };

            let program = parse_program(&source)
                .map_err(|_| format!("Syntax errors in imported module '{}'", path.display()))?;

            visiting.insert(path.clone());

            let mut dep_env = Environment::new();
            register_prelude_impls(&mut dep_env);
            let mut dep_interfaces = HashMap::new();
            for sub_import in &program.imports {
                if let Ok(sub_resolved) = resolve_import(&sub_import.node.source, Some(path), store)
                    && let Ok(sub_iface) = load_module_interface(&sub_resolved, store, visiting)
                {
                    let _ = sub_iface.import_into(
                        &mut dep_env,
                        &sub_import.node.clause,
                        sub_import.span,
                    );
                    dep_interfaces.insert(sub_iface.module_id.clone(), sub_iface);
                }
            }

            let _ = check_program_with_env(&program, &mut dep_env);

            visiting.remove(path);

            let iface = ModuleInterface::extract(
                ModuleId::new(path.clone()),
                &program,
                &dep_env,
                program.library.as_ref().map(|l| PathBuf::from(&l.node)),
                &dep_interfaces,
            )
            .map_err(|e| e.kind.to_string())?;

            if let Some(store) = store {
                store.cache_interface(path.clone(), iface.clone());
            }

            Ok(iface)
        }
    }
}

/// Convenience wrapper for diagnostics without store or uri (backwards-compatible with tests).
pub fn compute_diagnostics(
    text: &str,
    line_index: &LineIndex,
) -> (Option<Program>, Option<Environment>, Vec<Diagnostic>) {
    let (prog, env, _, diags) = compute_diagnostics_with_imports(text, line_index, None, None);
    (prog, env, diags)
}

/// Computes both syntactic and semantic diagnostics for Modus source code,
/// resolving and ingesting imported dependencies from open documents and disk.
pub fn compute_diagnostics_with_imports(
    text: &str,
    line_index: &LineIndex,
    doc_uri: Option<&Url>,
    store: Option<&DocumentStore>,
) -> (
    Option<Program>,
    Option<Environment>,
    Option<ModuleInterface>,
    Vec<Diagnostic>,
) {
    let mut diagnostics = Vec::new();

    // 1. Syntactic Parsing Phase (Chumsky lexer + parser)
    match parse_program(text) {
        Ok(program) => {
            let mut env = Environment::new();
            let mut visiting = HashSet::new();

            let current_file_path = doc_uri.and_then(|u| u.to_file_path().ok());
            if let Some(path) = &current_file_path {
                if let Ok(canon) = std::fs::canonicalize(path) {
                    visiting.insert(canon);
                } else {
                    visiting.insert(path.clone());
                }
            }

            // 2. Resolve and ingest imported dependencies
            let mut dep_interfaces = HashMap::new();
            register_prelude_impls(&mut env);
            for import_decl in &program.imports {
                match resolve_import(
                    &import_decl.node.source,
                    current_file_path.as_deref(),
                    store,
                ) {
                    Ok(resolved) => match load_module_interface(&resolved, store, &mut visiting) {
                        Ok(dep_interface) => {
                            if let Err(type_err) = dep_interface.import_into(
                                &mut env,
                                &import_decl.node.clause,
                                import_decl.span,
                            ) {
                                let range = line_index.span_to_range(import_decl.span);
                                diagnostics.push(Diagnostic {
                                    range,
                                    severity: Some(DiagnosticSeverity::ERROR),
                                    code: None,
                                    code_description: None,
                                    source: Some("modus".to_string()),
                                    message: type_err.kind.to_string(),
                                    related_information: None,
                                    tags: None,
                                    data: None,
                                });
                            }
                            dep_interfaces.insert(dep_interface.module_id.clone(), dep_interface);
                        }
                        Err(err_msg) => {
                            let range = line_index.span_to_range(import_decl.span);
                            diagnostics.push(Diagnostic {
                                range,
                                severity: Some(DiagnosticSeverity::ERROR),
                                code: None,
                                code_description: None,
                                source: Some("modus".to_string()),
                                message: err_msg,
                                related_information: None,
                                tags: None,
                                data: None,
                            });
                        }
                    },
                    Err(resolve_err) => {
                        let range = line_index.span_to_range(import_decl.span);
                        diagnostics.push(Diagnostic {
                            range,
                            severity: Some(DiagnosticSeverity::ERROR),
                            code: None,
                            code_description: None,
                            source: Some("modus".to_string()),
                            message: resolve_err.to_string(),
                            related_information: None,
                            tags: None,
                            data: None,
                        });
                    }
                }
            }

            // 3. Semantic Analysis & Typechecking Phase
            let check_result = check_program_with_env(&program, &mut env);
            if let Err(type_error) = check_result {
                let range = match type_error.span {
                    Some(span) => line_index.span_to_range(span),
                    None => Range::default(),
                };

                diagnostics.push(Diagnostic {
                    range,
                    severity: Some(DiagnosticSeverity::ERROR),
                    code: None,
                    code_description: None,
                    source: Some("modus".to_string()),
                    message: type_error.kind.to_string(),
                    related_information: None,
                    tags: None,
                    data: None,
                });
            }

            // 4. Extract module interface for current program
            let current_module_id = ModuleId::new(
                current_file_path
                    .clone()
                    .unwrap_or_else(|| PathBuf::from("entry.mds")),
            );
            let interface = ModuleInterface::extract(
                current_module_id,
                &program,
                &env,
                program.library.as_ref().map(|l| PathBuf::from(&l.node)),
                &dep_interfaces,
            )
            .ok();

            (Some(program), Some(env), interface, diagnostics)
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

            (None, None, None, diagnostics)
        }
    }
}

/// Formats raw chumsky/parser errors into clean, readable messages for LSP clients.
fn format_parse_error(raw_msg: &str) -> String {
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

    #[test]
    fn test_stdlib_io_import_diagnostics() {
        let text = r#"
import { println } from "std:io";

function main(): IO(void) {
    perform println("Hello, world!");
    return IO.pure(());
}
"#;
        let line_index = LineIndex::new(text);
        let (prog, env, diags) = compute_diagnostics(text, &line_index);
        assert!(prog.is_some());
        assert!(env.is_some());
        assert!(
            diags.is_empty(),
            "stdlib import should produce 0 diagnostics, got: {:?}",
            diags
        );
    }
}
