use modus::backend::ExecutionResult;
use modus::modules::{
    ModuleGraph, ModuleId, build_executable, jit_run_graph, resolve_module_path,
    stdlib::{STD_MATH_SOURCE, is_std_module},
};
use modus::parser::parse_program;
use modus::typechecker::check_program;
use std::path::{Path, PathBuf};

#[test]
fn test_stdlib_math_source_parses_and_typechecks() {
    assert!(is_std_module("std:math"));
    let prog = parse_program(STD_MATH_SOURCE).expect("Failed to parse std:math source");
    let env = check_program(&prog).expect("Failed to typecheck std:math source");

    // Verify key exported functions exist
    assert!(env.lookup_function("PI").is_some());
    assert!(env.lookup_function("E").is_some());
    assert!(env.lookup_function("abs").is_some());
    assert!(env.lookup_function("absI").is_some());
    assert!(env.lookup_function("min").is_some());
    assert!(env.lookup_function("max").is_some());
    assert!(env.lookup_function("sqrt").is_some());
    assert!(env.lookup_function("pow").is_some());
    assert!(env.lookup_function("sin").is_some());
    assert!(env.lookup_function("cos").is_some());
    assert!(env.lookup_function("floor").is_some());
    assert!(env.lookup_function("ceil").is_some());
    assert!(env.lookup_function("round").is_some());
    assert!(env.lookup_function("random").is_some());
}

#[test]
fn test_stdlib_math_module_resolution() {
    let resolved = resolve_module_path("std:math", None).expect("std:math should resolve");
    assert_eq!(resolved, PathBuf::from("std:math"));
}

#[test]
fn test_stdlib_math_graph_construction() {
    let user_src = r#"
        import { PI, sin } from "std:math";

        function main(): IO(f64) {
            let s: f64 = sin(PI() / 2.0);
            return IO.pure(s);
        }
    "#;

    let graph = ModuleGraph::build_from_source(Path::new("main.mds"), user_src)
        .expect("Failed to build graph with std:math");

    // main + std:math + implicit std:prelude + std:string
    assert_eq!(graph.modules.len(), 4);
    assert!(
        graph
            .modules
            .contains_key(&ModuleId::new(PathBuf::from("std:math")))
    );
}

#[test]
fn test_stdlib_math_jit_constants_and_trig() {
    let user_src = r#"
        import { PI, sin, cos } from "std:math";

        function main(): IO(i32) {
            let s: f64 = sin(PI() / 2.0);
            let c: f64 = cos(0.0);
            if (s > 0.99 && s < 1.01 && c > 0.99 && c < 1.01) {
                return IO.pure(1);
            } else {
                return IO.pure(0);
            }
        }
    "#;

    let graph = ModuleGraph::build_from_source(Path::new("main.mds"), user_src)
        .expect("Failed to build graph");
    let res = jit_run_graph(&graph).expect("JIT execution failed");
    assert_eq!(res, ExecutionResult::I32(1));
}

#[test]
fn test_stdlib_math_jit_powers_and_roots() {
    let user_src = r#"
        import { sqrt, cbrt, pow, hypot } from "std:math";

        function main(): IO(i32) {
            let s: f64 = sqrt(16.0);      // 4.0
            let c: f64 = cbrt(27.0);      // 3.0
            let p: f64 = pow(2.0, 3.0);   // 8.0
            let h: f64 = hypot(3.0, 4.0); // 5.0

            let ok_s: bool = s > 3.99 && s < 4.01;
            let ok_c: bool = c > 2.99 && c < 3.01;
            let ok_p: bool = p > 7.99 && p < 8.01;
            let ok_h: bool = h > 4.99 && h < 5.01;

            if (ok_s && ok_c && ok_p && ok_h) {
                return IO.pure(1);
            } else {
                return IO.pure(0);
            }
        }
    "#;

    let graph = ModuleGraph::build_from_source(Path::new("main.mds"), user_src)
        .expect("Failed to build graph");
    let res = jit_run_graph(&graph).expect("JIT execution failed");
    assert_eq!(res, ExecutionResult::I32(1));
}

#[test]
fn test_stdlib_math_jit_rounding_and_clamp() {
    let user_src = r#"
        import { floor, ceil, round, trunc, clamp, min, max } from "std:math";

        function main(): IO(i32) {
            let fl: f64 = floor(3.7);     // 3.0
            let ce: f64 = ceil(3.2);      // 4.0
            let ro: f64 = round(3.5);     // 4.0
            let tr: f64 = trunc(-3.7);    // -3.0
            let cl: f64 = clamp(15.0, 0.0, 10.0); // 10.0
            let mn: f64 = min(5.0, 2.0);  // 2.0
            let mx: f64 = max(5.0, 2.0);  // 5.0

            let ok_fl: bool = fl > 2.99 && fl < 3.01;
            let ok_ce: bool = ce > 3.99 && ce < 4.01;
            let ok_ro: bool = ro > 3.99 && ro < 4.01;
            let ok_tr: bool = tr > -3.01 && tr < -2.99;
            let ok_cl: bool = cl > 9.99 && cl < 10.01;
            let ok_mn: bool = mn > 1.99 && mn < 2.01;
            let ok_mx: bool = mx > 4.99 && mx < 5.01;

            if (ok_fl && ok_ce && ok_ro && ok_tr && ok_cl && ok_mn && ok_mx) {
                return IO.pure(1);
            } else {
                return IO.pure(0);
            }
        }
    "#;

    let graph = ModuleGraph::build_from_source(Path::new("main.mds"), user_src)
        .expect("Failed to build graph");
    let res = jit_run_graph(&graph).expect("JIT execution failed");
    assert_eq!(res, ExecutionResult::I32(1));
}

#[test]
fn test_stdlib_math_jit_integer_utils() {
    let user_src = r#"
        import { absI, signI, minI, maxI, clampI, imul, clz32 } from "std:math";

        function main(): IO(i32) {
            let a: i64 = absI(-42);            // 42
            let s: i64 = signI(-99);           // -1
            let mn: i64 = minI(10, 20);        // 10
            let mx: i64 = maxI(10, 20);        // 20
            let cl: i64 = clampI(100, 0, 50);  // 50
            let im: i64 = imul(10, 20);        // 200
            let lz: i64 = clz32(1);            // 31
            let lz0: i64 = clz32(0);           // 32

            if (a == 42 && s == -1 && mn == 10 && mx == 20 && cl == 50 && im == 200 && lz == 31 && lz0 == 32) {
                return IO.pure(1);
            } else {
                return IO.pure(0);
            }
        }
    "#;

    let graph = ModuleGraph::build_from_source(Path::new("main.mds"), user_src)
        .expect("Failed to build graph");
    let res = jit_run_graph(&graph).expect("JIT execution failed");
    assert_eq!(res, ExecutionResult::I32(1));
}

#[test]
fn test_stdlib_math_jit_random() {
    let user_src = r#"
        import { random } from "std:math";

        function main(): IO(i32) {
            let r: f64 = perform random();
            if (r >= 0.0 && r < 1.0) {
                return IO.pure(1);
            } else {
                return IO.pure(0);
            }
        }
    "#;

    let graph = ModuleGraph::build_from_source(Path::new("main.mds"), user_src)
        .expect("Failed to build graph");
    let res = jit_run_graph(&graph).expect("JIT execution failed");
    assert_eq!(res, ExecutionResult::I32(1));
}

#[test]
fn test_stdlib_math_aot_compile_and_execute() {
    let user_src = r#"
        import { PI, sin, cos, sqrt } from "std:math";

        function main(): IO(i32) {
            let s: f64 = sin(PI() / 6.0); // 0.5
            let sq: f64 = sqrt(4.0);      // 2.0
            if (s > 0.49 && s < 0.51 && sq > 1.99 && sq < 2.01) {
                return IO.pure(0);
            } else {
                return IO.pure(1);
            }
        }
    "#;

    let temp_dir = std::env::temp_dir().join("modus_test_math_aot");
    let _ = std::fs::create_dir_all(&temp_dir);
    let main_path = temp_dir.join("main.mds");
    let bin_path = temp_dir.join("test_math_bin");

    std::fs::write(&main_path, user_src).expect("Failed to write main.mds");
    let output_bin = build_executable(&main_path, &bin_path, None).expect("AOT build failed");
    assert!(output_bin.exists());

    let status = std::process::Command::new(&output_bin)
        .status()
        .expect("Failed to execute compiled binary");

    assert_eq!(status.code(), Some(0));

    let _ = std::fs::remove_dir_all(&temp_dir);
}
