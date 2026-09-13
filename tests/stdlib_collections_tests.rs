use modus::backend::ExecutionResult;
use modus::modules::{
    ModuleGraph, build_executable, jit_run_graph, resolve_module_path,
    stdlib::{STD_COLLECTIONS_SOURCE, is_std_module},
};
use modus::parser::parse_program;
use modus::typechecker::check_program;
use std::path::{Path, PathBuf};

#[test]
fn test_stdlib_collections_module_resolution() {
    assert!(is_std_module("std:collections"));
    let resolved =
        resolve_module_path("std:collections", None).expect("Failed to resolve std:collections");
    assert_eq!(resolved, PathBuf::from("std:collections"));
}

#[test]
fn test_stdlib_collections_source_parses_and_typechecks() {
    let prog =
        parse_program(STD_COLLECTIONS_SOURCE).expect("Failed to parse std:collections source");
    let env = check_program(&prog).expect("Failed to typecheck std:collections source");

    // Verify key exported functions and types exist
    assert!(env.lookup_function("map").is_some());
    assert!(env.lookup_function("filter").is_some());
    assert!(env.lookup_function("fold").is_some());
    assert!(env.lookup_function("reduce").is_some());
    assert!(env.lookup_function("find").is_some());
    assert!(env.lookup_function("findIndex").is_some());
    assert!(env.lookup_function("any").is_some());
    assert!(env.lookup_function("all").is_some());
    assert!(env.lookup_function("slice").is_some());
    assert!(env.lookup_function("concat").is_some());
    assert!(env.lookup_function("reverse").is_some());
    assert!(env.lookup_function("toList").is_some());
    assert!(env.lookup_function("toArray").is_some());
}

#[test]
fn test_array_builder_direct() {
    let user_src = r#"
        function main(): i32 {
            let b: ArrayBuilder(i64) = ArrayBuilder.new();
            if (!b.isEmpty()) { return 1; }
            if (b.length() != 0) { return 2; }
            if (b.capacity() < 4) { return 3; }

            let b1 = b.push(10);
            let b2 = b1.push(20);
            let b3 = b2.push(30);
            let b4 = b3.push(40);
            let b5 = b4.push(50); // triggers growth beyond 4

            if (b5.isEmpty()) { return 4; }
            if (b5.length() != 5) { return 5; }

            let arr: [i64] = b5.build();
            if (arr.length() != 5) { return 6; }
            if (arr[0] != 10) { return 7; }
            if (arr[1] != 20) { return 8; }
            if (arr[2] != 30) { return 9; }
            if (arr[3] != 40) { return 10; }
            if (arr[4] != 50) { return 11; }

            // Test withCapacity
            let bc: ArrayBuilder(String) = ArrayBuilder.withCapacity(16);
            if (bc.capacity() < 16) { return 12; }
            let s_arr = bc.push("hello").push("world").build();
            if (s_arr.length() != 2) { return 13; }
            if (s_arr[0] != "hello") { return 14; }
            if (s_arr[1] != "world") { return 15; }

            return 0;
        }
    "#;

    let graph = ModuleGraph::build_from_source(Path::new("main.mds"), user_src)
        .expect("Failed to build module graph");
    let res = jit_run_graph(&graph).expect("JIT execution failed");
    match res {
        ExecutionResult::I32(val) => {
            assert_eq!(val, 0, "ArrayBuilder test returned error code {val}");
        }
        _ => panic!("Expected I32 return"),
    }
}

#[test]
fn test_stdlib_collections_map_filter_fold() {
    let user_src = r#"
        import { map, filter, fold, reduce } from "std:collections";

        function main(): i32 {
            let nums: [i64] = [1, 2, 3, 4, 5];

            // 1. Map
            let doubled: [i64] = map(nums, (x: i64) => x * 2);
            if (doubled.length() != 5) { return 1; }
            if (doubled[0] != 2) { return 2; }
            if (doubled[4] != 10) { return 3; }
 
            // 2. Filter
            let evens: [i64] = filter(nums, (x: i64) => x % 2 == 0);
            if (evens.length() != 2) { return 4; }
            if (evens[0] != 2) { return 5; }
            if (evens[1] != 4) { return 6; }

            // 3. Fold
            let sum: i64 = fold(nums, 0, (acc: i64, x: i64) => acc + x);
            if (sum != 15) { return 7; }

            // 4. Reduce
            let red = reduce(nums, (a: i64, b: i64) => a * b);
            match (red) {
                Option.Some(prod) => {
                    if (prod != 120) { return 8; }
                },
                Option.None => { return 9; },
            }

            return 0;
        }
    "#;

    let graph = ModuleGraph::build_from_source(Path::new("main.mds"), user_src)
        .expect("Failed to build module graph");
    let res = jit_run_graph(&graph).expect("JIT execution failed");
    match res {
        ExecutionResult::I32(val) => {
            assert_eq!(val, 0, "Map/Filter/Fold test returned error code {val}");
        }
        _ => panic!("Expected I32 return"),
    }
}

#[test]
fn test_stdlib_collections_search_and_query() {
    let user_src = r#"
        import { find, findIndex, any, all } from "std:collections";

        function main(): i32 {
            let nums: [i64] = [10, 20, 35, 40, 50];

            // 1. Find
            let found = find(nums, (x: i64) => x > 30 && x < 40);
            let check1: i32 = match (found) {
                Option.Some(v) => {
                    if (v == 35) { 0 } else { 1 }
                },
                Option.None => 2,
            };
            if (check1 != 0) { return check1; }

            let not_found = find(nums, (x: i64) => x > 100);
            let check2: i32 = match (not_found) {
                Option.Some(v) => 3,
                Option.None => 0,
            };
            if (check2 != 0) { return check2; }

            // 2. FindIndex
            let idx1 = findIndex(nums, (x: i64) => x == 35);
            if (idx1 != 2) { return 4; }
            let idx2 = findIndex(nums, (x: i64) => x == 999);
            if (idx2 != -1) { return 5; }

            // 3. Any
            if (!any(nums, (x: i64) => x == 40)) { return 6; }
            if (any(nums, (x: i64) => x < 0)) { return 7; }

            // 4. All
            if (!all(nums, (x: i64) => x > 0)) { return 8; }
            if (all(nums, (x: i64) => x % 2 == 0)) { return 9; }

            return 0;
        }
    "#;

    let graph = ModuleGraph::build_from_source(Path::new("main.mds"), user_src)
        .expect("Failed to build module graph");
    let res = jit_run_graph(&graph).expect("JIT execution failed");
    match res {
        ExecutionResult::I32(val) => {
            assert_eq!(val, 0, "Search/Query test returned error code {val}");
        }
        _ => panic!("Expected I32 return"),
    }
}

#[test]
fn test_stdlib_collections_slice_concat_reverse() {
    let user_src = r#"
        import { slice, concat, reverse } from "std:collections";

        function main(): i32 {
            let arr1: [i64] = [1, 2, 3];
            let arr2: [i64] = [4, 5, 6];

            // 1. Concat
            let combined = concat(arr1, arr2);
            if (combined.length() != 6) { return 1; }
            if (combined[0] != 1) { return 2; }
            if (combined[3] != 4) { return 3; }
            if (combined[5] != 6) { return 4; }

            // 2. Slice
            let sub = slice(combined, 1, 3);
            if (sub.length() != 3) { return 5; }
            if (sub[0] != 2) { return 6; }
            if (sub[1] != 3) { return 7; }
            if (sub[2] != 4) { return 8; }

            // 3. Reverse
            let rev = reverse(arr1);
            if (rev.length() != 3) { return 9; }
            if (rev[0] != 3) { return 10; }
            if (rev[1] != 2) { return 11; }
            if (rev[2] != 1) { return 12; }

            return 0;
        }
    "#;

    let graph = ModuleGraph::build_from_source(Path::new("main.mds"), user_src)
        .expect("Failed to build module graph");
    let res = jit_run_graph(&graph).expect("JIT execution failed");
    match res {
        ExecutionResult::I32(val) => {
            assert_eq!(
                val, 0,
                "Slice/Concat/Reverse test returned error code {val}"
            );
        }
        _ => panic!("Expected I32 return"),
    }
}

#[test]
fn test_stdlib_collections_list() {
    let user_src = r#"
        import { toList, toArray, List } from "std:collections";

        function main(): i32 {
            let arr: [i64] = [100, 200, 300];
            let list: List(i64) = toList(arr);

            match (list) {
                List.Cons(n1) => {
                    if (n1.head != 100) { return 1; }
                    match (n1.tail) {
                        List.Cons(n2) => {
                            if (n2.head != 200) { return 2; }
                        },
                        List.Nil => { return 3; },
                    }
                },
                List.Nil => { return 4; },
            }

            let roundtrip: [i64] = toArray(list);
            if (roundtrip.length() != 3) { return 5; }
            if (roundtrip[0] != 100) { return 6; }
            if (roundtrip[1] != 200) { return 7; }
            if (roundtrip[2] != 300) { return 8; }

            return 0;
        }
    "#;

    let graph = ModuleGraph::build_from_source(Path::new("main.mds"), user_src)
        .expect("Failed to build module graph");
    let res = jit_run_graph(&graph).expect("JIT execution failed");
    match res {
        ExecutionResult::I32(val) => {
            assert_eq!(val, 0, "List test returned error code {val}");
        }
        _ => panic!("Expected I32 return"),
    }
}

#[test]
fn test_stdlib_collections_aot_compile_and_execute() {
    let user_src = r#"
        import { map, filter, fold } from "std:collections";

        function main(): IO(i32) {
            let input: [i64] = [1, 2, 3, 4, 5, 6, 7, 8, 9, 10];
            let evens = filter(input, (n: i64) => n % 2 == 0);
            let squares = map(evens, (n: i64) => n * n);
            let total = fold(squares, 0 as i64, (acc: i64, n: i64) => acc + n);
            // 4 + 16 + 36 + 64 + 100 = 220
            if (total == 220) {
                return IO.pure(0);
            } else {
                return IO.pure(1);
            }
        }
    "#;

    let temp_dir = std::env::temp_dir().join(format!("modus_col_aot_{}", std::process::id()));
    let _ = std::fs::create_dir_all(&temp_dir);
    let main_file = temp_dir.join("main.mds");
    let output_bin = temp_dir.join("col_app");
    std::fs::write(&main_file, user_src).expect("Failed to write main.mds");

    build_executable(&main_file, &output_bin, None).expect("AOT build failed");
    assert!(output_bin.exists());

    let status = std::process::Command::new(&output_bin)
        .status()
        .expect("Failed to run AOT binary");
    assert_eq!(status.code(), Some(0));

    let _ = std::fs::remove_dir_all(&temp_dir);
}
