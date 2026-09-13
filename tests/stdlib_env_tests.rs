use modus::backend::ExecutionResult;
use modus::modules::{
    ModuleGraph, ModuleId, ResolveError, build_executable, jit_run_graph, resolve_module_path,
    stdlib::{STD_ENV_SOURCE, is_std_module},
};
use modus::parser::parse_program;
use modus::typechecker::check_program;
use std::path::{Path, PathBuf};

#[test]
fn test_stdlib_env_source_parses_and_typechecks() {
    assert!(is_std_module("std:env"));
    let prog = parse_program(STD_ENV_SOURCE).expect("Failed to parse std:env source");
    let env = check_program(&prog).expect("Failed to typecheck std:env source");

    // Verify key types exist
    assert!(env.types.contains_key("IOError"));

    // Verify key exported functions exist
    assert!(env.lookup_function("getEnv").is_some());
    assert!(env.lookup_function("setEnv").is_some());
    assert!(env.lookup_function("removeEnv").is_some());
    assert!(env.lookup_function("currentDir").is_some());
    assert!(env.lookup_function("setCurrentDir").is_some());
    assert!(env.lookup_function("tempDir").is_some());
    assert!(env.lookup_function("currentExe").is_some());
}

#[test]
fn test_stdlib_env_module_resolution() {
    let resolved = resolve_module_path("std:env", None).expect("std:env should resolve");
    assert_eq!(resolved, PathBuf::from("std:env"));

    let err = resolve_module_path("std:nonexistent", None).unwrap_err();
    match err {
        ResolveError::UnknownStdModule { module } => {
            assert_eq!(module, "std:nonexistent");
        }
        _ => panic!("Expected UnknownStdModule error, got: {:?}", err),
    }
}

#[test]
fn test_stdlib_env_graph_construction() {
    let user_src = r#"
        import { tempDir } from "std:env";

        function main(): IO(String) {
            let t: String = perform tempDir();
            return IO.pure(t);
        }
    "#;

    let graph = ModuleGraph::build_from_source(Path::new("main.mds"), user_src)
        .expect("Failed to build graph with std:env");

    assert_eq!(graph.modules.len(), 2);
    assert!(
        graph
            .modules
            .contains_key(&ModuleId::new(PathBuf::from("std:env")))
    );
}

#[test]
fn test_stdlib_env_jit_set_and_get_env() {
    let user_src = r#"
        import { getEnv, setEnv, IOError } from "std:env";

        function main(): IO(i32) {
            let s: Result(void, IOError) = perform setEnv("MODUS_TEST_KEY_1", "MODUS_VAL_OK");
            let g: Result(String, IOError) = perform getEnv("MODUS_TEST_KEY_1");
            return match (g) {
                Result.Ok(val) => {
                    if (val == "MODUS_VAL_OK") {
                        IO.pure(1)
                    } else {
                        IO.pure(0)
                    }
                },
                Result.Err(_) => IO.pure(-1),
            };
        }
    "#;

    let graph = ModuleGraph::build_from_source(Path::new("main.mds"), user_src)
        .expect("Failed to build graph");
    let res = jit_run_graph(&graph).expect("JIT execution failed");
    assert_eq!(res, ExecutionResult::I32(1));
}

#[test]
fn test_stdlib_env_jit_remove_env() {
    let user_src = r#"
        import { getEnv, setEnv, removeEnv, IOError } from "std:env";

        function main(): IO(i32) {
            let s: Result(void, IOError) = perform setEnv("MODUS_TMP_KEY", "VALUE");
            let r: Result(void, IOError) = perform removeEnv("MODUS_TMP_KEY");
            let g: Result(String, IOError) = perform getEnv("MODUS_TMP_KEY");
            return match (g) {
                Result.Ok(_) => IO.pure(0),
                Result.Err(_) => IO.pure(1),
            };
        }
    "#;

    let graph = ModuleGraph::build_from_source(Path::new("main.mds"), user_src)
        .expect("Failed to build graph");
    let res = jit_run_graph(&graph).expect("JIT execution failed");
    assert_eq!(res, ExecutionResult::I32(1));
}

#[test]
fn test_stdlib_env_jit_current_dir() {
    let user_src = r#"
        import { currentDir, IOError } from "std:env";

        function main(): IO(i32) {
            let cwd_res: Result(String, IOError) = perform currentDir();
            return match (cwd_res) {
                Result.Ok(cwd) => {
                    if (cwd.length() > 0) {
                        IO.pure(1)
                    } else {
                        IO.pure(0)
                    }
                },
                Result.Err(_) => IO.pure(-1),
            };
        }
    "#;

    let graph = ModuleGraph::build_from_source(Path::new("main.mds"), user_src)
        .expect("Failed to build graph");
    let res = jit_run_graph(&graph).expect("JIT execution failed");
    assert_eq!(res, ExecutionResult::I32(1));
}

#[test]
fn test_stdlib_env_jit_set_current_dir() {
    let user_src = r#"
        import { currentDir, setCurrentDir, IOError } from "std:env";

        function main(): IO(i32) {
            let orig_res: Result(String, IOError) = perform currentDir();
            return match (orig_res) {
                Result.Ok(orig) => {
                    let ch: Result(void, IOError) = perform setCurrentDir("/tmp");
                    let check_res: Result(String, IOError) = perform currentDir();
                    let back: Result(void, IOError) = perform setCurrentDir(orig);
                    match (check_res) {
                        Result.Ok(new_cwd) => {
                            if (new_cwd == "/tmp") {
                                IO.pure(1)
                            } else {
                                IO.pure(0)
                            }
                        },
                        Result.Err(_) => IO.pure(-2),
                    }
                },
                Result.Err(_) => IO.pure(-1),
            };
        }
    "#;

    let graph = ModuleGraph::build_from_source(Path::new("main.mds"), user_src)
        .expect("Failed to build graph");
    let res = jit_run_graph(&graph).expect("JIT execution failed");
    assert_eq!(res, ExecutionResult::I32(1));
}

#[test]
fn test_stdlib_env_jit_temp_dir() {
    let user_src = r#"
        import { tempDir } from "std:env";

        function main(): IO(i32) {
            let tmp: String = perform tempDir();
            if (tmp.length() > 0) {
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
fn test_stdlib_env_jit_current_exe() {
    let user_src = r#"
        import { currentExe, IOError } from "std:env";

        function main(): IO(i32) {
            let exe_res: Result(String, IOError) = perform currentExe();
            return match (exe_res) {
                Result.Ok(exe) => {
                    if (exe.length() > 0) {
                        IO.pure(1)
                    } else {
                        IO.pure(0)
                    }
                },
                Result.Err(_) => IO.pure(0),
            };
        }
    "#;

    let graph = ModuleGraph::build_from_source(Path::new("main.mds"), user_src)
        .expect("Failed to build graph");
    let res = jit_run_graph(&graph).expect("JIT execution failed");
    assert_eq!(res, ExecutionResult::I32(1));
}

#[test]
fn test_stdlib_env_aot_compile_and_execute() {
    let temp_dir = std::env::temp_dir().join(format!("modus_test_env_aot_{}", std::process::id()));
    std::fs::create_dir_all(&temp_dir).expect("Failed to create tempdir");
    let main_path = temp_dir.join("main.mds");
    let bin_path = temp_dir.join("test_env_bin");

    let main_src = r#"
        import { getEnv, setEnv, IOError } from "std:env";

        function main(): IO(i32) {
            let s: Result(void, IOError) = perform setEnv("MODUS_AOT_ENV_TEST", "HELLO_AOT");
            let g: Result(String, IOError) = perform getEnv("MODUS_AOT_ENV_TEST");
            return match (g) {
                Result.Ok(val) => {
                    if (val == "HELLO_AOT") {
                        IO.pure(0)
                    } else {
                        IO.pure(1)
                    }
                },
                Result.Err(_) => IO.pure(2),
            };
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
