//! Module management subsystem for Modus.
//!
//! Provides:
//! - Module resolution and path canonicalization ([`resolver`])
//! - Dependency graph construction, cycle detection, and topological stratification ([`graph`])

pub mod builder;
pub mod cache;
pub mod graph;
pub mod interface;
pub mod mangling;
pub mod resolver;
pub mod stdlib;

pub use builder::{
    build_executable, build_shared_library, emit_export_map_header, jit_run_graph,
    jit_run_module_graph,
};
pub use cache::{CacheStore, compute_fingerprint, hash_source};
pub use graph::{GraphError, ModuleGraph, ModuleId, ModuleNode};
pub use interface::ModuleInterface;
pub use mangling::{is_mangled_symbol, mangle_symbol, module_ident_from_path};
pub use resolver::{ResolveError, resolve_module_path};
pub use stdlib::{get_std_module_source, is_std_module, is_std_module_path};

use crate::typechecker::Environment;
use crate::typechecker::error::{TypeError, TypeErrorKind};
use std::collections::HashMap;

pub type CheckedModuleGraph = (
    HashMap<ModuleId, ModuleInterface>,
    HashMap<ModuleId, Environment>,
);

/// Type-checks an entire ModuleGraph in topological order (dependencies first).
/// Returns a map of ModuleId -> ModuleInterface and ModuleId -> Environment for all modules in the graph.
pub fn check_module_graph_with_envs(graph: &ModuleGraph) -> Result<CheckedModuleGraph, TypeError> {
    let mut interfaces: HashMap<ModuleId, ModuleInterface> = HashMap::new();
    let mut envs: HashMap<ModuleId, Environment> = HashMap::new();

    for module_id in &graph.topo_order {
        let node = graph.modules.get(module_id).unwrap();
        let mut env = Environment::new();

        // Import all dependency interfaces into env
        for import_decl in &node.program.imports {
            let dep_path = resolve_module_path(&import_decl.node.source, Some(node.id.path()))
                .map_err(|e| {
                    TypeError::new(
                        TypeErrorKind::General(e.to_string()),
                        Some(import_decl.span),
                    )
                })?;
            let dep_id = ModuleId::new(dep_path);
            let dep_interface = interfaces.get(&dep_id).ok_or_else(|| {
                TypeError::new(
                    TypeErrorKind::General(format!("Missing dependency interface for '{dep_id}'")),
                    Some(import_decl.span),
                )
            })?;

            dep_interface.import_into(&mut env, &import_decl.node.clause, import_decl.span)?;
        }

        // Typecheck module
        crate::typechecker::check_program_with_env(&node.program, &mut env)?;

        // Extract interface
        let interface = ModuleInterface::extract(
            node.id.clone(),
            &node.program,
            &env,
            node.library_path.clone(),
        )?;

        // Update local function signatures with mangled symbol_name from interface
        for (name, sig) in &interface.exported_functions {
            if let Some(local_sig) = env.functions.get_mut(name) {
                local_sig.symbol_name = sig.symbol_name.clone();
            }
        }

        interfaces.insert(module_id.clone(), interface);
        envs.insert(module_id.clone(), env);
    }

    Ok((interfaces, envs))
}

/// Type-checks an entire ModuleGraph in topological order (dependencies first).
/// Returns a map of ModuleId -> ModuleInterface for all modules in the graph.
pub fn check_module_graph(
    graph: &ModuleGraph,
) -> Result<HashMap<ModuleId, ModuleInterface>, TypeError> {
    let (interfaces, _) = check_module_graph_with_envs(graph)?;
    Ok(interfaces)
}
