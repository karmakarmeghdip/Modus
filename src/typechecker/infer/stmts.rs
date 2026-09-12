//! Statement and block type checking for Modus.

use crate::ast::{Spanned, Stmt};
use crate::typechecker::error::{TypeError, TypeErrorKind};
use crate::typechecker::infer::TypeInferrer;
use crate::typechecker::types::Type;

impl<'a> TypeInferrer<'a> {
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
}
