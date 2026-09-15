//! Tests for `==`/`!=` lowering through the `Eq` trait impl in `std:prelude`.
//!
//! The `String` equality operator must resolve to the pure-Modus `stringEq`
//! (via `export impl Eq for String` in the prelude barrel) without any
//! explicit import, in both graph mode and single-module mode.

use inkwell::context::Context;
use modus::backend::ExecutionResult;
use modus::modules::{ModuleGraph, ModuleId, jit_run_graph};
use modus::parser::parse_program;
use std::path::{Path, PathBuf};

const EQ_PROGRAM: &str = r#"
function main(): i32 {
    let a: String = "hello";
    let b: String = "hello";
    let c: String = "world";
    if (!(a == b)) { return 1; }
    if (a != b) { return 2; }
    if (a == c) { return 3; }
    if (!(a != c)) { return 4; }
    if ("" == "x") { return 5; }
    if (!("" == "")) { return 6; }
    if (a == "hello!") { return 7; }
    return 0;
}
"#;

#[test]
fn test_export_impl_parses_and_marks_exported() {
    let src = r#"
export impl Eq for String {
    function eq(self: String, other: String): bool {
        return true;
    }
}
"#;
    let prog = parse_program(src).expect("export impl should parse");
    assert_eq!(prog.declarations.len(), 1);
    assert_eq!(prog.exports.len(), 1);
    match &prog.declarations[0].node {
        modus::ast::Declaration::Impl(im) => {
            assert!(im.is_exported);
            assert_eq!(im.trait_name, "Eq");
        }
        other => panic!("Expected impl declaration, got {:?}", other),
    }
}

#[test]
fn test_prelude_registers_eq_impl_for_string() {
    // The prelude is only meaningful as a graph module (it re-exports
    // std:string), so check it through the graph machinery.
    let graph = ModuleGraph::build_from_source(
        Path::new("std:prelude"),
        modus::modules::stdlib::STD_PRELUDE_SOURCE,
    )
    .expect("Prelude graph should build");
    let (interfaces, envs) =
        modus::modules::check_module_graph_with_envs(&graph).expect("Prelude should typecheck");

    let prelude_id = ModuleId::new(PathBuf::from("std:prelude"));
    let iface = interfaces.get(&prelude_id).expect("Prelude interface");
    // `export *` merges std:string's surface, including both operator impls.
    assert_eq!(iface.exported_impls.len(), 2);
    let eq_impl = iface
        .exported_impls
        .iter()
        .find(|d| d.trait_name == "Eq")
        .expect("Eq impl");
    assert_eq!(eq_impl.target_type, modus::typechecker::Type::string());
    let eq_sig = eq_impl.methods.get("eq").expect("eq method");
    assert_eq!(
        eq_sig.symbol_name.as_deref(),
        Some("_modus_M_std_string_Eq_eq_String")
    );
    let add_impl = iface
        .exported_impls
        .iter()
        .find(|d| d.trait_name == "Add")
        .expect("Add impl");
    assert_eq!(add_impl.target_type, modus::typechecker::Type::string());
    let add_sig = add_impl.methods.get("add").expect("add method");
    assert_eq!(
        add_sig.symbol_name.as_deref(),
        Some("_modus_M_std_string_Add_add_String")
    );

    // The re-exported barrel symbols keep their home-module symbols.
    let string_eq = iface
        .exported_functions
        .get("stringEq")
        .expect("re-exported stringEq");
    assert_eq!(
        string_eq.symbol_name.as_deref(),
        Some("_modus_M_std_string_stringEq")
    );
    let concat = iface
        .exported_functions
        .get("concat")
        .expect("re-exported concat");
    assert_eq!(
        concat.symbol_name.as_deref(),
        Some("_modus_M_std_string_concat")
    );

    // The home module's own env carries the impls with mangled symbols
    // (the pure-barrel prelude declares nothing itself).
    let string_id = ModuleId::new(PathBuf::from("std:string"));
    let env = envs.get(&string_id).unwrap();
    for (trait_name, method, symbol) in [
        ("Eq", "eq", "_modus_M_std_string_Eq_eq_String"),
        ("Add", "add", "_modus_M_std_string_Add_add_String"),
    ] {
        let registered = env
            .lookup_impls(trait_name)
            .and_then(|impls| {
                impls
                    .iter()
                    .find(|d| d.target_type == modus::typechecker::Type::string())
            })
            .unwrap_or_else(|| panic!("{trait_name} impl registered in std:string env"));
        assert_eq!(
            registered
                .methods
                .get(method)
                .and_then(|s| s.symbol_name.clone()),
            Some(symbol.to_string())
        );
    }
}

#[test]
fn test_string_concat_symbol_matches_intrinsic() {
    // `build_string_concat` intercepts calls to this exact symbol (see
    // `STRING_CONCAT_INTRINSIC`); if the mangling ever changes, inlining
    // silently stops and `concat`/`add` recurse forever. Pin it here.
    let graph = ModuleGraph::build_from_source(
        Path::new("std:prelude"),
        modus::modules::stdlib::STD_PRELUDE_SOURCE,
    )
    .expect("Prelude graph should build");
    let (interfaces, _) =
        modus::modules::check_module_graph_with_envs(&graph).expect("Prelude should typecheck");
    let iface = interfaces
        .get(&modus::prelude_module_id())
        .expect("Prelude interface");
    assert_eq!(
        iface
            .exported_functions
            .get("concat")
            .and_then(|s| s.symbol_name.clone()),
        Some("_modus_M_std_string_concat".to_string())
    );
}

#[test]
fn test_string_eq_graph_mode_without_import() {
    let graph = ModuleGraph::build_from_source(Path::new("main.mds"), EQ_PROGRAM)
        .expect("Graph should build");
    assert!(graph.modules.contains_key(&modus::prelude_module_id()));
    let res = jit_run_graph(&graph).expect("JIT execution failed");
    assert_eq!(res, ExecutionResult::I32(0));
}

#[test]
fn test_string_eq_single_module_ir_uses_prelude_impl() {
    let context = Context::create();
    let codegen =
        modus::compile_source(&context, EQ_PROGRAM, "test_eq").expect("Compilation failed");
    assert!(codegen.module.verify().is_ok());
    let ir = codegen.to_ir_string();
    assert!(
        ir.contains("call fastcc i1 @_modus_M_std_string_Eq_eq_String"),
        "expected Eq impl call in IR, got:\n{ir}"
    );
    assert!(
        ir.contains("define fastcc i1 @_modus_M_std_string_Eq_eq_String"),
        "expected Eq impl definition in single-module IR, got:\n{ir}"
    );
    assert!(
        !ir.contains("modus_str_eq"),
        "old runtime helper must be gone from IR"
    );
    assert!(!ir.contains("memcmp"), "memcmp must be gone from IR");
}

#[test]
fn test_string_eq_single_module_jit() {
    let context = Context::create();
    let codegen =
        modus::compile_source(&context, EQ_PROGRAM, "test_eq_jit").expect("Compilation failed");
    let res = codegen.jit_run().expect("JIT execution failed");
    assert_eq!(res, ExecutionResult::I32(0));
}

const ADD_PROGRAM: &str = r#"
function main(): i32 {
    let a: String = "foo";
    let b: String = "bar";
    let c: String = a + b;
    if (c != "foobar") { return 1; }
    if ((a + "") != "foo") { return 2; }
    if (("" + b) != "bar") { return 3; }
    if ((a + b + c) != "foobarfoobar") { return 4; }
    let n: i32 = 20 + 22;
    if (n != 42) { return 5; }
    return 0;
}
"#;

#[test]
fn test_string_add_graph_mode_without_import() {
    let graph = ModuleGraph::build_from_source(Path::new("main.mds"), ADD_PROGRAM)
        .expect("Graph should build");
    assert!(graph.modules.contains_key(&modus::prelude_module_id()));
    let res = jit_run_graph(&graph).expect("JIT execution failed");
    assert_eq!(res, ExecutionResult::I32(0));
}

#[test]
fn test_string_add_single_module_ir_uses_add_impl() {
    let context = Context::create();
    let codegen =
        modus::compile_source(&context, ADD_PROGRAM, "test_add").expect("Compilation failed");
    assert!(codegen.module.verify().is_ok());
    let ir = codegen.to_ir_string();
    assert!(
        ir.contains("call fastcc ptr @_modus_M_std_string_Add_add_String"),
        "expected Add impl call in IR, got:\n{ir}"
    );
    // The impl body inlines `concat`: no *call* to it may remain
    // (termination); its (dead but valid) definition is still emitted.
    assert!(
        !ir.contains("call fastcc ptr @_modus_M_std_string_concat"),
        "concat must be inlined, not called"
    );
    assert!(
        ir.contains("define fastcc ptr @_modus_M_std_string_concat"),
        "expected (dead) concat definition in merged IR, got:\n{ir}"
    );
    assert!(
        !ir.contains("modus_str_concat"),
        "old runtime helper must be gone from IR"
    );
    assert!(ir.contains("modus_alloc"), "expected inline alloc");
    assert!(ir.contains("memcpy"), "expected inline memcpy");
}

#[test]
fn test_string_add_single_module_jit() {
    let context = Context::create();
    let codegen =
        modus::compile_source(&context, ADD_PROGRAM, "test_add_jit").expect("Compilation failed");
    let res = codegen.jit_run().expect("JIT execution failed");
    assert_eq!(res, ExecutionResult::I32(0));
}

#[test]
fn test_concat_aliasing_and_empty() {
    // `x + x` (same var twice) and empty-string edge cases: RC balance check.
    let src = r#"
function main(): i32 {
    let x: String = "ab";
    if ((x + x) != "abab") { return 1; }
    if ((x + "") != "ab") { return 2; }
    if (("" + "") != "") { return 3; }
    let y: String = x + x;
    if ((y + y) != "abababab") { return 4; }
    return 0;
}
"#;
    let graph =
        ModuleGraph::build_from_source(Path::new("main.mds"), src).expect("Graph should build");
    assert_eq!(
        jit_run_graph(&graph).expect("JIT execution failed"),
        ExecutionResult::I32(0)
    );
    let context = Context::create();
    let codegen = modus::compile_source(&context, src, "test_alias").expect("Compilation failed");
    assert_eq!(
        codegen.jit_run().expect("JIT execution failed"),
        ExecutionResult::I32(0)
    );
}

fn check_err_kind(src: &str) -> modus::typechecker::TypeErrorKind {
    let prog = parse_program(src).expect("test snippet should parse");
    modus::typechecker::check_program(&prog)
        .expect_err("expected a type error")
        .kind
}

#[test]
fn test_eq_on_types_without_impl_is_error() {
    // Records, arrays, and unions have no `Eq` impl: `==` must be rejected.
    // (Bare `check_program` has no prelude impls; primitives/pointers are
    // still accepted via the builtin rules.)
    let rec_err = check_err_kind(
        r#"
type Point = { x: i32, y: i32 };
function main(): i32 {
    let a: Point = { x: 1, y: 2 };
    let b: Point = { x: 1, y: 2 };
    return if (a == b) { 1 } else { 0 };
}
"#,
    );
    assert!(
        matches!(
            rec_err,
            modus::typechecker::TypeErrorKind::TraitNotImplemented { ref trait_name, .. }
            if trait_name == "Eq"
        ),
        "expected missing-Eq error, got {rec_err:?}"
    );

    let arr_err = check_err_kind(
        r#"
function main(): i32 {
    let a: [i32] = [1, 2];
    let b: [i32] = [1, 2];
    return if (a == b) { 1 } else { 0 };
}
"#,
    );
    assert!(
        matches!(
            arr_err,
            modus::typechecker::TypeErrorKind::TraitNotImplemented { ref trait_name, .. }
            if trait_name == "Eq"
        ),
        "expected missing-Eq error, got {arr_err:?}"
    );

    // Primitives, bools, and pointers still compare without an impl.
    parse_check_ok(
        r#"
function main(): i32 {
    let p: Pointer(u8) = Pointer.null();
    let q: Pointer(u8) = Pointer.null();
    if (1 == 1 && true != false && p == q) { return 1; } else { return 0; }
}
"#,
    );
}

#[test]
fn test_add_on_types_without_impl_is_error() {
    let rec_err = check_err_kind(
        r#"
type Point = { x: i32, y: i32 };
function main(): i32 {
    let a: Point = { x: 1, y: 2 };
    let b: Point = { x: 0, y: 0 };
    let c: Point = { x: a.x + b.x, y: a.y + b.y };
    return c.x;
}
function bad(a: Point, b: Point): Point {
    return a + b;
}
"#,
    );
    assert!(
        matches!(
            rec_err,
            modus::typechecker::TypeErrorKind::TraitNotImplemented { ref trait_name, .. }
            if trait_name == "Add"
        ),
        "expected missing-Add error, got {rec_err:?}"
    );

    // Numerics still add without an impl.
    parse_check_ok(
        r#"
function main(): i32 {
    return 20 + 22;
}
"#,
    );
}

fn parse_check_ok(src: &str) {
    let prog = parse_program(src).expect("test snippet should parse");
    modus::typechecker::check_program(&prog).expect("expected snippet to typecheck");
}

#[test]
fn test_reexport_barrel_end_to_end() {
    // middle.mds re-exports from leaf.mds; main imports through the barrel.
    let temp_dir = std::env::temp_dir().join(format!("modus_test_barrel_{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&temp_dir);
    std::fs::create_dir_all(&temp_dir).unwrap();

    std::fs::write(
        temp_dir.join("leaf.mds"),
        "export function answer(): i32 { return 42; }\n",
    )
    .unwrap();
    std::fs::write(
        temp_dir.join("middle.mds"),
        "export { answer } from \"./leaf.mds\";\n",
    )
    .unwrap();
    let main_path = temp_dir.join("main.mds");
    std::fs::write(
        &main_path,
        "import { answer } from \"./middle.mds\";\nfunction main(): i32 { return answer(); }\n",
    )
    .unwrap();

    let graph = ModuleGraph::build(&main_path).expect("Graph should build");
    let res = jit_run_graph(&graph).expect("JIT execution failed");
    assert_eq!(res, ExecutionResult::I32(42));

    let _ = std::fs::remove_dir_all(&temp_dir);
}
