use modus::backend::ExecutionResult;
use modus::modules::{
    ModuleGraph, ModuleId, build_executable, jit_run_graph, resolve_module_path,
    stdlib::{STD_TIME_SOURCE, is_std_module},
};
use modus::parser::parse_program;
use modus::typechecker::check_program;
use std::path::{Path, PathBuf};

#[test]
fn test_stdlib_time_module_resolution() {
    assert!(is_std_module("std:time"));
    let resolved = resolve_module_path("std:time", None).expect("Failed to resolve std:time");
    assert_eq!(resolved, PathBuf::from("std:time"));
}

#[test]
fn test_stdlib_time_source_parses_and_typechecks() {
    assert!(is_std_module("std:time"));
    let prog = parse_program(STD_TIME_SOURCE).expect("Failed to parse std:time source");
    let env = check_program(&prog).expect("Failed to typecheck std:time source");

    // Verify key exported functions and types exist
    assert!(env.lookup_function("now").is_some());
    assert!(env.lookup_function("nowNanos").is_some());
    assert!(env.lookup_function("sleep").is_some());
    assert!(env.lookup_function("sleepDuration").is_some());
    assert!(env.lookup_function("durationFromNanos").is_some());
    assert!(env.lookup_function("durationFromMillis").is_some());
    assert!(env.lookup_function("durationFromSecs").is_some());
    assert!(env.lookup_function("durationToNanos").is_some());
    assert!(env.lookup_function("durationToMillis").is_some());
    assert!(env.lookup_function("durationToSecs").is_some());
    assert!(env.lookup_function("durationAdd").is_some());
    assert!(env.lookup_function("durationSub").is_some());
    assert!(env.lookup_function("instantNow").is_some());
    assert!(env.lookup_function("instantElapsed").is_some());
    assert!(env.lookup_function("instantDiff").is_some());
}

#[test]
fn test_stdlib_time_graph_construction() {
    let user_src = r#"
        import { now, sleep } from "std:time";

        function main(): IO(i32) {
            let t: u64 = perform now();
            perform sleep(1);
            return IO.pure(0);
        }
    "#;

    let graph = ModuleGraph::build_from_source(Path::new("main.mds"), user_src)
        .expect("Failed to build module graph");
    assert_eq!(graph.modules.len(), 2);
    assert!(
        graph
            .modules
            .contains_key(&ModuleId::new(PathBuf::from("std:time")))
    );
}

#[test]
fn test_stdlib_time_jit_durations() {
    let user_src = r#"
        import {
            durationFromMillis,
            durationFromSecs,
            durationToNanos,
            durationToMillis,
            durationToSecs,
            durationAdd,
            durationSub
        } from "std:time";

        function main(): i32 {
            let d1 = durationFromMillis(50);
            let d2 = durationFromMillis(20);
            let sum = durationAdd(d1, d2);
            let diff = durationSub(d1, d2);

            if (durationToMillis(sum) != 70) {
                return 1;
            }
            if (durationToMillis(diff) != 30) {
                return 2;
            }
            if (durationToNanos(sum) != 70000000) {
                return 3;
            }

            let d_sec = durationFromSecs(3);
            if (durationToSecs(d_sec) != 3) {
                return 4;
            }
            if (durationToMillis(d_sec) != 3000) {
                return 5;
            }

            // Underflow check on sub
            let neg = durationSub(d2, d1);
            if (durationToNanos(neg) != 0) {
                return 6;
            }

            return 0;
        }
    "#;

    let graph = ModuleGraph::build_from_source(Path::new("main.mds"), user_src)
        .expect("Failed to build module graph");
    let res = jit_run_graph(&graph).expect("JIT execution failed");
    match res {
        ExecutionResult::I32(val) => {
            assert_eq!(val, 0, "Time duration checks returned error code {val}")
        }
        _ => panic!("Expected I32 return"),
    }
}

#[test]
fn test_stdlib_time_jit_now_and_instants() {
    let user_src = r#"
        import {
            now,
            nowNanos,
            instantNow,
            instantDiff,
            instantElapsed,
            durationToNanos
        } from "std:time";

        function main(): IO(i32) {
            let t_nanos: u64 = perform nowNanos();
            let t_millis: u64 = perform now();

            if (t_nanos == 0) {
                return IO.pure(1);
            }
            if (t_millis == 0) {
                return IO.pure(2);
            }

            let i1 = perform instantNow();
            let i2 = perform instantNow();
            let diff = instantDiff(i2, i1);
            let elapsed = perform instantElapsed(i1);

            // Monotonic instants should be >= 0
            if (durationToNanos(diff) < 0) {
                return IO.pure(3);
            }
            if (durationToNanos(elapsed) < 0) {
                return IO.pure(4);
            }

            return IO.pure(0);
        }
    "#;

    let graph = ModuleGraph::build_from_source(Path::new("main.mds"), user_src)
        .expect("Failed to build module graph");
    let res = jit_run_graph(&graph).expect("JIT execution failed");
    match res {
        ExecutionResult::I32(val) => {
            assert_eq!(val, 0, "Time now/instants returned error code {val}")
        }
        _ => panic!("Expected I32 return"),
    }
}

#[test]
fn test_stdlib_time_jit_sleep() {
    let user_src = r#"
        import { sleep, sleepDuration, durationFromMillis } from "std:time";

        function main(): IO(i32) {
            perform sleep(1);
            let d = durationFromMillis(1);
            perform sleepDuration(d);
            return IO.pure(0);
        }
    "#;

    let graph = ModuleGraph::build_from_source(Path::new("main.mds"), user_src)
        .expect("Failed to build module graph");
    let res = jit_run_graph(&graph).expect("JIT execution failed");
    match res {
        ExecutionResult::I32(val) => {
            assert_eq!(val, 0, "Time sleep returned error code {val}")
        }
        _ => panic!("Expected I32 return"),
    }
}

#[test]
fn test_stdlib_time_aot_compile_and_execute() {
    let user_src = r#"
        import { now, durationFromSecs, durationToMillis } from "std:time";

        function main(): IO(i32) {
            let ts: u64 = perform now();
            let d = durationFromSecs(2);
            if (durationToMillis(d) == 2000 && ts > 0) {
                return IO.pure(42);
            } else {
                return IO.pure(1);
            }
        }
    "#;

    let temp_dir = std::env::temp_dir().join("modus_test_time_aot");
    let _ = std::fs::create_dir_all(&temp_dir);
    let main_file = temp_dir.join("main.mds");
    let output_bin = temp_dir.join("time_app");
    std::fs::write(&main_file, user_src).expect("Failed to write main.mds");

    build_executable(&main_file, &output_bin, None).expect("AOT build failed");
    assert!(output_bin.exists());

    let status = std::process::Command::new(&output_bin)
        .status()
        .expect("Failed to run AOT binary");
    assert_eq!(status.code(), Some(42));

    let _ = std::fs::remove_dir_all(&temp_dir);
}
