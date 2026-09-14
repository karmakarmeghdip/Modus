use super::cache::{CacheStore, compute_fingerprint, hash_source};
use super::graph::{ModuleGraph, ModuleNode};
use crate::ast::{self, Program};
use crate::backend::codegen::CodeGen;
use crate::backend::codegen::ExecutionResult;
use crate::typechecker::Environment;
use inkwell::context::Context;
use rayon::prelude::*;
use std::path::{Path, PathBuf};
use std::sync::Mutex;
use std::sync::atomic::{AtomicU64, Ordering};

static TEMP_COUNTER: AtomicU64 = AtomicU64::new(0);

fn unique_temp_dir(prefix: &str) -> Result<PathBuf, String> {
    let count = TEMP_COUNTER.fetch_add(1, Ordering::Relaxed);
    let dir = std::env::temp_dir().join(format!("{prefix}_{}_{count}", std::process::id()));
    std::fs::create_dir_all(&dir)
        .map_err(|e| format!("Failed to create temp directory {dir:?}: {e}"))?;
    Ok(dir)
}

/// Converts an AST type to its canonical Modus syntax representation string.
pub fn type_to_string(ty: &ast::Type) -> String {
    match ty {
        ast::Type::Primitive(p) => match p {
            ast::PrimitiveType::I8 => "i8".to_string(),
            ast::PrimitiveType::I16 => "i16".to_string(),
            ast::PrimitiveType::I32 => "i32".to_string(),
            ast::PrimitiveType::I64 => "i64".to_string(),
            ast::PrimitiveType::U8 => "u8".to_string(),
            ast::PrimitiveType::U16 => "u16".to_string(),
            ast::PrimitiveType::U32 => "u32".to_string(),
            ast::PrimitiveType::U64 => "u64".to_string(),
            ast::PrimitiveType::F32 => "f32".to_string(),
            ast::PrimitiveType::F64 => "f64".to_string(),
            ast::PrimitiveType::Bool => "bool".to_string(),
            ast::PrimitiveType::String => "String".to_string(),
            ast::PrimitiveType::Void => "void".to_string(),
        },
        ast::Type::Generic { name, type_args } => {
            let args: Vec<String> = type_args.iter().map(|a| type_to_string(&a.node)).collect();
            format!("{name}({})", args.join(", "))
        }
        ast::Type::Path(segments) => segments.join("."),
        ast::Type::Array(inner) => format!("[{}]", type_to_string(&inner.node)),
        ast::Type::Function {
            param_types,
            return_type,
        } => {
            let pts: Vec<String> = param_types
                .iter()
                .map(|p| type_to_string(&p.node))
                .collect();
            format!(
                "function({}): {}",
                pts.join(", "),
                type_to_string(&return_type.node)
            )
        }
        ast::Type::Record(fields) => {
            let flds: Vec<String> = fields
                .iter()
                .map(|(f, t)| format!("{f}: {}", type_to_string(&t.node)))
                .collect();
            format!("{{ {} }}", flds.join(", "))
        }
        ast::Type::Tuple(elems) => {
            let els: Vec<String> = elems.iter().map(|e| type_to_string(&e.node)).collect();
            format!("({})", els.join(", "))
        }
        ast::Type::Unit => "()".to_string(),
    }
}

/// Converts an AST TypeDef to its canonical Modus syntax representation string.
pub fn typedef_to_string(td: &ast::TypeDef) -> String {
    match td {
        ast::TypeDef::Alias(t) => type_to_string(t),
        ast::TypeDef::Union(variants) => {
            let vars: Vec<String> = variants
                .iter()
                .map(|v| {
                    if v.fields.is_empty() {
                        v.name.clone()
                    } else {
                        let flds: Vec<String> =
                            v.fields.iter().map(|f| type_to_string(&f.node)).collect();
                        format!("{}({})", v.name, flds.join(", "))
                    }
                })
                .collect();
            vars.join(" | ")
        }
    }
}

/// Generates an accompanying `.mds` export map header for a precompiled Modus shared library.
/// Uses the canonical `library "<path>";` directive and body-less function signatures ending with `;`.
pub fn emit_export_map_header(library_rel_path: &str, program: &Program) -> String {
    let mut out = format!("library \"{library_rel_path}\";\n\n");

    for decl in &program.declarations {
        if decl.node.is_exported() {
            match &decl.node {
                ast::Declaration::Function(f) => {
                    out.push_str("export function ");
                    out.push_str(&f.name);
                    if !f.type_params.is_empty() {
                        out.push('(');
                        let tps: Vec<String> = f
                            .type_params
                            .iter()
                            .map(|tp| {
                                if let Some(bound) = &tp.bound {
                                    format!("{}: {}", tp.name, type_to_string(&bound.node))
                                } else {
                                    tp.name.clone()
                                }
                            })
                            .collect();
                        out.push_str(&tps.join(", "));
                        out.push(')');
                    }
                    out.push('(');
                    let params: Vec<String> = f
                        .params
                        .iter()
                        .map(|p| format!("{}: {}", p.name, type_to_string(&p.ty.node)))
                        .collect();
                    out.push_str(&params.join(", "));
                    out.push(')');
                    if let Some(ret) = &f.return_type {
                        out.push_str(": ");
                        out.push_str(&type_to_string(&ret.node));
                    }
                    out.push_str(";\n");
                }
                ast::Declaration::Type(t) => {
                    out.push_str("export type ");
                    out.push_str(&t.name);
                    if !t.type_params.is_empty() {
                        out.push('(');
                        let tps: Vec<String> =
                            t.type_params.iter().map(|tp| tp.name.clone()).collect();
                        out.push_str(&tps.join(", "));
                        out.push(')');
                    }
                    out.push_str(" = ");
                    out.push_str(&typedef_to_string(&t.definition.node));
                    out.push_str(";\n");
                }
                ast::Declaration::Trait(tr) => {
                    out.push_str("export trait ");
                    out.push_str(&tr.name);
                    if !tr.type_params.is_empty() {
                        out.push('(');
                        let tps: Vec<String> =
                            tr.type_params.iter().map(|tp| tp.name.clone()).collect();
                        out.push_str(&tps.join(", "));
                        out.push(')');
                    }
                    out.push_str(" {\n");
                    for m in &tr.members {
                        out.push_str("    function ");
                        out.push_str(&m.node.name);
                        out.push('(');
                        let params: Vec<String> = m
                            .node
                            .params
                            .iter()
                            .map(|p| format!("{}: {}", p.name, type_to_string(&p.ty.node)))
                            .collect();
                        out.push_str(&params.join(", "));
                        out.push_str("): ");
                        out.push_str(&type_to_string(&m.node.return_type.node));
                        out.push_str(";\n");
                    }
                    out.push_str("}\n");
                }
                ast::Declaration::Impl(_) => {}
                ast::Declaration::Extern(_) => {}
            }
        }
    }

    out
}

/// Compiles a single AST module node into a native machine-code object file (.o).
pub fn compile_module_to_object(
    node: &ModuleNode,
    env: &Environment,
    output_obj: &Path,
) -> Result<(), String> {
    compile_module_to_object_with_ident(node, env, output_obj, &node.id.module_ident())
}

/// Compiles a single AST module node into an object file (.o) with a specified module identifier.
pub fn compile_module_to_object_with_ident(
    node: &ModuleNode,
    env: &Environment,
    output_obj: &Path,
    module_ident: &str,
) -> Result<(), String> {
    compile_module_to_object_with_options(node, env, output_obj, module_ident, false)
}

/// Compiles a single AST module node into an object file (.o) with options including library entrypoint mode.
pub fn compile_module_to_object_with_options(
    node: &ModuleNode,
    env: &Environment,
    output_obj: &Path,
    module_ident: &str,
    is_lib_entry: bool,
) -> Result<(), String> {
    let desugared = crate::desugar::desugar_program(&node.program, env);
    let mut anf = crate::ir::lower_program(&desugared);
    crate::ir::convert_closures(&mut anf);
    crate::ir::apply_perceus_and_fbip(&mut anf);

    let context = Context::create();
    let mut codegen = CodeGen::new(&context, module_ident);
    codegen.is_lib_entry = is_lib_entry;
    codegen.compile_program(&anf)?;
    codegen.optimize(None)?;
    codegen.compile_to_object(output_obj)?;

    Ok(())
}

/// Builds a Modus shared dynamic library (`.so` / `.dll`) and generates its export map header.
///
/// Steps:
/// 1. Typecheck and lower the library source.
/// 2. Compile to position-independent code (PIC) object file.
/// 3. Invoke clang to produce the shared library: `clang -shared -fPIC ... -o <lib.so>`.
/// 4. Generate the export map header `.mds` containing `library "./<libname.so>";` and prototypes.
pub fn build_shared_library(
    source_path: &Path,
    output_lib_path: &Path,
    emit_header_path: Option<&Path>,
) -> Result<PathBuf, String> {
    let graph = ModuleGraph::build(source_path).map_err(|e| e.to_string())?;
    let (_interfaces, envs) =
        super::check_module_graph_with_envs(&graph).map_err(|e| e.to_string())?;

    let root_node = graph.modules.get(&graph.entry).unwrap();

    let lib_ident = emit_header_path
        .map(super::mangling::module_ident_from_path)
        .unwrap_or_else(|| super::mangling::module_ident_from_path(output_lib_path));

    let mut envs = envs;
    if let Some(entry_env) = envs.get_mut(&graph.entry) {
        for sig in entry_env.functions.values_mut() {
            if sig.symbol_name.is_some() && !sig.is_c_abi {
                sig.symbol_name = Some(super::mangling::mangle_symbol(&lib_ident, &sig.name));
            }
        }
    }

    let temp_dir = unique_temp_dir("modus_shlib")?;

    let mut obj_files = Vec::new();

    // Compile all dependencies in topological order
    for module_id in &graph.topo_order {
        let node = graph.modules.get(module_id).unwrap();
        if node.library_path.is_some() {
            continue;
        }
        let env = envs.get(module_id).unwrap();
        let is_root = *module_id == graph.entry;
        let obj_ident = if is_root {
            &lib_ident
        } else {
            &node.id.module_ident()
        };
        let obj_path = temp_dir.join(format!("{}.o", obj_ident));
        compile_module_to_object_with_options(node, env, &obj_path, obj_ident, is_root)?;
        obj_files.push(obj_path);
    }

    if let Some(parent) = output_lib_path.parent() {
        let _ = std::fs::create_dir_all(parent);
    }

    // Link shared library using clang
    let mut cmd = std::process::Command::new("clang");
    cmd.arg("-shared").arg("-fPIC");
    for obj in &obj_files {
        cmd.arg(obj);
    }
    cmd.arg("-o").arg(output_lib_path);
    cmd.arg("-Wl,--allow-multiple-definition");
    cmd.arg("-lm");

    let status = cmd
        .status()
        .map_err(|e| format!("Failed to invoke clang to link shared library: {e}"))?;
    if !status.success() {
        return Err(format!(
            "clang -shared failed with exit status {:?}",
            status.code()
        ));
    }

    // Determine header output path
    let header_target = if let Some(hp) = emit_header_path {
        hp.to_path_buf()
    } else {
        source_path.with_extension("mds")
    };

    // Calculate relative path from header to shared library
    let lib_filename = output_lib_path
        .file_name()
        .and_then(|n| n.to_str())
        .unwrap_or("library.so");
    let lib_ref = format!("./{lib_filename}");

    let header_content = emit_export_map_header(&lib_ref, &root_node.program);
    std::fs::write(&header_target, header_content)
        .map_err(|e| format!("Failed to write export map header: {e}"))?;

    let _ = std::fs::remove_dir_all(&temp_dir);

    Ok(output_lib_path.to_path_buf())
}

/// Builds a standalone native executable from an application entry point and its module graph.
/// Supports incremental caching via `.modus-cache/` and parallel Rayon compilation waves.
pub fn build_executable(
    entry_file: &Path,
    output_binary: &Path,
    cache_root: Option<&Path>,
) -> Result<PathBuf, String> {
    let graph = ModuleGraph::build(entry_file).map_err(|e| e.to_string())?;
    let (interfaces, envs) =
        super::check_module_graph_with_envs(&graph).map_err(|e| e.to_string())?;

    let default_cache = entry_file
        .parent()
        .unwrap_or_else(|| Path::new("."))
        .join(".modus-cache");
    let cache_dir = cache_root.unwrap_or(&default_cache);
    let cache_store = Mutex::new(CacheStore::new(cache_dir));

    let temp_dir = unique_temp_dir("modus_build")?;

    let mut object_files: Vec<PathBuf> = Vec::new();
    let mut shared_libs: Vec<PathBuf> = Vec::new();

    // Process modules wave-by-wave
    for wave in &graph.topo_waves {
        // Collect libraries and modules needing compilation in this wave
        let mut compile_modules = Vec::new();

        for module_id in wave {
            let node = graph.modules.get(module_id).unwrap();
            if let Some(lib_path) = &node.library_path {
                let resolved = lib_path.clone();
                if !shared_libs.contains(&resolved) {
                    shared_libs.push(resolved);
                }
            } else {
                compile_modules.push(node);
            }
        }

        // Parallel compilation for source modules in this wave
        let wave_results: Result<Vec<PathBuf>, String> = compile_modules
            .par_iter()
            .map(|node| {
                let module_id = &node.id;
                let interface = interfaces.get(module_id).unwrap();
                let env = envs.get(module_id).unwrap();

                let source_hash = hash_source(&node.source);
                let mut dep_hashes = Vec::new();
                for dep in &node.dependencies {
                    if let Some(dep_iface) = interfaces.get(dep) {
                        dep_hashes.push(dep_iface.interface_hash);
                    }
                }
                let fingerprint = compute_fingerprint(source_hash, &dep_hashes);

                // Check cache
                {
                    let lock = cache_store.lock().unwrap();
                    if let Some(cached_obj) = lock.is_cached(module_id.path(), &fingerprint) {
                        return Ok(cached_obj);
                    }
                }

                // Compile module to temporary object
                let temp_obj = temp_dir.join(format!("{}.o", fingerprint));
                compile_module_to_object(node, env, &temp_obj)?;

                // Store in cache
                let cached_path = {
                    let mut lock = cache_store.lock().unwrap();
                    lock.store_object(
                        module_id.path(),
                        source_hash,
                        interface.interface_hash,
                        &fingerprint,
                        &temp_obj,
                    )
                    .map_err(|e| format!("Failed to cache object for '{module_id}': {e}"))?
                };

                let _ = std::fs::remove_file(&temp_obj);
                Ok(cached_path)
            })
            .collect();

        let wave_objs = wave_results?;
        object_files.extend(wave_objs);
    }

    if let Some(parent) = output_binary.parent() {
        let _ = std::fs::create_dir_all(parent);
    }

    // Link all objects and shared libraries using clang
    let mut cmd = std::process::Command::new("clang");
    for obj in &object_files {
        cmd.arg(obj);
    }
    for lib in &shared_libs {
        cmd.arg(lib);
        if let Some(parent) = lib.parent() {
            cmd.arg(format!("-Wl,-rpath,{}", parent.display()));
        }
    }
    cmd.arg("-o").arg(output_binary);
    cmd.arg("-Wl,-rpath,$ORIGIN");
    cmd.arg("-Wl,--allow-multiple-definition");
    cmd.arg("-lm");

    let status = cmd
        .status()
        .map_err(|e| format!("Failed to invoke clang linker: {e}"))?;
    if !status.success() {
        return Err(format!("clang linker failed with status {status}"));
    }

    let _ = std::fs::remove_dir_all(&temp_dir);

    Ok(output_binary.to_path_buf())
}

/// JIT-executes a Modus program from its entry file across its entire module graph.
/// Pre-loads any dynamic `.so` libraries and compiles dependency modules.
pub fn jit_run_module_graph(entry_file: &Path) -> Result<ExecutionResult, String> {
    let graph = ModuleGraph::build(entry_file).map_err(|e| e.to_string())?;
    jit_run_graph(&graph)
}

/// JIT-executes a validated ModuleGraph.
pub fn jit_run_graph(graph: &ModuleGraph) -> Result<ExecutionResult, String> {
    let (_interfaces, envs) =
        super::check_module_graph_with_envs(graph).map_err(|e| e.to_string())?;

    // Load any referenced dynamic libraries permanently into LLVM execution engine
    for node in graph.modules.values() {
        if let Some(lib_path) = &node.library_path {
            inkwell::support::load_library_permanently(lib_path)
                .map_err(|e| format!("Failed to load library '{:?}': {:?}", lib_path, e))?;
        }
    }

    // If there are non-root source modules, compile them to a temporary shared library and load it
    let non_root_modules: Vec<&ModuleNode> = graph
        .topo_order
        .iter()
        .filter(|id| **id != graph.entry)
        .filter_map(|id| graph.modules.get(id))
        .filter(|node| node.library_path.is_none())
        .collect();

    let temp_dir = if !non_root_modules.is_empty() {
        let dir = unique_temp_dir("modus_jit")?;
        let mut dep_objs = Vec::new();
        for node in non_root_modules {
            let env = envs.get(&node.id).unwrap();
            let obj_path = dir.join(format!("{}.o", node.id.module_ident()));
            compile_module_to_object(node, env, &obj_path)?;
            dep_objs.push(obj_path);
        }

        let dep_so = dir.join("libdeps.so");
        let mut cmd = std::process::Command::new("clang");
        cmd.arg("-shared").arg("-fPIC");
        for obj in &dep_objs {
            cmd.arg(obj);
        }
        cmd.arg("-o").arg(&dep_so);
        cmd.arg("-Wl,--allow-multiple-definition");
        cmd.arg("-lm");

        let status = cmd
            .status()
            .map_err(|e| format!("Failed to link JIT dependencies: {e}"))?;
        if !status.success() {
            return Err(format!("JIT dependency link failed with status {status}"));
        }

        inkwell::support::load_library_permanently(&dep_so)
            .map_err(|e| format!("Failed to load JIT dependencies: {:?}", e))?;
        Some(dir)
    } else {
        None
    };

    // Compile and JIT-execute the root module
    let root_node = graph.modules.get(&graph.entry).unwrap();
    let root_env = envs.get(&graph.entry).unwrap();

    let desugared = crate::desugar::desugar_program(&root_node.program, root_env);
    let mut anf = crate::ir::lower_program(&desugared);
    crate::ir::convert_closures(&mut anf);
    crate::ir::apply_perceus_and_fbip(&mut anf);

    let context = Context::create();
    let mut codegen = CodeGen::new(&context, &root_node.id.module_ident());
    codegen.compile_program(&anf)?;
    codegen.optimize(None)?;

    let result = codegen.jit_run();

    if let Some(dir) = temp_dir {
        let _ = std::fs::remove_dir_all(&dir);
    }

    result
}
