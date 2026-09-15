pub mod ast;
pub mod backend;
pub mod desugar;
pub mod ir;
pub mod lsp;
pub mod modules;
pub mod parser;
pub mod typechecker;

use backend::CodeGen;
use desugar::{DesugaredDecl, DesugaredProgram};
use inkwell::context::Context;
use modules::{ModuleGraph, ModuleId, check_module_graph_with_envs, stdlib};
use std::collections::HashSet;
use std::path::{Path, PathBuf};

/// Compiles Modus source code through the complete compiler pipeline into an LLVM CodeGen instance.
///
/// Single-module mode: the entry source is compiled on its own, but the
/// `std:prelude` subtree is loaded and its definitions merged in, so
/// operator impls (e.g. `Eq`/`Add` for `String`) behave exactly as in graph
/// mode.
pub fn compile_source<'ctx>(
    context: &'ctx Context,
    source: &str,
    module_name: &str,
) -> Result<CodeGen<'ctx>, String> {
    let program = parser::parse_program(source).map_err(|e| format!("Parse error: {e:?}"))?;

    // Load the prelude first: its trait impls must be visible while checking
    // (strictness: `==`/`+` require an impl), and its definitions are merged
    // into this module below.
    let prelude = load_prelude_definitions().map_err(|e| format!("Prelude error: {e}"))?;
    let mut env = typechecker::Environment::new();
    for impl_def in &prelude.impls {
        env.register_impl(impl_def.clone());
    }
    typechecker::check_program_with_env(&program, &mut env)
        .map_err(|e| format!("Type error: {e:?}"))?;

    let mut desugared = desugar::desugar_program(&program, &env);

    // Merge the prelude's compiled definitions (operator impls and the
    // stdlib functions they call) into this module.
    let mut local_symbols: HashSet<String> = HashSet::new();
    for decl in &desugared.declarations {
        collect_def_names(decl, &mut local_symbols);
    }
    for decl in &prelude.desugared.declarations {
        collect_def_names(decl, &mut local_symbols);
    }
    desugared
        .declarations
        .extend(prelude.desugared.declarations);
    desugared
        .extern_functions
        .retain(|e| !local_symbols.contains(&e.symbol_name));
    for ext in prelude.desugared.extern_functions {
        if !local_symbols.contains(&ext.symbol_name) {
            desugared.extern_functions.push(ext);
        }
    }

    let mut anf = ir::lower_program(&desugared);
    ir::convert_closures(&mut anf);
    ir::apply_perceus_and_fbip(&mut anf);
    let mut codegen = CodeGen::new(context, module_name);
    codegen.compile_program(&anf)?;
    Ok(codegen)
}

struct PreludeDefinitions {
    desugared: DesugaredProgram,
    impls: Vec<typechecker::ImplDef>,
}

/// Builds, checks, and desugars the `std:prelude` module and its dependencies,
/// returning their definitions (merged into one desugared program) and the
/// prelude's exported trait impls.
fn load_prelude_definitions() -> Result<PreludeDefinitions, String> {
    let prelude_id = prelude_module_id();
    let graph =
        ModuleGraph::build_from_source(Path::new(stdlib::STD_PRELUDE), stdlib::STD_PRELUDE_SOURCE)
            .map_err(|e| e.to_string())?;
    let (interfaces, envs) =
        check_module_graph_with_envs(&graph).map_err(|e| format!("Type error: {e:?}"))?;

    // The prelude is a pure barrel: its impls come from its interface
    // (merged from its home modules via `export *`).
    let impls: Vec<typechecker::ImplDef> = interfaces
        .get(&prelude_id)
        .map(|iface| iface.exported_impls.clone())
        .unwrap_or_default();

    let mut merged = DesugaredProgram {
        declarations: Vec::new(),
        extern_functions: Vec::new(),
    };
    for module_id in &graph.topo_order {
        let node = graph.modules.get(module_id).unwrap();
        let env = envs.get(module_id).unwrap();
        let d = desugar::desugar_program(&node.program, env);
        merged.declarations.extend(d.declarations);
        merged.extern_functions.extend(d.extern_functions);
    }

    let mut local_symbols: HashSet<String> = HashSet::new();
    for decl in &merged.declarations {
        collect_def_names(decl, &mut local_symbols);
    }
    merged
        .extern_functions
        .retain(|e| !local_symbols.contains(&e.symbol_name));

    Ok(PreludeDefinitions {
        desugared: merged,
        impls,
    })
}

fn collect_def_names(decl: &DesugaredDecl, out: &mut HashSet<String>) {
    match decl {
        DesugaredDecl::Function(f) => {
            out.insert(f.name.clone());
        }
        DesugaredDecl::Impl(im) => {
            for m in &im.methods {
                out.insert(m.name.clone());
            }
        }
        DesugaredDecl::Type(_) | DesugaredDecl::Trait(_) => {}
    }
}

/// Convenience accessor for the prelude module id (used by tests).
pub fn prelude_module_id() -> ModuleId {
    ModuleId::new(PathBuf::from(stdlib::STD_PRELUDE))
}
