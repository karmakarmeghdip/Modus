//! Block terminators, tail call elimination, and pattern matching.

use super::CodeGen;
use crate::ast::BinaryOp;
use crate::ir::node::*;
use crate::typechecker::Type;
use inkwell::AddressSpace;
use inkwell::IntPredicate;
use inkwell::types::BasicMetadataTypeEnum;
use inkwell::values::BasicMetadataValueEnum;

impl<'ctx> CodeGen<'ctx> {
    /// Compiles an ANF block terminator.
    pub(crate) fn compile_tail(&mut self, tail: &AnfTail) -> Result<(), String> {
        match tail {
            AnfTail::Return(Some(atom)) => {
                let val = self.eval_atom(atom)?;
                self.build_typed_return(Some(val))?;
            }
            AnfTail::Return(None) => {
                self.build_typed_return(None)?;
            }

            AnfTail::TailCall { callee, args } => {
                if let Atom::Var(name) = callee {
                    if name == "Ok" || name == "Some" {
                        let val = self.build_variant_constructor(0, args)?;
                        return self.build_typed_return(Some(val));
                    }
                    if name == "Err" || name == "None" {
                        let val = self.build_variant_constructor(1, args)?;
                        return self.build_typed_return(Some(val));
                    }
                    if self.variables.contains_key(name) {
                        let expr = AnfExpr::CallClosure {
                            closure: callee.clone(),
                            args: args.clone(),
                        };
                        let ret_ty = self.current_fn_ret.clone().unwrap_or_else(Type::void);
                        let val = self.compile_expr(&expr, &ret_ty)?;
                        return self.build_typed_return(Some(val));
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

                let call_site = self
                    .builder
                    .build_call(fn_val, &llvm_args, "tailcall")
                    .unwrap();
                call_site.set_call_convention(fn_val.get_call_conventions());
                // Direct recursion tail call optimization (`musttail`)
                call_site.set_tail_call(true);

                let ret_val = call_site.try_as_basic_value().basic();
                self.build_typed_return(ret_val)?;
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
                            IntPredicate::NE,
                            cond_int,
                            cond_int.get_type().const_zero(),
                            "cond_i1",
                        )
                        .unwrap()
                };

                let cur_fn = self.current_fn.unwrap();
                let then_bb = self.context.append_basic_block(cur_fn, "then");
                let else_bb = self.context.append_basic_block(cur_fn, "else");

                let _ = self
                    .builder
                    .build_conditional_branch(cond_i1, then_bb, else_bb);

                // Compile then branch
                self.builder.position_at_end(then_bb);
                self.compile_block(then_branch)?;

                // Compile else branch
                self.builder.position_at_end(else_bb);
                if let Some(eb) = else_branch {
                    self.compile_block(eb)?;
                } else {
                    self.build_typed_return(None)?;
                }
            }

            AnfTail::Match { scrutinee, arms } => {
                let sc_val = self.eval_atom(scrutinee)?;
                let cur_fn = self.current_fn.unwrap();

                for (i, arm) in arms.iter().enumerate() {
                    let is_last = i == arms.len() - 1;
                    let arm_bb = self.context.append_basic_block(cur_fn, &format!("arm_{i}"));
                    let next_bb = if !is_last {
                        Some(
                            self.context
                                .append_basic_block(cur_fn, &format!("next_{i}")),
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
                            let _ =
                                self.builder
                                    .build_conditional_branch(cond_i1, arm_bb, target_next);
                        }
                        crate::desugar::DesugaredPattern::Variant { variant, .. }
                            if sc_val.is_pointer_value() =>
                        {
                            let tag_ptr = unsafe {
                                self.builder
                                    .build_gep(
                                        self.context.i64_type(),
                                        sc_val.into_pointer_value(),
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
                            let exp_tag_val =
                                if variant == "Ok" || variant == "Some" || variant == "CircleShape"
                                {
                                    0
                                } else if variant == "Err"
                                    || variant == "None"
                                    || variant == "RectShape"
                                {
                                    1
                                } else {
                                    2
                                };
                            let exp_tag = self.context.i64_type().const_int(exp_tag_val, false);
                            let eq_tag = self
                                .builder
                                .build_int_compare(IntPredicate::EQ, tag, exp_tag, "eq_tag")
                                .unwrap();
                            let target_next = next_bb.unwrap_or(arm_bb);
                            let _ =
                                self.builder
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
                                        } else if name == "Option"
                                            && variant == "Some"
                                            && !args.is_empty()
                                        {
                                            Some(args[0].clone())
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
                                                &[self
                                                    .context
                                                    .i64_type()
                                                    .const_int((2 + j) as u64, false)],
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
                                                &[self
                                                    .context
                                                    .i64_type()
                                                    .const_int(f_idx as u64, false)],
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
                        crate::desugar::DesugaredPattern::Tuple(patterns)
                            if sc_val.is_pointer_value() =>
                        {
                            for (j, pat) in patterns.iter().enumerate() {
                                if let crate::desugar::DesugaredPattern::Ident(v) = pat {
                                    let f_ptr = unsafe {
                                        self.builder
                                            .build_gep(
                                                self.context.i64_type(),
                                                sc_val.into_pointer_value(),
                                                &[self
                                                    .context
                                                    .i64_type()
                                                    .const_int((1 + j) as u64, false)],
                                                "tup_gep",
                                            )
                                            .unwrap()
                                    };
                                    let f_val = self
                                        .builder
                                        .build_load(self.context.i64_type(), f_ptr, "f_val")
                                        .unwrap();
                                    self.variables.insert(v.clone(), f_val);
                                }
                            }
                        }
                        _ => {}
                    }

                    self.compile_block(&arm.body)?;

                    if let Some(nb) = next_bb {
                        self.builder.position_at_end(nb);
                    }
                }
            }

            AnfTail::Atom(atom) => {
                let val = self.eval_atom(atom)?;
                self.build_typed_return(Some(val))?;
            }
        }
        Ok(())
    }
}
