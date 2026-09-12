//! Expression code generation (literals, records, closures, arrays, FBIP).

use super::CodeGen;
use crate::ast::BinaryOp;
use crate::ir::node::*;
use crate::typechecker::Type;
use inkwell::AddressSpace;
use inkwell::types::{BasicMetadataTypeEnum, BasicType};
use inkwell::values::{BasicMetadataValueEnum, BasicValueEnum};
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
                if let Atom::Var(name) = callee {
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
                            f.set_call_conventions(8);
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

                // Discriminated union constructors: Result.Ok, Result.Err, Option.Some, Option.None
                if let Atom::Var(r) = receiver
                    && (r == "Result" || r == "Option")
                {
                    let tag = if method == "Ok" || method == "Some" {
                        0
                    } else {
                        1
                    };
                    return self.build_variant_constructor(tag, args);
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
                let recv_val = self.eval_atom(receiver)?;
                if !recv_val.is_pointer_value() {
                    return Err(format!("Field access on non-pointer: {field}"));
                }
                let ptr = recv_val.into_pointer_value();

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

            _ => Ok(self.context.i64_type().const_int(0, false).into()),
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
        // Fallback default index
        match field {
            "x" | "first" | "radius" | "host" | "value" => 1,
            "y" | "second" | "w" | "port" => 2,
            "z" | "h" | "tls" => 3,
            _ => 1,
        }
    }
}
