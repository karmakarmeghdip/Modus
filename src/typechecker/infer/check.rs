//! Top-down bidirectional expression checking for Modus.

use crate::ast::{self, ElseBranch, Expr, FunctionBody, Literal, Spanned, UnaryOp};
use crate::typechecker::error::{TypeError, TypeErrorKind};
use crate::typechecker::infer::TypeInferrer;
use crate::typechecker::traits::TraitResolver;
use crate::typechecker::types::Type;
use std::collections::HashMap;

impl<'a> TypeInferrer<'a> {
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
}
