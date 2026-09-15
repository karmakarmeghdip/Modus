use modus::modules::{
    ModuleGraph, ModuleId, ResolveError, build_executable, jit_run_graph, resolve_module_path,
    stdlib::{STD_IO_SOURCE, is_std_module},
};
use modus::parser::parse_program;
use modus::typechecker::check_program;
use std::path::{Path, PathBuf};

unsafe extern "C" {
    fn pipe(fds: *mut i32) -> i32;
    fn close(fd: i32) -> i32;
    fn write(fd: i32, buf: *const u8, count: usize) -> isize;
}

#[test]
fn test_stdlib_io_source_parses_and_typechecks() {
    assert!(is_std_module("std:io"));
    let prog = parse_program(STD_IO_SOURCE).expect("Failed to parse std:io source");
    let env = check_program(&prog).expect("Failed to typecheck std:io source");

    // Verify key functions are in env
    assert!(env.lookup_function("print").is_some());
    assert!(env.lookup_function("println").is_some());
    assert!(env.lookup_function("eprint").is_some());
    assert!(env.lookup_function("eprintln").is_some());
    assert!(env.lookup_function("flush").is_some());
    assert!(env.lookup_function("writeRaw").is_some());
    assert!(env.lookup_function("readRaw").is_some());
    assert!(env.lookup_function("readLine").is_some());
    assert!(env.lookup_function("readLineFrom").is_some());
    assert!(env.lookup_function("stdin_fileno").is_some());
    assert!(env.lookup_function("stdout_fileno").is_some());
    assert!(env.lookup_function("stderr_fileno").is_some());

    // Verify IOError type is defined
    assert!(env.types.contains_key("IOError"));
}

#[test]
fn test_stdlib_io_module_resolution() {
    let resolved = resolve_module_path("std:io", None).expect("std:io should resolve");
    assert_eq!(resolved, PathBuf::from("std:io"));

    let err = resolve_module_path("std:nonexistent", None).unwrap_err();
    match err {
        ResolveError::UnknownStdModule { module } => {
            assert_eq!(module, "std:nonexistent");
        }
        _ => panic!("Expected UnknownStdModule error, got: {:?}", err),
    }
}

#[test]
fn test_stdlib_io_graph_construction() {
    let user_src = r#"
        import { print, println, stdin_fileno, stdout_fileno, stderr_fileno, IOError, writeRaw } from "std:io";

        function main(): IO(void) {
            let fd: i32 = stdout_fileno();
            perform println("Hello, Modus std:io!");
            return IO.pure(());
        }
    "#;

    let graph = ModuleGraph::build_from_source(Path::new("main.mds"), user_src)
        .expect("Failed to build graph with std:io");

    // main + std:io + implicit std:prelude + std:string
    assert_eq!(graph.modules.len(), 4);
    let std_id = ModuleId::new(PathBuf::from("std:io"));
    let main_id = ModuleId::new(PathBuf::from("main.mds"));
    let prelude_id = ModuleId::new(PathBuf::from("std:prelude"));
    let string_id = ModuleId::new(PathBuf::from("std:string"));

    assert!(graph.modules.contains_key(&std_id));
    assert!(graph.modules.contains_key(&main_id));
    assert!(graph.modules.contains_key(&prelude_id));
    assert!(graph.modules.contains_key(&string_id));

    // In topological order, dependencies must precede main
    let pos = |id: &ModuleId| graph.topo_order.iter().position(|m| m == id).unwrap();
    assert!(pos(&std_id) < pos(&main_id));
    assert!(pos(&string_id) < pos(&prelude_id));
    assert!(pos(&prelude_id) < pos(&main_id));

    // Verify JIT execution
    let res = jit_run_graph(&graph).expect("JIT execution failed");
    assert_eq!(res, modus::backend::ExecutionResult::Void);
}

#[test]
fn test_stdlib_io_jit_fileno_constants() {
    let user_src = r#"
        import { stdin_fileno, stdout_fileno, stderr_fileno } from "std:io";

        function main(): IO(i32) {
            let in_fd: i32 = stdin_fileno();
            let out_fd: i32 = stdout_fileno();
            let err_fd: i32 = stderr_fileno();
            return IO.pure(in_fd * 100 + out_fd * 10 + err_fd);
        }
    "#;

    let graph = ModuleGraph::build_from_source(Path::new("main.mds"), user_src)
        .expect("Failed to build graph");
    let res = jit_run_graph(&graph).expect("JIT execution failed");
    // in=0, out=1, err=2 -> 0*100 + 1*10 + 2 = 12
    assert_eq!(res, modus::backend::ExecutionResult::I32(12));
}

#[test]
fn test_stdlib_io_jit_print_and_flush() {
    let user_src = r#"
        import { print, println, eprint, eprintln, flush } from "std:io";

        function main(): IO(void) {
            perform print("Testing print... ");
            perform println("OK");
            perform eprint("Testing eprint... ");
            perform eprintln("OK");
            perform flush();
            return IO.pure(());
        }
    "#;

    let graph = ModuleGraph::build_from_source(Path::new("main.mds"), user_src)
        .expect("Failed to build graph");
    let res = jit_run_graph(&graph).expect("JIT execution failed");
    assert_eq!(res, modus::backend::ExecutionResult::Void);
}

#[test]
fn test_stdlib_io_jit_raw_write_and_read() {
    // Create an OS pipe
    let mut pipe_fds = [0i32; 2];
    let pipe_res = unsafe { pipe(pipe_fds.as_mut_ptr()) };
    assert_eq!(pipe_res, 0, "pipe creation failed");

    let read_fd = pipe_fds[0];
    let write_fd = pipe_fds[1];

    let user_src = format!(
        r#"
        import {{ writeRaw, readRaw, IOError }} from "std:io";

        extern "C" {{
            function malloc(size: u64): IO(Pointer(u8));
        }}

        function main(): IO(i64) {{
            let write_buf: Pointer(u8) = String.toCString("PING");
            let write_res: Result(i64, IOError) = perform writeRaw({write_fd}, write_buf, 4);

            let read_buf: Pointer(u8) = perform malloc(16);
            let read_res: Result(i64, IOError) = perform readRaw({read_fd}, read_buf, 4);

            let fallback: i64 = -1;
            match (read_res) {{
                Result.Ok(n) => IO.pure(n),
                Result.Err(e) => IO.pure(fallback),
            }}
        }}
    "#
    );

    let graph = ModuleGraph::build_from_source(Path::new("main.mds"), &user_src)
        .expect("Failed to build graph");
    let res = jit_run_graph(&graph).expect("JIT execution failed");

    unsafe {
        close(read_fd);
        close(write_fd);
    }

    assert_eq!(res, modus::backend::ExecutionResult::I64(4));
}

#[test]
fn test_stdlib_io_jit_read_line_from_pipe() {
    let mut pipe_fds = [0i32; 2];
    let pipe_res = unsafe { pipe(pipe_fds.as_mut_ptr()) };
    assert_eq!(pipe_res, 0, "pipe creation failed");

    let read_fd = pipe_fds[0];
    let write_fd = pipe_fds[1];

    // Write a line with newline into write_fd
    let msg = b"Modus is functional and fast!\n";
    unsafe {
        write(write_fd, msg.as_ptr(), msg.len());
        close(write_fd); // Send EOF after newline
    }

    let user_src = format!(
        r#"
        import {{ readLineFrom, print, println, IOError }} from "std:io";

        function main(): IO(i32) {{
            let res: Result(String, IOError) = perform readLineFrom({read_fd});
            match (res) {{
                Result.Ok(line) => {{
                    perform print("Read line: ");
                    perform println(line);
                    IO.pure(1)
                }},
                Result.Err(e) => {{
                    perform print("Error message: ");
                    perform println(e.message);
                    IO.pure(0)
                }},
            }}
        }}
    "#
    );

    let graph = ModuleGraph::build_from_source(Path::new("main.mds"), &user_src)
        .expect("Failed to build graph");
    let res = jit_run_graph(&graph).expect("JIT execution failed");

    unsafe {
        close(read_fd);
    }

    assert_eq!(res, modus::backend::ExecutionResult::I32(1));
}

#[test]
fn test_stdlib_io_aot_compile_and_execute() {
    let temp_dir = std::env::temp_dir().join(format!("modus_aot_io_test_{}", std::process::id()));
    std::fs::create_dir_all(&temp_dir).unwrap();

    let main_path = temp_dir.join("main.mds");
    let bin_path = temp_dir.join("io_app");

    let code = r#"
        import { print, println, stdout_fileno } from "std:io";

        function main(): IO(i32) {
            let fd: i32 = stdout_fileno();
            perform println("AOT test with std:io success!");
            return IO.pure(0);
        }
    "#;
    std::fs::write(&main_path, code).unwrap();

    let output_bin = build_executable(&main_path, &bin_path, None).expect("AOT build failed");
    assert!(output_bin.exists());

    let output = std::process::Command::new(&output_bin)
        .output()
        .expect("Failed to run AOT binary");

    eprintln!(
        "AOT status: {:?}, stdout: {}, stderr: {}",
        output.status,
        String::from_utf8_lossy(&output.stdout),
        String::from_utf8_lossy(&output.stderr)
    );
    assert!(output.status.success());
    let stdout = String::from_utf8_lossy(&output.stdout);
    assert!(stdout.contains("AOT test with std:io success!"));

    let _ = std::fs::remove_dir_all(&temp_dir);
}
