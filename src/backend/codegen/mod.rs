//! LLVM Code Generation from Modus ANF IR.
//!
//! Features:
//! - Internal `fastcc`, entry/FFI `ccc` calling conventions
//! - Direct recursion with tail call elimination (`musttail`)
//! - Non-atomic Perceus reference counting (`inc_ref`, `dec_ref`)
//! - FBIP buffer reuse (`ReuseRecord`, `is_unique`)
//! - Unboxed primitives, heap objects allocated with `[u64 rc | payload]`
//! - LLVM `-O3` optimization pipeline

mod expr;
mod ops;
mod tail;
mod target;

pub use target::ExecutionResult;

use crate::ir::node::*;
use crate::typechecker::Type;
use inkwell::builder::Builder;
use inkwell::context::Context;
use inkwell::module::Module;
use inkwell::values::{BasicValueEnum, FunctionValue};
use std::collections::{BTreeMap, HashMap};

use super::runtime::Runtime;
use super::types::TypeLowerer;

/// Code generator targeting LLVM IR via Inkwell.
pub struct CodeGen<'ctx> {
    pub context: &'ctx Context,
    pub module: Module<'ctx>,
    pub builder: Builder<'ctx>,
    pub type_lowerer: TypeLowerer<'ctx>,
    pub runtime: Runtime<'ctx>,
    pub(crate) variables: HashMap<String, BasicValueEnum<'ctx>>,
    pub(crate) functions: HashMap<String, FunctionValue<'ctx>>,
    pub(crate) fn_ret_types: HashMap<String, Type>,
    pub(crate) current_fn: Option<FunctionValue<'ctx>>,
    pub(crate) current_fn_ret: Option<Type>,
    pub(crate) record_field_indices: HashMap<String, BTreeMap<String, u32>>,
}

impl<'ctx> CodeGen<'ctx> {
    pub fn new(context: &'ctx Context, module_name: &str) -> Self {
        let module = context.create_module(module_name);
        let builder = context.create_builder();
        let type_lowerer = TypeLowerer::new(context);
        let runtime = Runtime::new(context, &module);

        Self {
            context,
            module,
            builder,
            type_lowerer,
            runtime,
            variables: HashMap::new(),
            functions: HashMap::new(),
            fn_ret_types: HashMap::new(),
            current_fn: None,
            current_fn_ret: None,
            record_field_indices: HashMap::new(),
        }
    }

    /// Compiles an entire `AnfProgram` into the LLVM module.
    pub fn compile_program(&mut self, prog: &AnfProgram) -> Result<(), String> {
        // 0. Declare imported external functions
        for ext in &prog.extern_functions {
            self.declare_external_function(ext);
        }

        // 1. Declare all functions first to support mutual and forward references
        for func in &prog.functions {
            self.declare_function(func);
        }

        for im in &prog.impls {
            for m in &im.methods {
                self.declare_function(m);
            }
        }

        // 2. Compile function bodies
        for func in &prog.functions {
            self.compile_function(func)?;
        }

        for im in &prog.impls {
            for m in &im.methods {
                self.compile_function(m)?;
            }
        }

        // 3. Verify module
        self.module.verify().map_err(|e| e.to_string())?;

        Ok(())
    }

    /// Declares an imported external function signature in the LLVM module.
    pub(crate) fn declare_external_function(&mut self, ext: &AnfExternFunction) {
        let fn_val = if let Some(existing) = self.module.get_function(&ext.symbol_name) {
            existing
        } else {
            let fn_type = self
                .type_lowerer
                .function_type(&ext.param_types, &ext.return_type);
            let f = self.module.add_function(&ext.symbol_name, fn_type, None);
            f.set_call_conventions(0);
            f
        };
        self.functions.insert(ext.symbol_name.clone(), fn_val);
        self.functions.insert(ext.name.clone(), fn_val);
        self.fn_ret_types
            .insert(ext.symbol_name.clone(), ext.return_type.clone());
        self.fn_ret_types
            .insert(ext.name.clone(), ext.return_type.clone());
    }

    /// Declares a function signature in the LLVM module.
    pub(crate) fn declare_function(&mut self, func: &AnfFunction) {
        let param_types: Vec<Type> = func.params.iter().map(|(_, ty)| ty.clone()).collect();
        let fn_type = self
            .type_lowerer
            .function_type(&param_types, &func.return_type);

        let fn_val = self.module.add_function(&func.name, fn_type, None);

        // Entry point and exported library functions use standard calling convention (ccc = 0)
        // Unexported internal functions use fastcc (8)
        let is_exported_or_entry = func.name == "main" || func.name.starts_with("_modus_M_");
        let call_conv = if is_exported_or_entry { 0 } else { 8 };
        fn_val.set_call_conventions(call_conv);

        self.functions.insert(func.name.clone(), fn_val);
        self.fn_ret_types
            .insert(func.name.clone(), func.return_type.clone());
    }

    /// Compiles a function's ANF body into LLVM IR.
    pub(crate) fn compile_function(&mut self, func: &AnfFunction) -> Result<(), String> {
        let fn_val = *self.functions.get(&func.name).unwrap();
        self.current_fn = Some(fn_val);
        self.current_fn_ret = Some(func.return_type.clone());
        self.variables.clear();

        let entry_bb = self.context.append_basic_block(fn_val, "entry");
        self.builder.position_at_end(entry_bb);

        // Map function parameters to their LLVM argument values
        for (i, (param_name, _)) in func.params.iter().enumerate() {
            let arg_val = fn_val.get_nth_param(i as u32).unwrap();
            self.variables.insert(param_name.clone(), arg_val);
        }

        self.compile_block(&func.body)?;

        Ok(())
    }

    /// Compiles an `AnfBlock` into LLVM IR.
    pub(crate) fn compile_block(&mut self, block: &AnfBlock) -> Result<(), String> {
        for stmt in &block.stmts {
            self.compile_stmt(stmt)?;
        }

        self.compile_tail(&block.tail)?;
        Ok(())
    }

    /// Compiles an individual ANF statement.
    pub(crate) fn compile_stmt(&mut self, stmt: &AnfStmt) -> Result<(), String> {
        match stmt {
            AnfStmt::Let {
                var,
                ty,
                value,
                span: _,
            } => {
                let llvm_val = self.compile_expr(value, ty)?;
                self.variables.insert(var.clone(), llvm_val);

                if let AnfExpr::Record { fields } = value {
                    let mut idx_map = BTreeMap::new();
                    for (i, (f_name, _)) in fields.iter().enumerate() {
                        idx_map.insert(f_name.clone(), (i + 1) as u32);
                    }
                    self.record_field_indices.insert(var.clone(), idx_map);
                } else if let Type::Record(flds) = ty {
                    let mut idx_map = BTreeMap::new();
                    for (i, (f_name, _)) in flds.iter().enumerate() {
                        idx_map.insert(f_name.clone(), (i + 1) as u32);
                    }
                    self.record_field_indices.insert(var.clone(), idx_map);
                }
            }

            AnfStmt::Expr(expr) => {
                let _ = self.compile_expr(expr, &Type::void())?;
            }

            AnfStmt::IncRef { var } => {
                if let Some(val) = self.variables.get(var)
                    && val.is_pointer_value()
                {
                    let ptr = val.into_pointer_value();
                    let _ = self
                        .builder
                        .build_call(self.runtime.inc_ref_fn, &[ptr.into()], "");
                }
            }

            AnfStmt::DecRef { var } => {
                if let Some(val) = self.variables.get(var)
                    && val.is_pointer_value()
                {
                    let ptr = val.into_pointer_value();
                    let _ = self
                        .builder
                        .build_call(self.runtime.dec_ref_fn, &[ptr.into()], "");
                }
            }

            AnfStmt::SetField {
                receiver,
                field,
                value,
            } => {
                let recv_val = self.eval_atom(receiver)?;
                let field_val = self.eval_atom(value)?;
                if recv_val.is_pointer_value()
                    && let Some(var_name) = receiver.as_var()
                    && let Some(idx_map) = self.record_field_indices.get(var_name)
                    && let Some(&field_idx) = idx_map.get(field)
                {
                    let ptr = recv_val.into_pointer_value();
                    let field_ptr = unsafe {
                        self.builder
                            .build_gep(
                                self.context.i64_type(),
                                ptr,
                                &[self.context.i64_type().const_int(field_idx as u64, false)],
                                "field_gep",
                            )
                            .unwrap()
                    };
                    let _ = self.builder.build_store(field_ptr, field_val);
                }
            }
        }
        Ok(())
    }
}
