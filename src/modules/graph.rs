//! Module dependency graph, cycle detection, and topological stratification.

use super::resolver::{ResolveError, resolve_module_path};
use crate::ast::Program;
use crate::parser::{ParseError, parse_program};
use std::collections::{HashMap, HashSet, VecDeque};
use std::fs;
use std::path::{Path, PathBuf};

#[derive(Debug, Clone, PartialEq, Eq, Hash, PartialOrd, Ord)]
pub struct ModuleId(pub PathBuf);

impl ModuleId {
    pub fn new(path: PathBuf) -> Self {
        Self(path)
    }

    pub fn path(&self) -> &Path {
        &self.0
    }

    pub fn module_ident(&self) -> String {
        crate::modules::mangling::module_ident_from_path(&self.0)
    }
}

impl std::fmt::Display for ModuleId {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "{}", self.0.display())
    }
}

#[derive(Debug, Clone)]
pub struct ModuleNode {
    pub id: ModuleId,
    pub program: Program,
    pub source: String,
    pub dependencies: Vec<ModuleId>,
    pub library_path: Option<PathBuf>,
}

#[derive(Debug)]
pub enum GraphError {
    Resolve(ResolveError),
    Parse {
        path: PathBuf,
        errors: Vec<ParseError>,
    },
    CircularDependency {
        cycle: Vec<PathBuf>,
    },
    Io {
        path: PathBuf,
        message: String,
    },
}

impl From<ResolveError> for GraphError {
    fn from(err: ResolveError) -> Self {
        Self::Resolve(err)
    }
}

impl std::fmt::Display for GraphError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::Resolve(e) => write!(f, "{e}"),
            Self::Parse { path, errors } => {
                write!(
                    f,
                    "Failed to parse '{}':\n{}",
                    path.display(),
                    errors
                        .iter()
                        .map(|e| format!("  - {e}"))
                        .collect::<Vec<_>>()
                        .join("\n")
                )
            }
            Self::CircularDependency { cycle } => {
                let cycle_str = cycle
                    .iter()
                    .map(|p| p.display().to_string())
                    .collect::<Vec<_>>()
                    .join(" -> ");
                write!(f, "Circular dependency detected: {cycle_str}")
            }
            Self::Io { path, message } => {
                write!(f, "IO error on '{}': {}", path.display(), message)
            }
        }
    }
}

impl std::error::Error for GraphError {}

#[derive(Debug, Clone)]
pub struct ModuleGraph {
    pub entry: ModuleId,
    pub modules: HashMap<ModuleId, ModuleNode>,
    pub topo_order: Vec<ModuleId>,
    pub topo_waves: Vec<Vec<ModuleId>>,
}

impl ModuleGraph {
    /// Builds and validates the complete module dependency graph starting from an entry file.
    pub fn build(entry_path: &Path) -> Result<Self, GraphError> {
        let entry_canonical = resolve_module_path(entry_path.to_str().unwrap(), None)?;
        let entry_id = ModuleId::new(entry_canonical);

        let mut modules = HashMap::new();
        let mut queue = VecDeque::new();
        let mut visited = HashSet::new();

        queue.push_back(entry_id.clone());
        visited.insert(entry_id.clone());

        while let Some(current_id) = queue.pop_front() {
            let file_path = current_id.path();
            let source = fs::read_to_string(file_path).map_err(|e| GraphError::Io {
                path: file_path.to_path_buf(),
                message: e.to_string(),
            })?;

            let program = parse_program(&source).map_err(|errors| GraphError::Parse {
                path: file_path.to_path_buf(),
                errors,
            })?;

            // Resolve dynamic library path if present
            let library_path = if let Some(lib_directive) = &program.library {
                let raw_lib = Path::new(&lib_directive.node);
                let resolved_lib = if raw_lib.is_relative() {
                    file_path
                        .parent()
                        .map(|p| p.join(raw_lib))
                        .unwrap_or_else(|| raw_lib.to_path_buf())
                } else {
                    raw_lib.to_path_buf()
                };
                Some(resolved_lib)
            } else {
                None
            };

            // Discover all imported dependencies
            let mut dependencies = Vec::new();
            for import_decl in &program.imports {
                let dep_path = resolve_module_path(&import_decl.node.source, Some(file_path))?;
                let dep_id = ModuleId::new(dep_path);
                dependencies.push(dep_id.clone());

                if visited.insert(dep_id.clone()) {
                    queue.push_back(dep_id);
                }
            }

            // Also check re-exports with 'from' sources
            for export_decl in &program.exports {
                let reexport_src = match &export_decl.node {
                    crate::ast::ExportDecl::Named {
                        source: Some(src), ..
                    } => Some(src.as_str()),
                    crate::ast::ExportDecl::All { source, .. } => Some(source.as_str()),
                    _ => None,
                };

                if let Some(src) = reexport_src {
                    let dep_path = resolve_module_path(src, Some(file_path))?;
                    let dep_id = ModuleId::new(dep_path);
                    if !dependencies.contains(&dep_id) {
                        dependencies.push(dep_id.clone());
                    }
                    if visited.insert(dep_id.clone()) {
                        queue.push_back(dep_id);
                    }
                }
            }

            modules.insert(
                current_id.clone(),
                ModuleNode {
                    id: current_id,
                    program,
                    source,
                    dependencies,
                    library_path,
                },
            );
        }

        // Cycle detection and topological ordering
        let (topo_order, topo_waves) = Self::compute_topological_sort(&entry_id, &modules)?;

        Ok(Self {
            entry: entry_id,
            modules,
            topo_order,
            topo_waves,
        })
    }

    /// Computes topological order (dependencies first) and stratified waves for parallel execution.
    fn compute_topological_sort(
        entry: &ModuleId,
        modules: &HashMap<ModuleId, ModuleNode>,
    ) -> Result<(Vec<ModuleId>, Vec<Vec<ModuleId>>), GraphError> {
        // 0: White (unvisited), 1: Gray (visiting), 2: Black (visited)
        let mut state: HashMap<&ModuleId, u8> = HashMap::new();
        let mut path_stack: Vec<&ModuleId> = Vec::new();
        let mut post_order: Vec<ModuleId> = Vec::new();

        fn dfs<'a>(
            node: &'a ModuleId,
            modules: &'a HashMap<ModuleId, ModuleNode>,
            state: &mut HashMap<&'a ModuleId, u8>,
            path_stack: &mut Vec<&'a ModuleId>,
            post_order: &mut Vec<ModuleId>,
        ) -> Result<(), GraphError> {
            state.insert(node, 1);
            path_stack.push(node);

            if let Some(mod_node) = modules.get(node) {
                for dep in &mod_node.dependencies {
                    match state.get(dep).copied().unwrap_or(0) {
                        1 => {
                            // Cycle detected!
                            let cycle_start = path_stack.iter().position(|&p| p == dep).unwrap();
                            let mut cycle: Vec<PathBuf> = path_stack[cycle_start..]
                                .iter()
                                .map(|p| p.0.clone())
                                .collect();
                            cycle.push(dep.0.clone());
                            return Err(GraphError::CircularDependency { cycle });
                        }
                        0 => {
                            dfs(dep, modules, state, path_stack, post_order)?;
                        }
                        _ => {}
                    }
                }
            }

            path_stack.pop();
            state.insert(node, 2);
            post_order.push(node.clone());
            Ok(())
        }

        dfs(entry, modules, &mut state, &mut path_stack, &mut post_order)?;

        // post_order has dependencies before dependents (bottom-up)
        let topo_order = post_order.clone();

        // Compute stratified waves based on depth in DAG
        // depth(leaf) = 0, depth(u) = 1 + max(depth(dep))
        let mut depths: HashMap<&ModuleId, usize> = HashMap::new();
        for node_id in &topo_order {
            let max_dep_depth = modules
                .get(node_id)
                .map(|n| {
                    n.dependencies
                        .iter()
                        .filter_map(|d| depths.get(d).copied())
                        .max()
                        .map(|d| d + 1)
                        .unwrap_or(0)
                })
                .unwrap_or(0);
            depths.insert(node_id, max_dep_depth);
        }

        let max_depth = depths.values().copied().max().unwrap_or(0);
        let mut topo_waves = vec![Vec::new(); max_depth + 1];
        for (node_id, &depth) in &depths {
            topo_waves[depth].push((*node_id).clone());
        }

        Ok((topo_order, topo_waves))
    }
}
