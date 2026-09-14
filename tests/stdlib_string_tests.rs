use modus::backend::ExecutionResult;
use modus::modules::{
    ModuleGraph, ModuleId, build_executable, jit_run_graph, resolve_module_path,
    stdlib::{STD_STRING_SOURCE, is_std_module},
};
use modus::parser::parse_program;
use modus::typechecker::check_program;
use std::path::{Path, PathBuf};

#[test]
fn test_stdlib_string_module_resolution() {
    assert!(is_std_module("std:string"));
    let resolved = resolve_module_path("std:string", None).expect("Failed to resolve std:string");
    assert_eq!(resolved, PathBuf::from("std:string"));
}

#[test]
fn test_stdlib_string_source_parses_and_typechecks() {
    assert!(is_std_module("std:string"));
    let prog = parse_program(STD_STRING_SOURCE).expect("Failed to parse std:string source");
    let env = check_program(&prog).expect("Failed to typecheck std:string source");

    // Verify key exported functions exist
    assert!(env.lookup_function("length").is_some());
    assert!(env.lookup_function("charAt").is_some());
    assert!(env.lookup_function("charCodeAt").is_some());
    assert!(env.lookup_function("codePointAt").is_some());
    assert!(env.lookup_function("isEmpty").is_some());
    assert!(env.lookup_function("substring").is_some());
    assert!(env.lookup_function("slice").is_some());
    assert!(env.lookup_function("substr").is_some());
    assert!(env.lookup_function("indexOf").is_some());
    assert!(env.lookup_function("indexOfFrom").is_some());
    assert!(env.lookup_function("lastIndexOf").is_some());
    assert!(env.lookup_function("lastIndexOfFrom").is_some());
    assert!(env.lookup_function("startsWith").is_some());
    assert!(env.lookup_function("startsWithFrom").is_some());
    assert!(env.lookup_function("endsWith").is_some());
    assert!(env.lookup_function("includes").is_some());
    assert!(env.lookup_function("contains").is_some());
    assert!(env.lookup_function("replace").is_some());
    assert!(env.lookup_function("replaceAll").is_some());
    assert!(env.lookup_function("repeat").is_some());
    assert!(env.lookup_function("padStart").is_some());
    assert!(env.lookup_function("padEnd").is_some());
    assert!(env.lookup_function("trim").is_some());
    assert!(env.lookup_function("trimStart").is_some());
    assert!(env.lookup_function("trimEnd").is_some());
    assert!(env.lookup_function("toLowerCase").is_some());
    assert!(env.lookup_function("toUpperCase").is_some());
    assert!(env.lookup_function("split").is_some());
    assert!(env.lookup_function("join").is_some());
    assert!(env.lookup_function("concat").is_some());
    assert!(env.lookup_function("parseInt").is_some());
    assert!(env.lookup_function("parseIntRadix").is_some());
    assert!(env.lookup_function("parseFloat").is_some());
    assert!(env.lookup_function("fromCharCode").is_some());
}

#[test]
fn test_stdlib_string_graph_construction() {
    let user_src = r#"
        import { length, trim } from "std:string";

        function main(): IO(i32) {
            let s: String = trim("  hello  ");
            let l: i64 = length(s);
            return IO.pure(0);
        }
    "#;

    let graph = ModuleGraph::build_from_source(Path::new("main.mds"), user_src)
        .expect("Failed to build module graph with std:string");
    assert_eq!(graph.modules.len(), 2);
    assert!(
        graph
            .modules
            .contains_key(&ModuleId::new(PathBuf::from("std:string")))
    );
}

#[test]
fn test_stdlib_string_jit_access_and_substrings() {
    let user_src = r#"
        import {
            length,
            charAt,
            charCodeAt,
            codePointAt,
            isEmpty,
            substring,
            slice,
            substr
        } from "std:string";

        function main(): i32 {
            let s: String = "Hello, World!";

            if (length(s) != 13) { return 1; }
            if (isEmpty(s)) { return 2; }
            if (!isEmpty("")) { return 3; }

            if (charCodeAt(s, 0) != 72) { return 4; } // 'H'
            if (codePointAt(s, 1) != 101) { return 5; } // 'e'
            if (charAt(s, 7) != "W") { return 6; }
            if (charAt(s, 20) != "") { return 7; }

            // Substrings & slices
            let sub1: String = substring(s, 0, 5);
            if (sub1 != "Hello") { return 8; }

            let sub2: String = substring(s, 7, 12);
            if (sub2 != "World") { return 9; }

            let sl1: String = slice(s, 7, 12);
            if (sl1 != "World") { return 10; }

            let sl2: String = slice(s, -6, -1);
            if (sl2 != "World") { return 11; }

            let sb: String = substr(s, 7, 5);
            if (sb != "World") { return 12; }

            return 0;
        }
    "#;

    let graph = ModuleGraph::build_from_source(Path::new("main.mds"), user_src)
        .expect("Failed to build module graph");
    let res = jit_run_graph(&graph).expect("JIT execution failed");
    match res {
        ExecutionResult::I32(val) => {
            assert_eq!(val, 0, "Access and substrings returned error code {val}")
        }
        _ => panic!("Expected I32 return"),
    }
}

#[test]
fn test_stdlib_string_jit_search_and_matching() {
    let user_src = r#"
        import {
            indexOf,
            indexOfFrom,
            lastIndexOf,
            lastIndexOfFrom,
            startsWith,
            startsWithFrom,
            endsWith,
            includes,
            contains
        } from "std:string";

        function main(): i32 {
            let s: String = "banana";

            if (indexOf(s, "an") != 1) { return 1; }
            if (indexOfFrom(s, "an", 2) != 3) { return 2; }
            if (indexOf(s, "xyz") != -1) { return 3; }
            if (indexOf(s, "") != 0) { return 4; }

            if (lastIndexOf(s, "an") != 3) { return 5; }
            if (lastIndexOfFrom(s, "an", 2) != 1) { return 6; }
            if (lastIndexOf(s, "xyz") != -1) { return 7; }

            if (!startsWith(s, "ban")) { return 8; }
            if (startsWith(s, "nan")) { return 9; }
            if (!startsWithFrom(s, "nan", 2)) { return 10; }

            if (!endsWith(s, "ana")) { return 11; }
            if (endsWith(s, "ban")) { return 12; }

            if (!includes(s, "nan")) { return 13; }
            if (includes(s, "xyz")) { return 14; }
            if (!contains(s, "banana")) { return 15; }

            return 0;
        }
    "#;

    let graph = ModuleGraph::build_from_source(Path::new("main.mds"), user_src)
        .expect("Failed to build module graph");
    let res = jit_run_graph(&graph).expect("JIT execution failed");
    match res {
        ExecutionResult::I32(val) => {
            assert_eq!(val, 0, "Search and matching returned error code {val}")
        }
        _ => panic!("Expected I32 return"),
    }
}

#[test]
fn test_stdlib_string_jit_transformations() {
    let user_src = r#"
        import {
            replace,
            replaceAll,
            repeat,
            padStart,
            padEnd,
            trim,
            trimStart,
            trimEnd,
            toLowerCase,
            toUpperCase,
            split,
            join,
            concat
        } from "std:string";

        function main(): i32 {
            // Replace
            let s: String = "foo bar foo";
            if (replace(s, "foo", "baz") != "baz bar foo") { return 1; }
            if (replaceAll(s, "foo", "baz") != "baz bar baz") { return 2; }

            // Repeat
            if (repeat("ab", 3) != "ababab") { return 3; }
            if (repeat("ab", 0) != "") { return 4; }

            // Padding
            if (padStart("5", 3, "0") != "005") { return 5; }
            if (padEnd("5", 3, "0") != "500") { return 6; }

            // Trimming
            let padded: String = "   hello   ";
            if (trim(padded) != "hello") { return 7; }
            // Case conversion
            if (toLowerCase("HeLLo WoRLd") != "hello world") { return 10; }
            if (toUpperCase("hello world") != "HELLO WORLD") { return 11; }

            // Split and Join
            let parts: [String] = split("a,b,c", ",");
            if (parts.length() != 3) { return 12; }
            if (parts[0] != "a") { return 13; }
            if (parts[1] != "b") { return 14; }
            if (parts[2] != "c") { return 15; }

            let joined: String = join(parts, "-");
            if (joined != "a-b-c") { return 16; }

            if (concat("Hello, ", "World!") != "Hello, World!") { return 17; }

            return 0;
        }
    "#;

    let graph = ModuleGraph::build_from_source(Path::new("main.mds"), user_src)
        .expect("Failed to build module graph");
    let res = jit_run_graph(&graph).expect("JIT execution failed");
    match res {
        ExecutionResult::I32(val) => {
            assert_eq!(val, 0, "Transformations returned error code {val}")
        }
        _ => panic!("Expected I32 return"),
    }
}

#[test]
fn test_stdlib_string_jit_parsing_and_construction() {
    let user_src = r#"
        import {
            parseInt,
            parseIntRadix,
            parseFloat,
            fromCharCode
        } from "std:string";

        function main(): i32 {
            // parseInt
            let n1: i64 = match parseInt("  12345  ") {
                Ok(v) => v,
                Err(_) => 0,
            };
            if (n1 != 12345) { return 1; }

            let n2: i64 = match parseInt("-42") {
                Ok(v) => v,
                Err(_) => 0,
            };
            if (n2 != -42) { return 3; }

            let invalid_ok: bool = match parseInt("invalid") {
                Ok(_) => false,
                Err(_) => true,
            };
            if (!invalid_ok) { return 5; }

            // parseIntRadix
            let r1: i64 = match parseIntRadix("FF", 16) {
                Ok(v) => v,
                Err(_) => 0,
            };
            if (r1 != 255) { return 6; }

            let r2: i64 = match parseIntRadix("1010", 2) {
                Ok(v) => v,
                Err(_) => 0,
            };
            if (r2 != 10) { return 8; }

            // parseFloat
            let f1: f64 = match parseFloat("3.14159") {
                Ok(v) => v,
                Err(_) => 0.0,
            };
            if (f1 < 3.14 || f1 > 3.15) { return 10; }

            // fromCharCode
            if (fromCharCode(65) != "A") { return 12; }
            if (fromCharCode(90) != "Z") { return 13; }

            return 0;
        }
    "#;

    let graph = ModuleGraph::build_from_source(Path::new("main.mds"), user_src)
        .expect("Failed to build module graph");
    let res = jit_run_graph(&graph).expect("JIT execution failed");
    match res {
        ExecutionResult::I32(val) => {
            assert_eq!(val, 0, "Parsing and construction returned error code {val}")
        }
        _ => panic!("Expected I32 return"),
    }
}

#[test]
fn test_stdlib_string_aot_compile_and_execute() {
    let user_src = r#"
        import {
            trim,
            toUpperCase,
            repeat,
            split,
            join
        } from "std:string";

        function main(): IO(i32) {
            let raw = "  modus  ";
            let cleaned = trim(raw);
            let upper = toUpperCase(cleaned);
            let repeated = repeat(upper, 2);
            let parts = split(repeated, "US");
            let reconstructed = join(parts, "--");

            if (reconstructed == "MOD--MOD--") {
                return IO.pure(0);
            } else {
                return IO.pure(1);
            }
        }
    "#;

    let temp_dir = std::env::temp_dir().join("modus_test_string_aot");
    let _ = std::fs::create_dir_all(&temp_dir);
    let main_file = temp_dir.join("main.mds");
    let output_bin = temp_dir.join("string_app");
    std::fs::write(&main_file, user_src).expect("Failed to write main.mds");

    build_executable(&main_file, &output_bin, None).expect("AOT build failed");
    assert!(output_bin.exists());

    let status = std::process::Command::new(&output_bin)
        .status()
        .expect("Failed to run AOT binary");
    assert_eq!(status.code(), Some(0));

    let _ = std::fs::remove_dir_all(&temp_dir);
}

#[test]
fn test_native_string_methods_and_from_char_code() {
    let user_src = r#"
        function main(): i32 {
            let s = "Hello, Modus!";
            if (s.length() != 13) { return 1; }
            if (s.charCodeAt(0) != 72) { return 2; }
            if (s.charCodeAt(12) != 33) { return 3; }
            if (s.charCodeAt(100) != -1) { return 4; }

            let sub = s.substring(7, 12);
            if (sub != "Modus") { return 5; }
            if (sub.length() != 5) { return 6; }

            let ch = String.fromCharCode(65);
            if (ch != "A") { return 7; }
            if (ch.length() != 1) { return 8; }

            let empty = s.substring(5, 5);
            if (empty != "") { return 9; }
            if (empty.length() != 0) { return 10; }

            return 0;
        }
    "#;

    let graph = ModuleGraph::build_from_source(Path::new("main.mds"), user_src)
        .expect("Failed to build module graph");
    let res = jit_run_graph(&graph).expect("JIT execution failed");
    assert_eq!(res, ExecutionResult::I32(0));
}

#[test]
fn test_native_string_jit_execution_result() {
    let user_src = r#"
        function main(): String {
            let a = "Hello, ";
            let b = "World!";
            return a + b;
        }
    "#;

    let graph = ModuleGraph::build_from_source(Path::new("main.mds"), user_src)
        .expect("Failed to build module graph");
    let res = jit_run_graph(&graph).expect("JIT execution failed");
    assert_eq!(res, ExecutionResult::String("Hello, World!".to_string()));
}

#[test]
fn test_string_fbip_concatenation_loop() {
    let user_src = r#"
        function concat_loop(count: i32, acc: String): String {
            if (count <= 0) {
                return acc;
            } else {
                return concat_loop(count - 1, acc + "x");
            }
        }

        function main(): i32 {
            let res = concat_loop(20, "");
            if (res.length() != 20) { return 1; }
            if (res != "xxxxxxxxxxxxxxxxxxxx") { return 2; }
            return 0;
        }
    "#;

    let graph = ModuleGraph::build_from_source(Path::new("main.mds"), user_src)
        .expect("Failed to build module graph");
    let res = jit_run_graph(&graph).expect("JIT execution failed");
    assert_eq!(res, ExecutionResult::I32(0));
}
