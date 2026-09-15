use modus::backend::ExecutionResult;
use modus::modules::{
    ModuleGraph, ModuleId, ResolveError, build_executable, jit_run_graph, resolve_module_path,
    stdlib::{STD_PROCESS_SOURCE, is_std_module},
};
use modus::parser::parse_program;
use modus::typechecker::check_program;
use std::path::{Path, PathBuf};

#[test]
fn test_stdlib_process_source_parses_and_typechecks() {
    assert!(is_std_module("std:process"));
    let prog = parse_program(STD_PROCESS_SOURCE).expect("Failed to parse std:process source");
    let env = check_program(&prog).expect("Failed to typecheck std:process source");

    // Verify key exported functions exist
    assert!(env.lookup_function("pid").is_some());
    assert!(env.lookup_function("parentPid").is_some());
    assert!(env.lookup_function("exit").is_some());
    assert!(env.lookup_function("abort").is_some());
}

#[test]
fn test_stdlib_process_module_resolution() {
    let resolved = resolve_module_path("std:process", None).expect("std:process should resolve");
    assert_eq!(resolved, PathBuf::from("std:process"));

    let err = resolve_module_path("std:unknown_proc", None).unwrap_err();
    match err {
        ResolveError::UnknownStdModule { module } => {
            assert_eq!(module, "std:unknown_proc");
        }
        _ => panic!("Expected UnknownStdModule error, got: {:?}", err),
    }
}

#[test]
fn test_stdlib_process_graph_construction() {
    let user_src = r#"
        import { pid } from "std:process";

        function main(): IO(i32) {
            let p: i32 = perform pid();
            return IO.pure(p);
        }
    "#;

    let graph = ModuleGraph::build_from_source(Path::new("main.mds"), user_src)
        .expect("Failed to build graph with std:process");

    // main + std:process + implicit std:prelude + std:string
    assert_eq!(graph.modules.len(), 4);
    assert!(
        graph
            .modules
            .contains_key(&ModuleId::new(PathBuf::from("std:process")))
    );
}

#[test]
fn test_stdlib_process_jit_pid() {
    let user_src = r#"
        import { pid } from "std:process";

        function main(): IO(i32) {
            let p: i32 = perform pid();
            return IO.pure(p);
        }
    "#;

    let graph = ModuleGraph::build_from_source(Path::new("main.mds"), user_src)
        .expect("Failed to build graph");
    let res = jit_run_graph(&graph).expect("JIT execution failed");

    let expected_pid = std::process::id() as i32;
    assert_eq!(res, ExecutionResult::I32(expected_pid));
}

#[test]
fn test_stdlib_process_jit_parent_pid() {
    let user_src = r#"
        import { parentPid } from "std:process";

        function main(): IO(i32) {
            let pp: i32 = perform parentPid();
            if (pp > 0) {
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
fn test_stdlib_process_aot_compile_and_exit_code() {
    let temp_dir =
        std::env::temp_dir().join(format!("modus_test_proc_exit_{}", std::process::id()));
    std::fs::create_dir_all(&temp_dir).expect("Failed to create tempdir");
    let main_path = temp_dir.join("main.mds");
    let bin_path = temp_dir.join("test_exit_bin");

    let main_src = r#"
        import { exit } from "std:process";

        function main(): IO(void) {
            perform exit(42);
            return IO.pure(());
        }
    "#;

    std::fs::write(&main_path, main_src).expect("Failed to write main.mds");
    let output_bin = build_executable(&main_path, &bin_path, None).expect("AOT build failed");
    assert!(output_bin.exists());

    let status = std::process::Command::new(&output_bin)
        .status()
        .expect("Failed to execute AOT binary");

    assert_eq!(status.code(), Some(42));

    let _ = std::fs::remove_dir_all(&temp_dir);
}

#[test]
fn test_stdlib_process_aot_compile_and_exit_zero() {
    let temp_dir =
        std::env::temp_dir().join(format!("modus_test_proc_exit0_{}", std::process::id()));
    std::fs::create_dir_all(&temp_dir).expect("Failed to create tempdir");
    let main_path = temp_dir.join("main.mds");
    let bin_path = temp_dir.join("test_exit0_bin");

    let main_src = r#"
        import { exit } from "std:process";

        function main(): IO(void) {
            perform exit(0);
            return IO.pure(());
        }
    "#;

    std::fs::write(&main_path, main_src).expect("Failed to write main.mds");
    let output_bin = build_executable(&main_path, &bin_path, None).expect("AOT build failed");
    assert!(output_bin.exists());

    let status = std::process::Command::new(&output_bin)
        .status()
        .expect("Failed to execute AOT binary");

    assert_eq!(status.code(), Some(0));

    let _ = std::fs::remove_dir_all(&temp_dir);
}

#[test]
fn test_stdlib_process_aot_compile_and_abort() {
    let temp_dir =
        std::env::temp_dir().join(format!("modus_test_proc_abort_{}", std::process::id()));
    std::fs::create_dir_all(&temp_dir).expect("Failed to create tempdir");
    let main_path = temp_dir.join("main.mds");
    let bin_path = temp_dir.join("test_abort_bin");

    let main_src = r#"
        import { abort } from "std:process";

        function main(): IO(void) {
            perform abort();
            return IO.pure(());
        }
    "#;

    std::fs::write(&main_path, main_src).expect("Failed to write main.mds");
    let output_bin = build_executable(&main_path, &bin_path, None).expect("AOT build failed");
    assert!(output_bin.exists());

    let status = std::process::Command::new(&output_bin)
        .status()
        .expect("Failed to execute AOT binary");

    // Abort terminates via signal (SIGABRT on Unix), so success is false and status code is not 0
    assert!(!status.success());

    let _ = std::fs::remove_dir_all(&temp_dir);
}
