#![cfg(feature = "llvm")]

use inkwell::context::Context;
use modus::backend::CodeGen;
use modus::desugar::desugar_program;
use modus::ir::{apply_perceus_and_fbip, convert_closures, lower_program};
use modus::parser::parse_program;
use modus::typechecker::check_program;
use std::fs;
use std::path::Path;

fn read_fixture(subpath: &str) -> String {
    let full_path = Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("tests")
        .join("fixtures")
        .join(subpath);
    fs::read_to_string(&full_path).unwrap_or_else(|err| {
        panic!(
            "Failed to read fixture file {}: {}",
            full_path.display(),
            err
        )
    })
}

fn compile_to_llvm<'ctx>(
    context: &'ctx Context,
    source: &str,
    module_name: &str,
) -> Result<CodeGen<'ctx>, String> {
    let program = parse_program(source).map_err(|e| format!("{e:?}"))?;
    let env = check_program(&program).map_err(|e| format!("{e:?}"))?;
    let desugared = desugar_program(&program, &env);
    let mut anf = lower_program(&desugared);
    convert_closures(&mut anf);
    apply_perceus_and_fbip(&mut anf);
    let mut codegen = CodeGen::new(context, module_name);
    codegen.compile_program(&anf)?;
    Ok(codegen)
}

// ============================================================================
// 1. Primitive Arithmetic and Unboxed Types
// ============================================================================

#[test]
fn test_codegen_arithmetic_and_unboxed_primitives() {
    let source = r#"
        function arithmetic(a: i32, b: i32): i32 {
            let sum = a + b;
            let diff = a - b;
            let prod = sum * diff;
            let quot = prod / 2;
            return quot;
        }
    "#;
    let context = Context::create();
    let codegen = compile_to_llvm(&context, source, "test_arithmetic")
        .expect("Codegen failed for arithmetic");

    let ir = codegen.to_ir_string();
    assert!(ir.contains("define fastcc i32 @arithmetic(i32 %0, i32 %1)"));
    assert!(ir.contains("add i32"));
    assert!(ir.contains("sub i32"));
    assert!(ir.contains("mul i32"));
    assert!(ir.contains("sdiv i32"));
    assert!(codegen.module.verify().is_ok());
}

// ============================================================================
// 2. Control Flow and Comparisons
// ============================================================================

#[test]
fn test_codegen_control_flow_and_comparisons() {
    let source = r#"
        function clamp(val: i32, min_val: i32, max_val: i32): i32 {
            if (val < min_val) {
                return min_val;
            } else if (val > max_val) {
                return max_val;
            } else {
                return val;
            }
        }
    "#;
    let context = Context::create();
    let codegen =
        compile_to_llvm(&context, source, "test_clamp").expect("Codegen failed for clamp");

    let ir = codegen.to_ir_string();
    assert!(ir.contains("define fastcc i32 @clamp(i32 %0, i32 %1, i32 %2)"));
    assert!(ir.contains("icmp slt i32"));
    assert!(ir.contains("icmp sgt i32"));
    assert!(ir.contains("br i1"));
    assert!(codegen.module.verify().is_ok());
}

// ============================================================================
// 3. Tail Recursion with musttail / fastcc
// ============================================================================

#[test]
fn test_codegen_direct_recursion_tail_call() {
    let source = r#"
        function gcd(a: i32, b: i32): i32 {
            if (b == 0) {
                return a;
            } else {
                return gcd(b, a % b);
            }
        }
    "#;
    let context = Context::create();
    let codegen = compile_to_llvm(&context, source, "test_gcd").expect("Codegen failed for gcd");

    let ir = codegen.to_ir_string();
    assert!(ir.contains("define fastcc i32 @gcd(i32 %0, i32 %1)"));
    assert!(ir.contains("tail call fastcc i32 @gcd"));
    assert!(codegen.module.verify().is_ok());
}

// ============================================================================
// 4. Record Allocation and Perceus Reference Counting
// ============================================================================

#[test]
fn test_codegen_record_allocation_and_rc_header() {
    let source = r#"
        type Point = { x: i32, y: i32 };

        function make_point(x: i32, y: i32): Point {
            let pt: Point = { x: x, y: y };
            return pt;
        }
    "#;
    let context = Context::create();
    let codegen =
        compile_to_llvm(&context, source, "test_record").expect("Codegen failed for record");

    let ir = codegen.to_ir_string();
    assert!(ir.contains("call ptr @modus_alloc(i64 24)"));
    assert!(ir.contains("define ptr @modus_alloc(i64 %0)"));
    assert!(codegen.module.verify().is_ok());
}

// ============================================================================
// 5. FBIP (Functional But In-Place) Buffer Reuse
// ============================================================================

#[test]
fn test_codegen_fbip_record_update() {
    let source = r#"
        type Point = { x: i32, y: i32 };

        function move_x(pt: Point, dx: i32): Point {
            return { ...pt, x: pt.x + dx };
        }
    "#;
    let context = Context::create();
    let codegen = compile_to_llvm(&context, source, "test_fbip").expect("Codegen failed for fbip");

    let ir = codegen.to_ir_string();
    assert!(ir.contains("call i1 @modus_is_unique(ptr"));
    assert!(ir.contains("fbip_inplace:"));
    assert!(ir.contains("fbip_alloc:"));
    assert!(ir.contains("fbip_merge:"));
    assert!(ir.contains("phi ptr"));
    assert!(codegen.module.verify().is_ok());
}

// ============================================================================
// 6. Closure Invocation via Indirect Call
// ============================================================================

#[test]
fn test_codegen_closure_call() {
    let source = r#"
        function apply_fn(x: i32, f: (i32) => i32): i32 {
            return f(x);
        }
    "#;
    let context = Context::create();
    let codegen =
        compile_to_llvm(&context, source, "test_closure").expect("Codegen failed for closure call");

    let ir = codegen.to_ir_string();
    assert!(ir.contains("define fastcc i32 @apply_fn(i32 %0, ptr %1)"));
    assert!(ir.contains("call fastcc i32 %"));
    assert!(codegen.module.verify().is_ok());
}

// ============================================================================
// 7. Optimization Pipeline (LLVM -O3)
// ============================================================================

#[test]
fn test_codegen_optimization_o3() {
    let source = r#"
        function compute(x: i32): i32 {
            let a = x + 10;
            let b = a * 2;
            let c = b - 4;
            return c;
        }
    "#;
    let context = Context::create();
    let codegen = compile_to_llvm(&context, source, "test_opt").expect("Codegen failed for opt");

    assert!(codegen.module.verify().is_ok());
    codegen
        .optimize(None)
        .expect("LLVM -O3 optimization failed");
    assert!(codegen.module.verify().is_ok());
}

// ============================================================================
// 8. Compilation of All 12 Valid Fixtures to LLVM IR
// ============================================================================

#[test]
fn test_codegen_fixture_01_primitives() {
    let source = read_fixture("valid/01_primitives.mds");
    let context = Context::create();
    let codegen = compile_to_llvm(&context, &source, "fixture_01").expect("Codegen 01 failed");
    assert!(codegen.module.verify().is_ok());
}

#[test]
fn test_codegen_fixture_02_functions() {
    let source = read_fixture("valid/02_functions.mds");
    let context = Context::create();
    let codegen = compile_to_llvm(&context, &source, "fixture_02").expect("Codegen 02 failed");
    assert!(codegen.module.verify().is_ok());
}

#[test]
fn test_codegen_fixture_03_control_flow() {
    let source = read_fixture("valid/03_control_flow.mds");
    let context = Context::create();
    let codegen = compile_to_llvm(&context, &source, "fixture_03").expect("Codegen 03 failed");
    assert!(codegen.module.verify().is_ok());
}

#[test]
fn test_codegen_fixture_04_records() {
    let source = read_fixture("valid/04_records.mds");
    let context = Context::create();
    let codegen = compile_to_llvm(&context, &source, "fixture_04").expect("Codegen 04 failed");
    assert!(codegen.module.verify().is_ok());
}

#[test]
fn test_codegen_fixture_05_generics() {
    let source = read_fixture("valid/05_generics.mds");
    let context = Context::create();
    let codegen = compile_to_llvm(&context, &source, "fixture_05").expect("Codegen 05 failed");
    assert!(codegen.module.verify().is_ok());
}

#[test]
fn test_codegen_fixture_06_traits() {
    let source = read_fixture("valid/06_traits.mds");
    let context = Context::create();
    let codegen = compile_to_llvm(&context, &source, "fixture_06").expect("Codegen 06 failed");
    assert!(codegen.module.verify().is_ok());
}

#[test]
fn test_codegen_fixture_07_effects() {
    let source = read_fixture("valid/07_effects.mds");
    let context = Context::create();
    let codegen = compile_to_llvm(&context, &source, "fixture_07").expect("Codegen 07 failed");
    assert!(codegen.module.verify().is_ok());
}

#[test]
fn test_codegen_fixture_08_match() {
    let source = read_fixture("valid/08_match.mds");
    let context = Context::create();
    let codegen = compile_to_llvm(&context, &source, "fixture_08").expect("Codegen 08 failed");
    assert!(codegen.module.verify().is_ok());
}

#[test]
fn test_codegen_fixture_09_closures() {
    let source = read_fixture("valid/09_closures.mds");
    let context = Context::create();
    let codegen = compile_to_llvm(&context, &source, "fixture_09").expect("Codegen 09 failed");
    assert!(codegen.module.verify().is_ok());
}

#[test]
fn test_codegen_fixture_10_arrays() {
    let source = read_fixture("valid/10_arrays.mds");
    let context = Context::create();
    let codegen = compile_to_llvm(&context, &source, "fixture_10").expect("Codegen 10 failed");
    assert!(codegen.module.verify().is_ok());
}

#[test]
fn test_codegen_fixture_11_operators() {
    let source = read_fixture("valid/11_operators.mds");
    let context = Context::create();
    let codegen = compile_to_llvm(&context, &source, "fixture_11").expect("Codegen 11 failed");
    assert!(codegen.module.verify().is_ok());
}

#[test]
fn test_codegen_fixture_12_full_program() {
    let source = read_fixture("valid/12_full_program.mds");
    let context = Context::create();
    let codegen = compile_to_llvm(&context, &source, "fixture_12").expect("Codegen 12 failed");
    assert!(codegen.module.verify().is_ok());
}

#[test]
fn test_codegen_jit_execution() {
    let source = r#"
        function factorial(n: i64, acc: i64): i64 {
            if (n <= 1) {
                return acc;
            } else {
                return factorial(n - 1, n * acc);
            }
        }

        function main(): i64 {
            let res: i64 = factorial(5, 1);
            return res;
        }
    "#;
    inkwell::targets::Target::initialize_native(&inkwell::targets::InitializationConfig::default())
        .expect("Failed to init native target");
    let context = Context::create();
    let codegen = compile_to_llvm(&context, source, "test_jit").expect("Codegen failed");
    codegen.optimize(None).expect("Optimization failed");
    let execution_engine = codegen
        .module
        .create_jit_execution_engine(inkwell::OptimizationLevel::Aggressive)
        .expect("Failed to create JIT engine");

    unsafe {
        let main_fn = execution_engine
            .get_function::<unsafe extern "C" fn() -> i64>("main")
            .expect("Failed to find main function");
        let result = main_fn.call();
        assert_eq!(result, 120);
    }
}

#[test]
fn test_codegen_jit_records_and_distance() {
    let source = r#"
        type Point = {
            x: i32,
            y: i32,
        };

        function manhattan_distance(p: Point): i32 {
            return p.x + p.y;
        }

        function main(): i32 {
            let p: Point = { x: 30, y: 12 };
            return manhattan_distance(p);
        }
    "#;
    inkwell::targets::Target::initialize_native(&inkwell::targets::InitializationConfig::default())
        .expect("Failed to init native target");
    let context = Context::create();
    let codegen = compile_to_llvm(&context, source, "test_records").expect("Codegen failed");
    codegen.optimize(None).expect("Optimization failed");
    let execution_engine = codegen
        .module
        .create_jit_execution_engine(inkwell::OptimizationLevel::Aggressive)
        .expect("Failed to create JIT engine");

    unsafe {
        let main_fn = execution_engine
            .get_function::<unsafe extern "C" fn() -> i32>("main")
            .expect("Failed to find main function");
        let result = main_fn.call();
        assert_eq!(result, 42);
    }
}

#[test]
fn test_codegen_demo_sample_program() {
    let demo_path = Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("examples")
        .join("demo.mds");
    let source = fs::read_to_string(&demo_path).expect("Failed to read demo.mds");

    let context = Context::create();
    let codegen = modus::compile_source(&context, &source, "demo").expect("Failed to compile demo");
    codegen.optimize(None).expect("Failed to optimize demo");

    let result = codegen.jit_run().expect("Failed to JIT execute demo");
    assert_eq!(result, modus::backend::ExecutionResult::I32(300));
}

#[test]
fn test_codegen_compile_to_binary_and_execute() {
    let source = r#"
        function factorial(n: i32, acc: i32): i32 {
            if (n <= 1) {
                return acc;
            } else {
                return factorial(n - 1, n * acc);
            }
        }

        function main(): i32 {
            let res: i32 = factorial(4, 1);
            return res + 18;
        }
    "#;
    let context = Context::create();
    let codegen = modus::compile_source(&context, source, "test_bin").expect("Compile failed");
    codegen.optimize(None).expect("Optimize failed");

    let target_dir = Path::new(env!("CARGO_MANIFEST_DIR")).join("target");
    let out_bin = target_dir.join("test_modus_binary");
    codegen
        .compile_to_binary(&out_bin)
        .expect("Failed to link binary");

    let output = std::process::Command::new(&out_bin)
        .output()
        .expect("Failed to run binary");
    let _ = fs::remove_file(&out_bin);
    assert_eq!(output.status.code(), Some(42));
}

#[test]
fn test_codegen_ffi_malloc_free_and_pointer_memory() {
    let source = r#"
        extern "C" {
            function malloc(size: u64): IO(Pointer(void));
            function free(ptr: Pointer(void)): IO(void);
        }

        function main(): IO(i32) {
            let raw: Pointer(void) = perform malloc(4);
            let p: Pointer(i32) = raw.cast();
            perform p.write(1337);
            let val: i32 = perform p.read();
            perform free(p.cast());
            return IO.pure(val);
        }
    "#;
    let context = Context::create();
    let codegen = modus::compile_source(&context, source, "test_ffi_mem").expect("Compile failed");
    codegen.optimize(None).expect("Optimize failed");
    let res = codegen.jit_run().expect("JIT run failed");
    assert_eq!(res, modus::backend::ExecutionResult::I32(1337));
}

#[test]
fn test_codegen_ffi_pointer_offset_arithmetic() {
    let source = r#"
        extern "C" {
            function malloc(size: u64): IO(Pointer(void));
            function free(ptr: Pointer(void)): IO(void);
        }

        function main(): IO(i32) {
            let raw: Pointer(void) = perform malloc(8);
            let p: Pointer(i32) = raw.cast();
            perform p.write(111);
            let p1: Pointer(i32) = p.offset(1);
            perform p1.write(222);
            let v0: i32 = perform p.read();
            let v1: i32 = perform p1.read();
            perform free(p.cast());
            return IO.pure(v0 + v1);
        }
    "#;
    let context = Context::create();
    let codegen =
        modus::compile_source(&context, source, "test_ffi_offset").expect("Compile failed");
    codegen.optimize(None).expect("Optimize failed");
    let res = codegen.jit_run().expect("JIT run failed");
    assert_eq!(res, modus::backend::ExecutionResult::I32(333));
}

#[test]
fn test_codegen_ffi_puts_and_strings() {
    let source = r#"
        extern "C" {
            function puts(s: CString): IO(i32);
        }

        function main(): IO(i32) {
            let s: CString = String.toCString("Hello from Modus FFI!");
            let r: i32 = perform puts(s);
            return IO.pure(0);
        }
    "#;
    let context = Context::create();
    let codegen = modus::compile_source(&context, source, "test_ffi_puts").expect("Compile failed");
    codegen.optimize(None).expect("Optimize failed");
    let res = codegen.jit_run().expect("JIT run failed");
    assert_eq!(res, modus::backend::ExecutionResult::I32(0));
}

#[test]
fn test_codegen_if_without_else_early_return() {
    let source = r#"
        function sum_range(n: i32, acc: i32): i32 {
            if (n <= 0) {
                return acc;
            }
            return sum_range(n - 1, acc + n);
        }

        function main(): i32 {
            return sum_range(10, 0);
        }
    "#;
    let context = Context::create();
    let codegen =
        modus::compile_source(&context, source, "test_if_no_else").expect("Compile failed");
    codegen.optimize(None).expect("Optimize failed");
    let res = codegen.jit_run().expect("JIT run failed");
    assert_eq!(res, modus::backend::ExecutionResult::I32(55));
}

#[test]
fn test_codegen_let_if_expression() {
    let source = r#"
        function clamp_val(x: i32): i32 {
            let res: i32 = if (x > 10) {
                10
            } else {
                x
            };
            return res;
        }

        function main(): i32 {
            return clamp_val(15) + clamp_val(3);
        }
    "#;
    let context = Context::create();
    let codegen = modus::compile_source(&context, source, "test_let_if").expect("Compile failed");
    codegen.optimize(None).expect("Optimize failed");
    let res = codegen.jit_run().expect("JIT run failed");
    assert_eq!(res, modus::backend::ExecutionResult::I32(13));
}
