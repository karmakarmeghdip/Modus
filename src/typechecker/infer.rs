//! Bidirectional Type Inference for Modus.

use crate::ast::{
    self, BinaryOp, ElseBranch, Expr, FunctionBody, Literal, MatchArmBody, Pattern, Span, Spanned,
    Stmt, UnaryOp,
};
use crate::typechecker::error::{TypeError, TypeErrorKind};
use crate::typechecker::purity::EffectContext;
use crate::typechecker::scope::{ConstructorInfo, Environment, FunctionSig};
use crate::typechecker::traits::TraitResolver;
use crate::typechecker::types::{Substitution, Type, TypeVarGen};
use std::collections::{BTreeMap, HashMap};

pub struct TypeInferrer<'a> {
    pub env: &'a mut Environment,
    pub var_gen: TypeVarGen,
    pub subst: Substitution,
    pub effect_ctx: Option<EffectContext>,
    pub generic_bounds: HashMap<String, String>,
}

impl<'a> TypeInferrer<'a> {
    pub fn new(env: &'a mut Environment, effect_ctx: Option<EffectContext>) -> Self {
        Self {
            env,
            var_gen: TypeVarGen::new(),
            subst: Substitution::new(),
            effect_ctx,
            generic_bounds: HashMap::new(),
        }
    }

    pub fn set_generic_bounds(&mut self, bounds: HashMap<String, String>) {
        self.generic_bounds = bounds;
    }

    /// Check an expression against an expected type (top-down)
    pub fn check_expr(&mut self, expr: &Spanned<Expr>, expected: &Type) -> Result<Type, TypeError> {
        let expected = self.expand_type(&self.subst.apply(expected));

        match (&expr.node, &expected) {
            // Case 1: Structural Record Literal with expected Record or Record alias
            (Expr::Record(fields), Type::Record(expected_fields)) => {
                let mut provided_fields = HashMap::new();
                for (name, val) in fields {
                    provided_fields.insert(name.as_str(), val);
                }

                // Check all expected fields are present
                for (req_name, req_type) in expected_fields {
                    if let Some(val_expr) = provided_fields.remove(req_name.as_str()) {
                        self.check_expr(val_expr, req_type)?;
                    } else {
                        return Err(TypeError::new(
                            TypeErrorKind::MissingRecordField {
                                field: req_name.clone(),
                                record_type: expected.to_string(),
                            },
                            Some(expr.span),
                        ));
                    }
                }

                // Any leftover fields are extraneous
                if let Some((extra_name, extra_expr)) = provided_fields.into_iter().next() {
                    return Err(TypeError::new(
                        TypeErrorKind::ExtraneousRecordField {
                            field: extra_name.to_string(),
                            record_type: expected.to_string(),
                        },
                        Some(extra_expr.span),
                    ));
                }

                Ok(expected.clone())
            }

            // Case 2: Array literal checked against [T]
            (Expr::Array(elements), Type::Array(elem_type)) => {
                for elem in elements {
                    self.check_expr(elem, elem_type)?;
                }
                Ok(expected.clone())
            }

            // Case 3: Dynamic trait object coercion (e.g. c: Circle passed where Drawable expected)
            (_, Type::TraitObject(trait_name)) => {
                let found_type = self.synth_expr(expr)?;
                let resolver = TraitResolver::new(self.env);
                if resolver.implements_trait(&found_type, trait_name) {
                    Ok(expected.clone())
                } else {
                    Err(TypeError::new(
                        TypeErrorKind::TraitNotImplemented {
                            ty: found_type.to_string(),
                            trait_name: trait_name.clone(),
                        },
                        Some(expr.span),
                    ))
                }
            }

            // Case 4: Integer literal checked against integer type
            (Expr::Literal(Literal::Int(_)), _) if expected.is_integer() => Ok(expected.clone()),
            (Expr::Literal(Literal::UInt(_)), _) if expected.is_integer() => Ok(expected.clone()),

            // Case 5: Float literal checked against float type
            (Expr::Literal(Literal::Float(_)), _) if expected.is_float() => Ok(expected.clone()),

            // Case 5b: Negation unary operator checked against numeric type
            (
                Expr::Unary {
                    op: UnaryOp::Neg,
                    expr: sub_expr,
                },
                _,
            ) if expected.is_numeric() => {
                self.check_expr(sub_expr, &expected)?;
                Ok(expected.clone())
            }

            // Case 6: Closure checked against expected function type
            (
                Expr::Closure {
                    params,
                    return_type,
                    body,
                },
                Type::Function {
                    params: exp_params,
                    ret: exp_ret,
                },
            ) => {
                if params.len() != exp_params.len() {
                    return Err(TypeError::new(
                        TypeErrorKind::ArgCountMismatch {
                            expected: exp_params.len(),
                            found: params.len(),
                        },
                        Some(expr.span),
                    ));
                }

                self.env.enter_scope();
                for (param, exp_ty) in params.iter().zip(exp_params.iter()) {
                    let p_ty = if param.ty.node != ast::Type::Unit {
                        let resolved =
                            self.env
                                .resolve_ast_type(&param.ty.node, &[], Some(param.ty.span))?;
                        self.unify(&resolved, exp_ty, Some(param.ty.span))?;
                        resolved
                    } else {
                        exp_ty.clone()
                    };
                    self.env.define_var(param.name.clone(), p_ty, expr.span)?;
                }

                let ret_ty = if let Some(ret_ann) = return_type {
                    let ann_ty =
                        self.env
                            .resolve_ast_type(&ret_ann.node, &[], Some(ret_ann.span))?;
                    self.unify(&ann_ty, exp_ret, Some(ret_ann.span))?;
                    ann_ty
                } else {
                    *exp_ret.clone()
                };

                match body {
                    FunctionBody::Expr(ret_expr) => {
                        self.check_expr(ret_expr, &ret_ty)?;
                    }
                    FunctionBody::Block(stmts) => {
                        self.check_block(stmts, &ret_ty)?;
                    }
                }

                self.env.exit_scope();
                Ok(expected.clone())
            }

            // Case 7: If expression checked against expected type
            (
                Expr::If {
                    condition,
                    then_branch,
                    else_branch,
                },
                _,
            ) => {
                let bool_ty = Type::bool();
                self.check_expr(condition, &bool_ty)?;

                self.env.enter_scope();
                let then_ty = self.check_block_or_synth(then_branch, Some(&expected))?;
                self.env.exit_scope();

                if let Some(else_br) = else_branch {
                    match else_br {
                        ElseBranch::Block(else_stmts) => {
                            self.env.enter_scope();
                            self.check_block_or_synth(else_stmts, Some(&expected))?;
                            self.env.exit_scope();
                        }
                        ElseBranch::If(else_if_expr) => {
                            self.check_expr(else_if_expr, &expected)?;
                        }
                    }
                }

                Ok(then_ty)
            }

            // Case 8: Default bottom-up synthesis and unification
            _ => {
                let synth_ty = self.synth_expr(expr)?;
                self.unify(&synth_ty, &expected, Some(expr.span))?;
                Ok(self.subst.apply(&expected))
            }
        }
    }

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
                // Check if receiver is a type or namespace name (e.g. IO.pure, Result.Ok, Option.None)
                if let Expr::Ident(type_or_mod) = &receiver.node {
                    let qualified = format!("{type_or_mod}.{field}");
                    if let Some(ctor) = self.env.constructors.get(&qualified).cloned() {
                        return Ok(self.instantiate_constructor(&ctor));
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
                // Check if receiver is a type or namespace name (e.g. IO.pure, Result.Ok, Option.Some)
                if let Expr::Ident(type_name) = &receiver.node {
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
                }

                let receiver_ty = self.synth_expr(receiver)?;
                let resolver = TraitResolver::new(self.env);
                let method_sig = resolver.resolve_method(
                    &receiver_ty,
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
                    let lhs_ty = self.synth_expr(lhs)?;
                    self.check_expr(rhs, &lhs_ty)?;
                    if !lhs_ty.is_numeric() && lhs_ty != Type::string() {
                        return Err(TypeError::new(
                            TypeErrorKind::TypeMismatch {
                                expected: "numeric or String type".to_string(),
                                found: lhs_ty.to_string(),
                            },
                            Some(lhs.span),
                        ));
                    }
                    lhs_ty
                }
                BinaryOp::Eq | BinaryOp::NotEq => {
                    let lhs_ty = self.synth_expr(lhs)?;
                    self.check_expr(rhs, &lhs_ty)?;
                    Type::bool()
                }
                BinaryOp::Lt | BinaryOp::LtEq | BinaryOp::Gt | BinaryOp::GtEq => {
                    let lhs_ty = self.synth_expr(lhs)?;
                    self.check_expr(rhs, &lhs_ty)?;
                    if !lhs_ty.is_numeric() {
                        return Err(TypeError::new(
                            TypeErrorKind::TypeMismatch {
                                expected: "numeric type".to_string(),
                                found: lhs_ty.to_string(),
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
                        self.env
                            .resolve_ast_type(&param.ty.node, &[], Some(param.ty.span))?
                    } else {
                        self.var_gen.fresh()
                    };
                    self.env
                        .define_var(param.name.clone(), p_ty.clone(), expr.span)?;
                    param_types.push(p_ty);
                }

                let ret_ty = if let Some(ret_ann) = return_type {
                    let ann = self
                        .env
                        .resolve_ast_type(&ret_ann.node, &[], Some(ret_ann.span))?;
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
                        MatchArmBody::Expr(b_expr) => self.synth_expr(b_expr)?,
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
        };

        Ok(self.subst.apply(&ty))
    }

    /// Check a pattern against a target type, binding variables into the current scope
    pub fn check_pattern(
        &mut self,
        pat: &Spanned<Pattern>,
        target_ty: &Type,
    ) -> Result<(), TypeError> {
        let expanded = self.expand_type(&self.subst.apply(target_ty));

        match (&pat.node, &expanded) {
            (Pattern::Wildcard, _) => Ok(()),

            (Pattern::Ident(name), _) => {
                // If name is a 0-argument variant like None or PointShape
                if let Some(ctor) = self.env.constructors.get(name).cloned() {
                    let ctor_ty = self.instantiate_constructor(&ctor);
                    if self.unify(&ctor_ty, target_ty, Some(pat.span)).is_ok() {
                        return Ok(());
                    }
                }
                // Otherwise binds variable in arm scope
                self.env
                    .define_var(name.clone(), target_ty.clone(), pat.span)
            }

            (Pattern::Literal(lit), _) => match lit {
                Literal::Int(_) if target_ty.is_integer() => Ok(()),
                Literal::UInt(_) if target_ty.is_integer() => Ok(()),
                Literal::Float(_) if target_ty.is_float() => Ok(()),
                Literal::String(_) if *target_ty == Type::string() => Ok(()),
                Literal::Bool(_) if target_ty.is_bool() => Ok(()),
                _ => Err(TypeError::new(
                    TypeErrorKind::TypeMismatch {
                        expected: target_ty.to_string(),
                        found: format!("{lit:?}"),
                    },
                    Some(pat.span),
                )),
            },

            (
                Pattern::Variant {
                    type_name,
                    variant,
                    patterns,
                },
                _,
            ) => {
                let qualified = if let Some(t_name) = type_name {
                    format!("{t_name}.{variant}")
                } else {
                    variant.clone()
                };

                let ctor = self
                    .env
                    .constructors
                    .get(&qualified)
                    .or_else(|| self.env.constructors.get(variant))
                    .cloned()
                    .ok_or_else(|| {
                        TypeError::new(
                            TypeErrorKind::General(format!(
                                "Unknown variant or constructor '{qualified}'"
                            )),
                            Some(pat.span),
                        )
                    })?;

                match ctor {
                    ConstructorInfo::Value {
                        parent_type,
                        type_params,
                    } => {
                        if !patterns.is_empty() {
                            return Err(TypeError::new(
                                TypeErrorKind::ArgCountMismatch {
                                    expected: 0,
                                    found: patterns.len(),
                                },
                                Some(pat.span),
                            ));
                        }
                        let instantiated = self.instantiate_parent_type(&parent_type, &type_params);
                        self.unify(&instantiated, target_ty, Some(pat.span))?;
                        Ok(())
                    }
                    ConstructorInfo::Function {
                        params,
                        return_type,
                        type_params,
                    } => {
                        if params.len() != patterns.len() {
                            return Err(TypeError::new(
                                TypeErrorKind::ArgCountMismatch {
                                    expected: params.len(),
                                    found: patterns.len(),
                                },
                                Some(pat.span),
                            ));
                        }
                        let (inst_params, inst_ret) =
                            self.instantiate_ctor_fn(&params, &return_type, &type_params);
                        self.unify(&inst_ret, target_ty, Some(pat.span))?;
                        for (sub_pat, param_ty) in patterns.iter().zip(inst_params.iter()) {
                            self.check_pattern(sub_pat, param_ty)?;
                        }
                        Ok(())
                    }
                }
            }

            (Pattern::Record(fields), Type::Record(expected_fields)) => {
                for (name, sub_pat) in fields {
                    if let Some(expected_field_ty) = expected_fields.get(name) {
                        if let Some(p) = sub_pat {
                            self.check_pattern(p, expected_field_ty)?;
                        } else {
                            self.env.define_var(
                                name.clone(),
                                expected_field_ty.clone(),
                                pat.span,
                            )?;
                        }
                    } else {
                        return Err(TypeError::new(
                            TypeErrorKind::MissingRecordField {
                                field: name.clone(),
                                record_type: target_ty.to_string(),
                            },
                            Some(pat.span),
                        ));
                    }
                }
                Ok(())
            }

            (Pattern::Tuple(sub_pats), Type::Tuple(elem_types)) => {
                if sub_pats.len() != elem_types.len() {
                    return Err(TypeError::new(
                        TypeErrorKind::TypeMismatch {
                            expected: target_ty.to_string(),
                            found: format!("tuple of length {}", sub_pats.len()),
                        },
                        Some(pat.span),
                    ));
                }
                for (sub_pat, elem_ty) in sub_pats.iter().zip(elem_types.iter()) {
                    self.check_pattern(sub_pat, elem_ty)?;
                }
                Ok(())
            }

            _ => Err(TypeError::new(
                TypeErrorKind::InvalidPattern(format!(
                    "Pattern does not match expected type '{}'",
                    target_ty
                )),
                Some(pat.span),
            )),
        }
    }

    /// Check a block of statements against an expected return type
    pub fn check_block(
        &mut self,
        stmts: &[Spanned<Stmt>],
        expected_ret: &Type,
    ) -> Result<(), TypeError> {
        let actual_ty = self.check_block_or_synth(stmts, Some(expected_ret))?;
        self.unify(&actual_ty, expected_ret, None)
    }

    /// Check statements in a block, returning the type of the final expression or return type
    pub fn check_block_or_synth(
        &mut self,
        stmts: &[Spanned<Stmt>],
        expected_ret: Option<&Type>,
    ) -> Result<Type, TypeError> {
        let mut last_expr_ty = Type::void();

        for (idx, stmt) in stmts.iter().enumerate() {
            let is_last = idx == stmts.len() - 1;
            match &stmt.node {
                Stmt::Let {
                    name,
                    ty,
                    initializer,
                } => {
                    let var_ty = if let Some(type_ann) = ty {
                        let declared_ty =
                            self.env
                                .resolve_ast_type(&type_ann.node, &[], Some(type_ann.span))?;
                        self.check_expr(initializer, &declared_ty)?;
                        declared_ty
                    } else {
                        self.synth_expr(initializer)?
                    };

                    self.env.define_var(name.clone(), var_ty, stmt.span)?;
                    last_expr_ty = Type::void();
                }

                Stmt::Expr(expr) => {
                    if is_last && let Some(exp) = expected_ret {
                        last_expr_ty = self.check_expr(expr, exp)?;
                    } else {
                        last_expr_ty = self.synth_expr(expr)?;
                    }
                }

                Stmt::Return(ret_expr) => {
                    let target_ret = expected_ret
                        .cloned()
                        .or_else(|| self.effect_ctx.as_ref().map(|c| c.return_type.clone()))
                        .unwrap_or(Type::void());

                    if let Some(expr) = ret_expr {
                        let ret_ty = if let Some(inner) = target_ret.unwrap_io() {
                            let inner_clone = inner.clone();
                            let mut fork = self.subst.clone();
                            if fork
                                .unify(
                                    &self.synth_expr(expr).unwrap_or(Type::void()),
                                    &target_ret,
                                    None,
                                    &|n, a| self.env.expand_type_alias(n, a),
                                )
                                .is_ok()
                            {
                                self.check_expr(expr, &target_ret)?;
                            } else {
                                self.check_expr(expr, &inner_clone)?;
                            }
                            target_ret.clone()
                        } else {
                            self.check_expr(expr, &target_ret)?
                        };
                        last_expr_ty = ret_ty;
                    } else {
                        if !target_ret.is_void() {
                            return Err(TypeError::new(
                                TypeErrorKind::TypeMismatch {
                                    expected: target_ret.to_string(),
                                    found: "void".to_string(),
                                },
                                Some(stmt.span),
                            ));
                        }
                        last_expr_ty = Type::void();
                    }
                }
            }
        }

        Ok(last_expr_ty)
    }

    fn expand_type(&self, ty: &Type) -> Type {
        match ty {
            Type::Named { name, args } => {
                if let Some(expanded) = self.env.expand_type_alias(name, args) {
                    self.expand_type(&expanded)
                } else {
                    ty.clone()
                }
            }
            _ => ty.clone(),
        }
    }

    fn unify(&mut self, t1: &Type, t2: &Type, span: Option<Span>) -> Result<(), TypeError> {
        self.subst.unify(t1, t2, span, &|name, args| {
            self.env.expand_type_alias(name, args)
        })
    }

    fn instantiate_function(&mut self, sig: &FunctionSig) -> Type {
        let mut param_map = HashMap::new();
        for tp in &sig.type_params {
            param_map.insert(tp.name.clone(), self.var_gen.fresh());
        }

        let inst_params = sig
            .params
            .iter()
            .map(|(_, ty)| self.subst_generic_params(ty, &param_map))
            .collect();
        let inst_ret = self.subst_generic_params(&sig.return_type, &param_map);

        Type::Function {
            params: inst_params,
            ret: Box::new(inst_ret),
        }
    }

    fn instantiate_constructor(&mut self, ctor: &ConstructorInfo) -> Type {
        match ctor {
            ConstructorInfo::Value {
                parent_type,
                type_params,
            } => self.instantiate_parent_type(parent_type, type_params),
            ConstructorInfo::Function {
                params,
                return_type,
                type_params,
            } => {
                let (inst_params, inst_ret) =
                    self.instantiate_ctor_fn(params, return_type, type_params);
                Type::Function {
                    params: inst_params,
                    ret: Box::new(inst_ret),
                }
            }
        }
    }

    fn instantiate_parent_type(
        &mut self,
        parent_type: &Type,
        type_params: &[ast::TypeParam],
    ) -> Type {
        let mut param_map = HashMap::new();
        for tp in type_params {
            param_map.insert(tp.name.clone(), self.var_gen.fresh());
        }
        self.subst_generic_params(parent_type, &param_map)
    }

    fn instantiate_ctor_fn(
        &mut self,
        params: &[Type],
        return_type: &Type,
        type_params: &[ast::TypeParam],
    ) -> (Vec<Type>, Type) {
        let mut param_map = HashMap::new();
        for tp in type_params {
            param_map.insert(tp.name.clone(), self.var_gen.fresh());
        }
        let inst_params = params
            .iter()
            .map(|p| self.subst_generic_params(p, &param_map))
            .collect();
        let inst_ret = self.subst_generic_params(return_type, &param_map);
        (inst_params, inst_ret)
    }

    fn subst_generic_params(&self, ty: &Type, param_map: &HashMap<String, Type>) -> Type {
        match ty {
            Type::GenericParam(name) => {
                if let Some(target) = param_map.get(name) {
                    target.clone()
                } else {
                    ty.clone()
                }
            }
            Type::Array(inner) => {
                Type::Array(Box::new(self.subst_generic_params(inner, param_map)))
            }
            Type::Function { params, ret } => Type::Function {
                params: params
                    .iter()
                    .map(|p| self.subst_generic_params(p, param_map))
                    .collect(),
                ret: Box::new(self.subst_generic_params(ret, param_map)),
            },
            Type::Record(fields) => {
                let mut new_fields = BTreeMap::new();
                for (k, v) in fields {
                    new_fields.insert(k.clone(), self.subst_generic_params(v, param_map));
                }
                Type::Record(new_fields)
            }
            Type::Tuple(elems) => Type::Tuple(
                elems
                    .iter()
                    .map(|e| self.subst_generic_params(e, param_map))
                    .collect(),
            ),
            Type::Named { name, args } => Type::Named {
                name: name.clone(),
                args: args
                    .iter()
                    .map(|a| self.subst_generic_params(a, param_map))
                    .collect(),
            },
            _ => ty.clone(),
        }
    }
}
