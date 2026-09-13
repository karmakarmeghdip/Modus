//! Integration tests for Modus ESM module system and dynamic library export maps.

use modus::ast::*;
use modus::parser::parse_program;
use modus::typechecker::{TypeErrorKind, check_program};

#[test]
fn test_parse_library_directive() {
    let src = r#"
library "./libmath.so";

export function add(a: i32, b: i32): i32;
export function subtract(a: i32, b: i32): i32;
"#;
    let prog = parse_program(src).expect("Library header should parse successfully");
    assert_eq!(
        prog.library.as_ref().map(|s| s.node.as_str()),
        Some("./libmath.so")
    );
    assert_eq!(prog.declarations.len(), 2);
    assert_eq!(prog.exports.len(), 2);

    match &prog.declarations[0].node {
        Declaration::Function(f) => {
            assert_eq!(f.name, "add");
            assert!(f.is_exported);
            assert!(f.body.is_none(), "Header function should have no body");
        }
        _ => panic!("Expected function declaration"),
    }
}

#[test]
fn test_parse_esm_named_imports() {
    let src = r#"
import { add, subtract as sub } from "./math.mds";

function main(): i32 {
    return 0;
}
"#;
    let prog = parse_program(src).expect("Named imports should parse successfully");
    assert_eq!(prog.imports.len(), 1);
    let imp = &prog.imports[0].node;
    assert_eq!(imp.source, "./math.mds");
    match &imp.clause {
        ImportClause::Named(specs) => {
            assert_eq!(specs.len(), 2);
            assert_eq!(specs[0].name, "add");
            assert_eq!(specs[0].alias, None);
            assert_eq!(specs[1].name, "subtract");
            assert_eq!(specs[1].alias.as_deref(), Some("sub"));
        }
        _ => panic!("Expected named import clause"),
    }
}

#[test]
fn test_parse_esm_namespace_import() {
    let src = r#"
import * as Math from "./math.mds";

function main(): i32 {
    return 0;
}
"#;
    let prog = parse_program(src).expect("Namespace import should parse successfully");
    assert_eq!(prog.imports.len(), 1);
    let imp = &prog.imports[0].node;
    assert_eq!(imp.source, "./math.mds");
    assert_eq!(imp.clause, ImportClause::Namespace("Math".to_string()));
}

#[test]
fn test_parse_esm_side_effect_import() {
    let src = r#"
import "./setup.mds";

function main(): i32 {
    return 0;
}
"#;
    let prog = parse_program(src).expect("Side effect import should parse successfully");
    assert_eq!(prog.imports.len(), 1);
    let imp = &prog.imports[0].node;
    assert_eq!(imp.source, "./setup.mds");
    assert_eq!(imp.clause, ImportClause::SideEffect);
}

#[test]
fn test_parse_esm_inline_exports() {
    let src = r#"
export function add(a: i32, b: i32): i32 {
    return a + b;
}

export type Point = { x: i32, y: i32 };

export trait Printable(Self) {
    function print(self: Self): IO(void);
}
"#;
    let prog = parse_program(src).expect("Inline exports should parse successfully");
    assert_eq!(prog.declarations.len(), 3);
    assert_eq!(prog.exports.len(), 3);

    for decl in &prog.declarations {
        assert!(
            decl.node.is_exported(),
            "All declarations should be marked is_exported"
        );
    }
}

#[test]
fn test_parse_esm_export_clauses_and_reexports() {
    let src = r#"
function helper(): i32 { return 42; }

export { helper, helper as myHelper };
export { add, sub as subtract } from "./math.mds";
export * from "./types.mds";
export * as Geometry from "./geometry.mds";
"#;
    let prog = parse_program(src).expect("Export clauses should parse successfully");
    // Declarations: helper
    assert_eq!(prog.declarations.len(), 1);
    // Exports:
    // 1. export { helper, helper as myHelper }
    // 2. export { add, sub as subtract } from "./math.mds"
    // 3. export * from "./types.mds"
    // 4. export * as Geometry from "./geometry.mds"
    assert_eq!(prog.exports.len(), 4);

    match &prog.exports[0].node {
        ExportDecl::Named { specifiers, source } => {
            assert_eq!(specifiers.len(), 2);
            assert_eq!(source, &None);
        }
        _ => panic!("Expected named export"),
    }

    match &prog.exports[1].node {
        ExportDecl::Named { specifiers, source } => {
            assert_eq!(specifiers.len(), 2);
            assert_eq!(source.as_deref(), Some("./math.mds"));
        }
        _ => panic!("Expected re-export named"),
    }

    match &prog.exports[2].node {
        ExportDecl::All { alias, source } => {
            assert_eq!(alias, &None);
            assert_eq!(source, "./types.mds");
        }
        _ => panic!("Expected re-export all"),
    }

    match &prog.exports[3].node {
        ExportDecl::All { alias, source } => {
            assert_eq!(alias.as_deref(), Some("Geometry"));
            assert_eq!(source, "./geometry.mds");
        }
        _ => panic!("Expected re-export all with alias"),
    }
}

#[test]
fn test_invariant_empty_body_forbidden_in_normal_source_file() {
    let src = r#"
function missing_body(a: i32, b: i32): i32;
"#;
    let prog = parse_program(src).expect("Should parse syntactically");
    let err =
        check_program(&prog).expect_err("Must reject empty function body in non-library file");
    match err.kind {
        TypeErrorKind::MissingFunctionBody { function_name } => {
            assert_eq!(function_name, "missing_body");
        }
        other => panic!("Expected MissingFunctionBody error, got: {:?}", other),
    }
}

#[test]
fn test_invariant_body_forbidden_in_library_header_file() {
    let src = r#"
library "./libmath.so";

export function add(a: i32, b: i32): i32 {
    return a + b;
}
"#;
    let prog = parse_program(src).expect("Should parse syntactically");
    let err = check_program(&prog).expect_err("Must reject function body in library header file");
    match err.kind {
        TypeErrorKind::UnexpectedFunctionBodyInHeader { function_name } => {
            assert_eq!(function_name, "add");
        }
        other => panic!(
            "Expected UnexpectedFunctionBodyInHeader error, got: {:?}",
            other
        ),
    }
}

#[test]
fn test_invariant_empty_body_permitted_in_library_header_file() {
    let src = r#"
library "./libmath.so";

export function add(a: i32, b: i32): i32;
export function multiply(a: i32, b: i32): i32;
"#;
    let prog = parse_program(src).expect("Should parse syntactically");
    let env = check_program(&prog).expect("Valid library header should pass typechecking");
    assert!(env.lookup_function("add").is_some());
    assert!(env.lookup_function("multiply").is_some());
}

#[test]
fn test_module_graph_linear_dependencies() {
    let temp_dir = std::env::temp_dir().join(format!("modus_test_linear_{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&temp_dir);
    std::fs::create_dir_all(&temp_dir).unwrap();

    let a_path = temp_dir.join("a.mds");
    let b_path = temp_dir.join("b.mds");
    let main_path = temp_dir.join("main.mds");

    std::fs::write(&a_path, "export function a(): i32 { return 1; }\n").unwrap();
    std::fs::write(
        &b_path,
        "import { a } from \"./a.mds\";\nexport function b(): i32 { return a(); }\n",
    )
    .unwrap();
    std::fs::write(
        &main_path,
        "import { b } from \"./b.mds\";\nfunction main(): i32 { return b(); }\n",
    )
    .unwrap();

    let graph = modus::modules::ModuleGraph::build(&main_path).expect("Graph build should succeed");
    assert_eq!(graph.modules.len(), 3);

    // Dependencies must precede dependents in topo_order: a -> b -> main
    let a_id = modus::modules::ModuleId::new(a_path.canonicalize().unwrap());
    let b_id = modus::modules::ModuleId::new(b_path.canonicalize().unwrap());
    let main_id = modus::modules::ModuleId::new(main_path.canonicalize().unwrap());

    assert_eq!(
        graph.topo_order,
        vec![a_id.clone(), b_id.clone(), main_id.clone()]
    );

    // Topo waves: Wave 0: [a], Wave 1: [b], Wave 2: [main]
    assert_eq!(graph.topo_waves.len(), 3);
    assert_eq!(graph.topo_waves[0], vec![a_id]);
    assert_eq!(graph.topo_waves[1], vec![b_id]);
    assert_eq!(graph.topo_waves[2], vec![main_id]);

    let _ = std::fs::remove_dir_all(&temp_dir);
}

#[test]
fn test_module_graph_diamond_dependencies() {
    let temp_dir = std::env::temp_dir().join(format!("modus_test_diamond_{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&temp_dir);
    std::fs::create_dir_all(&temp_dir).unwrap();

    let leaf_path = temp_dir.join("leaf.mds");
    let b1_path = temp_dir.join("b1.mds");
    let b2_path = temp_dir.join("b2.mds");
    let root_path = temp_dir.join("root.mds");

    std::fs::write(&leaf_path, "export function base(): i32 { return 42; }\n").unwrap();
    std::fs::write(
        &b1_path,
        "import { base } from \"./leaf.mds\";\nexport function f1(): i32 { return base(); }\n",
    )
    .unwrap();
    std::fs::write(
        &b2_path,
        "import { base } from \"./leaf.mds\";\nexport function f2(): i32 { return base(); }\n",
    )
    .unwrap();
    std::fs::write(
        &root_path,
        "import { f1 } from \"./b1.mds\";\nimport { f2 } from \"./b2.mds\";\nfunction main(): i32 { return f1() + f2(); }\n",
    )
    .unwrap();

    let graph = modus::modules::ModuleGraph::build(&root_path).expect("Graph build should succeed");
    assert_eq!(graph.modules.len(), 4);

    let leaf_id = modus::modules::ModuleId::new(leaf_path.canonicalize().unwrap());
    let b1_id = modus::modules::ModuleId::new(b1_path.canonicalize().unwrap());
    let b2_id = modus::modules::ModuleId::new(b2_path.canonicalize().unwrap());
    let root_id = modus::modules::ModuleId::new(root_path.canonicalize().unwrap());

    // Wave 0: [leaf]
    assert_eq!(graph.topo_waves[0], vec![leaf_id]);

    // Wave 1: [b1, b2] in parallel
    assert_eq!(graph.topo_waves[1].len(), 2);
    assert!(graph.topo_waves[1].contains(&b1_id));
    assert!(graph.topo_waves[1].contains(&b2_id));

    // Wave 2: [root]
    assert_eq!(graph.topo_waves[2], vec![root_id]);

    let _ = std::fs::remove_dir_all(&temp_dir);
}

#[test]
fn test_module_graph_circular_dependency_detected() {
    let temp_dir = std::env::temp_dir().join(format!("modus_test_cycle_{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&temp_dir);
    std::fs::create_dir_all(&temp_dir).unwrap();

    let c1_path = temp_dir.join("c1.mds");
    let c2_path = temp_dir.join("c2.mds");

    std::fs::write(
        &c1_path,
        "import \"./c2.mds\";\nfunction f1(): i32 { return 1; }\n",
    )
    .unwrap();
    std::fs::write(
        &c2_path,
        "import \"./c1.mds\";\nfunction f2(): i32 { return 2; }\n",
    )
    .unwrap();

    let err =
        modus::modules::ModuleGraph::build(&c1_path).expect_err("Must detect circular dependency");
    match err {
        modus::modules::GraphError::CircularDependency { cycle } => {
            assert!(cycle.len() >= 2);
        }
        other => panic!("Expected CircularDependency, got {:?}", other),
    }

    let _ = std::fs::remove_dir_all(&temp_dir);
}

#[test]
fn test_module_graph_with_library_header() {
    let temp_dir = std::env::temp_dir().join(format!("modus_test_lib_{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&temp_dir);
    std::fs::create_dir_all(&temp_dir).unwrap();

    let lib_header_path = temp_dir.join("math.mds");
    let app_path = temp_dir.join("app.mds");

    std::fs::write(
        &lib_header_path,
        "library \"./libmath.so\";\nexport function add(a: i32, b: i32): i32;\n",
    )
    .unwrap();
    std::fs::write(
        &app_path,
        "import { add } from \"./math.mds\";\nfunction main(): i32 { return add(1, 2); }\n",
    )
    .unwrap();

    let graph = modus::modules::ModuleGraph::build(&app_path).expect("Graph should build");
    assert_eq!(graph.modules.len(), 2);

    let header_id = modus::modules::ModuleId::new(lib_header_path.canonicalize().unwrap());
    let header_node = graph.modules.get(&header_id).unwrap();
    assert_eq!(
        header_node.library_path,
        Some(temp_dir.canonicalize().unwrap().join("libmath.so"))
    );

    let _ = std::fs::remove_dir_all(&temp_dir);
}

#[test]
fn test_check_module_graph_named_imports() {
    let temp_dir =
        std::env::temp_dir().join(format!("modus_test_typecheck_named_{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&temp_dir);
    std::fs::create_dir_all(&temp_dir).unwrap();

    let math_path = temp_dir.join("math.mds");
    let app_path = temp_dir.join("app.mds");

    std::fs::write(
        &math_path,
        r#"
export type Point = { x: i32, y: i32 };

export function add(a: i32, b: i32): i32 {
    return a + b;
}
"#,
    )
    .unwrap();

    std::fs::write(
        &app_path,
        r#"
import { Point, add } from "./math.mds";

function main(): i32 {
    let pt: Point = { x: 10, y: 20 };
    return add(pt.x, pt.y);
}
"#,
    )
    .unwrap();

    let graph = modus::modules::ModuleGraph::build(&app_path).expect("Graph should build");
    let interfaces =
        modus::modules::check_module_graph(&graph).expect("Typechecking should succeed");

    assert_eq!(interfaces.len(), 2);
    let math_id = modus::modules::ModuleId::new(math_path.canonicalize().unwrap());
    let math_intf = interfaces.get(&math_id).unwrap();
    assert!(math_intf.exported_functions.contains_key("add"));
    assert!(math_intf.exported_types.contains_key("Point"));

    let _ = std::fs::remove_dir_all(&temp_dir);
}

#[test]
fn test_check_module_graph_namespace_imports() {
    let temp_dir =
        std::env::temp_dir().join(format!("modus_test_typecheck_ns_{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&temp_dir);
    std::fs::create_dir_all(&temp_dir).unwrap();

    let math_path = temp_dir.join("math.mds");
    let app_path = temp_dir.join("app.mds");

    std::fs::write(
        &math_path,
        r#"
export type Point = { x: i32, y: i32 };

export function add(a: i32, b: i32): i32 {
    return a + b;
}
"#,
    )
    .unwrap();

    std::fs::write(
        &app_path,
        r#"
import * as Math from "./math.mds";

function main(): i32 {
    let pt: Math.Point = { x: 10, y: 20 };
    return Math.add(pt.x, pt.y);
}
"#,
    )
    .unwrap();

    let graph = modus::modules::ModuleGraph::build(&app_path).expect("Graph should build");
    let interfaces =
        modus::modules::check_module_graph(&graph).expect("Namespace typechecking should succeed");
    assert_eq!(interfaces.len(), 2);

    let _ = std::fs::remove_dir_all(&temp_dir);
}

#[test]
fn test_check_module_graph_reject_unexported_symbol() {
    let temp_dir =
        std::env::temp_dir().join(format!("modus_test_typecheck_unexp_{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&temp_dir);
    std::fs::create_dir_all(&temp_dir).unwrap();

    let helper_path = temp_dir.join("helper.mds");
    let app_path = temp_dir.join("app.mds");

    // helper.mds does NOT export secret_fn
    std::fs::write(
        &helper_path,
        r#"
function secret_fn(): i32 {
    return 42;
}
"#,
    )
    .unwrap();

    std::fs::write(
        &app_path,
        r#"
import { secret_fn } from "./helper.mds";

function main(): i32 {
    return secret_fn();
}
"#,
    )
    .unwrap();

    let graph = modus::modules::ModuleGraph::build(&app_path).expect("Graph should build");
    let err = modus::modules::check_module_graph(&graph)
        .expect_err("Must reject importing unexported symbol");
    assert!(err.to_string().contains("not exported by module"));

    let _ = std::fs::remove_dir_all(&temp_dir);
}

#[test]
fn test_check_module_graph_with_library_header() {
    let temp_dir =
        std::env::temp_dir().join(format!("modus_test_typecheck_lib_{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&temp_dir);
    std::fs::create_dir_all(&temp_dir).unwrap();

    let lib_header_path = temp_dir.join("matrix.mds");
    let app_path = temp_dir.join("app.mds");

    std::fs::write(
        &lib_header_path,
        r#"
library "./libmatrix.so";

export type Matrix = { rows: i32, cols: i32 };

export function matrix_create(rows: i32, cols: i32): Matrix;
export function matrix_sum(m: Matrix): i32;
"#,
    )
    .unwrap();

    std::fs::write(
        &app_path,
        r#"
import { Matrix, matrix_create, matrix_sum } from "./matrix.mds";

function main(): i32 {
    let m: Matrix = matrix_create(2, 2);
    return matrix_sum(m);
}
"#,
    )
    .unwrap();

    let graph = modus::modules::ModuleGraph::build(&app_path).expect("Graph should build");
    let interfaces = modus::modules::check_module_graph(&graph)
        .expect("Library header typechecking should succeed");

    assert_eq!(interfaces.len(), 2);

    let _ = std::fs::remove_dir_all(&temp_dir);
}

#[test]
fn test_multimodule_aot_and_jit_execution() {
    let temp_dir =
        std::env::temp_dir().join(format!("modus_test_mmod_exec_{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&temp_dir);
    std::fs::create_dir_all(&temp_dir).unwrap();

    let math_path = temp_dir.join("math.mds");
    let util_path = temp_dir.join("util.mds");
    let main_path = temp_dir.join("main.mds");
    let binary_path = temp_dir.join("app_bin");

    std::fs::write(
        &math_path,
        r#"
export function add(a: i32, b: i32): i32 {
    return a + b;
}

export function sub(a: i32, b: i32): i32 {
    return a - b;
}
"#,
    )
    .unwrap();

    std::fs::write(
        &util_path,
        r#"
import { add } from "./math.mds";

export function double_add(x: i32): i32 {
    return add(x, x);
}
"#,
    )
    .unwrap();

    std::fs::write(
        &main_path,
        r#"
import { double_add } from "./util.mds";
import { sub } from "./math.mds";

function main(): i32 {
    let d: i32 = double_add(20);
    return sub(d, 5);
}
"#,
    )
    .unwrap();

    // 1. Test AOT executable compilation and execution
    let bin = modus::modules::build_executable(&main_path, &binary_path, None)
        .expect("AOT build should succeed");
    assert!(bin.exists());

    let output = std::process::Command::new(&bin)
        .output()
        .expect("Failed to execute compiled binary");
    assert_eq!(output.status.code(), Some(35)); // 20 + 20 - 5 = 35

    // 2. Test in-process JIT execution
    let jit_res = modus::modules::jit_run_module_graph(&main_path)
        .expect("JIT execution of multi-module graph should succeed");
    assert_eq!(jit_res, modus::backend::ExecutionResult::I32(35));

    let _ = std::fs::remove_dir_all(&temp_dir);
}

#[test]
fn test_multimodule_namespace_import_execution() {
    let temp_dir = std::env::temp_dir().join(format!("modus_test_ns_exec_{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&temp_dir);
    std::fs::create_dir_all(&temp_dir).unwrap();

    let geo_path = temp_dir.join("geometry.mds");
    let app_path = temp_dir.join("app.mds");
    let binary_path = temp_dir.join("geo_bin");

    std::fs::write(
        &geo_path,
        r#"
export type Point = { x: i32, y: i32 };

export function distance_sq(p: Point): i32 {
    return p.x * p.x + p.y * p.y;
}
"#,
    )
    .unwrap();

    std::fs::write(
        &app_path,
        r#"
import * as Geo from "./geometry.mds";

function main(): i32 {
    let pt: Geo.Point = { x: 3, y: 4 };
    return Geo.distance_sq(pt);
}
"#,
    )
    .unwrap();

    // 1. AOT build and run
    let bin = modus::modules::build_executable(&app_path, &binary_path, None)
        .expect("AOT namespace build should succeed");
    let output = std::process::Command::new(&bin)
        .output()
        .expect("Failed to execute namespace binary");
    assert_eq!(output.status.code(), Some(25)); // 3^2 + 4^2 = 25

    // 2. JIT execution
    let jit_res = modus::modules::jit_run_module_graph(&app_path)
        .expect("JIT execution of namespace import should succeed");
    assert_eq!(jit_res, modus::backend::ExecutionResult::I32(25));

    let _ = std::fs::remove_dir_all(&temp_dir);
}

#[test]
fn test_build_shared_library_and_dynamic_linking() {
    let temp_dir =
        std::env::temp_dir().join(format!("modus_test_shlib_dyn_{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&temp_dir);
    std::fs::create_dir_all(&temp_dir).unwrap();

    let calc_src = temp_dir.join("calc_src.mds");
    let libcalc_so = temp_dir.join("libcalc.so");
    let calc_header = temp_dir.join("calc.mds");
    let app_path = temp_dir.join("app.mds");
    let binary_path = temp_dir.join("app_dyn_bin");

    // 1. Library implementation source file
    std::fs::write(
        &calc_src,
        r#"
export function add(a: i32, b: i32): i32 {
    return a + b;
}

export function multiply(a: i32, b: i32): i32 {
    return a * b;
}
"#,
    )
    .unwrap();

    // 2. Build precompiled shared library and emit header export map
    let lib_path = modus::modules::build_shared_library(&calc_src, &libcalc_so, Some(&calc_header))
        .expect("Shared library build should succeed");

    assert!(lib_path.exists(), "libcalc.so must exist");
    assert!(
        calc_header.exists(),
        "calc.mds export map header must exist"
    );

    // Verify header contents: invariant requires `library "..."` and empty function bodies `;`
    let header_contents = std::fs::read_to_string(&calc_header).unwrap();
    assert!(header_contents.contains("library \"./libcalc.so\";"));
    assert!(header_contents.contains("export function add(a: i32, b: i32): i32;"));
    assert!(header_contents.contains("export function multiply(a: i32, b: i32): i32;"));
    assert!(!header_contents.contains("return")); // Must NOT contain function bodies

    // 3. Application importing the precompiled shared library via the export map header
    std::fs::write(
        &app_path,
        r#"
import { add, multiply } from "./calc.mds";

function main(): i32 {
    let prod: i32 = multiply(4, 5);
    return add(prod, 7);
}
"#,
    )
    .unwrap();

    // 4. Build standalone executable linking against libcalc.so
    let bin = modus::modules::build_executable(&app_path, &binary_path, None)
        .expect("Executable dynamically linking against shared library should succeed");

    let output = std::process::Command::new(&bin)
        .output()
        .expect("Failed to execute dynamically linked binary");
    assert_eq!(output.status.code(), Some(27)); // 4 * 5 + 7 = 27

    // 5. Run via JIT by dynamically loading libcalc.so
    let jit_res = modus::modules::jit_run_module_graph(&app_path)
        .expect("JIT execution with dynamic library should succeed");
    assert_eq!(jit_res, modus::backend::ExecutionResult::I32(27));

    let _ = std::fs::remove_dir_all(&temp_dir);
}

#[test]
fn test_incremental_caching_and_early_cutoff() {
    let temp_dir = std::env::temp_dir().join(format!("modus_test_cache_{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&temp_dir);
    std::fs::create_dir_all(&temp_dir).unwrap();

    let cache_dir = temp_dir.join(".modus-cache");
    let dep_path = temp_dir.join("dep.mds");
    let app_path = temp_dir.join("app.mds");
    let bin_path = temp_dir.join("app_cache_bin");

    std::fs::write(
        &dep_path,
        r#"
export function calculate(): i32 {
    return 10;
}
"#,
    )
    .unwrap();

    std::fs::write(
        &app_path,
        r#"
import { calculate } from "./dep.mds";

function main(): i32 {
    return calculate();
}
"#,
    )
    .unwrap();

    // 1. Initial build: populates cache
    let bin1 = modus::modules::build_executable(&app_path, &bin_path, Some(&cache_dir))
        .expect("First build should succeed");
    let out1 = std::process::Command::new(&bin1).output().unwrap();
    assert_eq!(out1.status.code(), Some(10));

    let cache_store = modus::modules::CacheStore::new(&cache_dir);
    assert_eq!(cache_store.manifest.entries.len(), 2);
    let app_entry_v1 = cache_store
        .manifest
        .entries
        .get(&app_path.to_string_lossy().to_string())
        .unwrap()
        .clone();

    // 2. Modify dep's implementation body WITHOUT changing public interface
    std::fs::write(
        &dep_path,
        r#"
export function calculate(): i32 {
    return 42;
}
"#,
    )
    .unwrap();

    // 3. Second build: Early cutoff kicks in for app.mds!
    let bin2 = modus::modules::build_executable(&app_path, &bin_path, Some(&cache_dir))
        .expect("Second build should succeed");
    let out2 = std::process::Command::new(&bin2).output().unwrap();
    assert_eq!(out2.status.code(), Some(42));

    let cache_store_v2 = modus::modules::CacheStore::new(&cache_dir);
    let app_entry_v2 = cache_store_v2
        .manifest
        .entries
        .get(&app_path.to_string_lossy().to_string())
        .unwrap();

    // Early Cutoff Invariant: app.mds build_fingerprint and cached object remain UNCHANGED
    // because dep.mds interface hash did not change!
    assert_eq!(
        app_entry_v1.build_fingerprint,
        app_entry_v2.build_fingerprint
    );
    assert_eq!(app_entry_v1.object_file, app_entry_v2.object_file);

    let _ = std::fs::remove_dir_all(&temp_dir);
}

#[test]
fn test_calling_conventions_fastcc_vs_ccc() {
    use inkwell::context::Context;
    use modus::backend::CodeGen;
    use modus::desugar::desugar_program;
    use modus::ir::{apply_perceus_and_fbip, convert_closures, lower_program};
    use modus::modules::{ModuleGraph, check_module_graph_with_envs};

    let temp_dir = std::env::temp_dir().join(format!("modus_test_cc_conv_{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&temp_dir);
    std::fs::create_dir_all(&temp_dir).unwrap();

    let lib_src = temp_dir.join("mylib.mds");
    let app_src = temp_dir.join("app.mds");

    std::fs::write(
        &lib_src,
        r#"
function internal_helper(x: i32): i32 {
    return x + 1;
}

export function exported_fn(x: i32): i32 {
    return internal_helper(x) * 2;
}
"#,
    )
    .unwrap();

    std::fs::write(
        &app_src,
        r#"
import { exported_fn } from "./mylib.mds";

function main(): i32 {
    return exported_fn(5);
}
"#,
    )
    .unwrap();

    let graph = ModuleGraph::build(&app_src).expect("ModuleGraph build should succeed");
    let (_interfaces, envs) =
        check_module_graph_with_envs(&graph).expect("Graph typecheck should succeed");

    let lib_node = graph
        .modules
        .values()
        .find(|n| n.id.path().ends_with("mylib.mds"))
        .unwrap();
    let lib_env = envs.get(&lib_node.id).unwrap();

    // 1. Normal compilation (not --lib): All Modus functions use fastcc!
    {
        let desugared = desugar_program(&lib_node.program, lib_env);
        let mut anf = lower_program(&desugared);
        convert_closures(&mut anf);
        apply_perceus_and_fbip(&mut anf);

        let context = Context::create();
        let mut codegen = CodeGen::new(&context, &lib_node.id.module_ident());
        codegen.is_lib_entry = false;
        codegen.compile_program(&anf).unwrap();
        let ir = codegen.to_ir_string();

        assert!(ir.contains("define fastcc i32 @internal_helper(i32 %0)"));
        assert!(ir.contains("define fastcc i32 @_modus_M_mylib_exported_fn(i32 %0)"));
        assert!(ir.contains("call fastcc i32 @internal_helper"));
    }

    // 2. Library compilation (--lib on entrypoint file): Exported functions use ccc (0), internal use fastcc (8)!
    {
        let desugared = desugar_program(&lib_node.program, lib_env);
        let mut anf = lower_program(&desugared);
        convert_closures(&mut anf);
        apply_perceus_and_fbip(&mut anf);

        let context = Context::create();
        let mut codegen = CodeGen::new(&context, &lib_node.id.module_ident());
        codegen.is_lib_entry = true;
        codegen.compile_program(&anf).unwrap();
        let ir = codegen.to_ir_string();

        assert!(ir.contains("define fastcc i32 @internal_helper(i32 %0)"));
        assert!(ir.contains("define i32 @_modus_M_mylib_exported_fn(i32 %0)")); // Standard ccc (no fastcc prefix)
        assert!(ir.contains("call fastcc i32 @internal_helper"));
    }

    // 3. Consumer importing from source module: Uses fastcc for the imported function!
    {
        let app_node = graph.modules.get(&graph.entry).unwrap();
        let app_env = envs.get(&app_node.id).unwrap();

        let desugared = desugar_program(&app_node.program, app_env);
        let mut anf = lower_program(&desugared);
        convert_closures(&mut anf);
        apply_perceus_and_fbip(&mut anf);

        let context = Context::create();
        let mut codegen = CodeGen::new(&context, &app_node.id.module_ident());
        codegen.compile_program(&anf).unwrap();
        let ir = codegen.to_ir_string();

        assert!(ir.contains("declare fastcc i32 @_modus_M_mylib_exported_fn(i32)"));
        assert!(ir.contains("call fastcc i32 @_modus_M_mylib_exported_fn"));
        assert!(ir.contains("define i32 @main()")); // main uses ccc
    }

    let _ = std::fs::remove_dir_all(&temp_dir);
}
