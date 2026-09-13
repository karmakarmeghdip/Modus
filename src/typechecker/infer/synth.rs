//! Bottom-up type synthesis for Modus expressions.

use crate::ast::{
    self, BinaryOp, ElseBranch, Expr, FunctionBody, Literal, MatchArmBody, Span, Spanned, UnaryOp,
};
use crate::typechecker::error::{TypeError, TypeErrorKind};
use crate::typechecker::infer::TypeInferrer;
use crate::typechecker::traits::TraitResolver;
use crate::typechecker::types::Type;
use std::collections::BTreeMap;

impl<'a> TypeInferrer<'a> {
    /// Synthesize the type of an expression bottom-up
    pub fn synth_expr(&mut self, expr: &Spanned<Expr>) -> Result<Type, TypeError> {
        let ty = match &expr.node {
            Expr::Literal(lit) => match lit {
                Literal::Int(_) => Type::i32(),
                Literal::UInt(_) => Type::u64(),
                Literal::Float(_) => Type::f64(),
                Literal::String(_) => Type::string(),
                Literal::Bool(_) => Type::bool(),
                Literal::Unit => Type::void(),
            },

            Expr::Ident(name) => {
                if let Some((var_ty, _)) = self.env.lookup_var(name) {
                    var_ty.clone()
                } else if let Some(sig) = self.env.lookup_function(name).cloned() {
                    self.instantiate_function(&sig)
                } else if let Some(ctor) = self.env.constructors.get(name).cloned() {
                    self.instantiate_constructor(&ctor)
                } else {
                    return Err(TypeError::new(
                        TypeErrorKind::UndeclaredVariable(name.clone()),
                        Some(expr.span),
                    ));
                }
            }

            Expr::FieldAccess { receiver, field } => {
                // Check if receiver is a type or namespace name (e.g. IO.pure, Result.Ok, Option.None, Math.add)
                if let Expr::Ident(type_or_mod) = &receiver.node {
                    let qualified = format!("{type_or_mod}.{field}");
                    if let Some(ctor) = self.env.constructors.get(&qualified).cloned() {
                        return Ok(self.instantiate_constructor(&ctor));
                    }
                    if let Some(sig) = self.env.lookup_function(&qualified).cloned() {
                        return Ok(self.instantiate_function(&sig));
                    }
                }

                let receiver_ty = self.synth_expr(receiver)?;
                let expanded = self.expand_type(&self.subst.apply(&receiver_ty));

                match expanded {
                    Type::Record(fields) => {
                        if let Some(f_type) = fields.get(field) {
                            f_type.clone()
                        } else {
                            return Err(TypeError::new(
                                TypeErrorKind::MissingRecordField {
                                    field: field.clone(),
                                    record_type: receiver_ty.to_string(),
                                },
                                Some(expr.span),
                            ));
                        }
                    }
                    _ => {
                        return Err(TypeError::new(
                            TypeErrorKind::General(format!(
                                "Field access '.{field}' on non-record type '{receiver_ty}'"
                            )),
                            Some(expr.span),
                        ));
                    }
                }
            }

            Expr::Call { callee, args } => {
                // Special check for qualified constructor call like Result.Ok(...) or IO.pure(...)
                let callee_ty = self.synth_expr(callee)?;
                let expanded_callee = self.subst.apply(&callee_ty);

                match expanded_callee {
                    Type::Function { params, ret } => {
                        if params.len() != args.len() {
                            return Err(TypeError::new(
                                TypeErrorKind::ArgCountMismatch {
                                    expected: params.len(),
                                    found: args.len(),
                                },
                                Some(expr.span),
                            ));
                        }

                        // Verify purity: cannot call IO function from pure function unless allowed
                        if let Some(ctx) = &self.effect_ctx
                            && ret.is_io()
                            && !ctx.is_io
                        {
                            // If calling inside perform, perform unwraps it
                            // But calling directly without perform in pure function is purity violation
                            ctx.verify_call_allowed("<anonymous>", &ret, Some(expr.span))?;
                        }

                        for (arg, param_ty) in args.iter().zip(params.iter()) {
                            self.check_expr(arg, param_ty)?;
                        }

                        self.subst.apply(&ret)
                    }
                    _ => {
                        return Err(TypeError::new(
                            TypeErrorKind::General(format!(
                                "Cannot call non-function type '{callee_ty}'"
                            )),
                            Some(callee.span),
                        ));
                    }
                }
            }

            Expr::MethodCall {
                receiver,
                method,
                args,
            } => {
                // Check if receiver is a type or namespace name (e.g. IO.pure, Result.Ok, Option.Some, Math.add, Pointer.null)
                if let Expr::Ident(type_name) = &receiver.node {
                    if type_name == "Pointer" {
                        match method.as_str() {
                            "null" => {
                                if !args.is_empty() {
                                    return Err(TypeError::new(
                                        TypeErrorKind::ArgCountMismatch {
                                            expected: 0,
                                            found: args.len(),
                                        },
                                        Some(expr.span),
                                    ));
                                }
                                return Ok(Type::pointer(self.var_gen.fresh()));
                            }
                            "fromAddress" => {
                                if args.len() != 1 {
                                    return Err(TypeError::new(
                                        TypeErrorKind::ArgCountMismatch {
                                            expected: 1,
                                            found: args.len(),
                                        },
                                        Some(expr.span),
                                    ));
                                }
                                let addr_ty = self.synth_expr(&args[0])?;
                                if !self.subst.apply(&addr_ty).is_integer() {
                                    return Err(TypeError::new(
                                        TypeErrorKind::TypeMismatch {
                                            expected: "integer".to_string(),
                                            found: addr_ty.to_string(),
                                        },
                                        Some(args[0].span),
                                    ));
                                }
                                return Ok(Type::pointer(self.var_gen.fresh()));
                            }
                            _ => {}
                        }
                    }

                    if type_name == "ArrayBuilder" {
                        match method.as_str() {
                            "new" => {
                                if !args.is_empty() {
                                    return Err(TypeError::new(
                                        TypeErrorKind::ArgCountMismatch {
                                            expected: 0,
                                            found: args.len(),
                                        },
                                        Some(expr.span),
                                    ));
                                }
                                return Ok(Type::array_builder(self.var_gen.fresh()));
                            }
                            "withCapacity" => {
                                if args.len() != 1 {
                                    return Err(TypeError::new(
                                        TypeErrorKind::ArgCountMismatch {
                                            expected: 1,
                                            found: args.len(),
                                        },
                                        Some(expr.span),
                                    ));
                                }
                                let cap_ty = self.synth_expr(&args[0])?;
                                if !self.subst.apply(&cap_ty).is_integer() {
                                    return Err(TypeError::new(
                                        TypeErrorKind::TypeMismatch {
                                            expected: "integer".to_string(),
                                            found: cap_ty.to_string(),
                                        },
                                        Some(args[0].span),
                                    ));
                                }
                                return Ok(Type::array_builder(self.var_gen.fresh()));
                            }
                            _ => {}
                        }
                    }

                    if type_name == "String" && method == "toCString" {
                        if args.len() != 1 {
                            return Err(TypeError::new(
                                TypeErrorKind::ArgCountMismatch {
                                    expected: 1,
                                    found: args.len(),
                                },
                                Some(expr.span),
                            ));
                        }
                        self.check_expr(&args[0], &Type::string())?;
                        return Ok(Type::cstring());
                    }

                    if type_name == "CString" && method == "toString" {
                        if args.len() != 1 {
                            return Err(TypeError::new(
                                TypeErrorKind::ArgCountMismatch {
                                    expected: 1,
                                    found: args.len(),
                                },
                                Some(expr.span),
                            ));
                        }
                        let arg_ty = self.synth_expr(&args[0])?;
                        let applied = self.subst.apply(&arg_ty);
                        let expanded = self.expand_type(&applied);
                        if !expanded.is_cstring() && expanded.unwrap_pointer() != Some(&Type::u8())
                        {
                            return Err(TypeError::new(
                                TypeErrorKind::TypeMismatch {
                                    expected: "CString".to_string(),
                                    found: arg_ty.to_string(),
                                },
                                Some(args[0].span),
                            ));
                        }
                        let ret = Type::io(Type::string());
                        if let Some(ctx) = &self.effect_ctx
                            && !ctx.is_io
                        {
                            ctx.verify_call_allowed("CString.toString", &ret, Some(expr.span))?;
                        }
                        return Ok(ret);
                    }

                    let qualified = format!("{type_name}.{method}");
                    if let Some(ctor) = self.env.constructors.get(&qualified).cloned() {
                        let ctor_ty = self.instantiate_constructor(&ctor);
                        if let Type::Function { params, ret } = ctor_ty {
                            if params.len() != args.len() {
                                return Err(TypeError::new(
                                    TypeErrorKind::ArgCountMismatch {
                                        expected: params.len(),
                                        found: args.len(),
                                    },
                                    Some(expr.span),
                                ));
                            }
                            for (arg, param_ty) in args.iter().zip(params.iter()) {
                                self.check_expr(arg, param_ty)?;
                            }
                            return Ok(self.subst.apply(&ret));
                        }
                    }

                    if let Some(sig) = self.env.lookup_function(&qualified).cloned() {
                        let fn_ty = self.instantiate_function(&sig);
                        if let Type::Function { params, ret } = fn_ty {
                            if params.len() != args.len() {
                                return Err(TypeError::new(
                                    TypeErrorKind::ArgCountMismatch {
                                        expected: params.len(),
                                        found: args.len(),
                                    },
                                    Some(expr.span),
                                ));
                            }
                            if let Some(ctx) = &self.effect_ctx
                                && ret.is_io()
                                && !ctx.is_io
                            {
                                ctx.verify_call_allowed(&qualified, &ret, Some(expr.span))?;
                            }
                            for (arg, param_ty) in args.iter().zip(params.iter()) {
                                self.check_expr(arg, param_ty)?;
                            }
                            return Ok(self.subst.apply(&ret));
                        }
                    }
                }

                let receiver_ty = self.synth_expr(receiver)?;
                let applied_ty = self.subst.apply(&receiver_ty);
                let expanded_recv = self.expand_type(&applied_ty);

                if let Some(inner) = expanded_recv.unwrap_pointer() {
                    let inner = inner.clone();
                    match method.as_str() {
                        "read" => {
                            if !args.is_empty() {
                                return Err(TypeError::new(
                                    TypeErrorKind::ArgCountMismatch {
                                        expected: 0,
                                        found: args.len(),
                                    },
                                    Some(expr.span),
                                ));
                            }
                            let ret = Type::io(inner);
                            if let Some(ctx) = &self.effect_ctx
                                && !ctx.is_io
                            {
                                ctx.verify_call_allowed("read", &ret, Some(expr.span))?;
                            }
                            return Ok(ret);
                        }
                        "write" => {
                            if args.len() != 1 {
                                return Err(TypeError::new(
                                    TypeErrorKind::ArgCountMismatch {
                                        expected: 1,
                                        found: args.len(),
                                    },
                                    Some(expr.span),
                                ));
                            }
                            self.check_expr(&args[0], &inner)?;
                            let ret = Type::io(Type::void());
                            if let Some(ctx) = &self.effect_ctx
                                && !ctx.is_io
                            {
                                ctx.verify_call_allowed("write", &ret, Some(expr.span))?;
                            }
                            return Ok(ret);
                        }
                        "offset" => {
                            if args.len() != 1 {
                                return Err(TypeError::new(
                                    TypeErrorKind::ArgCountMismatch {
                                        expected: 1,
                                        found: args.len(),
                                    },
                                    Some(expr.span),
                                ));
                            }
                            let count_ty = self.synth_expr(&args[0])?;
                            if !self.subst.apply(&count_ty).is_integer() {
                                return Err(TypeError::new(
                                    TypeErrorKind::TypeMismatch {
                                        expected: "integer".to_string(),
                                        found: count_ty.to_string(),
                                    },
                                    Some(args[0].span),
                                ));
                            }
                            return Ok(applied_ty);
                        }
                        "address" => {
                            if !args.is_empty() {
                                return Err(TypeError::new(
                                    TypeErrorKind::ArgCountMismatch {
                                        expected: 0,
                                        found: args.len(),
                                    },
                                    Some(expr.span),
                                ));
                            }
                            return Ok(Type::u64());
                        }
                        "isNull" => {
                            if !args.is_empty() {
                                return Err(TypeError::new(
                                    TypeErrorKind::ArgCountMismatch {
                                        expected: 0,
                                        found: args.len(),
                                    },
                                    Some(expr.span),
                                ));
                            }
                            return Ok(Type::bool());
                        }
                        "cast" => {
                            if !args.is_empty() {
                                return Err(TypeError::new(
                                    TypeErrorKind::ArgCountMismatch {
                                        expected: 0,
                                        found: args.len(),
                                    },
                                    Some(expr.span),
                                ));
                            }
                            return Ok(Type::pointer(self.var_gen.fresh()));
                        }
                        "toString" if inner == Type::u8() || expanded_recv.is_cstring() => {
                            if !args.is_empty() {
                                return Err(TypeError::new(
                                    TypeErrorKind::ArgCountMismatch {
                                        expected: 0,
                                        found: args.len(),
                                    },
                                    Some(expr.span),
                                ));
                            }
                            let ret = Type::io(Type::string());
                            if let Some(ctx) = &self.effect_ctx
                                && !ctx.is_io
                            {
                                ctx.verify_call_allowed("toString", &ret, Some(expr.span))?;
                            }
                            return Ok(ret);
                        }
                        _ => {}
                    }
                }

                if expanded_recv.is_array_builder() {
                    let elem_ty = expanded_recv.unwrap_array_builder().unwrap().clone();
                    match method.as_str() {
                        "push" => {
                            if args.len() != 1 {
                                return Err(TypeError::new(
                                    TypeErrorKind::ArgCountMismatch {
                                        expected: 1,
                                        found: args.len(),
                                    },
                                    Some(expr.span),
                                ));
                            }
                            self.check_expr(&args[0], &elem_ty)?;
                            return Ok(applied_ty);
                        }
                        "build" => {
                            if !args.is_empty() {
                                return Err(TypeError::new(
                                    TypeErrorKind::ArgCountMismatch {
                                        expected: 0,
                                        found: args.len(),
                                    },
                                    Some(expr.span),
                                ));
                            }
                            return Ok(Type::Array(Box::new(elem_ty)));
                        }
                        "length" => {
                            if !args.is_empty() {
                                return Err(TypeError::new(
                                    TypeErrorKind::ArgCountMismatch {
                                        expected: 0,
                                        found: args.len(),
                                    },
                                    Some(expr.span),
                                ));
                            }
                            return Ok(Type::i64());
                        }
                        "capacity" => {
                            if !args.is_empty() {
                                return Err(TypeError::new(
                                    TypeErrorKind::ArgCountMismatch {
                                        expected: 0,
                                        found: args.len(),
                                    },
                                    Some(expr.span),
                                ));
                            }
                            return Ok(Type::i64());
                        }
                        "isEmpty" => {
                            if !args.is_empty() {
                                return Err(TypeError::new(
                                    TypeErrorKind::ArgCountMismatch {
                                        expected: 0,
                                        found: args.len(),
                                    },
                                    Some(expr.span),
                                ));
                            }
                            return Ok(Type::bool());
                        }
                        _ => {
                            return Err(TypeError::new(
                                TypeErrorKind::General(format!(
                                    "Method '{method}' not found on ArrayBuilder"
                                )),
                                Some(expr.span),
                            ));
                        }
                    }
                }

                if let Type::Array(_) = expanded_recv
                    && method == "length"
                {
                    if !args.is_empty() {
                        return Err(TypeError::new(
                            TypeErrorKind::ArgCountMismatch {
                                expected: 0,
                                found: args.len(),
                            },
                            Some(expr.span),
                        ));
                    }
                    return Ok(Type::i64());
                }

                if expanded_recv.is_string() && method == "length" {
                    if !args.is_empty() {
                        return Err(TypeError::new(
                            TypeErrorKind::ArgCountMismatch {
                                expected: 0,
                                found: args.len(),
                            },
                            Some(expr.span),
                        ));
                    }
                    return Ok(Type::i64());
                }

                if expanded_recv.is_string() && method == "charCodeAt" {
                    if args.len() != 1 {
                        return Err(TypeError::new(
                            TypeErrorKind::ArgCountMismatch {
                                expected: 1,
                                found: args.len(),
                            },
                            Some(expr.span),
                        ));
                    }
                    let idx_ty = self.synth_expr(&args[0])?;
                    if !idx_ty.is_integer() {
                        return Err(TypeError::new(
                            TypeErrorKind::TypeMismatch {
                                expected: "integer index".to_string(),
                                found: idx_ty.to_string(),
                            },
                            Some(args[0].span),
                        ));
                    }
                    return Ok(Type::i32());
                }

                let resolver = TraitResolver::new(self.env);
                let method_sig = resolver.resolve_method(
                    &applied_ty,
                    method,
                    &self.generic_bounds,
                    Some(expr.span),
                )?;

                // Parameter 0 in method_sig is self
                let expected_params = if method_sig.params.is_empty() {
                    &[][..]
                } else {
                    &method_sig.params[1..]
                };

                if expected_params.len() != args.len() {
                    return Err(TypeError::new(
                        TypeErrorKind::ArgCountMismatch {
                            expected: expected_params.len(),
                            found: args.len(),
                        },
                        Some(expr.span),
                    ));
                }

                for (arg, (_, param_ty)) in args.iter().zip(expected_params.iter()) {
                    self.check_expr(arg, param_ty)?;
                }

                method_sig.return_type
            }

            Expr::Unary { op, expr: sub_expr } => match op {
                UnaryOp::Not => {
                    let bool_ty = Type::bool();
                    self.check_expr(sub_expr, &bool_ty)?;
                    Type::bool()
                }
                UnaryOp::Neg => {
                    let sub_ty = self.synth_expr(sub_expr)?;
                    if !sub_ty.is_numeric() {
                        return Err(TypeError::new(
                            TypeErrorKind::TypeMismatch {
                                expected: "numeric type".to_string(),
                                found: sub_ty.to_string(),
                            },
                            Some(sub_expr.span),
                        ));
                    }
                    sub_ty
                }
                UnaryOp::Perform => {
                    if let Some(ctx) = &self.effect_ctx {
                        ctx.verify_perform_allowed(Some(expr.span))?;
                    } else {
                        return Err(TypeError::new(
                            TypeErrorKind::PurityViolation {
                                function_name: "<global>".to_string(),
                                reason: "Cannot use 'perform' outside an IO function".to_string(),
                            },
                            Some(expr.span),
                        ));
                    }

                    let sub_ty = self.synth_expr(sub_expr)?;
                    let expanded = self.subst.apply(&sub_ty);
                    if let Some(inner) = expanded.unwrap_io() {
                        inner.clone()
                    } else {
                        return Err(TypeError::new(
                            TypeErrorKind::PerformOnNonIO(sub_ty.to_string()),
                            Some(sub_expr.span),
                        ));
                    }
                }
                UnaryOp::Check => {
                    let sub_ty = self.synth_expr(sub_expr)?;
                    let expanded = self.subst.apply(&sub_ty);
                    if let Some((ok_ty, err_ty)) = expanded.unwrap_result() {
                        if let Some(ctx) = &self.effect_ctx {
                            ctx.verify_check_allowed(err_ty, Some(expr.span))?;
                        }
                        ok_ty.clone()
                    } else {
                        return Err(TypeError::new(
                            TypeErrorKind::CheckOnNonResult(sub_ty.to_string()),
                            Some(sub_expr.span),
                        ));
                    }
                }
            },

            Expr::Binary { lhs, op, rhs } => match op {
                BinaryOp::Add | BinaryOp::Sub | BinaryOp::Mul | BinaryOp::Div | BinaryOp::Rem => {
                    let lhs_is_lit = matches!(
                        &lhs.node,
                        Expr::Literal(Literal::Int(_) | Literal::UInt(_) | Literal::Float(_))
                    );
                    let rhs_is_lit = matches!(
                        &rhs.node,
                        Expr::Literal(Literal::Int(_) | Literal::UInt(_) | Literal::Float(_))
                    );
                    let op_ty = if lhs_is_lit && !rhs_is_lit {
                        let rhs_ty = self.synth_expr(rhs)?;
                        self.check_expr(lhs, &rhs_ty)?;
                        rhs_ty
                    } else {
                        let lhs_ty = self.synth_expr(lhs)?;
                        self.check_expr(rhs, &lhs_ty)?;
                        lhs_ty
                    };
                    if *op == BinaryOp::Add {
                        if !op_ty.is_numeric() && op_ty != Type::string() {
                            return Err(TypeError::new(
                                TypeErrorKind::TypeMismatch {
                                    expected: "numeric or String type".to_string(),
                                    found: op_ty.to_string(),
                                },
                                Some(lhs.span),
                            ));
                        }
                    } else if !op_ty.is_numeric() {
                        return Err(TypeError::new(
                            TypeErrorKind::TypeMismatch {
                                expected: "numeric type".to_string(),
                                found: op_ty.to_string(),
                            },
                            Some(lhs.span),
                        ));
                    }
                    op_ty
                }
                BinaryOp::Eq | BinaryOp::NotEq => {
                    let lhs_is_lit = matches!(
                        &lhs.node,
                        Expr::Literal(Literal::Int(_) | Literal::UInt(_) | Literal::Float(_))
                    );
                    let rhs_is_lit = matches!(
                        &rhs.node,
                        Expr::Literal(Literal::Int(_) | Literal::UInt(_) | Literal::Float(_))
                    );
                    if lhs_is_lit && !rhs_is_lit {
                        let rhs_ty = self.synth_expr(rhs)?;
                        self.check_expr(lhs, &rhs_ty)?;
                    } else {
                        let lhs_ty = self.synth_expr(lhs)?;
                        self.check_expr(rhs, &lhs_ty)?;
                    }
                    Type::bool()
                }
                BinaryOp::Lt | BinaryOp::LtEq | BinaryOp::Gt | BinaryOp::GtEq => {
                    let lhs_is_lit = matches!(
                        &lhs.node,
                        Expr::Literal(Literal::Int(_) | Literal::UInt(_) | Literal::Float(_))
                    );
                    let rhs_is_lit = matches!(
                        &rhs.node,
                        Expr::Literal(Literal::Int(_) | Literal::UInt(_) | Literal::Float(_))
                    );
                    let op_ty = if lhs_is_lit && !rhs_is_lit {
                        let rhs_ty = self.synth_expr(rhs)?;
                        self.check_expr(lhs, &rhs_ty)?;
                        rhs_ty
                    } else {
                        let lhs_ty = self.synth_expr(lhs)?;
                        self.check_expr(rhs, &lhs_ty)?;
                        lhs_ty
                    };
                    if !op_ty.is_numeric() {
                        return Err(TypeError::new(
                            TypeErrorKind::TypeMismatch {
                                expected: "numeric type".to_string(),
                                found: op_ty.to_string(),
                            },
                            Some(lhs.span),
                        ));
                    }
                    Type::bool()
                }
                BinaryOp::And | BinaryOp::Or => {
                    let bool_ty = Type::bool();
                    self.check_expr(lhs, &bool_ty)?;
                    self.check_expr(rhs, &bool_ty)?;
                    Type::bool()
                }
            },

            Expr::Record(fields) => {
                let mut rec_fields = BTreeMap::new();
                for (name, f_expr) in fields {
                    let f_ty = self.synth_expr(f_expr)?;
                    rec_fields.insert(name.clone(), f_ty);
                }
                Type::Record(rec_fields)
            }

            Expr::RecordUpdate { base, fields } => {
                let base_ty = self.synth_expr(base)?;
                let expanded_base = self.expand_type(&self.subst.apply(&base_ty));

                match expanded_base {
                    Type::Record(base_fields) => {
                        for (name, f_expr) in fields {
                            if let Some(orig_ty) = base_fields.get(name) {
                                self.check_expr(f_expr, orig_ty)?;
                            } else {
                                return Err(TypeError::new(
                                    TypeErrorKind::ExtraneousRecordField {
                                        field: name.clone(),
                                        record_type: base_ty.to_string(),
                                    },
                                    Some(f_expr.span),
                                ));
                            }
                        }
                        Type::Record(base_fields)
                    }
                    _ => {
                        return Err(TypeError::new(
                            TypeErrorKind::General(format!(
                                "Cannot perform record update on non-record type '{base_ty}'"
                            )),
                            Some(base.span),
                        ));
                    }
                }
            }

            Expr::Array(elements) => {
                if elements.is_empty() {
                    let elem_var = self.var_gen.fresh();
                    Type::Array(Box::new(elem_var))
                } else {
                    let first_ty = self.synth_expr(&elements[0])?;
                    for elem in &elements[1..] {
                        self.check_expr(elem, &first_ty)?;
                    }
                    Type::Array(Box::new(first_ty))
                }
            }

            Expr::Index { receiver, index } => {
                let rec_ty = self.synth_expr(receiver)?;
                let exp_rec = self.subst.apply(&rec_ty);

                let idx_ty = self.synth_expr(index)?;
                if !idx_ty.is_integer() {
                    return Err(TypeError::new(
                        TypeErrorKind::TypeMismatch {
                            expected: "integer index".to_string(),
                            found: idx_ty.to_string(),
                        },
                        Some(index.span),
                    ));
                }

                match exp_rec {
                    Type::Array(elem) => *elem,
                    _ => {
                        return Err(TypeError::new(
                            TypeErrorKind::General(format!(
                                "Indexing '[]' not supported on non-array type '{rec_ty}'"
                            )),
                            Some(receiver.span),
                        ));
                    }
                }
            }

            Expr::Closure {
                params,
                return_type,
                body,
            } => {
                self.env.enter_scope();
                let mut param_types = Vec::new();
                for param in params {
                    let p_ty = if param.ty.node != ast::Type::Unit {
                        self.env.resolve_ast_type(
                            &param.ty.node,
                            &self.generics_in_scope,
                            Some(param.ty.span),
                        )?
                    } else {
                        self.var_gen.fresh()
                    };
                    self.env
                        .define_var(param.name.clone(), p_ty.clone(), expr.span)?;
                    param_types.push(p_ty);
                }

                let ret_ty = if let Some(ret_ann) = return_type {
                    let ann = self.env.resolve_ast_type(
                        &ret_ann.node,
                        &self.generics_in_scope,
                        Some(ret_ann.span),
                    )?;
                    match body {
                        FunctionBody::Expr(ret_expr) => {
                            self.check_expr(ret_expr, &ann)?;
                        }
                        FunctionBody::Block(stmts) => {
                            self.check_block(stmts, &ann)?;
                        }
                    }
                    ann
                } else {
                    match body {
                        FunctionBody::Expr(ret_expr) => self.synth_expr(ret_expr)?,
                        FunctionBody::Block(stmts) => self.check_block_or_synth(stmts, None)?,
                    }
                };

                self.env.exit_scope();
                Type::Function {
                    params: param_types,
                    ret: Box::new(ret_ty),
                }
            }

            Expr::If {
                condition,
                then_branch,
                else_branch,
            } => {
                let bool_ty = Type::bool();
                self.check_expr(condition, &bool_ty)?;

                self.env.enter_scope();
                let then_ty = self.check_block_or_synth(then_branch, None)?;
                self.env.exit_scope();

                if let Some(else_br) = else_branch {
                    match else_br {
                        ElseBranch::Block(else_stmts) => {
                            self.env.enter_scope();
                            let else_ty = self.check_block_or_synth(else_stmts, Some(&then_ty))?;
                            self.env.exit_scope();
                            self.unify(&then_ty, &else_ty, Some(expr.span))?;
                        }
                        ElseBranch::If(else_if_expr) => {
                            let else_ty = self.synth_expr(else_if_expr)?;
                            self.unify(&then_ty, &else_ty, Some(expr.span))?;
                        }
                    }
                }

                then_ty
            }

            Expr::Match {
                expr: match_target,
                arms,
            } => {
                let target_ty = self.synth_expr(match_target)?;
                let mut common_body_ty: Option<Type> = None;

                for arm in arms {
                    self.env.enter_scope();
                    self.check_pattern(&arm.pattern, &target_ty)?;

                    let arm_ty = match &arm.body {
                        MatchArmBody::Expr(b_expr) => {
                            if let Some(existing) = &common_body_ty {
                                self.check_expr(b_expr, existing)?
                            } else {
                                self.synth_expr(b_expr)?
                            }
                        }
                        MatchArmBody::Block(b_stmts) => {
                            self.check_block_or_synth(b_stmts, common_body_ty.as_ref())?
                        }
                    };
                    self.env.exit_scope();

                    if let Some(existing) = &common_body_ty {
                        self.unify(&arm_ty, existing, Some(arm.pattern.span))?;
                    } else {
                        common_body_ty = Some(arm_ty);
                    }
                }

                common_body_ty.unwrap_or(Type::void())
            }

            Expr::Block(stmts) => {
                self.env.enter_scope();
                let ty = self.check_block_or_synth(stmts, None)?;
                self.env.exit_scope();
                ty
            }

            Expr::Cast {
                expr: sub_expr,
                target_type,
            } => {
                let from_ty = self.synth_expr(sub_expr)?;
                let from_ty_expanded = self.subst.apply(&from_ty);
                let to_ty = self.env.resolve_ast_type(
                    &target_type.node,
                    &self.generics_in_scope,
                    Some(target_type.span),
                )?;
                let to_ty_expanded = self.subst.apply(&to_ty);
                self.validate_cast(&from_ty_expanded, &to_ty_expanded, expr.span)?;
                to_ty_expanded
            }
        };

        Ok(self.subst.apply(&ty))
    }

    pub(crate) fn validate_cast(
        &self,
        from_ty: &Type,
        to_ty: &Type,
        span: Span,
    ) -> Result<(), TypeError> {
        let from = self.expand_type(from_ty);
        let to = self.expand_type(to_ty);

        // 1. Identity cast: T as T is always valid
        if from == to {
            return Ok(());
        }

        // 2. Numeric to numeric: (i8..i64, u8..u64, f32, f64)
        if from.is_numeric() && to.is_numeric() {
            return Ok(());
        }

        // 3. Bool to integer, and integer to bool
        if (from.is_bool() && to.is_integer()) || (from.is_integer() && to.is_bool()) {
            return Ok(());
        }

        // 4. Pointer / CString conversions
        let from_is_ptr = from.is_pointer() || from.is_cstring();
        let to_is_ptr = to.is_pointer() || to.is_cstring();
        let from_is_int = from.is_integer();
        let to_is_int = to.is_integer();

        if (from_is_ptr && to_is_ptr) || (from_is_ptr && to_is_int) || (from_is_int && to_is_ptr) {
            return Ok(());
        }

        Err(TypeError::new(
            TypeErrorKind::InvalidCast {
                from: from.to_string(),
                to: to.to_string(),
            },
            Some(span),
        ))
    }
}
