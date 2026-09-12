use inkwell::context::Context;
use modus::backend::ExecutionResult;
use modus::compile_source;
use modus::modules::{build_executable, build_shared_library, jit_run_module_graph};
use std::env;
use std::fs;
use std::path::{Path, PathBuf};
use std::time::Instant;

fn print_usage() {
    eprintln!("Modus Compiler CLI");
    eprintln!();
    eprintln!("USAGE:");
    eprintln!(
        "    modus run <file.mds>                                JIT execute a Modus program"
    );
    eprintln!(
        "    modus build <file.mds> [-o <out>]                   Compile to native standalone executable"
    );
    eprintln!("    modus build --lib <file.mds> [-o <out.so>] [--emit-header <file.mds>]");
    eprintln!(
        "                                                        Compile to shared library and emit export map"
    );
    eprintln!(
        "    modus clean                                         Clean compiler cache (.modus-cache/)"
    );
    eprintln!(
        "    modus emit-llvm <file.mds>                          Print optimized LLVM IR assembly"
    );
    eprintln!("    modus --demo                                        Run built-in demo program");
}

fn main() -> Result<(), Box<dyn std::error::Error>> {
    let args: Vec<String> = env::args().collect();

    if args.len() < 2 || args[1] == "--help" || args[1] == "-h" {
        print_usage();
        return Ok(());
    }

    if args[1] == "--demo" {
        return run_demo();
    }

    let command = &args[1];

    match command.as_str() {
        "run" => {
            if args.len() < 3 {
                eprintln!("Error: missing file path for 'run'");
                print_usage();
                std::process::exit(1);
            }
            let file_path = Path::new(&args[2]);

            println!("Running '{file_path:?}' through Modus pipeline...");
            let start = Instant::now();
            let result = jit_run_module_graph(file_path)
                .map_err(|e| format!("Runtime/Module error: {e}"))?;
            let elapsed = start.elapsed();

            match result {
                ExecutionResult::I64(v) => {
                    println!("Result (i64): {v} (executed in {elapsed:.2?})")
                }
                ExecutionResult::I32(v) => {
                    println!("Result (i32): {v} (executed in {elapsed:.2?})")
                }
                ExecutionResult::F64(v) => {
                    println!("Result (f64): {v} (executed in {elapsed:.2?})")
                }
                ExecutionResult::Bool(v) => {
                    println!("Result (bool): {v} (executed in {elapsed:.2?})")
                }
                ExecutionResult::Void => println!("Execution completed (void) in {elapsed:.2?}"),
            }
        }
        "build" => {
            let is_lib = args.iter().any(|a| a == "--lib");
            if is_lib {
                // Shared library mode
                let mut source_file = None;
                let mut out_path = None;
                let mut emit_header = None;

                let mut i = 2;
                while i < args.len() {
                    if args[i] == "--lib" {
                        i += 1;
                    } else if args[i] == "-o" && i + 1 < args.len() {
                        out_path = Some(PathBuf::from(&args[i + 1]));
                        i += 2;
                    } else if args[i] == "--emit-header" && i + 1 < args.len() {
                        emit_header = Some(PathBuf::from(&args[i + 1]));
                        i += 2;
                    } else if !args[i].starts_with('-') && source_file.is_none() {
                        source_file = Some(&args[i]);
                        i += 1;
                    } else {
                        i += 1;
                    }
                }

                let src = source_file.ok_or_else(|| {
                    eprintln!("Error: missing source file for 'build --lib'");
                    print_usage();
                    std::process::exit(1);
                })?;

                let src_path = Path::new(src);
                let stem = src_path
                    .file_stem()
                    .unwrap_or_default()
                    .to_str()
                    .unwrap_or("library");
                let target_lib =
                    out_path.unwrap_or_else(|| src_path.with_file_name(format!("lib{stem}.so")));

                println!(
                    "Building shared library '{}' -> '{}'...",
                    src_path.display(),
                    target_lib.display()
                );
                let start = Instant::now();
                build_shared_library(src_path, &target_lib, emit_header.as_deref())
                    .map_err(|e| format!("Library build error: {e}"))?;

                println!(
                    "Successfully built shared library '{}' in {:.2?}!",
                    target_lib.display(),
                    start.elapsed()
                );
            } else {
                // Standalone executable mode
                if args.len() < 3 {
                    eprintln!("Error: missing file path for 'build'");
                    print_usage();
                    std::process::exit(1);
                }
                let file_path = Path::new(&args[2]);
                let mut out_path = PathBuf::from(
                    file_path
                        .file_stem()
                        .unwrap_or_default()
                        .to_str()
                        .unwrap_or("output"),
                );

                if args.len() >= 5 && args[3] == "-o" {
                    out_path = PathBuf::from(&args[4]);
                }

                println!(
                    "Building executable '{}' -> '{}'...",
                    file_path.display(),
                    out_path.display()
                );
                let start = Instant::now();
                build_executable(file_path, &out_path, None)
                    .map_err(|e| format!("Build error: {e}"))?;

                println!(
                    "Successfully built binary '{}' in {:.2?}!",
                    out_path.display(),
                    start.elapsed()
                );
            }
        }
        "clean" => {
            let cache_dir = Path::new(".modus-cache");
            if cache_dir.exists() {
                fs::remove_dir_all(cache_dir).map_err(|e| format!("Failed to clean cache: {e}"))?;
                println!("Cleaned .modus-cache/");
            } else {
                println!(".modus-cache/ does not exist, nothing to clean.");
            }
        }
        "emit-llvm" => {
            if args.len() < 3 {
                eprintln!("Error: missing file path for 'emit-llvm'");
                print_usage();
                std::process::exit(1);
            }
            let file_path = &args[2];
            let source = fs::read_to_string(file_path)
                .map_err(|e| format!("Failed to read '{file_path}': {e}"))?;

            let context = Context::create();
            let codegen = compile_source(&context, &source, file_path)
                .map_err(|e| format!("Compilation error: {e}"))?;
            codegen
                .optimize(None)
                .map_err(|e| format!("Optimization error: {e}"))?;
            println!("{}", codegen.to_ir_string());
        }
        other => {
            eprintln!("Unknown command: '{other}'");
            print_usage();
            std::process::exit(1);
        }
    }

    Ok(())
}

fn run_demo() -> Result<(), Box<dyn std::error::Error>> {
    let demo_src = r#"
type Point = { x: i32, y: i32 };

function manhattan(p: Point): i32 {
    return p.x + p.y;
}

function fib(n: i32): i32 {
    return if (n <= 1) {
        n
    } else {
        fib(n - 1) + fib(n - 2)
    };
}

function main(): i32 {
    let pt: Point = { x: 12, y: 18 };
    let d: i32 = manhattan(pt);
    let f: i32 = fib(10);
    return d + f;
}
"#;
    println!("=== Modus Compiler Live Demo ===");
    println!("Source program:");
    println!("{demo_src}");

    let context = Context::create();
    let codegen = compile_source(&context, demo_src, "demo")?;
    codegen.optimize(None)?;

    println!("Executing demo via JIT (Inkwell)...");
    let result = codegen.jit_run()?;
    println!("Program returned: {result:?} (expected 30 + 55 = 85)");
    assert_eq!(result, ExecutionResult::I32(85));
    println!("Demo completed successfully!");
    Ok(())
}
