use inkwell::context::Context;
use modus::backend::ExecutionResult;
use modus::compile_source;
use std::env;
use std::fs;
use std::path::{Path, PathBuf};
use std::time::Instant;

fn print_usage() {
    eprintln!("Modus Compiler CLI");
    eprintln!();
    eprintln!("USAGE:");
    eprintln!("    modus run <file.mds>                 JIT execute a Modus program");
    eprintln!("    modus build <file.mds> [-o <out>]    Compile to native standalone executable");
    eprintln!("    modus emit-llvm <file.mds>           Print optimized LLVM IR assembly");
    eprintln!("    modus --demo                         Run built-in demo program");
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
            let file_path = &args[2];
            let source = fs::read_to_string(file_path)
                .map_err(|e| format!("Failed to read '{file_path}': {e}"))?;

            println!("Compiling '{file_path}' through Modus pipeline...");
            let start = Instant::now();
            let context = Context::create();
            let codegen = compile_source(&context, &source, file_path)
                .map_err(|e| format!("Compilation error: {e}"))?;
            codegen
                .optimize(None)
                .map_err(|e| format!("Optimization error: {e}"))?;

            println!(
                "Compilation & LLVM -O3 finished in {:.2?}. Executing via JIT...",
                start.elapsed()
            );
            let run_start = Instant::now();
            let result = codegen
                .jit_run()
                .map_err(|e| format!("Runtime error: {e}"))?;
            let elapsed = run_start.elapsed();

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
            if args.len() < 3 {
                eprintln!("Error: missing file path for 'build'");
                print_usage();
                std::process::exit(1);
            }
            let file_path = &args[2];
            let mut out_path = PathBuf::from(
                Path::new(file_path)
                    .file_stem()
                    .unwrap_or_default()
                    .to_str()
                    .unwrap_or("output"),
            );

            // Check for -o flag
            if args.len() >= 5 && args[3] == "-o" {
                out_path = PathBuf::from(&args[4]);
            }

            let source = fs::read_to_string(file_path)
                .map_err(|e| format!("Failed to read '{file_path}': {e}"))?;

            println!("Building '{file_path}' -> '{}'...", out_path.display());
            let start = Instant::now();
            let context = Context::create();
            let codegen = compile_source(&context, &source, file_path)
                .map_err(|e| format!("Compilation error: {e}"))?;
            codegen
                .optimize(None)
                .map_err(|e| format!("Optimization error: {e}"))?;

            codegen
                .compile_to_binary(&out_path)
                .map_err(|e| format!("Linking error: {e}"))?;

            println!(
                "Successfully built binary '{}' in {:.2?}!",
                out_path.display(),
                start.elapsed()
            );
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
            // Assume file path to run directly if it exists
            if Path::new(other).exists() {
                let source = fs::read_to_string(other)
                    .map_err(|e| format!("Failed to read '{other}': {e}"))?;
                let context = Context::create();
                let codegen = compile_source(&context, &source, other)
                    .map_err(|e| format!("Compilation error: {e}"))?;
                codegen
                    .optimize(None)
                    .map_err(|e| format!("Optimization error: {e}"))?;
                let result = codegen
                    .jit_run()
                    .map_err(|e| format!("Runtime error: {e}"))?;
                println!("Result: {result:?}");
            } else {
                eprintln!("Unknown command: '{other}'");
                print_usage();
                std::process::exit(1);
            }
        }
    }

    Ok(())
}

fn run_demo() -> Result<(), Box<dyn std::error::Error>> {
    let demo_path = Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("examples")
        .join("demo.mds");
    let source = fs::read_to_string(&demo_path)?;

    println!("==================================================");
    println!("             Modus Compiler Demo");
    println!("==================================================");
    println!("Running source file: {}", demo_path.display());
    println!("--------------------------------------------------");
    println!("{}", source.trim());
    println!("--------------------------------------------------");

    let context = Context::create();
    let start = Instant::now();
    let codegen =
        compile_source(&context, &source, "demo").map_err(|e| format!("Compilation error: {e}"))?;
    codegen
        .optimize(None)
        .map_err(|e| format!("Optimization error: {e}"))?;

    println!("Pipeline: parse -> typecheck -> desugar -> ANF/Perceus -> LLVM -O3");
    println!("Compiled in {:.2?}", start.elapsed());

    let exec_start = Instant::now();
    let result = codegen
        .jit_run()
        .map_err(|e| format!("Execution error: {e}"))?;
    println!(
        "JIT Execution Result: {result:?} in {:.2?}",
        exec_start.elapsed()
    );
    println!("==================================================");

    Ok(())
}
