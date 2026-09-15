//! Expression code generation (literals, records, closures, arrays, FBIP).

use super::CodeGen;
use crate::ast::BinaryOp;
use crate::ir::node::*;
use crate::typechecker::Type;
use inkwell::AddressSpace;
use inkwell::types::{BasicMetadataTypeEnum, BasicType};
use inkwell::values::{BasicMetadataValueEnum, BasicValue, BasicValueEnum};
use std::collections::BTreeMap;

impl<'ctx> CodeGen<'ctx> {
    /// Compiles an ANF expression producing a `BasicValueEnum`.
    pub(crate) fn compile_expr(
        &mut self,
        expr: &AnfExpr,
        ty: &Type,
    ) -> Result<BasicValueEnum<'ctx>, String> {
        match expr {
            AnfExpr::Atom(atom) => self.eval_atom(atom),

            AnfExpr::Binary { op, lhs, rhs } => {
                let mut l_val = self.eval_atom(lhs)?;
                let mut r_val = self.eval_atom(rhs)?;

                let is_cmp = matches!(
                    op,
                    BinaryOp::Eq
                        | BinaryOp::NotEq
                        | BinaryOp::Lt
                        | BinaryOp::LtEq
                        | BinaryOp::Gt
                        | BinaryOp::GtEq
                );
                if !is_cmp {
                    let target_ty = self.type_lowerer.llvm_type(ty);
                    if target_ty.is_int_type() || target_ty.is_float_type() {
                        l_val = self.coerce_to_type(l_val, target_ty)?;
                        r_val = self.coerce_to_type(r_val, target_ty)?;
                    }
                }
                self.compile_binary_op(*op, l_val, r_val)
            }

            AnfExpr::Unary { op, operand } => {
                let val = self.eval_atom(operand)?;
                self.compile_unary_op(*op, val)
            }

            AnfExpr::Call { callee, args } => {
                // `std:string.concat` is a codegen-owned intrinsic: emit the
                // concatenation inline (see `try_build_string_concat_call`)
                // instead of a call.
                if let Some(res) = self.try_build_string_concat_call(callee, args) {
                    return res;
                }
                if let Atom::Var(name) = callee {
                    if let Some(&tag) = self.union_variants.get(name) {
                        return self.build_variant_constructor(tag, args);
                    }
                    if name == "Ok" || name == "Some" {
                        return self.build_variant_constructor(0, args);
                    }
                    if name == "Err" || name == "None" {
                        return self.build_variant_constructor(1, args);
                    }
                    if self.variables.contains_key(name) {
                        return self.compile_expr(
                            &AnfExpr::CallClosure {
                                closure: callee.clone(),
                                args: args.clone(),
                            },
                            ty,
                        );
                    }
                }

                let fn_val = match callee {
                    Atom::Var(name) => match self.functions.get(name).copied() {
                        Some(f) => f,
                        None => {
                            let mut param_tys: Vec<BasicMetadataTypeEnum<'ctx>> = Vec::new();
                            for _ in args {
                                param_tys
                                    .push(self.context.ptr_type(AddressSpace::default()).into());
                            }
                            let fn_ty = self
                                .context
                                .ptr_type(AddressSpace::default())
                                .fn_type(&param_tys, false);
                            let f = self.module.add_function(name, fn_ty, None);
                            let is_main = name == "main";
                            let is_exported_lib =
                                self.is_lib_entry && name.starts_with("_modus_M_");
                            let call_conv = if is_main || is_exported_lib { 0 } else { 8 };
                            f.set_call_conventions(call_conv);
                            self.functions.insert(name.clone(), f);
                            f
                        }
                    },
                    _ => return Err("Callee must be a function name".to_string()),
                };

                let mut llvm_args: Vec<BasicMetadataValueEnum<'ctx>> = Vec::new();
                for (i, arg) in args.iter().enumerate() {
                    let arg_val = self.eval_atom(arg)?;
                    let target_ty = fn_val
                        .get_nth_param(i as u32)
                        .map(|p| p.get_type())
                        .unwrap_or(arg_val.get_type());
                    llvm_args.push(self.coerce_to_type(arg_val, target_ty)?.into());
                }

                let call_site = self.builder.build_call(fn_val, &llvm_args, "call").unwrap();
                call_site.set_call_convention(fn_val.get_call_conventions());

                Ok(call_site
                    .try_as_basic_value()
                    .basic()
                    .unwrap_or_else(|| self.context.i8_type().const_int(0, false).into()))
            }

            AnfExpr::MethodCall {
                receiver,
                method,
                args,
            } => {
                // Check for IO.pure built-in
                if let Atom::Var(r) = receiver
                    && r == "IO"
                    && method == "pure"
                {
                    if let Some(first_arg) = args.first() {
                        return self.eval_atom(first_arg);
                    } else {
                        return Ok(self.context.i8_type().const_int(0, false).into());
                    }
                }

                // Discriminated union constructors: Result.Ok, Result.Err, Option.Some, Option.None, List.Cons, List.Nil, etc.
                if let Atom::Var(r) = receiver {
                    let qualified = format!("{r}.{method}");
                    if let Some(&tag) = self.union_variants.get(&qualified).or_else(|| {
                        if r == "Result" || r == "Option" || r == "List" {
                            self.union_variants.get(method)
                        } else {
                            None
                        }
                    }) {
                        return self.build_variant_constructor(tag, args);
                    }
                }

                // Pointer built-ins: Pointer.null, Pointer.fromAddress
                if let Atom::Var(r) = receiver
                    && r == "Pointer"
                {
                    if method == "null" {
                        return Ok(self
                            .context
                            .ptr_type(AddressSpace::default())
                            .const_null()
                            .into());
                    }
                    if method == "fromAddress" && !args.is_empty() {
                        let addr_val = self.eval_atom(&args[0])?;
                        let ptr_ty = self.context.ptr_type(AddressSpace::default());
                        let ptr_val = self
                            .builder
                            .build_int_to_ptr(addr_val.into_int_value(), ptr_ty, "from_addr")
                            .unwrap();
                        return Ok(ptr_val.into());
                    }
                }

                // Array and ArrayBuilder static constructors: Array.new, Array.withCapacity, ArrayBuilder.new, ArrayBuilder.withCapacity
                if let Atom::Var(r) = receiver
                    && (r == "Array" || r == "ArrayBuilder")
                {
                    if method == "new" {
                        let cap_val = self.context.i64_type().const_int(4, false);
                        let call = self
                            .builder
                            .build_call(self.runtime.array_new_fn, &[cap_val.into()], "arr_new")
                            .unwrap();
                        return Ok(call.try_as_basic_value().basic().unwrap());
                    }
                    if method == "withCapacity" {
                        let cap_val = if let Some(arg) = args.first() {
                            let cv = self.eval_atom(arg)?;
                            self.coerce_to_type(cv, self.context.i64_type().into())?
                                .into_int_value()
                        } else {
                            self.context.i64_type().const_int(4, false)
                        };
                        let call = self
                            .builder
                            .build_call(
                                self.runtime.array_new_fn,
                                &[cap_val.into()],
                                "arr_with_cap",
                            )
                            .unwrap();
                        return Ok(call.try_as_basic_value().basic().unwrap());
                    }
                }

                // String.toCString built-in: returns pointer to data payload at offset 24
                if let Atom::Var(r) = receiver
                    && r == "String"
                    && method == "toCString"
                    && let Some(first_arg) = args.first()
                {
                    let arg_val = self.eval_atom(first_arg)?;
                    let ptr_ty = self.context.ptr_type(inkwell::AddressSpace::default());
                    let ptr = if arg_val.is_pointer_value() {
                        arg_val.into_pointer_value()
                    } else {
                        self.builder
                            .build_int_to_ptr(arg_val.into_int_value(), ptr_ty, "str_ptr")
                            .unwrap()
                    };
                    let cstr = unsafe {
                        self.builder
                            .build_gep(
                                self.context.i8_type(),
                                ptr,
                                &[self.context.i64_type().const_int(24, false)],
                                "to_cstr",
                            )
                            .unwrap()
                    };
                    return Ok(cstr.into());
                }

                // CString.toString built-in: converts C-string (const char*) to Modus String
                if let Atom::Var(r) = receiver
                    && r == "CString"
                    && method == "toString"
                    && let Some(first_arg) = args.first()
                {
                    let arg_val = self.eval_atom(first_arg)?;
                    let call = self
                        .builder
                        .build_call(
                            self.runtime.string_from_c_str_fn,
                            &[arg_val.into()],
                            "c_to_str",
                        )
                        .unwrap();
                    return Ok(call.try_as_basic_value().basic().unwrap());
                }

                // String.fromCharCode built-in: creates 1-char Modus String from integer code
                if let Atom::Var(r) = receiver
                    && r == "String"
                    && method == "fromCharCode"
                    && let Some(first_arg) = args.first()
                {
                    let arg_val = self.eval_atom(first_arg)?;
                    let code_i32 = self
                        .coerce_to_type(arg_val, self.context.i32_type().into())?
                        .into_int_value();
                    let call = self
                        .builder
                        .build_call(
                            self.runtime.str_from_char_code_fn,
                            &[code_i32.into()],
                            "char_str",
                        )
                        .unwrap();
                    return Ok(call.try_as_basic_value().basic().unwrap());
                }

                // Pointer instance methods: read, write, offset, address, isNull, cast, toString
                if method == "read" && args.is_empty() {
                    let recv_val = self.eval_atom(receiver)?;
                    let recv_ty = self.get_atom_type(receiver);
                    let ptr_ty = self.context.ptr_type(AddressSpace::default());
                    let ptr = if recv_val.is_pointer_value() {
                        recv_val.into_pointer_value()
                    } else {
                        self.builder
                            .build_int_to_ptr(recv_val.into_int_value(), ptr_ty, "read_ptr")
                            .unwrap()
                    };
                    let inner_ty = recv_ty.as_ref().and_then(|t| t.unwrap_pointer());
                    let load_ty = if let Some(inner) = inner_ty {
                        self.type_lowerer.llvm_type(inner)
                    } else {
                        self.type_lowerer.llvm_type(ty)
                    };
                    let loaded = self.builder.build_load(load_ty, ptr, "ptr_read").unwrap();
                    return Ok(loaded);
                }

                if method == "write" && args.len() == 1 {
                    let recv_val = self.eval_atom(receiver)?;
                    let recv_ty = self.get_atom_type(receiver);
                    let ptr_ty = self.context.ptr_type(AddressSpace::default());
                    let ptr = if recv_val.is_pointer_value() {
                        recv_val.into_pointer_value()
                    } else {
                        self.builder
                            .build_int_to_ptr(recv_val.into_int_value(), ptr_ty, "write_ptr")
                            .unwrap()
                    };
                    let val_to_write = self.eval_atom(&args[0])?;
                    let inner_ty = recv_ty.as_ref().and_then(|t| t.unwrap_pointer());
                    let val_cast = if let Some(inner) = inner_ty {
                        let target_llvm_ty = self.type_lowerer.llvm_type(inner);
                        if target_llvm_ty.is_int_type() && val_to_write.is_int_value() {
                            let target_int_ty = target_llvm_ty.into_int_type();
                            let val_int = val_to_write.into_int_value();
                            if val_int.get_type().get_bit_width() > target_int_ty.get_bit_width() {
                                self.builder
                                    .build_int_truncate(val_int, target_int_ty, "trunc_val")
                                    .unwrap()
                                    .into()
                            } else if val_int.get_type().get_bit_width()
                                < target_int_ty.get_bit_width()
                            {
                                self.builder
                                    .build_int_z_extend(val_int, target_int_ty, "zext_val")
                                    .unwrap()
                                    .into()
                            } else {
                                val_to_write
                            }
                        } else {
                            val_to_write
                        }
                    } else {
                        val_to_write
                    };
                    let _ = self.builder.build_store(ptr, val_cast);
                    return Ok(self.context.i8_type().const_int(0, false).into());
                }

                if method == "offset" && args.len() == 1 {
                    let recv_val = self.eval_atom(receiver)?;
                    if recv_val.is_pointer_value() {
                        let ptr = recv_val.into_pointer_value();
                        let count_val = self.eval_atom(&args[0])?.into_int_value();
                        let i64_type = self.context.i64_type();
                        let count_i64 = if count_val.get_type().get_bit_width() < 64 {
                            self.builder
                                .build_int_s_extend(count_val, i64_type, "ext_count")
                                .unwrap()
                        } else {
                            count_val
                        };
                        let ptr_int = self
                            .builder
                            .build_ptr_to_int(ptr, i64_type, "ptr_int")
                            .unwrap();
                        let elem_size_bytes: u64 = match ty.unwrap_pointer() {
                            Some(t) => match t {
                                Type::Primitive(p) => match p {
                                    crate::ast::PrimitiveType::U8
                                    | crate::ast::PrimitiveType::I8
                                    | crate::ast::PrimitiveType::Bool => 1,
                                    crate::ast::PrimitiveType::U16
                                    | crate::ast::PrimitiveType::I16 => 2,
                                    crate::ast::PrimitiveType::U32
                                    | crate::ast::PrimitiveType::I32
                                    | crate::ast::PrimitiveType::F32 => 4,
                                    crate::ast::PrimitiveType::U64
                                    | crate::ast::PrimitiveType::I64
                                    | crate::ast::PrimitiveType::F64 => 8,
                                    crate::ast::PrimitiveType::String
                                    | crate::ast::PrimitiveType::Void => 8,
                                },
                                _ => 8,
                            },
                            None => 1,
                        };
                        let elem_size = i64_type.const_int(elem_size_bytes, false);
                        let byte_offset = self
                            .builder
                            .build_int_mul(count_i64, elem_size, "byte_off")
                            .unwrap();
                        let new_addr = self
                            .builder
                            .build_int_add(ptr_int, byte_offset, "new_addr")
                            .unwrap();
                        let new_ptr = self
                            .builder
                            .build_int_to_ptr(
                                new_addr,
                                self.context.ptr_type(AddressSpace::default()),
                                "off_ptr",
                            )
                            .unwrap();
                        return Ok(new_ptr.into());
                    }
                }

                if method == "address" && args.is_empty() {
                    let recv_val = self.eval_atom(receiver)?;
                    if recv_val.is_pointer_value() {
                        let ptr = recv_val.into_pointer_value();
                        let i64_type = self.context.i64_type();
                        let ptr_int = self
                            .builder
                            .build_ptr_to_int(ptr, i64_type, "ptr_addr")
                            .unwrap();
                        return Ok(ptr_int.into());
                    }
                }

                if method == "isNull" && args.is_empty() {
                    let recv_val = self.eval_atom(receiver)?;
                    if recv_val.is_pointer_value() {
                        let ptr = recv_val.into_pointer_value();
                        let i64_type = self.context.i64_type();
                        let ptr_int = self
                            .builder
                            .build_ptr_to_int(ptr, i64_type, "ptr_val")
                            .unwrap();
                        let is_null = self
                            .builder
                            .build_int_compare(
                                inkwell::IntPredicate::EQ,
                                ptr_int,
                                i64_type.const_int(0, false),
                                "is_null",
                            )
                            .unwrap();
                        return Ok(is_null.into());
                    }
                }

                if method == "cast" && args.is_empty() {
                    let recv_val = self.eval_atom(receiver)?;
                    if recv_val.is_pointer_value() {
                        return Ok(recv_val);
                    }
                }

                if method == "toString" && args.is_empty() {
                    let recv_val = self.eval_atom(receiver)?;
                    let call = self
                        .builder
                        .build_call(
                            self.runtime.string_from_c_str_fn,
                            &[recv_val.into()],
                            "ptr_to_str",
                        )
                        .unwrap();
                    return Ok(call.try_as_basic_value().basic().unwrap());
                }

                if method == "push" && args.len() == 1 {
                    let recv_ty = self.get_atom_type(receiver);
                    let is_builder = recv_ty
                        .as_ref()
                        .map(|t| t.is_array_builder())
                        .unwrap_or(false);
                    if is_builder || recv_ty.is_none() {
                        let recv_val = self.eval_atom(receiver)?;
                        let ptr_ty = self.context.ptr_type(inkwell::AddressSpace::default());
                        let ptr = if recv_val.is_pointer_value() {
                            recv_val.into_pointer_value()
                        } else {
                            self.builder
                                .build_int_to_ptr(recv_val.into_int_value(), ptr_ty, "b_ptr")
                                .unwrap()
                        };
                        let elem_val = self.eval_atom(&args[0])?;
                        let elem_i64 = if elem_val.is_pointer_value() {
                            self.builder
                                .build_ptr_to_int(
                                    elem_val.into_pointer_value(),
                                    self.context.i64_type(),
                                    "ptr_int",
                                )
                                .unwrap()
                        } else if elem_val.is_float_value() {
                            let fv = elem_val.into_float_value();
                            if fv.get_type() == self.context.f32_type() {
                                let f64_val = self
                                    .builder
                                    .build_float_ext(fv, self.context.f64_type(), "f_ext")
                                    .unwrap();
                                self.builder
                                    .build_bit_cast(f64_val, self.context.i64_type(), "f_bits")
                                    .unwrap()
                                    .into_int_value()
                            } else {
                                self.builder
                                    .build_bit_cast(fv, self.context.i64_type(), "f_bits")
                                    .unwrap()
                                    .into_int_value()
                            }
                        } else {
                            self.coerce_to_type(elem_val, self.context.i64_type().into())?
                                .into_int_value()
                        };
                        let is_heap = self
                            .get_atom_type(&args[0])
                            .map(|t| crate::ir::liveness::is_heap_type(&t))
                            .unwrap_or(false);
                        let is_heap_val = self
                            .context
                            .bool_type()
                            .const_int(if is_heap { 1 } else { 0 }, false);
                        let call = self
                            .builder
                            .build_call(
                                self.runtime.array_push_fn,
                                &[ptr.into(), elem_i64.into(), is_heap_val.into()],
                                "arr_push",
                            )
                            .unwrap();
                        return Ok(call.try_as_basic_value().basic().unwrap());
                    }
                }

                if method == "set" && args.len() == 2 {
                    let recv_ty = self.get_atom_type(receiver);
                    let is_arr = recv_ty
                        .as_ref()
                        .map(|t| t.is_array_builder())
                        .unwrap_or(false);
                    if is_arr || recv_ty.is_none() {
                        let recv_val = self.eval_atom(receiver)?;
                        let ptr_ty = self.context.ptr_type(inkwell::AddressSpace::default());
                        let ptr = if recv_val.is_pointer_value() {
                            recv_val.into_pointer_value()
                        } else {
                            self.builder
                                .build_int_to_ptr(recv_val.into_int_value(), ptr_ty, "arr_ptr")
                                .unwrap()
                        };
                        let idx_val = self.eval_atom(&args[0])?;
                        let idx_i64 = self
                            .coerce_to_type(idx_val, self.context.i64_type().into())?
                            .into_int_value();
                        let elem_val = self.eval_atom(&args[1])?;
                        let elem_i64 = if elem_val.is_pointer_value() {
                            self.builder
                                .build_ptr_to_int(
                                    elem_val.into_pointer_value(),
                                    self.context.i64_type(),
                                    "ptr_int",
                                )
                                .unwrap()
                        } else if elem_val.is_float_value() {
                            let fv = elem_val.into_float_value();
                            if fv.get_type() == self.context.f32_type() {
                                let f64_val = self
                                    .builder
                                    .build_float_ext(fv, self.context.f64_type(), "f_ext")
                                    .unwrap();
                                self.builder
                                    .build_bit_cast(f64_val, self.context.i64_type(), "f_bits")
                                    .unwrap()
                                    .into_int_value()
                            } else {
                                self.builder
                                    .build_bit_cast(fv, self.context.i64_type(), "f_bits")
                                    .unwrap()
                                    .into_int_value()
                            }
                        } else {
                            self.coerce_to_type(elem_val, self.context.i64_type().into())?
                                .into_int_value()
                        };
                        let is_heap = self
                            .get_atom_type(&args[1])
                            .map(|t| crate::ir::liveness::is_heap_type(&t))
                            .unwrap_or(false);
                        let is_heap_val = self
                            .context
                            .bool_type()
                            .const_int(if is_heap { 1 } else { 0 }, false);
                        let call = self
                            .builder
                            .build_call(
                                self.runtime.array_set_fn,
                                &[
                                    ptr.into(),
                                    idx_i64.into(),
                                    elem_i64.into(),
                                    is_heap_val.into(),
                                ],
                                "arr_set",
                            )
                            .unwrap();
                        return Ok(call.try_as_basic_value().basic().unwrap());
                    }
                }

                if method == "pop" && args.is_empty() {
                    let recv_ty = self.get_atom_type(receiver);
                    let is_arr = recv_ty
                        .as_ref()
                        .map(|t| t.is_array_builder())
                        .unwrap_or(false);
                    if is_arr || recv_ty.is_none() {
                        let recv_val = self.eval_atom(receiver)?;
                        let ptr_ty = self.context.ptr_type(inkwell::AddressSpace::default());
                        let ptr = if recv_val.is_pointer_value() {
                            recv_val.into_pointer_value()
                        } else {
                            self.builder
                                .build_int_to_ptr(recv_val.into_int_value(), ptr_ty, "arr_ptr")
                                .unwrap()
                        };
                        let is_heap = recv_ty
                            .as_ref()
                            .and_then(|t| t.unwrap_array_builder().cloned())
                            .map(|et| crate::ir::liveness::is_heap_type(&et))
                            .unwrap_or(false);
                        let is_heap_val = self
                            .context
                            .bool_type()
                            .const_int(if is_heap { 1 } else { 0 }, false);
                        let call = self
                            .builder
                            .build_call(
                                self.runtime.array_pop_fn,
                                &[ptr.into(), is_heap_val.into()],
                                "arr_pop",
                            )
                            .unwrap();
                        return Ok(call.try_as_basic_value().basic().unwrap());
                    }
                }

                if method == "build" && args.is_empty() {
                    let recv_ty = self.get_atom_type(receiver);
                    let is_builder = recv_ty
                        .as_ref()
                        .map(|t| t.is_array_builder())
                        .unwrap_or(false);
                    if is_builder || recv_ty.is_none() {
                        let recv_val = self.eval_atom(receiver)?;
                        let ptr_ty = self.context.ptr_type(inkwell::AddressSpace::default());
                        let ptr = if recv_val.is_pointer_value() {
                            recv_val.into_pointer_value()
                        } else {
                            self.builder
                                .build_int_to_ptr(recv_val.into_int_value(), ptr_ty, "b_ptr")
                                .unwrap()
                        };
                        let is_heap = recv_ty
                            .as_ref()
                            .and_then(|t| t.unwrap_array_builder().cloned())
                            .map(|et| crate::ir::liveness::is_heap_type(&et))
                            .unwrap_or(false);
                        let is_heap_val = self
                            .context
                            .bool_type()
                            .const_int(if is_heap { 1 } else { 0 }, false);
                        let call = self
                            .builder
                            .build_call(
                                self.runtime.array_build_fn,
                                &[ptr.into(), is_heap_val.into()],
                                "arr_build",
                            )
                            .unwrap();
                        return Ok(call.try_as_basic_value().basic().unwrap());
                    }
                }

                if method == "capacity" && args.is_empty() {
                    let recv_val = self.eval_atom(receiver)?;
                    let ptr_ty = self.context.ptr_type(inkwell::AddressSpace::default());
                    let ptr = if recv_val.is_pointer_value() {
                        recv_val.into_pointer_value()
                    } else {
                        self.builder
                            .build_int_to_ptr(recv_val.into_int_value(), ptr_ty, "b_ptr")
                            .unwrap()
                    };
                    let cap_ptr = unsafe {
                        self.builder
                            .build_gep(
                                self.context.i64_type(),
                                ptr,
                                &[self.context.i64_type().const_int(2, false)],
                                "ab_cap_ptr",
                            )
                            .unwrap()
                    };
                    let cap_val = self
                        .builder
                        .build_load(self.context.i64_type(), cap_ptr, "ab_cap")
                        .unwrap();
                    return Ok(cap_val);
                }

                if method == "isEmpty" && args.is_empty() {
                    let recv_val = self.eval_atom(receiver)?;
                    let ptr_ty = self.context.ptr_type(inkwell::AddressSpace::default());
                    let ptr = if recv_val.is_pointer_value() {
                        recv_val.into_pointer_value()
                    } else {
                        self.builder
                            .build_int_to_ptr(recv_val.into_int_value(), ptr_ty, "b_ptr")
                            .unwrap()
                    };
                    let len_ptr = unsafe {
                        self.builder
                            .build_gep(
                                self.context.i64_type(),
                                ptr,
                                &[self.context.i64_type().const_int(1, false)],
                                "ab_len_ptr",
                            )
                            .unwrap()
                    };
                    let len_val = self
                        .builder
                        .build_load(self.context.i64_type(), len_ptr, "ab_len")
                        .unwrap()
                        .into_int_value();
                    let is_empty = self
                        .builder
                        .build_int_compare(
                            inkwell::IntPredicate::EQ,
                            len_val,
                            self.context.i64_type().const_int(0, false),
                            "ab_is_empty",
                        )
                        .unwrap();
                    return Ok(is_empty.into());
                }

                if method == "length" && args.is_empty() {
                    let recv_ty = self.get_atom_type(receiver);
                    let is_arr = recv_ty
                        .as_ref()
                        .map(|t| t.is_array() || t.is_array_builder())
                        .unwrap_or(false);
                    let is_str = recv_ty.as_ref().map(|t| t.is_string()).unwrap_or(false)
                        || matches!(receiver, Atom::Literal(crate::ast::Literal::String(_)));
                    if is_arr || is_str || recv_ty.is_none() {
                        let recv_val = self.eval_atom(receiver)?;
                        let ptr_ty = self.context.ptr_type(inkwell::AddressSpace::default());
                        let ptr = if recv_val.is_pointer_value() {
                            recv_val.into_pointer_value()
                        } else {
                            self.builder
                                .build_int_to_ptr(recv_val.into_int_value(), ptr_ty, "len_ptr_cast")
                                .unwrap()
                        };
                        let len_ptr = unsafe {
                            self.builder
                                .build_gep(
                                    self.context.i64_type(),
                                    ptr,
                                    &[self.context.i64_type().const_int(1, false)],
                                    "len_ptr",
                                )
                                .unwrap()
                        };
                        let len_val = self
                            .builder
                            .build_load(self.context.i64_type(), len_ptr, "len_val")
                            .unwrap();
                        return Ok(len_val);
                    }
                }

                if method == "charCodeAt" && args.len() == 1 {
                    let recv_ty = self.get_atom_type(receiver);
                    let is_str = recv_ty.as_ref().map(|t| t.is_string()).unwrap_or(true);
                    if is_str {
                        let recv_val = self.eval_atom(receiver)?;
                        let ptr_ty = self.context.ptr_type(inkwell::AddressSpace::default());
                        let ptr = if recv_val.is_pointer_value() {
                            recv_val.into_pointer_value()
                        } else {
                            self.builder
                                .build_int_to_ptr(recv_val.into_int_value(), ptr_ty, "str_ptr")
                                .unwrap()
                        };
                        let idx_val = self.eval_atom(&args[0])?.into_int_value();
                        let i64_type = self.context.i64_type();
                        let i32_type = self.context.i32_type();
                        let idx_i64 = if idx_val.get_type().get_bit_width() < 64 {
                            self.builder
                                .build_int_s_extend(idx_val, i64_type, "ext_idx")
                                .unwrap()
                        } else {
                            idx_val
                        };

                        let ptr_int = self
                            .builder
                            .build_ptr_to_int(ptr, i64_type, "ptr_int")
                            .unwrap();
                        let is_null = self
                            .builder
                            .build_int_compare(
                                inkwell::IntPredicate::EQ,
                                ptr_int,
                                i64_type.const_int(0, false),
                                "is_null",
                            )
                            .unwrap();

                        let len_ptr = unsafe {
                            self.builder
                                .build_gep(
                                    i64_type,
                                    ptr,
                                    &[i64_type.const_int(1, false)],
                                    "len_ptr",
                                )
                                .unwrap()
                        };
                        let len = self
                            .builder
                            .build_load(i64_type, len_ptr, "len")
                            .unwrap()
                            .into_int_value();

                        let lt_zero = self
                            .builder
                            .build_int_compare(
                                inkwell::IntPredicate::SLT,
                                idx_i64,
                                i64_type.const_int(0, false),
                                "lt_zero",
                            )
                            .unwrap();
                        let ge_len = self
                            .builder
                            .build_int_compare(inkwell::IntPredicate::SGE, idx_i64, len, "ge_len")
                            .unwrap();
                        let oob = self.builder.build_or(lt_zero, ge_len, "oob").unwrap();
                        let invalid = self.builder.build_or(is_null, oob, "invalid").unwrap();

                        let cur_fn = self.current_fn.unwrap();
                        let in_bounds_bb = self
                            .context
                            .append_basic_block(cur_fn, "char_code_in_bounds");
                        let oob_bb = self.context.append_basic_block(cur_fn, "char_code_oob");
                        let merge_bb = self.context.append_basic_block(cur_fn, "char_code_merge");

                        let _ =
                            self.builder
                                .build_conditional_branch(invalid, oob_bb, in_bounds_bb);

                        self.builder.position_at_end(in_bounds_bb);
                        // Data starts at offset 24 bytes in the Modus String struct
                        let char_offset = self
                            .builder
                            .build_int_add(i64_type.const_int(24, false), idx_i64, "char_off")
                            .unwrap();
                        let char_ptr = unsafe {
                            self.builder
                                .build_gep(self.context.i8_type(), ptr, &[char_offset], "char_ptr")
                                .unwrap()
                        };
                        let byte_val = self
                            .builder
                            .build_load(self.context.i8_type(), char_ptr, "byte_val")
                            .unwrap()
                            .into_int_value();
                        let char_code = self
                            .builder
                            .build_int_z_extend(byte_val, i32_type, "char_code")
                            .unwrap();
                        let in_bounds_done_bb = self.builder.get_insert_block().unwrap();
                        let _ = self.builder.build_unconditional_branch(merge_bb);

                        self.builder.position_at_end(oob_bb);
                        let minus_one = i32_type.const_int((-1i32) as u64, true);
                        let oob_done_bb = self.builder.get_insert_block().unwrap();
                        let _ = self.builder.build_unconditional_branch(merge_bb);

                        self.builder.position_at_end(merge_bb);
                        let phi = self.builder.build_phi(i32_type, "res_char_code").unwrap();
                        phi.add_incoming(&[
                            (&char_code, in_bounds_done_bb),
                            (&minus_one, oob_done_bb),
                        ]);
                        return Ok(phi.as_basic_value());
                    }
                }

                if method == "substring" && args.len() == 2 {
                    let recv_ty = self.get_atom_type(receiver);
                    let is_str = recv_ty.as_ref().map(|t| t.is_string()).unwrap_or(true);
                    if is_str {
                        let recv_val = self.eval_atom(receiver)?;
                        let ptr_ty = self.context.ptr_type(inkwell::AddressSpace::default());
                        let ptr = if recv_val.is_pointer_value() {
                            recv_val.into_pointer_value()
                        } else {
                            self.builder
                                .build_int_to_ptr(recv_val.into_int_value(), ptr_ty, "str_ptr")
                                .unwrap()
                        };
                        let start_val = self.eval_atom(&args[0])?.into_int_value();
                        let end_val = self.eval_atom(&args[1])?.into_int_value();
                        let i64_type = self.context.i64_type();
                        let start_i64 = if start_val.get_type().get_bit_width() < 64 {
                            self.builder
                                .build_int_s_extend(start_val, i64_type, "ext_start")
                                .unwrap()
                        } else {
                            start_val
                        };
                        let end_i64 = if end_val.get_type().get_bit_width() < 64 {
                            self.builder
                                .build_int_s_extend(end_val, i64_type, "ext_end")
                                .unwrap()
                        } else {
                            end_val
                        };
                        let call = self
                            .builder
                            .build_call(
                                self.runtime.str_substring_fn,
                                &[ptr.into(), start_i64.into(), end_i64.into()],
                                "substr_res",
                            )
                            .unwrap();
                        return Ok(call.try_as_basic_value().basic().unwrap());
                    }
                }

                // Built-in Show.show for primitives
                if method == "show" && args.is_empty() {
                    let recv_ty = self.get_atom_type(receiver);
                    let recv_val = self.eval_atom(receiver)?;

                    // Bool -> "true" | "false"
                    let is_bool = recv_ty.as_ref().map(|t| t.is_bool()).unwrap_or(false)
                        || (recv_val.is_int_value()
                            && recv_val.into_int_value().get_type().get_bit_width() == 1);
                    if is_bool {
                        let bool_val = if recv_val.into_int_value().get_type().get_bit_width() == 1
                        {
                            recv_val.into_int_value()
                        } else {
                            self.builder
                                .build_int_compare(
                                    inkwell::IntPredicate::NE,
                                    recv_val.into_int_value(),
                                    recv_val.into_int_value().get_type().const_int(0, false),
                                    "to_bool",
                                )
                                .unwrap()
                        };
                        let true_str = self.get_or_create_string_literal("true");
                        let false_str = self.get_or_create_string_literal("false");
                        let res = self
                            .builder
                            .build_select(
                                bool_val,
                                true_str.as_basic_value_enum(),
                                false_str.as_basic_value_enum(),
                                "bool_show",
                            )
                            .unwrap();
                        return Ok(res);
                    }

                    // String -> identity
                    let is_string = recv_ty.as_ref().map(|t| t.is_string()).unwrap_or(false)
                        || matches!(receiver, Atom::Literal(crate::ast::Literal::String(_)));
                    if is_string && recv_val.is_pointer_value() {
                        return Ok(recv_val);
                    }

                    // Integers -> snprintf "%ld" or "%lu" into Modus string buffer
                    if recv_val.is_int_value() {
                        let int_val = recv_val.into_int_value();
                        let i64_type = self.context.i64_type();
                        let is_unsigned = recv_ty
                            .as_ref()
                            .map(|t| t.is_unsigned_integer())
                            .unwrap_or(false)
                            || matches!(receiver, Atom::Literal(crate::ast::Literal::UInt(_)));
                        let ext_val = if is_unsigned {
                            self.builder
                                .build_int_z_extend(int_val, i64_type, "zext")
                                .unwrap()
                        } else {
                            self.builder
                                .build_int_s_extend(int_val, i64_type, "sext")
                                .unwrap()
                        };
                        let fmt = if is_unsigned { "%lu" } else { "%ld" };
                        let fmt_str = self
                            .builder
                            .build_global_string_ptr(fmt, "fmt_int")
                            .unwrap();
                        let cap_val = 32u64;
                        let alloc_bytes = i64_type.const_int(24 + cap_val + 1, false);
                        let buf_call = self
                            .builder
                            .build_call(self.runtime.alloc_fn, &[alloc_bytes.into()], "int_str")
                            .unwrap();
                        let buf = buf_call
                            .try_as_basic_value()
                            .basic()
                            .unwrap()
                            .into_pointer_value();

                        let data_ptr = unsafe {
                            self.builder
                                .build_gep(
                                    self.context.i8_type(),
                                    buf,
                                    &[i64_type.const_int(24, false)],
                                    "data_ptr",
                                )
                                .unwrap()
                        };
                        let snp_call = self
                            .builder
                            .build_call(
                                self.runtime.snprintf_fn,
                                &[
                                    data_ptr.into(),
                                    i64_type.const_int(cap_val + 1, false).into(),
                                    fmt_str.as_basic_value_enum().into(),
                                    ext_val.into(),
                                ],
                                "snp",
                            )
                            .unwrap();
                        let written = snp_call
                            .try_as_basic_value()
                            .basic()
                            .unwrap()
                            .into_int_value();
                        let len_i64 = self
                            .builder
                            .build_int_s_extend(written, i64_type, "len_i64")
                            .unwrap();

                        let len_ptr = unsafe {
                            self.builder
                                .build_gep(
                                    i64_type,
                                    buf,
                                    &[i64_type.const_int(1, false)],
                                    "len_ptr",
                                )
                                .unwrap()
                        };
                        let _ = self.builder.build_store(len_ptr, len_i64);
                        let cap_ptr = unsafe {
                            self.builder
                                .build_gep(
                                    i64_type,
                                    buf,
                                    &[i64_type.const_int(2, false)],
                                    "cap_ptr",
                                )
                                .unwrap()
                        };
                        let _ = self
                            .builder
                            .build_store(cap_ptr, i64_type.const_int(cap_val, false));

                        return Ok(buf.into());
                    }

                    // Floats -> snprintf "%g" into Modus string buffer
                    if recv_val.is_float_value() {
                        let flt_val = recv_val.into_float_value();
                        let f64_type = self.context.f64_type();
                        let ext_val = if flt_val.get_type() == self.context.f32_type() {
                            self.builder
                                .build_float_ext(flt_val, f64_type, "fpext")
                                .unwrap()
                        } else {
                            flt_val
                        };
                        let fmt_str = self
                            .builder
                            .build_global_string_ptr("%g", "fmt_float")
                            .unwrap();
                        let cap_val = 64u64;
                        let i64_type = self.context.i64_type();
                        let alloc_bytes = i64_type.const_int(24 + cap_val + 1, false);
                        let buf_call = self
                            .builder
                            .build_call(self.runtime.alloc_fn, &[alloc_bytes.into()], "flt_str")
                            .unwrap();
                        let buf = buf_call
                            .try_as_basic_value()
                            .basic()
                            .unwrap()
                            .into_pointer_value();

                        let data_ptr = unsafe {
                            self.builder
                                .build_gep(
                                    self.context.i8_type(),
                                    buf,
                                    &[i64_type.const_int(24, false)],
                                    "data_ptr",
                                )
                                .unwrap()
                        };
                        let snp_call = self
                            .builder
                            .build_call(
                                self.runtime.snprintf_fn,
                                &[
                                    data_ptr.into(),
                                    i64_type.const_int(cap_val + 1, false).into(),
                                    fmt_str.as_basic_value_enum().into(),
                                    ext_val.into(),
                                ],
                                "snp",
                            )
                            .unwrap();
                        let written = snp_call
                            .try_as_basic_value()
                            .basic()
                            .unwrap()
                            .into_int_value();
                        let len_i64 = self
                            .builder
                            .build_int_s_extend(written, i64_type, "len_i64")
                            .unwrap();

                        let len_ptr = unsafe {
                            self.builder
                                .build_gep(
                                    i64_type,
                                    buf,
                                    &[i64_type.const_int(1, false)],
                                    "len_ptr",
                                )
                                .unwrap()
                        };
                        let _ = self.builder.build_store(len_ptr, len_i64);
                        let cap_ptr = unsafe {
                            self.builder
                                .build_gep(
                                    i64_type,
                                    buf,
                                    &[i64_type.const_int(2, false)],
                                    "cap_ptr",
                                )
                                .unwrap()
                        };
                        let _ = self
                            .builder
                            .build_store(cap_ptr, i64_type.const_int(cap_val, false));

                        return Ok(buf.into());
                    }
                }

                // Standard method call: lookup or declare method function
                let fn_val = match self.functions.get(method).copied() {
                    Some(f) => f,
                    None => {
                        let mut param_tys: Vec<BasicMetadataTypeEnum<'ctx>> = Vec::new();
                        param_tys.push(self.context.ptr_type(AddressSpace::default()).into());
                        for _ in args {
                            param_tys.push(self.context.ptr_type(AddressSpace::default()).into());
                        }
                        let fn_ty = self
                            .context
                            .ptr_type(AddressSpace::default())
                            .fn_type(&param_tys, false);
                        let f = self.module.add_function(method, fn_ty, None);
                        f.set_call_conventions(8);
                        self.functions.insert(method.clone(), f);
                        f
                    }
                };

                let mut llvm_args: Vec<BasicMetadataValueEnum<'ctx>> = Vec::new();
                let recv_val = self.eval_atom(receiver)?;
                let recv_target = fn_val
                    .get_nth_param(0)
                    .map(|p| p.get_type())
                    .unwrap_or(recv_val.get_type());
                llvm_args.push(self.coerce_to_type(recv_val, recv_target)?.into());
                for (i, arg) in args.iter().enumerate() {
                    let arg_val = self.eval_atom(arg)?;
                    let target_ty = fn_val
                        .get_nth_param((i + 1) as u32)
                        .map(|p| p.get_type())
                        .unwrap_or(arg_val.get_type());
                    llvm_args.push(self.coerce_to_type(arg_val, target_ty)?.into());
                }

                let call_site = self
                    .builder
                    .build_call(fn_val, &llvm_args, "mcall")
                    .unwrap();
                call_site.set_call_convention(fn_val.get_call_conventions());

                Ok(call_site
                    .try_as_basic_value()
                    .basic()
                    .unwrap_or_else(|| self.context.i8_type().const_int(0, false).into()))
            }

            AnfExpr::FieldAccess { receiver, field } => {
                // Check if this is a nullary union constructor like Option.None, List.Nil
                if let Atom::Var(r) = receiver {
                    let qualified = format!("{r}.{field}");
                    if let Some(&tag) = self.union_variants.get(&qualified).or_else(|| {
                        if r == "Option" || r == "Result" || r == "List" {
                            self.union_variants.get(field)
                        } else {
                            None
                        }
                    }) {
                        return self.build_variant_constructor(tag, &[]);
                    }
                }

                let recv_val = self.eval_atom(receiver)?;
                let ptr = if recv_val.is_pointer_value() {
                    recv_val.into_pointer_value()
                } else if recv_val.is_int_value() {
                    self.builder
                        .build_int_to_ptr(
                            recv_val.into_int_value(),
                            self.context.ptr_type(inkwell::AddressSpace::default()),
                            "recv_ptr",
                        )
                        .unwrap()
                } else {
                    return Err(format!("Field access on non-pointer: {field}"));
                };

                // Look up field index
                let field_idx = if let Some(var_name) = receiver.as_var() {
                    self.record_field_indices
                        .get(var_name)
                        .and_then(|m| m.get(field).copied())
                        .unwrap_or_else(|| self.get_field_index(receiver, field))
                } else {
                    self.get_field_index(receiver, field)
                };

                let elem_ty = self.type_lowerer.llvm_type(ty);
                let field_gep = unsafe {
                    self.builder
                        .build_gep(
                            self.context.i64_type(),
                            ptr,
                            &[self.context.i64_type().const_int(field_idx as u64, false)],
                            "gep",
                        )
                        .unwrap()
                };
                let loaded = self.builder.build_load(elem_ty, field_gep, "fld").unwrap();
                Ok(loaded)
            }

            AnfExpr::Record { fields } => {
                // Allocate buffer with `modus_alloc`:
                // size = 8 (u64 rc) + fields.len() * 8 bytes
                let size_bytes = (1 + fields.len()) * 8;
                let size_val = self.context.i64_type().const_int(size_bytes as u64, false);
                let alloc_call = self
                    .builder
                    .build_call(self.runtime.alloc_fn, &[size_val.into()], "rec")
                    .unwrap();
                let rec_ptr = alloc_call
                    .try_as_basic_value()
                    .basic()
                    .unwrap()
                    .into_pointer_value();

                // Store fields at index 1, 2, ...
                let mut idx_map = BTreeMap::new();
                for (i, (f_name, atom)) in fields.iter().enumerate() {
                    let field_idx = (i + 1) as u32;
                    idx_map.insert(f_name.clone(), field_idx);

                    let f_val = self.eval_atom(atom)?;
                    let field_ptr = unsafe {
                        self.builder
                            .build_gep(
                                self.context.i64_type(),
                                rec_ptr,
                                &[self.context.i64_type().const_int(field_idx as u64, false)],
                                "fld_ptr",
                            )
                            .unwrap()
                    };
                    let _ = self.builder.build_store(field_ptr, f_val);
                }

                Ok(rec_ptr.into())
            }

            AnfExpr::Array { elements } => {
                // Array buffer: { i64 rc, i64 len, i64 cap, ptr data }
                let size_bytes = (4 + elements.len()) * 8;
                let size_val = self.context.i64_type().const_int(size_bytes as u64, false);
                let alloc_call = self
                    .builder
                    .build_call(self.runtime.alloc_fn, &[size_val.into()], "arr")
                    .unwrap();
                let arr_ptr = alloc_call
                    .try_as_basic_value()
                    .basic()
                    .unwrap()
                    .into_pointer_value();

                // Store length at index 1
                let len_val = self
                    .context
                    .i64_type()
                    .const_int(elements.len() as u64, false);
                let len_ptr = unsafe {
                    self.builder
                        .build_gep(
                            self.context.i64_type(),
                            arr_ptr,
                            &[self.context.i64_type().const_int(1, false)],
                            "len_ptr",
                        )
                        .unwrap()
                };
                let _ = self.builder.build_store(len_ptr, len_val);

                // Store cap at index 2
                let cap_ptr = unsafe {
                    self.builder
                        .build_gep(
                            self.context.i64_type(),
                            arr_ptr,
                            &[self.context.i64_type().const_int(2, false)],
                            "cap_ptr",
                        )
                        .unwrap()
                };
                let _ = self.builder.build_store(cap_ptr, len_val);

                // Store 0 at index 3
                let res_ptr = unsafe {
                    self.builder
                        .build_gep(
                            self.context.i64_type(),
                            arr_ptr,
                            &[self.context.i64_type().const_int(3, false)],
                            "res_ptr",
                        )
                        .unwrap()
                };
                let _ = self
                    .builder
                    .build_store(res_ptr, self.context.i64_type().const_int(0, false));

                // Store elements
                for (i, el) in elements.iter().enumerate() {
                    let el_val = self.eval_atom(el)?;
                    let el_ptr = unsafe {
                        self.builder
                            .build_gep(
                                self.context.i64_type(),
                                arr_ptr,
                                &[self.context.i64_type().const_int((4 + i) as u64, false)],
                                "el_ptr",
                            )
                            .unwrap()
                    };
                    let _ = self.builder.build_store(el_ptr, el_val);
                }

                Ok(arr_ptr.into())
            }

            AnfExpr::Tuple { elements } => {
                let size_bytes = (1 + elements.len()) * 8;
                let size_val = self.context.i64_type().const_int(size_bytes as u64, false);
                let alloc_call = self
                    .builder
                    .build_call(self.runtime.alloc_fn, &[size_val.into()], "tup")
                    .unwrap();
                let tup_ptr = alloc_call
                    .try_as_basic_value()
                    .basic()
                    .unwrap()
                    .into_pointer_value();

                for (i, el) in elements.iter().enumerate() {
                    let el_val = self.eval_atom(el)?;
                    let el_ptr = unsafe {
                        self.builder
                            .build_gep(
                                self.context.i64_type(),
                                tup_ptr,
                                &[self.context.i64_type().const_int((1 + i) as u64, false)],
                                "el_ptr",
                            )
                            .unwrap()
                    };
                    let _ = self.builder.build_store(el_ptr, el_val);
                }

                Ok(tup_ptr.into())
            }

            AnfExpr::MakeClosure { fn_name, env } => {
                // Closure struct layout: { i64 rc, ptr fn_ptr, ptr env_ptr }
                let size_val = self.context.i64_type().const_int(24, false);
                let alloc_call = self
                    .builder
                    .build_call(self.runtime.alloc_fn, &[size_val.into()], "cls")
                    .unwrap();
                let cls_ptr = alloc_call
                    .try_as_basic_value()
                    .basic()
                    .unwrap()
                    .into_pointer_value();

                let fn_val = self
                    .functions
                    .get(fn_name)
                    .copied()
                    .ok_or_else(|| format!("Unknown closure function: {fn_name}"))?;

                // Store fn_ptr at index 1
                let fn_slot = unsafe {
                    self.builder
                        .build_gep(
                            self.context.i64_type(),
                            cls_ptr,
                            &[self.context.i64_type().const_int(1, false)],
                            "fn_slot",
                        )
                        .unwrap()
                };
                let _ = self
                    .builder
                    .build_store(fn_slot, fn_val.as_global_value().as_pointer_value());

                // Store env_ptr at index 2
                let env_ptr = if let Some(e) = env {
                    self.eval_atom(e)?
                } else {
                    self.context
                        .ptr_type(AddressSpace::default())
                        .const_null()
                        .into()
                };

                let env_slot = unsafe {
                    self.builder
                        .build_gep(
                            self.context.i64_type(),
                            cls_ptr,
                            &[self.context.i64_type().const_int(2, false)],
                            "env_slot",
                        )
                        .unwrap()
                };
                let _ = self.builder.build_store(env_slot, env_ptr);

                Ok(cls_ptr.into())
            }

            AnfExpr::CallClosure { closure, args } => {
                let cls_val = self.eval_atom(closure)?;
                let cls_ptr = cls_val.into_pointer_value();

                // Load fn_ptr
                let fn_slot = unsafe {
                    self.builder
                        .build_gep(
                            self.context.i64_type(),
                            cls_ptr,
                            &[self.context.i64_type().const_int(1, false)],
                            "fn_slot",
                        )
                        .unwrap()
                };
                let fn_ptr = self
                    .builder
                    .build_load(
                        self.context.ptr_type(AddressSpace::default()),
                        fn_slot,
                        "fn_ptr",
                    )
                    .unwrap()
                    .into_pointer_value();

                // Load env_ptr
                let env_slot = unsafe {
                    self.builder
                        .build_gep(
                            self.context.i64_type(),
                            cls_ptr,
                            &[self.context.i64_type().const_int(2, false)],
                            "env_slot",
                        )
                        .unwrap()
                };
                let env_ptr = self
                    .builder
                    .build_load(
                        self.context.ptr_type(AddressSpace::default()),
                        env_slot,
                        "env_ptr",
                    )
                    .unwrap();

                let mut call_args: Vec<BasicMetadataValueEnum<'ctx>> = vec![env_ptr.into()];
                let mut param_types: Vec<inkwell::types::BasicMetadataTypeEnum<'ctx>> =
                    vec![self.context.ptr_type(AddressSpace::default()).into()];
                for arg in args {
                    let arg_val = self.eval_atom(arg)?;
                    param_types.push(arg_val.get_type().into());
                    call_args.push(arg_val.into());
                }

                let ret_ty = self.type_lowerer.llvm_type(ty);
                let fn_type = ret_ty.fn_type(&param_types, false);

                let call_site = self
                    .builder
                    .build_indirect_call(fn_type, fn_ptr, &call_args, "cls_call")
                    .unwrap();
                call_site.set_call_convention(8);

                Ok(call_site
                    .try_as_basic_value()
                    .basic()
                    .unwrap_or_else(|| self.context.i8_type().const_int(0, false).into()))
            }

            AnfExpr::ReuseRecord { base, fields } => {
                // FBIP: Reuses `base` buffer if rc == 1, else allocates new
                let base_val = self.eval_atom(base)?;
                let base_ptr = base_val.into_pointer_value();

                // Test is_unique(base)
                let is_uniq_call = self
                    .builder
                    .build_call(self.runtime.is_unique_fn, &[base_ptr.into()], "is_uniq")
                    .unwrap();
                let is_uniq = is_uniq_call
                    .try_as_basic_value()
                    .basic()
                    .unwrap()
                    .into_int_value();

                let cur_fn = self.current_fn.unwrap();
                let inplace_bb = self.context.append_basic_block(cur_fn, "fbip_inplace");
                let alloc_bb = self.context.append_basic_block(cur_fn, "fbip_alloc");
                let merge_bb = self.context.append_basic_block(cur_fn, "fbip_merge");

                let _ = self
                    .builder
                    .build_conditional_branch(is_uniq, inplace_bb, alloc_bb);

                // In-place path: mutate specified fields directly in base_ptr
                self.builder.position_at_end(inplace_bb);
                for (f_name, atom) in fields {
                    let f_val = self.eval_atom(atom)?;
                    let f_idx = self.get_field_index(base, f_name);
                    let f_ptr = unsafe {
                        self.builder
                            .build_gep(
                                self.context.i64_type(),
                                base_ptr,
                                &[self.context.i64_type().const_int(f_idx as u64, false)],
                                "f_ptr",
                            )
                            .unwrap()
                    };
                    let _ = self.builder.build_store(f_ptr, f_val);
                }
                let _ = self.builder.build_unconditional_branch(merge_bb);

                // Alloc path: allocate new record and dec_ref old
                self.builder.position_at_end(alloc_bb);
                let size_bytes = 24; // 8 rc + 16 payload
                let size_val = self.context.i64_type().const_int(size_bytes, false);
                let new_alloc = self
                    .builder
                    .build_call(self.runtime.alloc_fn, &[size_val.into()], "new_rec")
                    .unwrap();
                let new_ptr = new_alloc
                    .try_as_basic_value()
                    .basic()
                    .unwrap()
                    .into_pointer_value();

                for (f_name, atom) in fields {
                    let f_val = self.eval_atom(atom)?;
                    let f_idx = self.get_field_index(base, f_name);
                    let f_ptr = unsafe {
                        self.builder
                            .build_gep(
                                self.context.i64_type(),
                                new_ptr,
                                &[self.context.i64_type().const_int(f_idx as u64, false)],
                                "f_ptr",
                            )
                            .unwrap()
                    };
                    let _ = self.builder.build_store(f_ptr, f_val);
                }
                let _ = self
                    .builder
                    .build_call(self.runtime.dec_ref_fn, &[base_ptr.into()], "");
                let _ = self.builder.build_unconditional_branch(merge_bb);

                // Merge with phi
                self.builder.position_at_end(merge_bb);
                let phi = self
                    .builder
                    .build_phi(self.context.ptr_type(AddressSpace::default()), "fbip_res")
                    .unwrap();
                phi.add_incoming(&[(&base_ptr, inplace_bb), (&new_ptr, alloc_bb)]);

                Ok(phi.as_basic_value())
            }

            AnfExpr::IsUnique(atom) => {
                let val = self.eval_atom(atom)?;
                let ptr = val.into_pointer_value();
                let call = self
                    .builder
                    .build_call(self.runtime.is_unique_fn, &[ptr.into()], "uniq")
                    .unwrap();
                Ok(call.try_as_basic_value().basic().unwrap())
            }

            AnfExpr::TupleAccess { receiver, index } => {
                let recv_val = self.eval_atom(receiver)?;
                let ptr = recv_val.into_pointer_value();
                let elem_ptr = unsafe {
                    self.builder
                        .build_gep(
                            self.context.i64_type(),
                            ptr,
                            &[self.context.i64_type().const_int((1 + index) as u64, false)],
                            "tup_gep",
                        )
                        .unwrap()
                };
                let llvm_ty = self.type_lowerer.llvm_type(ty);
                let loaded = self
                    .builder
                    .build_load(llvm_ty, elem_ptr, "tup_elem")
                    .unwrap();
                Ok(loaded)
            }

            AnfExpr::Index { receiver, index } => {
                let recv_val = self.eval_atom(receiver)?;
                let ptr = recv_val.into_pointer_value();
                let idx_val = self.eval_atom(index)?;
                let idx_int = self
                    .coerce_to_type(idx_val, self.context.i64_type().into())?
                    .into_int_value();
                let offset = self
                    .builder
                    .build_int_add(
                        idx_int,
                        self.context.i64_type().const_int(4, false),
                        "arr_off",
                    )
                    .unwrap();
                let elem_ptr = unsafe {
                    self.builder
                        .build_gep(self.context.i64_type(), ptr, &[offset], "arr_gep")
                        .unwrap()
                };
                let llvm_ty = self.type_lowerer.llvm_type(ty);
                let loaded = self
                    .builder
                    .build_load(llvm_ty, elem_ptr, "arr_elem")
                    .unwrap();
                Ok(loaded)
            }

            AnfExpr::Variant { variant, args, .. } => {
                let tag = if variant == "Ok" || variant == "Some" {
                    0
                } else {
                    1
                };
                self.build_variant_constructor(tag, args)
            }

            AnfExpr::If {
                cond,
                then_branch,
                else_branch,
            } => {
                let cond_val = self.eval_atom(cond)?;
                let cond_int = cond_val.into_int_value();
                let cond_i1 = if cond_int.get_type().get_bit_width() == 1 {
                    cond_int
                } else {
                    self.builder
                        .build_int_compare(
                            inkwell::IntPredicate::NE,
                            cond_int,
                            cond_int.get_type().const_zero(),
                            "cond_i1",
                        )
                        .unwrap()
                };

                let cur_fn = self.current_fn.unwrap();
                let then_bb = self.context.append_basic_block(cur_fn, "if_expr_then");
                let else_bb = self.context.append_basic_block(cur_fn, "if_expr_else");
                let merge_bb = self.context.append_basic_block(cur_fn, "if_expr_merge");

                let _ = self
                    .builder
                    .build_conditional_branch(cond_i1, then_bb, else_bb);

                let llvm_ty = self.type_lowerer.llvm_type(ty);
                let mut incoming = Vec::new();

                // Compile then branch
                self.builder.position_at_end(then_bb);
                self.compile_block_into_merge(then_branch, merge_bb, llvm_ty, &mut incoming)?;

                // Compile else branch
                self.builder.position_at_end(else_bb);
                self.compile_block_into_merge(else_branch, merge_bb, llvm_ty, &mut incoming)?;

                self.builder.position_at_end(merge_bb);

                if ty.is_void() || incoming.is_empty() {
                    return Ok(self.context.i8_type().const_int(0, false).into());
                }

                let phi = self.builder.build_phi(llvm_ty, "if_expr_res").unwrap();
                for (val, bb) in &incoming {
                    phi.add_incoming(&[(val as &dyn inkwell::values::BasicValue<'ctx>, *bb)]);
                }

                Ok(phi.as_basic_value())
            }

            AnfExpr::Match { scrutinee, arms } => {
                let cur_fn = self.current_fn.unwrap();
                let merge_bb = self.context.append_basic_block(cur_fn, "match_expr_merge");
                let llvm_ty = self.type_lowerer.llvm_type(ty);
                let mut incoming = Vec::new();

                self.compile_match_into_merge(scrutinee, arms, merge_bb, llvm_ty, &mut incoming)?;

                self.builder.position_at_end(merge_bb);
                if ty.is_void() || incoming.is_empty() {
                    return Ok(self.context.i8_type().const_int(0, false).into());
                }

                let phi = self.builder.build_phi(llvm_ty, "match_expr_res").unwrap();
                for (val, bb) in &incoming {
                    phi.add_incoming(&[(val as &dyn inkwell::values::BasicValue<'ctx>, *bb)]);
                }
                Ok(phi.as_basic_value())
            }

            AnfExpr::Cast { expr, target_type } => {
                let val = self.eval_atom(expr)?;
                let src_ty = self.get_atom_type(expr);
                self.compile_cast(val, src_ty.as_ref(), target_type)
            }

            _ => Ok(self.context.i64_type().const_int(0, false).into()),
        }
    }

    pub(crate) fn compile_match_into_merge(
        &mut self,
        scrutinee: &Atom,
        arms: &[AnfMatchArm],
        merge_bb: inkwell::basic_block::BasicBlock<'ctx>,
        target_ty: inkwell::types::BasicTypeEnum<'ctx>,
        incoming: &mut Vec<(BasicValueEnum<'ctx>, inkwell::basic_block::BasicBlock<'ctx>)>,
    ) -> Result<(), String> {
        let sc_val = self.eval_atom(scrutinee)?;
        let cur_fn = self.current_fn.unwrap();

        for (i, arm) in arms.iter().enumerate() {
            let is_last = i == arms.len() - 1;
            let arm_bb = self
                .context
                .append_basic_block(cur_fn, &format!("match_arm_{i}"));
            let next_bb = if !is_last {
                Some(
                    self.context
                        .append_basic_block(cur_fn, &format!("match_next_{i}")),
                )
            } else {
                None
            };

            match &arm.pattern {
                crate::desugar::DesugaredPattern::Wildcard
                | crate::desugar::DesugaredPattern::Ident(_) => {
                    let _ = self.builder.build_unconditional_branch(arm_bb);
                }
                crate::desugar::DesugaredPattern::Literal(lit) => {
                    let lit_atom = Atom::Literal(lit.clone());
                    let lit_val = self.eval_atom(&lit_atom)?;
                    let eq_val = self.compile_binary_op(BinaryOp::Eq, sc_val, lit_val)?;
                    let cond_i1 = self
                        .coerce_to_type(eq_val, self.context.bool_type().into())?
                        .into_int_value();
                    let target_next = next_bb.unwrap_or(arm_bb);
                    let _ = self
                        .builder
                        .build_conditional_branch(cond_i1, arm_bb, target_next);
                }
                crate::desugar::DesugaredPattern::Variant { variant, .. } => {
                    let sc_ptr = if sc_val.is_pointer_value() {
                        sc_val.into_pointer_value()
                    } else {
                        self.builder
                            .build_int_to_ptr(
                                sc_val.into_int_value(),
                                self.context.ptr_type(inkwell::AddressSpace::default()),
                                "sc_ptr",
                            )
                            .unwrap()
                    };
                    let tag_ptr = unsafe {
                        self.builder
                            .build_gep(
                                self.context.i64_type(),
                                sc_ptr,
                                &[self.context.i64_type().const_int(1, false)],
                                "tag_ptr",
                            )
                            .unwrap()
                    };
                    let tag = self
                        .builder
                        .build_load(self.context.i64_type(), tag_ptr, "tag")
                        .unwrap()
                        .into_int_value();
                    let exp_tag_val = if let Some(&tag) = self.union_variants.get(variant) {
                        tag
                    } else if variant == "Ok"
                        || variant == "Some"
                        || variant == "CircleShape"
                        || variant == "Cons"
                    {
                        0
                    } else if variant == "Err"
                        || variant == "None"
                        || variant == "RectShape"
                        || variant == "Nil"
                    {
                        1
                    } else {
                        2
                    };
                    let exp_tag = self.context.i64_type().const_int(exp_tag_val, false);
                    let eq_tag = self
                        .builder
                        .build_int_compare(inkwell::IntPredicate::EQ, tag, exp_tag, "eq_tag")
                        .unwrap();
                    let target_next = next_bb.unwrap_or(arm_bb);
                    let _ = self
                        .builder
                        .build_conditional_branch(eq_tag, arm_bb, target_next);
                }
                _ => {
                    let _ = self.builder.build_unconditional_branch(arm_bb);
                }
            }

            // Position in arm_bb and bind pattern variables
            self.builder.position_at_end(arm_bb);
            let sc_ty = self.get_atom_type(scrutinee);
            match &arm.pattern {
                crate::desugar::DesugaredPattern::Ident(name) => {
                    self.variables.insert(name.clone(), sc_val);
                    if let Some(t) = &sc_ty {
                        self.var_types.insert(name.clone(), t.clone());
                    }
                }
                crate::desugar::DesugaredPattern::Variant {
                    variant, patterns, ..
                } => {
                    let sc_ptr = if sc_val.is_pointer_value() {
                        sc_val.into_pointer_value()
                    } else {
                        self.builder
                            .build_int_to_ptr(
                                sc_val.into_int_value(),
                                self.context.ptr_type(inkwell::AddressSpace::default()),
                                "sc_ptr",
                            )
                            .unwrap()
                    };
                    for (j, pat) in patterns.iter().enumerate() {
                        if let crate::desugar::DesugaredPattern::Ident(v) = pat {
                            let elem_ty = if let Some(Type::Named { name, args }) = &sc_ty {
                                if name == "Result" {
                                    if variant == "Ok" && !args.is_empty() {
                                        Some(args[0].clone())
                                    } else if variant == "Err" && args.len() > 1 {
                                        Some(args[1].clone())
                                    } else {
                                        None
                                    }
                                } else if name == "Option" && variant == "Some" && !args.is_empty()
                                {
                                    Some(args[0].clone())
                                } else if name == "List" && variant == "Cons" {
                                    let t_elem = args.first().cloned().unwrap_or(Type::i64());
                                    let mut rec = BTreeMap::new();
                                    rec.insert("head".to_string(), t_elem.clone());
                                    rec.insert(
                                        "tail".to_string(),
                                        Type::Named {
                                            name: "List".to_string(),
                                            args: vec![t_elem],
                                        },
                                    );
                                    Some(Type::Record(rec))
                                } else {
                                    None
                                }
                            } else {
                                None
                            };

                            let llvm_load_ty = if let Some(t) = &elem_ty {
                                self.type_lowerer.llvm_type(t)
                            } else {
                                self.context.i64_type().into()
                            };

                            let field_ptr = unsafe {
                                self.builder
                                    .build_gep(
                                        self.context.i64_type(),
                                        sc_ptr,
                                        &[self.context.i64_type().const_int((2 + j) as u64, false)],
                                        "payload_gep",
                                    )
                                    .unwrap()
                            };
                            let f_val = self
                                .builder
                                .build_load(llvm_load_ty, field_ptr, "f_val")
                                .unwrap();
                            self.variables.insert(v.clone(), f_val);

                            if let Some(t) = elem_ty {
                                self.var_types.insert(v.clone(), t);
                            }
                        }
                    }
                }
                crate::desugar::DesugaredPattern::Record(fields) => {
                    let sc_ptr = if sc_val.is_pointer_value() {
                        sc_val.into_pointer_value()
                    } else {
                        self.builder
                            .build_int_to_ptr(
                                sc_val.into_int_value(),
                                self.context.ptr_type(inkwell::AddressSpace::default()),
                                "sc_ptr",
                            )
                            .unwrap()
                    };
                    for (f_name, opt_pat) in fields {
                        if let Some(crate::desugar::DesugaredPattern::Ident(v)) = opt_pat {
                            let f_idx = self.get_field_index(scrutinee, f_name);
                            let field_mod_ty = if let Some(Type::Record(flds)) = &sc_ty {
                                flds.iter()
                                    .find(|(n, _)| n.as_str() == f_name.as_str())
                                    .map(|(_, fty)| fty.clone())
                            } else {
                                None
                            };
                            let llvm_load_ty = if let Some(t) = &field_mod_ty {
                                self.type_lowerer.llvm_type(t)
                            } else {
                                self.context.i64_type().into()
                            };
                            let f_ptr = unsafe {
                                self.builder
                                    .build_gep(
                                        self.context.i64_type(),
                                        sc_ptr,
                                        &[self.context.i64_type().const_int(f_idx as u64, false)],
                                        "fld_ptr",
                                    )
                                    .unwrap()
                            };
                            let f_val = self
                                .builder
                                .build_load(llvm_load_ty, f_ptr, "fld_val")
                                .unwrap();
                            self.variables.insert(v.clone(), f_val);

                            if let Some(t) = field_mod_ty {
                                self.var_types.insert(v.clone(), t);
                            }
                        }
                    }
                }
                _ => {}
            }

            // Compile arm body into merge_bb
            self.compile_block_into_merge(&arm.body, merge_bb, target_ty, incoming)?;

            // Continue to next arm from next_bb
            if let Some(nb) = next_bb {
                self.builder.position_at_end(nb);
            }
        }

        Ok(())
    }

    pub(crate) fn compile_block_into_merge(
        &mut self,
        block: &AnfBlock,
        merge_bb: inkwell::basic_block::BasicBlock<'ctx>,
        target_ty: inkwell::types::BasicTypeEnum<'ctx>,
        incoming: &mut Vec<(BasicValueEnum<'ctx>, inkwell::basic_block::BasicBlock<'ctx>)>,
    ) -> Result<(), String> {
        for stmt in &block.stmts {
            self.compile_stmt(stmt)?;
        }

        match &block.tail {
            AnfTail::Atom(a) => {
                let v = self.eval_atom(a)?;
                let coerced = self.coerce_to_type(v, target_ty)?;
                let cur_bb = self.builder.get_insert_block().unwrap();
                let _ = self.builder.build_unconditional_branch(merge_bb);
                incoming.push((coerced, cur_bb));
            }
            AnfTail::Return(_) | AnfTail::TailCall { .. } => {
                self.compile_tail(&block.tail)?;
            }
            AnfTail::If {
                cond,
                then_branch,
                else_branch,
            } => {
                let cond_val = self.eval_atom(cond)?;
                let cond_int = cond_val.into_int_value();
                let cond_i1 = if cond_int.get_type().get_bit_width() == 1 {
                    cond_int
                } else {
                    self.builder
                        .build_int_compare(
                            inkwell::IntPredicate::NE,
                            cond_int,
                            cond_int.get_type().const_zero(),
                            "cond_i1",
                        )
                        .unwrap()
                };

                let cur_fn = self.current_fn.unwrap();
                let inner_then = self.context.append_basic_block(cur_fn, "inner_then");
                let inner_else = self.context.append_basic_block(cur_fn, "inner_else");

                let _ = self
                    .builder
                    .build_conditional_branch(cond_i1, inner_then, inner_else);

                self.builder.position_at_end(inner_then);
                self.compile_block_into_merge(then_branch, merge_bb, target_ty, incoming)?;

                self.builder.position_at_end(inner_else);
                if let Some(eb) = else_branch {
                    self.compile_block_into_merge(eb, merge_bb, target_ty, incoming)?;
                } else {
                    let cur_bb = self.builder.get_insert_block().unwrap();
                    let _ = self.builder.build_unconditional_branch(merge_bb);
                    let def_val = self.const_zero_for_type(target_ty);
                    incoming.push((def_val, cur_bb));
                }
            }
            AnfTail::Match { scrutinee, arms } => {
                self.compile_match_into_merge(scrutinee, arms, merge_bb, target_ty, incoming)?;
            }
        }
        Ok(())
    }

    fn const_zero_for_type(&self, ty: inkwell::types::BasicTypeEnum<'ctx>) -> BasicValueEnum<'ctx> {
        match ty {
            inkwell::types::BasicTypeEnum::IntType(it) => it.const_zero().into(),
            inkwell::types::BasicTypeEnum::FloatType(ft) => ft.const_zero().into(),
            inkwell::types::BasicTypeEnum::PointerType(pt) => pt.const_null().into(),
            _ => self.context.i64_type().const_zero().into(),
        }
    }
    pub(crate) fn build_variant_constructor(
        &mut self,
        tag: u64,
        args: &[Atom],
    ) -> Result<BasicValueEnum<'ctx>, String> {
        let size_bytes = (2 + args.len()) * 8;
        let size_val = self.context.i64_type().const_int(size_bytes as u64, false);
        let alloc_call = self
            .builder
            .build_call(self.runtime.alloc_fn, &[size_val.into()], "variant")
            .unwrap();
        let ptr = alloc_call
            .try_as_basic_value()
            .basic()
            .unwrap()
            .into_pointer_value();

        // Store tag at index 1
        let tag_val = self.context.i64_type().const_int(tag, false);
        let tag_ptr = unsafe {
            self.builder
                .build_gep(
                    self.context.i64_type(),
                    ptr,
                    &[self.context.i64_type().const_int(1, false)],
                    "tag_ptr",
                )
                .unwrap()
        };
        let _ = self.builder.build_store(tag_ptr, tag_val);

        // Store args at index 2..
        for (i, arg) in args.iter().enumerate() {
            let arg_val = self.eval_atom(arg)?;
            let field_ptr = unsafe {
                self.builder
                    .build_gep(
                        self.context.i64_type(),
                        ptr,
                        &[self.context.i64_type().const_int((2 + i) as u64, false)],
                        "payload_ptr",
                    )
                    .unwrap()
            };
            let _ = self.builder.build_store(field_ptr, arg_val);
        }

        Ok(ptr.into())
    }

    pub(crate) fn get_field_index(&self, base: &Atom, field: &str) -> u32 {
        if let Some(var_name) = base.as_var()
            && let Some(m) = self.record_field_indices.get(var_name)
            && let Some(&idx) = m.get(field)
        {
            return idx;
        }
        if let Some(recv_ty) = self.get_atom_type(base) {
            if let Type::Named { name, .. } = &recv_ty
                && let Some(m) = self.type_field_indices.get(name)
                && let Some(&idx) = m.get(field)
            {
                return idx;
            }
            if let Type::Record(flds) = &recv_ty {
                for (i, (f_name, _)) in flds.iter().enumerate() {
                    if f_name == field {
                        return (i + 1) as u32;
                    }
                }
            }
        }
        // Fallback default index
        match field {
            "head" | "x" | "first" | "radius" | "host" | "value" | "code" | "fd" | "size"
            | "read" => 1,
            "tail" | "y" | "second" | "w" | "port" | "message" | "path" | "is_file" | "write" => 2,
            "z" | "h" | "tls" | "is_dir" | "create" => 3,
            "append" | "modified_at" => 4,
            "truncate" => 5,
            _ => 1,
        }
    }

    pub(crate) fn get_atom_type(&self, atom: &Atom) -> Option<Type> {
        match atom {
            Atom::Var(name) => self.var_types.get(name).cloned(),
            Atom::Literal(lit) => match lit {
                crate::ast::Literal::Int(_) => Some(Type::i32()),
                crate::ast::Literal::UInt(_) => Some(Type::u64()),
                crate::ast::Literal::Float(_) => Some(Type::f64()),
                crate::ast::Literal::Bool(_) => Some(Type::bool()),
                crate::ast::Literal::String(_) => Some(Type::string()),
                crate::ast::Literal::Unit => Some(Type::void()),
            },
        }
    }

    pub(crate) fn compile_cast(
        &mut self,
        val: BasicValueEnum<'ctx>,
        src_ty: Option<&Type>,
        target_ty: &Type,
    ) -> Result<BasicValueEnum<'ctx>, String> {
        let dst_llvm_ty = self.type_lowerer.llvm_type(target_ty);

        // 1. Target is boolean
        if target_ty.is_bool() {
            if val.is_int_value() {
                let val_int = val.into_int_value();
                if val_int.get_type().get_bit_width() == 1 {
                    return Ok(val);
                }
                let zero = val_int.get_type().const_zero();
                let cmp = self
                    .builder
                    .build_int_compare(inkwell::IntPredicate::NE, val_int, zero, "cast_itob")
                    .unwrap();
                return Ok(cmp.into());
            } else if val.is_float_value() {
                let val_flt = val.into_float_value();
                let zero = val_flt.get_type().const_zero();
                let cmp = self
                    .builder
                    .build_float_compare(inkwell::FloatPredicate::ONE, val_flt, zero, "cast_ftob")
                    .unwrap();
                return Ok(cmp.into());
            } else if val.is_pointer_value() {
                let val_ptr = val.into_pointer_value();
                let val_int = self
                    .builder
                    .build_ptr_to_int(val_ptr, self.context.i64_type(), "ptr_int")
                    .unwrap();
                let cmp = self
                    .builder
                    .build_int_compare(
                        inkwell::IntPredicate::NE,
                        val_int,
                        self.context.i64_type().const_zero(),
                        "cast_ptob",
                    )
                    .unwrap();
                return Ok(cmp.into());
            }
        }

        // 2. Target is integer
        if dst_llvm_ty.is_int_type() {
            let dst_int = dst_llvm_ty.into_int_type();
            if val.is_int_value() {
                let val_int = val.into_int_value();
                let src_width = val_int.get_type().get_bit_width();
                let dst_width = dst_int.get_bit_width();
                if src_width > dst_width {
                    return Ok(self
                        .builder
                        .build_int_truncate(val_int, dst_int, "cast_trunc")
                        .unwrap()
                        .into());
                } else if src_width < dst_width {
                    let is_unsigned = src_ty
                        .map(|t| t.is_unsigned_integer() || t.is_bool())
                        .unwrap_or(false);
                    if is_unsigned {
                        return Ok(self
                            .builder
                            .build_int_z_extend(val_int, dst_int, "cast_zext")
                            .unwrap()
                            .into());
                    } else {
                        return Ok(self
                            .builder
                            .build_int_s_extend(val_int, dst_int, "cast_sext")
                            .unwrap()
                            .into());
                    }
                } else {
                    return Ok(val);
                }
            } else if val.is_float_value() {
                let val_flt = val.into_float_value();
                if target_ty.is_unsigned_integer() {
                    return Ok(self
                        .builder
                        .build_float_to_unsigned_int(val_flt, dst_int, "cast_ftoui")
                        .unwrap()
                        .into());
                } else {
                    return Ok(self
                        .builder
                        .build_float_to_signed_int(val_flt, dst_int, "cast_ftosi")
                        .unwrap()
                        .into());
                }
            } else if val.is_pointer_value() {
                return Ok(self
                    .builder
                    .build_ptr_to_int(val.into_pointer_value(), dst_int, "cast_ptoi")
                    .unwrap()
                    .into());
            }
        }

        // 3. Target is float
        if dst_llvm_ty.is_float_type() {
            let dst_flt = dst_llvm_ty.into_float_type();
            if val.is_float_value() {
                return Ok(self
                    .builder
                    .build_float_cast(val.into_float_value(), dst_flt, "cast_fcast")
                    .unwrap()
                    .into());
            } else if val.is_int_value() {
                let val_int = val.into_int_value();
                let is_unsigned = src_ty
                    .map(|t| t.is_unsigned_integer() || t.is_bool())
                    .unwrap_or(false);
                if is_unsigned {
                    return Ok(self
                        .builder
                        .build_unsigned_int_to_float(val_int, dst_flt, "cast_uitof")
                        .unwrap()
                        .into());
                } else {
                    return Ok(self
                        .builder
                        .build_signed_int_to_float(val_int, dst_flt, "cast_sitof")
                        .unwrap()
                        .into());
                }
            }
        }

        // 4. Target is pointer
        if dst_llvm_ty.is_pointer_type() {
            let dst_ptr = dst_llvm_ty.into_pointer_type();
            if val.is_int_value() {
                return Ok(self
                    .builder
                    .build_int_to_ptr(val.into_int_value(), dst_ptr, "cast_itop")
                    .unwrap()
                    .into());
            } else if val.is_pointer_value() {
                return Ok(val);
            }
        }

        Ok(val)
    }
}
