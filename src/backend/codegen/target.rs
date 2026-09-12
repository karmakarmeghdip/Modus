//! Optimization pipelines, native object/binary emission, and JIT execution.

use super::CodeGen;
use crate::typechecker::Type;
use inkwell::passes::PassBuilderOptions;
use inkwell::targets::TargetMachine;

/// The result of JIT-executing a Modus program.
#[derive(Debug, PartialEq, Clone)]
pub enum ExecutionResult {
    I64(i64),
    I32(i32),
    F64(f64),
    Bool(bool),
    Void,
}

impl<'ctx> CodeGen<'ctx> {
    /// Optimizes the generated LLVM module using LLVM's -O3 pipeline.
    pub fn optimize(&self, target_machine: Option<&TargetMachine>) -> Result<(), String> {
        let pass_options = PassBuilderOptions::create();
        if let Some(tm) = target_machine {
            self.module
                .run_passes("default<O3>", tm, pass_options)
                .map_err(|e| e.to_string())?;
        } else {
            inkwell::targets::Target::initialize_native(
                &inkwell::targets::InitializationConfig::default(),
            )
            .map_err(|e| e.to_string())?;
            let triple = TargetMachine::get_default_triple();
            let target =
                inkwell::targets::Target::from_triple(&triple).map_err(|e| e.to_string())?;
            let tm = target
                .create_target_machine(
                    &triple,
                    "",
                    "",
                    inkwell::OptimizationLevel::Aggressive,
                    inkwell::targets::RelocMode::Default,
                    inkwell::targets::CodeModel::Default,
                )
                .ok_or_else(|| "Failed to create target machine".to_string())?;
            self.module
                .run_passes("default<O3>", &tm, pass_options)
                .map_err(|e| e.to_string())?;
        }
        Ok(())
    }

    /// Emits LLVM IR assembly string.
    pub fn to_ir_string(&self) -> String {
        self.module.print_to_string().to_string()
    }

    /// Emits a native machine-code object file (.o) for the host platform.
    pub fn compile_to_object(&self, output_path: &std::path::Path) -> Result<(), String> {
        inkwell::targets::Target::initialize_native(
            &inkwell::targets::InitializationConfig::default(),
        )
        .map_err(|e| e.to_string())?;
        let triple = TargetMachine::get_default_triple();
        let target = inkwell::targets::Target::from_triple(&triple).map_err(|e| e.to_string())?;
        let tm = target
            .create_target_machine(
                &triple,
                "",
                "",
                inkwell::OptimizationLevel::Aggressive,
                inkwell::targets::RelocMode::PIC,
                inkwell::targets::CodeModel::Default,
            )
            .ok_or_else(|| "Failed to create target machine".to_string())?;

        tm.write_to_file(
            &self.module,
            inkwell::targets::FileType::Object,
            output_path,
        )
        .map_err(|e| e.to_string())?;
        Ok(())
    }

    /// Compiles the module to an object file and links it into a native executable binary using clang.
    pub fn compile_to_binary(&self, output_binary: &std::path::Path) -> Result<(), String> {
        let obj_path = output_binary.with_extension("o");
        self.compile_to_object(&obj_path)?;

        let status = std::process::Command::new("clang")
            .arg(&obj_path)
            .arg("-o")
            .arg(output_binary)
            .status()
            .map_err(|e| format!("Failed to invoke clang linker: {e}"))?;

        let _ = std::fs::remove_file(&obj_path);

        if !status.success() {
            return Err(format!("clang linker failed with status {status}"));
        }
        Ok(())
    }

    /// JIT-compiles and executes the `main` function in-process, returning the typed result.
    pub fn jit_run(&self) -> Result<ExecutionResult, String> {
        inkwell::targets::Target::initialize_native(
            &inkwell::targets::InitializationConfig::default(),
        )
        .map_err(|e| e.to_string())?;
        let ee = self
            .module
            .create_jit_execution_engine(inkwell::OptimizationLevel::Aggressive)
            .map_err(|e| e.to_string())?;

        let _ = self
            .module
            .get_function("main")
            .ok_or_else(|| "No 'main' function found in module".to_string())?;

        let ret_type = self.fn_ret_types.get("main").cloned();
        let inner_ty = match &ret_type {
            Some(Type::Named { name, args }) if name == "IO" && args.len() == 1 => &args[0],
            Some(other) => other,
            None => &Type::Primitive(crate::ast::PrimitiveType::I32),
        };

        unsafe {
            match inner_ty {
                Type::Primitive(crate::ast::PrimitiveType::I64) => {
                    let f = ee
                        .get_function::<unsafe extern "C" fn() -> i64>("main")
                        .map_err(|e| e.to_string())?;
                    Ok(ExecutionResult::I64(f.call()))
                }
                Type::Primitive(crate::ast::PrimitiveType::I32) => {
                    let f = ee
                        .get_function::<unsafe extern "C" fn() -> i32>("main")
                        .map_err(|e| e.to_string())?;
                    Ok(ExecutionResult::I32(f.call()))
                }
                Type::Primitive(crate::ast::PrimitiveType::F64) => {
                    let f = ee
                        .get_function::<unsafe extern "C" fn() -> f64>("main")
                        .map_err(|e| e.to_string())?;
                    Ok(ExecutionResult::F64(f.call()))
                }
                Type::Primitive(crate::ast::PrimitiveType::Bool) => {
                    let f = ee
                        .get_function::<unsafe extern "C" fn() -> bool>("main")
                        .map_err(|e| e.to_string())?;
                    Ok(ExecutionResult::Bool(f.call()))
                }
                Type::Primitive(crate::ast::PrimitiveType::Void) | Type::Unit => {
                    let f = ee
                        .get_function::<unsafe extern "C" fn()>("main")
                        .map_err(|e| e.to_string())?;
                    f.call();
                    Ok(ExecutionResult::Void)
                }
                _ => {
                    let f = ee
                        .get_function::<unsafe extern "C" fn() -> i64>("main")
                        .map_err(|e| e.to_string())?;
                    Ok(ExecutionResult::I64(f.call()))
                }
            }
        }
    }
}
