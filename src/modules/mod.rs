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
use std::collections::{HashMap, HashSet};
use std::path::PathBuf;

pub type CheckedModuleGraph = (
    HashMap<ModuleId, ModuleInterface>,
    HashMap<ModuleId, Environment>,
);

/// All modules the prelude depends on (transitively), including itself.
fn prelude_closure(graph: &ModuleGraph) -> HashSet<ModuleId> {
    let prelude_id = ModuleId::new(PathBuf::from(super::stdlib::STD_PRELUDE));
    let mut closure = HashSet::new();
    let mut stack = vec![prelude_id];
    while let Some(cur) = stack.pop() {
        if closure.insert(cur.clone())
            && let Some(node) = graph.modules.get(&cur)
        {
            stack.extend(node.dependencies.iter().cloned());
        }
    }
    closure
}

/// Type-checks an entire ModuleGraph in topological order (dependencies first).
/// Returns a map of ModuleId -> ModuleInterface and ModuleId -> Environment for all modules in the graph.
///
/// The `std:prelude` subtree is checked first and its exported trait impls
/// (e.g. `Eq` for `String`) are registered into every other module's
/// environment, so operators like `==` resolve without an explicit import.
pub fn check_module_graph_with_envs(graph: &ModuleGraph) -> Result<CheckedModuleGraph, TypeError> {
    let mut interfaces: HashMap<ModuleId, ModuleInterface> = HashMap::new();
    let mut envs: HashMap<ModuleId, Environment> = HashMap::new();

    let has_prelude = graph
        .modules
        .contains_key(&ModuleId::new(PathBuf::from(super::stdlib::STD_PRELUDE)));
    let prelude_ids = if has_prelude {
        Some(prelude_closure(graph))
    } else {
        None
    };

    // Stable partition of the topo order: prelude subtree first (its exported
    // impls are needed by every other module's environment), relative order
    // (still topological) preserved within each group.
    let mut order: Vec<&ModuleId> = graph.topo_order.iter().collect();
    order.sort_by_key(|id| {
        if prelude_ids.as_ref().is_some_and(|ids| ids.contains(*id)) {
            0
        } else {
            1
        }
    });

    for module_id in order {
        let in_prelude = prelude_ids
            .as_ref()
            .is_some_and(|ids| ids.contains(module_id));
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

        // Register the prelude's trait impls into every non-prelude module.
        if !in_prelude
            && let Some(prelude_iface) =
                interfaces.get(&ModuleId::new(PathBuf::from(super::stdlib::STD_PRELUDE)))
        {
            for impl_def in &prelude_iface.exported_impls {
                env.register_impl(impl_def.clone());
            }
        }

        // Typecheck module
        crate::typechecker::check_program_with_env(&node.program, &mut env)?;

        // Extract interface (re-exports are merged from already-extracted
        // dependency interfaces; topo order guarantees they exist).
        let interface = ModuleInterface::extract(
            node.id.clone(),
            &node.program,
            &env,
            node.library_path.clone(),
            &interfaces,
        )?;

        // Update local function signatures with mangled symbol_name from interface
        for (name, sig) in &interface.exported_functions {
            if let Some(local_sig) = env.functions.get_mut(name) {
                local_sig.symbol_name = sig.symbol_name.clone();
            }
        }

        // Update local impl method signatures with mangled symbol_name from
        // interface so desugar emits the cross-module symbol.
        for impl_def in &interface.exported_impls {
            if let Some(impls) = env.impls.get_mut(&impl_def.trait_name)
                && let Some(local) = impls
                    .iter_mut()
                    .find(|d| d.target_type == impl_def.target_type)
            {
                for (method_name, sig) in &impl_def.methods {
                    if let Some(local_sig) = local.methods.get_mut(method_name) {
                        local_sig.symbol_name = sig.symbol_name.clone();
                    }
                }
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
