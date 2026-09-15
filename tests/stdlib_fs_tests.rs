use modus::backend::ExecutionResult;
use modus::modules::{
    ModuleGraph, ModuleId, ResolveError, build_executable, jit_run_graph, resolve_module_path,
    stdlib::{STD_FS_SOURCE, is_std_module},
};
use std::path::{Path, PathBuf};

#[test]
fn test_stdlib_fs_source_parses_and_typechecks() {
    assert!(is_std_module("std:fs"));
    // Checked through the module graph: std:fs uses `==` on `String`, whose
    // `Eq` impl comes from the prelude (unavailable to bare `check_program`).
    let graph = ModuleGraph::build_from_source(Path::new("std:fs"), STD_FS_SOURCE)
        .expect("Failed to build std:fs graph");
    let (_, envs) = modus::modules::check_module_graph_with_envs(&graph)
        .expect("Failed to typecheck std:fs source");
    let env = envs
        .get(&ModuleId::new(PathBuf::from("std:fs")))
        .expect("std:fs env");

    // Verify key types exist
    assert!(env.types.contains_key("IOError"));
    assert!(env.types.contains_key("File"));
    assert!(env.types.contains_key("OpenOptions"));
    assert!(env.types.contains_key("FileMetadata"));

    // Verify key exported functions exist
    assert!(env.lookup_function("openFile").is_some());
    assert!(env.lookup_function("closeFile").is_some());
    assert!(env.lookup_function("readFile").is_some());
    assert!(env.lookup_function("writeFile").is_some());
    assert!(env.lookup_function("appendFile").is_some());
    assert!(env.lookup_function("removeFile").is_some());
    assert!(env.lookup_function("copyFile").is_some());
    assert!(env.lookup_function("rename").is_some());
    assert!(env.lookup_function("createDir").is_some());
    assert!(env.lookup_function("removeDir").is_some());
    assert!(env.lookup_function("readDir").is_some());
    assert!(env.lookup_function("exists").is_some());
    assert!(env.lookup_function("metadata").is_some());
    assert!(env.lookup_function("defaultOpenOptions").is_some());
    assert!(env.lookup_function("readOptions").is_some());
    assert!(env.lookup_function("writeOptions").is_some());
}

#[test]
fn test_stdlib_fs_module_resolution() {
    let resolved = resolve_module_path("std:fs", None).expect("std:fs should resolve");
    assert_eq!(resolved, PathBuf::from("std:fs"));

    let err = resolve_module_path("std:nonexistent", None).unwrap_err();
    match err {
        ResolveError::UnknownStdModule { module } => {
            assert_eq!(module, "std:nonexistent");
        }
        _ => panic!("Expected UnknownStdModule error, got: {:?}", err),
    }
}

#[test]
fn test_stdlib_fs_graph_construction() {
    let user_src = r#"
        import { exists } from "std:fs";

        function main(): IO(bool) {
            let ex: bool = perform exists("nonexistent_file_abc123.tmp");
            return IO.pure(ex);
        }
    "#;

    let graph = ModuleGraph::build_from_source(Path::new("main.mds"), user_src)
        .expect("Failed to build graph with std:fs");

    // main + std:fs + implicit std:prelude + std:string
    assert_eq!(graph.modules.len(), 4);
    let std_id = ModuleId::new(PathBuf::from("std:fs"));
    let main_id = ModuleId::new(PathBuf::from("main.mds"));
    let prelude_id = ModuleId::new(PathBuf::from("std:prelude"));
    let string_id = ModuleId::new(PathBuf::from("std:string"));

    assert!(graph.modules.contains_key(&std_id));
    assert!(graph.modules.contains_key(&main_id));
    assert!(graph.modules.contains_key(&prelude_id));
    assert!(graph.modules.contains_key(&string_id));

    // Dependencies must precede main in topological order
    let pos = |id: &ModuleId| graph.topo_order.iter().position(|m| m == id).unwrap();
    assert!(pos(&std_id) < pos(&main_id));
    assert!(pos(&string_id) < pos(&prelude_id));
    assert!(pos(&prelude_id) < pos(&main_id));

    let res = jit_run_graph(&graph).expect("JIT execution failed");
    assert_eq!(res, ExecutionResult::Bool(false));
}

#[test]
fn test_stdlib_fs_jit_write_and_read_file() {
    let temp_dir = std::env::temp_dir().join(format!("modus_test_fs_rw_{}", std::process::id()));
    std::fs::create_dir_all(&temp_dir).unwrap();
    let test_file = temp_dir.join("hello.txt");
    let test_file_str = test_file.to_str().unwrap();

    let user_src = format!(
        r#"
        import {{ writeFile, readFile, IOError }} from "std:fs";

        function main(): IO(i32) {{
            let write_res: Result(void, IOError) = perform writeFile("{test_file_str}", "Hello Modus Filesystem!");
            match (write_res) {{
                Result.Ok(_) => {{
                    let read_res: Result(String, IOError) = perform readFile("{test_file_str}");
                    match (read_res) {{
                        Result.Ok(content) => {{
                            if (content == "Hello Modus Filesystem!") {{
                                IO.pure(1)
                            }} else {{
                                IO.pure(0)
                            }}
                        }},
                        Result.Err(e) => IO.pure(-1),
                    }}
                }},
                Result.Err(e) => IO.pure(-2),
            }}
        }}
    "#
    );

    let graph = ModuleGraph::build_from_source(Path::new("main.mds"), &user_src)
        .expect("Failed to build graph");
    let res = jit_run_graph(&graph).expect("JIT execution failed");
    assert_eq!(res, ExecutionResult::I32(1));

    let _ = std::fs::remove_dir_all(&temp_dir);
}

#[test]
fn test_stdlib_fs_jit_append_file() {
    let temp_dir =
        std::env::temp_dir().join(format!("modus_test_fs_append_{}", std::process::id()));
    std::fs::create_dir_all(&temp_dir).unwrap();
    let test_file = temp_dir.join("append.txt");
    let test_file_str = test_file.to_str().unwrap();

    let user_src = format!(
        r#"
        import {{ writeFile, appendFile, readFile, IOError }} from "std:fs";

        function main(): IO(i32) {{
            perform writeFile("{test_file_str}", "Part1;");
            perform appendFile("{test_file_str}", "Part2;");
            let read_res: Result(String, IOError) = perform readFile("{test_file_str}");
            match (read_res) {{
                Result.Ok(content) => {{
                    if (content == "Part1;Part2;") {{
                        IO.pure(42)
                    }} else {{
                        IO.pure(0)
                    }}
                }},
                Result.Err(e) => IO.pure(-1),
            }}
        }}
    "#
    );

    let graph = ModuleGraph::build_from_source(Path::new("main.mds"), &user_src)
        .expect("Failed to build graph");
    let res = jit_run_graph(&graph).expect("JIT execution failed");
    assert_eq!(res, ExecutionResult::I32(42));

    let _ = std::fs::remove_dir_all(&temp_dir);
}

#[test]
fn test_stdlib_fs_jit_open_and_close_file() {
    let temp_dir = std::env::temp_dir().join(format!("modus_test_fs_open_{}", std::process::id()));
    std::fs::create_dir_all(&temp_dir).unwrap();
    let test_file = temp_dir.join("open_test.txt");
    let test_file_str = test_file.to_str().unwrap();

    let user_src = format!(
        r#"
        import {{ openFile, closeFile, OpenOptions, File, IOError }} from "std:fs";

        function main(): IO(i32) {{
            let opts: OpenOptions = {{
                read: true,
                write: true,
                create: true,
                append: false,
                truncate: false,
            }};
            let open_res: Result(File, IOError) = perform openFile("{test_file_str}", opts);
            match (open_res) {{
                Result.Ok(f) => {{
                    if (f.fd >= 0) {{
                        let close_res: Result(void, IOError) = perform closeFile(f);
                        match (close_res) {{
                            Result.Ok(_) => IO.pure(100),
                            Result.Err(e) => IO.pure(-1),
                        }}
                    }} else {{
                        IO.pure(-2)
                    }}
                }},
                Result.Err(e) => IO.pure(-3),
            }}
        }}
    "#
    );

    let graph = ModuleGraph::build_from_source(Path::new("main.mds"), &user_src)
        .expect("Failed to build graph");
    let res = jit_run_graph(&graph).expect("JIT execution failed");
    assert_eq!(res, ExecutionResult::I32(100));

    let _ = std::fs::remove_dir_all(&temp_dir);
}

#[test]
fn test_stdlib_fs_jit_remove_file() {
    let temp_dir = std::env::temp_dir().join(format!("modus_test_fs_rm_{}", std::process::id()));
    std::fs::create_dir_all(&temp_dir).unwrap();
    let test_file = temp_dir.join("to_delete.txt");
    let test_file_str = test_file.to_str().unwrap();

    let user_src = format!(
        r#"
        import {{ writeFile, removeFile, exists, IOError }} from "std:fs";

        function main(): IO(i32) {{
            perform writeFile("{test_file_str}", "temporary data");
            let exists_before: bool = perform exists("{test_file_str}");
            let rm_res: Result(void, IOError) = perform removeFile("{test_file_str}");
            let exists_after: bool = perform exists("{test_file_str}");

            if (exists_before && !exists_after) {{
                IO.pure(1)
            }} else {{
                IO.pure(0)
            }}
        }}
    "#
    );

    let graph = ModuleGraph::build_from_source(Path::new("main.mds"), &user_src)
        .expect("Failed to build graph");
    let res = jit_run_graph(&graph).expect("JIT execution failed");
    assert_eq!(res, ExecutionResult::I32(1));

    let _ = std::fs::remove_dir_all(&temp_dir);
}

#[test]
fn test_stdlib_fs_jit_copy_file() {
    let temp_dir = std::env::temp_dir().join(format!("modus_test_fs_copy_{}", std::process::id()));
    std::fs::create_dir_all(&temp_dir).unwrap();
    let src_file = temp_dir.join("src.txt");
    let dest_file = temp_dir.join("dest.txt");
    let src_str = src_file.to_str().unwrap();
    let dest_str = dest_file.to_str().unwrap();

    let user_src = format!(
        r#"
        import {{ writeFile, readFile, copyFile, IOError }} from "std:fs";

        function main(): IO(i32) {{
            perform writeFile("{src_str}", "Copy me over completely!");
            let copy_res: Result(u64, IOError) = perform copyFile("{src_str}", "{dest_str}");
            match (copy_res) {{
                Result.Ok(bytes_copied) => {{
                    let read_res: Result(String, IOError) = perform readFile("{dest_str}");
                    match (read_res) {{
                        Result.Ok(content) => {{
                            if (content == "Copy me over completely!" && bytes_copied == 24) {{
                                IO.pure(24)
                            }} else {{
                                IO.pure(0)
                            }}
                        }},
                        Result.Err(e) => IO.pure(-1),
                    }}
                }},
                Result.Err(e) => IO.pure(-2),
            }}
        }}
    "#
    );

    let graph = ModuleGraph::build_from_source(Path::new("main.mds"), &user_src)
        .expect("Failed to build graph");
    let res = jit_run_graph(&graph).expect("JIT execution failed");
    assert_eq!(res, ExecutionResult::I32(24));

    let _ = std::fs::remove_dir_all(&temp_dir);
}

#[test]
fn test_stdlib_fs_jit_rename() {
    let temp_dir =
        std::env::temp_dir().join(format!("modus_test_fs_rename_{}", std::process::id()));
    std::fs::create_dir_all(&temp_dir).unwrap();
    let old_file = temp_dir.join("old.txt");
    let new_file = temp_dir.join("new.txt");
    let old_str = old_file.to_str().unwrap();
    let new_str = new_file.to_str().unwrap();

    let user_src = format!(
        r#"
        import {{ writeFile, readFile, rename, exists, IOError }} from "std:fs";

        function main(): IO(i32) {{
            perform writeFile("{old_str}", "renamed data");
            let ren_res: Result(void, IOError) = perform rename("{old_str}", "{new_str}");
            match (ren_res) {{
                Result.Ok(_) => {{
                    let old_ex: bool = perform exists("{old_str}");
                    let new_ex: bool = perform exists("{new_str}");
                    if (!old_ex && new_ex) {{
                        let content_res: Result(String, IOError) = perform readFile("{new_str}");
                        match (content_res) {{
                            Result.Ok(content) => {{
                                if (content == "renamed data") {{
                                    IO.pure(777)
                                }} else {{
                                    IO.pure(0)
                                }}
                            }},
                            Result.Err(e) => IO.pure(-1),
                        }}
                    }} else {{
                        IO.pure(-2)
                    }}
                }},
                Result.Err(e) => IO.pure(-3),
            }}
        }}
    "#
    );

    let graph = ModuleGraph::build_from_source(Path::new("main.mds"), &user_src)
        .expect("Failed to build graph");
    let res = jit_run_graph(&graph).expect("JIT execution failed");
    assert_eq!(res, ExecutionResult::I32(777));

    let _ = std::fs::remove_dir_all(&temp_dir);
}

#[test]
fn test_stdlib_fs_jit_rename_file() {
    let temp_dir =
        std::env::temp_dir().join(format!("modus_test_fs_rename_file_{}", std::process::id()));
    std::fs::create_dir_all(&temp_dir).unwrap();
    let old_file = temp_dir.join("old_f.txt");
    let new_file = temp_dir.join("new_f.txt");
    let old_str = old_file.to_str().unwrap();
    let new_str = new_file.to_str().unwrap();

    let user_src = format!(
        r#"
        import {{ writeFile, readFile, renameFile, exists, IOError }} from "std:fs";

        function main(): IO(i32) {{
            perform writeFile("{old_str}", "renamed with renameFile");
            let ren_res: Result(void, IOError) = perform renameFile("{old_str}", "{new_str}");
            match (ren_res) {{
                Result.Ok(_) => {{
                    let old_ex: bool = perform exists("{old_str}");
                    let new_ex: bool = perform exists("{new_str}");
                    if (!old_ex && new_ex) {{
                        let content_res: Result(String, IOError) = perform readFile("{new_str}");
                        match (content_res) {{
                            Result.Ok(content) => {{
                                if (content == "renamed with renameFile") {{
                                    IO.pure(888)
                                }} else {{
                                    IO.pure(0)
                                }}
                            }},
                            Result.Err(e) => IO.pure(-1),
                        }}
                    }} else {{
                        IO.pure(-2)
                    }}
                }},
                Result.Err(e) => IO.pure(-3),
            }}
        }}
    "#
    );

    let graph = ModuleGraph::build_from_source(Path::new("main.mds"), &user_src)
        .expect("Failed to build graph");
    let res = jit_run_graph(&graph).expect("JIT execution failed");
    assert_eq!(res, ExecutionResult::I32(888));

    let _ = std::fs::remove_dir_all(&temp_dir);
}

#[test]
fn test_stdlib_fs_jit_create_and_remove_dir() {
    let temp_dir = std::env::temp_dir().join(format!("modus_test_fs_dir_{}", std::process::id()));
    std::fs::create_dir_all(&temp_dir).unwrap();
    let sub_dir = temp_dir.join("subdir");
    let sub_dir_str = sub_dir.to_str().unwrap();

    let user_src = format!(
        r#"
        import {{ createDir, removeDir, exists, metadata, FileMetadata, IOError }} from "std:fs";

        function main(): IO(i32) {{
            let create_res: Result(void, IOError) = perform createDir("{sub_dir_str}");
            match (create_res) {{
                Result.Ok(_) => {{
                    let ex: bool = perform exists("{sub_dir_str}");
                    let meta_res: Result(FileMetadata, IOError) = perform metadata("{sub_dir_str}");
                    let is_dir: bool = match (meta_res) {{
                        Result.Ok(m) => m.is_dir,
                        Result.Err(e) => false,
                    }};

                    let rm_res: Result(void, IOError) = perform removeDir("{sub_dir_str}");
                    let ex_after: bool = perform exists("{sub_dir_str}");

                    if (ex && is_dir && !ex_after) {{
                        IO.pure(55)
                    }} else {{
                        IO.pure(0)
                    }}
                }},
                Result.Err(e) => IO.pure(-1),
            }}
        }}
    "#
    );

    let graph = ModuleGraph::build_from_source(Path::new("main.mds"), &user_src)
        .expect("Failed to build graph");
    let res = jit_run_graph(&graph).expect("JIT execution failed");
    assert_eq!(res, ExecutionResult::I32(55));

    let _ = std::fs::remove_dir_all(&temp_dir);
}

#[test]
fn test_stdlib_fs_jit_metadata() {
    let temp_dir = std::env::temp_dir().join(format!("modus_test_fs_meta_{}", std::process::id()));
    std::fs::create_dir_all(&temp_dir).unwrap();
    let file_path = temp_dir.join("meta.txt");
    let file_str = file_path.to_str().unwrap();

    let user_src = format!(
        r#"
        import {{ writeFile, metadata, FileMetadata, IOError }} from "std:fs";

        function main(): IO(i32) {{
            perform writeFile("{file_str}", "1234567890");
            let meta_res: Result(FileMetadata, IOError) = perform metadata("{file_str}");
            match (meta_res) {{
                Result.Ok(m) => {{
                    if (m.size == 10 && m.is_file && !m.is_dir && m.modified_at > 0) {{
                        IO.pure(1)
                    }} else {{
                        IO.pure(0)
                    }}
                }},
                Result.Err(e) => IO.pure(-1),
            }}
        }}
    "#
    );

    let graph = ModuleGraph::build_from_source(Path::new("main.mds"), &user_src)
        .expect("Failed to build graph");
    let res = jit_run_graph(&graph).expect("JIT execution failed");
    assert_eq!(res, ExecutionResult::I32(1));

    let _ = std::fs::remove_dir_all(&temp_dir);
}

#[test]
fn test_stdlib_fs_jit_read_dir() {
    let temp_dir =
        std::env::temp_dir().join(format!("modus_test_fs_readdir_{}", std::process::id()));
    std::fs::create_dir_all(&temp_dir).unwrap();
    let file1 = temp_dir.join("alpha.txt");
    let file2 = temp_dir.join("beta.txt");
    std::fs::write(&file1, "A").unwrap();
    std::fs::write(&file2, "B").unwrap();
    let dir_str = temp_dir.to_str().unwrap();

    let user_src = format!(
        r#"
        import {{ readDir, IOError }} from "std:fs";

        function main(): IO(i64) {{
            let res: Result([String], IOError) = perform readDir("{dir_str}");
            match (res) {{
                Result.Ok(entries) => {{
                    let count: i64 = entries.length();
                    IO.pure(count)
                }},
                Result.Err(e) => IO.pure(-1),
            }}
        }}
    "#
    );

    let graph = ModuleGraph::build_from_source(Path::new("main.mds"), &user_src)
        .expect("Failed to build graph");
    let res = jit_run_graph(&graph).expect("JIT execution failed");
    assert_eq!(res, ExecutionResult::I64(2));

    let _ = std::fs::remove_dir_all(&temp_dir);
}

#[test]
fn test_stdlib_fs_aot_compile_and_execute() {
    let temp_dir = std::env::temp_dir().join(format!("modus_aot_fs_test_{}", std::process::id()));
    std::fs::create_dir_all(&temp_dir).unwrap();

    let main_path = temp_dir.join("main.mds");
    let bin_path = temp_dir.join("fs_app");
    let target_file = temp_dir.join("aot_fs_output.txt");
    let target_file_str = target_file.to_str().unwrap();

    let code = format!(
        r#"
        import {{ writeFile, readFile, metadata, exists, FileMetadata, IOError }} from "std:fs";

        extern "C" {{
            function puts(s: CString): IO(i32);
        }}

        function main(): IO(i32) {{
            let msg: String = "Hello from Modus AOT Filesystem!";
            let write_res: Result(void, IOError) = perform writeFile("{target_file_str}", msg);
            match (write_res) {{
                Result.Ok(_) => {{
                    let ex: bool = perform exists("{target_file_str}");
                    if (!ex) {{
                        return IO.pure(1);
                    }} else {{
                        let read_res: Result(String, IOError) = perform readFile("{target_file_str}");
                        match (read_res) {{
                            Result.Ok(content) => {{
                                if (content == msg) {{
                                    perform puts(String.toCString("AOT std:fs SUCCESS"));
                                    return IO.pure(0);
                                }} else {{
                                    return IO.pure(2);
                                }}
                            }},
                            Result.Err(e) => IO.pure(3),
                        }}
                    }}
                }},
                Result.Err(e) => IO.pure(4),
            }}
        }}
    "#
    );
    std::fs::write(&main_path, code).unwrap();

    let output_bin = build_executable(&main_path, &bin_path, None).expect("AOT build failed");
    assert!(output_bin.exists());

    let output = std::process::Command::new(&output_bin)
        .output()
        .expect("Failed to run AOT binary");

    assert!(output.status.success());
    let stdout = String::from_utf8_lossy(&output.stdout);
    assert!(stdout.contains("AOT std:fs SUCCESS"));

    let _ = std::fs::remove_dir_all(&temp_dir);
}
