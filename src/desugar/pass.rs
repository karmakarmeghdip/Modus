//! Desugaring pass implementation.

use crate::ast::{
    self, ElseBranch, FunctionBody, FunctionDecl, ImplDecl, MatchArmBody, Pattern, Program,
    Spanned, Stmt, UnaryOp,
};
use crate::desugar::node::*;
use crate::typechecker::{EffectContext, Environment, FunctionSig, Type, TypeInferrer};
use std::collections::{BTreeMap, HashMap};

pub fn desugar_program(program: &Program, env: &Environment) -> DesugaredProgram {
    let mut cloned_env = env.clone();
    let mut ctx = DesugarContext::new(&mut cloned_env);
    let mut declarations = Vec::new();

    for decl in &program.declarations {
        match &decl.node {
            ast::Declaration::Function(f) => {
                declarations.push(DesugaredDecl::Function(ctx.desugar_function(f, decl.span)));
            }
            ast::Declaration::Type(t) => {
                declarations.push(DesugaredDecl::Type(t.clone()));
            }
            ast::Declaration::Trait(tr) => {
                declarations.push(DesugaredDecl::Trait(tr.clone()));
            }
            ast::Declaration::Impl(im) => {
                declarations.push(DesugaredDecl::Impl(ctx.desugar_impl(im, decl.span)));
            }
        }
    }

    let local_fn_names: std::collections::HashSet<&str> = program
        .declarations
        .iter()
        .filter_map(|d| match &d.node {
            ast::Declaration::Function(f) => Some(f.name.as_str()),
            _ => None,
        })
        .collect();

    let mut extern_functions = Vec::new();
    let mut seen_symbols = std::collections::HashSet::new();
    for sig in env.functions.values() {
        if local_fn_names.contains(sig.name.as_str()) {
            continue;
        }
        let Some(sym) = &sig.symbol_name else {
            continue;
        };
        if seen_symbols.insert(sym.clone()) {
            extern_functions.push(DesugaredExternFunction {
                name: sig.name.clone(),
                symbol_name: sym.clone(),
                param_types: sig.params.iter().map(|(_, ty)| ty.clone()).collect(),
                return_type: sig.return_type.clone(),
                is_effectful: sig.is_effectful,
            });
        }
    }

    DesugaredProgram {
        declarations,
        extern_functions,
    }
}

struct DesugarContext<'a> {
    inferrer: TypeInferrer<'a>,
    enclosing_fn_ret_type: Option<Type>,
    tmp_counter: usize,
}

impl<'a> DesugarContext<'a> {
    fn new(env: &'a mut Environment) -> Self {
        let inferrer = TypeInferrer::new(env, None);
        Self {
            inferrer,
            enclosing_fn_ret_type: None,
            tmp_counter: 0,
        }
    }

    fn next_tmp(&mut self, prefix: &str) -> String {
        let name = format!("_{prefix}_{}", self.tmp_counter);
        self.tmp_counter += 1;
        name
    }

    fn desugar_function(&mut self, func: &FunctionDecl, span: ast::Span) -> DesugaredFunction {
        let sig = if let Some(sig) = self.inferrer.env.lookup_function(&func.name).cloned() {
            sig
        } else {
            let generic_names: Vec<String> =
                func.type_params.iter().map(|p| p.name.clone()).collect();
            let mut params = Vec::new();
            for p in &func.params {
                let p_ty = self
                    .inferrer
                    .env
                    .resolve_ast_type(&p.ty.node, &generic_names, Some(p.ty.span))
                    .unwrap_or(Type::void());
                params.push((p.name.clone(), p_ty));
            }
            let return_type = if let Some(ret) = &func.return_type {
                self.inferrer
                    .env
                    .resolve_ast_type(&ret.node, &generic_names, Some(ret.span))
                    .unwrap_or(Type::void())
            } else {
                Type::void()
            };
            let is_effectful = return_type.is_io();
            FunctionSig {
                name: func.name.clone(),
                type_params: func.type_params.clone(),
                params,
                return_type,
                is_effectful,
                span,
                symbol_name: None,
            }
        };
        let old_ret = self.enclosing_fn_ret_type.replace(sig.return_type.clone());

        // Setup inferrer context
        let effect_ctx = EffectContext::new(func.name.clone(), sig.return_type.clone(), span).ok();
        self.inferrer.effect_ctx = effect_ctx;

        let mut bounds = HashMap::new();
        for tp in &func.type_params {
            if let Some(bound) = &tp.bound {
                match &bound.node {
                    ast::Type::Generic { name, .. } => {
                        bounds.insert(tp.name.clone(), name.clone());
                    }
                    ast::Type::Path(segments) => {
                        bounds.insert(tp.name.clone(), segments.join("."));
                    }
                    _ => {}
                }
            }
        }
        self.inferrer.set_generic_bounds(bounds);

        self.inferrer.env.enter_scope();
        for (name, ty) in &sig.params {
            let _ = self.inferrer.env.define_var(name.clone(), ty.clone(), span);
        }

        let body = match &func.body {
            Some(FunctionBody::Expr(e)) => {
                let desugared_e = self.desugar_expr(e);
                vec![DesugaredStmt::Return(Some(desugared_e), e.span)]
            }
            Some(FunctionBody::Block(stmts)) => self.desugar_stmts(stmts),
            None => vec![],
        };

        self.inferrer.env.exit_scope();
        self.enclosing_fn_ret_type = old_ret;

        DesugaredFunction {
            name: sig.symbol_name().to_string(),
            type_params: func.type_params.clone(),
            params: sig.params,
            return_type: sig.return_type,
            body,
            is_effectful: sig.is_effectful,
            span,
        }
    }

    fn desugar_impl(&mut self, im: &ImplDecl, span: ast::Span) -> DesugaredImpl {
        let target_type = self
            .inferrer
            .env
            .resolve_ast_type(&im.target_type.node, &[], Some(im.target_type.span))
            .unwrap_or(Type::void());

        let mut methods = Vec::new();
        for m in &im.methods {
            let m_span = m.span;
            methods.push(self.desugar_function(&m.node, m_span));
        }

        DesugaredImpl {
            trait_name: im.trait_name.clone(),
            target_type,
            methods,
            span,
        }
    }

    fn desugar_stmts(&mut self, stmts: &[Spanned<Stmt>]) -> Vec<DesugaredStmt> {
        let mut desugared = Vec::new();
        for stmt in stmts {
            match &stmt.node {
                Stmt::Let {
                    name,
                    ty: _,
                    initializer,
                } => {
                    let desugared_init = self.desugar_expr(initializer);
                    let var_ty = desugared_init.ty.clone();
                    let _ = self
                        .inferrer
                        .env
                        .define_var(name.clone(), var_ty.clone(), stmt.span);

                    desugared.push(DesugaredStmt::Let {
                        name: name.clone(),
                        ty: var_ty,
                        initializer: desugared_init,
                        span: stmt.span,
                    });
                }
                Stmt::Expr(expr) => {
                    let desugared_e = self.desugar_expr(expr);
                    desugared.push(DesugaredStmt::Expr(desugared_e));
                }
                Stmt::Return(ret_opt) => {
                    let desugared_ret = ret_opt.as_ref().map(|e| self.desugar_expr(e));
                    desugared.push(DesugaredStmt::Return(desugared_ret, stmt.span));
                }
            }
        }
        desugared
    }

    fn desugar_expr(&mut self, expr: &Spanned<ast::Expr>) -> DesugaredExpr {
        let expr_ty = self.inferrer.synth_expr(expr).unwrap_or(Type::void());

        match &expr.node {
            ast::Expr::Literal(lit) => {
                DesugaredExpr::new(DesugaredExprKind::Literal(lit.clone()), expr_ty, expr.span)
            }

            ast::Expr::Ident(name) => {
                DesugaredExpr::new(DesugaredExprKind::Ident(name.clone()), expr_ty, expr.span)
            }

            ast::Expr::Unary { op, expr: sub_expr } => match op {
                UnaryOp::Not => {
                    let desugared_sub = self.desugar_expr(sub_expr);
                    DesugaredExpr::new(
                        DesugaredExprKind::Unary {
                            op: DesugaredUnaryOp::Not,
                            expr: Box::new(desugared_sub),
                        },
                        expr_ty,
                        expr.span,
                    )
                }
                UnaryOp::Neg => {
                    let desugared_sub = self.desugar_expr(sub_expr);
                    DesugaredExpr::new(
                        DesugaredExprKind::Unary {
                            op: DesugaredUnaryOp::Neg,
                            expr: Box::new(desugared_sub),
                        },
                        expr_ty,
                        expr.span,
                    )
                }
                UnaryOp::Perform => {
                    let desugared_sub = self.desugar_expr(sub_expr);
                    DesugaredExpr::new(
                        DesugaredExprKind::Unary {
                            op: DesugaredUnaryOp::Perform,
                            expr: Box::new(desugared_sub),
                        },
                        expr_ty,
                        expr.span,
                    )
                }
                UnaryOp::Check => {
                    // Desugar check sub_expr into Match on Result(T, E)
                    let desugared_sub = self.desugar_expr(sub_expr);
                    self.desugar_check(desugared_sub, expr_ty, expr.span)
                }
            },

            ast::Expr::Binary { lhs, op, rhs } => {
                let desugared_lhs = self.desugar_expr(lhs);
                let desugared_rhs = self.desugar_expr(rhs);
                DesugaredExpr::new(
                    DesugaredExprKind::Binary {
                        lhs: Box::new(desugared_lhs),
                        op: *op,
                        rhs: Box::new(desugared_rhs),
                    },
                    expr_ty,
                    expr.span,
                )
            }

            ast::Expr::Call { callee, args } => {
                let mut desugared_callee = self.desugar_expr(callee);
                if let DesugaredExprKind::Ident(ref name) = desugared_callee.kind {
                    let sym = self
                        .inferrer
                        .env
                        .lookup_function(name)
                        .and_then(|sig| sig.symbol_name.clone());
                    if let Some(sym) = sym {
                        desugared_callee = DesugaredExpr::new(
                            DesugaredExprKind::Ident(sym),
                            desugared_callee.ty,
                            desugared_callee.span,
                        );
                    }
                }
                let desugared_args = args.iter().map(|a| self.desugar_expr(a)).collect();
                DesugaredExpr::new(
                    DesugaredExprKind::Call {
                        callee: Box::new(desugared_callee),
                        args: desugared_args,
                    },
                    expr_ty,
                    expr.span,
                )
            }

            ast::Expr::MethodCall {
                receiver,
                method,
                args,
            } => {
                if let ast::Expr::Ident(ns) = &receiver.node {
                    let qualified_dot = format!("{ns}.{method}");
                    let qualified_under = format!("{ns}_{method}");
                    let target_fn = self
                        .inferrer
                        .env
                        .lookup_function(&qualified_dot)
                        .or_else(|| self.inferrer.env.lookup_function(&qualified_under));

                    if let Some(sig) = target_fn {
                        let callee_name = sig.symbol_name().to_string();
                        let desugared_args = args.iter().map(|a| self.desugar_expr(a)).collect();
                        return DesugaredExpr::new(
                            DesugaredExprKind::Call {
                                callee: Box::new(DesugaredExpr::new(
                                    DesugaredExprKind::Ident(callee_name),
                                    expr_ty.clone(),
                                    receiver.span,
                                )),
                                args: desugared_args,
                            },
                            expr_ty,
                            expr.span,
                        );
                    }
                }

                let desugared_receiver = self.desugar_expr(receiver);
                let desugared_args = args.iter().map(|a| self.desugar_expr(a)).collect();
                DesugaredExpr::new(
                    DesugaredExprKind::MethodCall {
                        receiver: Box::new(desugared_receiver),
                        method: method.clone(),
                        args: desugared_args,
                    },
                    expr_ty,
                    expr.span,
                )
            }

            ast::Expr::FieldAccess { receiver, field } => {
                let desugared_receiver = self.desugar_expr(receiver);
                DesugaredExpr::new(
                    DesugaredExprKind::FieldAccess {
                        receiver: Box::new(desugared_receiver),
                        field: field.clone(),
                    },
                    expr_ty,
                    expr.span,
                )
            }

            ast::Expr::Index { receiver, index } => {
                let desugared_receiver = self.desugar_expr(receiver);
                let desugared_index = self.desugar_expr(index);
                DesugaredExpr::new(
                    DesugaredExprKind::Index {
                        receiver: Box::new(desugared_receiver),
                        index: Box::new(desugared_index),
                    },
                    expr_ty,
                    expr.span,
                )
            }

            ast::Expr::Record(fields) => {
                let desugared_fields = fields
                    .iter()
                    .map(|(n, e)| (n.clone(), self.desugar_expr(e)))
                    .collect();
                DesugaredExpr::new(
                    DesugaredExprKind::Record(desugared_fields),
                    expr_ty,
                    expr.span,
                )
            }

            ast::Expr::RecordUpdate { base, fields } => {
                // Lower { ...base, updated_fields } into full record literal with all fields
                let desugared_base = self.desugar_expr(base);
                let base_ty = desugared_base.ty.clone();

                let record_fields = self.get_record_fields(&base_ty);
                let mut updated_map: HashMap<String, DesugaredExpr> = HashMap::new();
                for (name, val) in fields {
                    updated_map.insert(name.clone(), self.desugar_expr(val));
                }

                let mut full_fields = Vec::new();
                for (field_name, field_ty) in record_fields {
                    if let Some(new_val) = updated_map.remove(&field_name) {
                        full_fields.push((field_name, new_val));
                    } else {
                        // Project field from base: base.field
                        let proj = DesugaredExpr::new(
                            DesugaredExprKind::FieldAccess {
                                receiver: Box::new(desugared_base.clone()),
                                field: field_name.clone(),
                            },
                            field_ty,
                            expr.span,
                        );
                        full_fields.push((field_name, proj));
                    }
                }

                DesugaredExpr::new(DesugaredExprKind::Record(full_fields), expr_ty, expr.span)
            }

            ast::Expr::Array(elements) => {
                let desugared_elements = elements.iter().map(|e| self.desugar_expr(e)).collect();
                DesugaredExpr::new(
                    DesugaredExprKind::Array(desugared_elements),
                    expr_ty,
                    expr.span,
                )
            }

            ast::Expr::Closure {
                params,
                return_type: _,
                body,
            } => {
                self.inferrer.env.enter_scope();
                let mut desugared_params = Vec::new();
                for p in params {
                    let p_ty = self
                        .inferrer
                        .env
                        .lookup_var(&p.name)
                        .map(|(t, _)| t.clone())
                        .unwrap_or(Type::void());
                    desugared_params.push((p.name.clone(), p_ty));
                }

                let (closure_ret, desugared_body) = match body {
                    FunctionBody::Expr(e) => {
                        let desugared_e = self.desugar_expr(e);
                        let ret_ty = desugared_e.ty.clone();
                        (
                            ret_ty,
                            vec![DesugaredStmt::Return(Some(desugared_e), e.span)],
                        )
                    }
                    FunctionBody::Block(stmts) => {
                        let b = self.desugar_stmts(stmts);
                        (Type::void(), b)
                    }
                };
                self.inferrer.env.exit_scope();

                DesugaredExpr::new(
                    DesugaredExprKind::Closure {
                        params: desugared_params,
                        return_type: closure_ret,
                        body: desugared_body,
                    },
                    expr_ty,
                    expr.span,
                )
            }

            ast::Expr::If {
                condition,
                then_branch,
                else_branch,
            } => {
                let desugared_cond = self.desugar_expr(condition);
                let desugared_then = self.desugar_stmts(then_branch);
                let desugared_else = else_branch.as_ref().map(|eb| match eb {
                    ElseBranch::Block(stmts) => self.desugar_stmts(stmts),
                    ElseBranch::If(else_if) => {
                        let else_e = self.desugar_expr(else_if);
                        vec![DesugaredStmt::Expr(else_e)]
                    }
                });

                DesugaredExpr::new(
                    DesugaredExprKind::If {
                        condition: Box::new(desugared_cond),
                        then_branch: desugared_then,
                        else_branch: desugared_else,
                    },
                    expr_ty,
                    expr.span,
                )
            }

            ast::Expr::Match {
                expr: scrutinee,
                arms,
            } => {
                let desugared_scrut = self.desugar_expr(scrutinee);
                let mut desugared_arms = Vec::new();

                for arm in arms {
                    let pat = self.desugar_pattern(&arm.pattern);
                    let body = match &arm.body {
                        MatchArmBody::Expr(e) => vec![DesugaredStmt::Expr(self.desugar_expr(e))],
                        MatchArmBody::Block(stmts) => self.desugar_stmts(stmts),
                    };
                    desugared_arms.push(DesugaredMatchArm { pattern: pat, body });
                }

                DesugaredExpr::new(
                    DesugaredExprKind::Match {
                        expr: Box::new(desugared_scrut),
                        arms: desugared_arms,
                    },
                    expr_ty,
                    expr.span,
                )
            }

            ast::Expr::Block(stmts) => {
                let desugared_stmts = self.desugar_stmts(stmts);
                DesugaredExpr::new(
                    DesugaredExprKind::Block(desugared_stmts),
                    expr_ty,
                    expr.span,
                )
            }
        }
    }

    fn desugar_check(
        &mut self,
        res_expr: DesugaredExpr,
        ok_ty: Type,
        span: ast::Span,
    ) -> DesugaredExpr {
        let val_var = self.next_tmp("ok");
        let err_var = self.next_tmp("err");

        // Ok arm: yields the unwrapped Ok value
        let ok_arm = DesugaredMatchArm {
            pattern: DesugaredPattern::Variant {
                type_name: Some("Result".to_string()),
                variant: "Ok".to_string(),
                patterns: vec![DesugaredPattern::Ident(val_var.clone())],
            },
            body: vec![DesugaredStmt::Expr(DesugaredExpr::new(
                DesugaredExprKind::Ident(val_var),
                ok_ty.clone(),
                span,
            ))],
        };

        // Err arm: early returns Result.Err(err)
        let err_ret_expr = DesugaredExpr::new(
            DesugaredExprKind::MethodCall {
                receiver: Box::new(DesugaredExpr::new(
                    DesugaredExprKind::Ident("Result".to_string()),
                    Type::Named {
                        name: "Result".to_string(),
                        args: Vec::new(),
                    },
                    span,
                )),
                method: "Err".to_string(),
                args: vec![DesugaredExpr::new(
                    DesugaredExprKind::Ident(err_var.clone()),
                    Type::string(), // err type
                    span,
                )],
            },
            self.enclosing_fn_ret_type.clone().unwrap_or(Type::void()),
            span,
        );

        let err_arm = DesugaredMatchArm {
            pattern: DesugaredPattern::Variant {
                type_name: Some("Result".to_string()),
                variant: "Err".to_string(),
                patterns: vec![DesugaredPattern::Ident(err_var)],
            },
            body: vec![DesugaredStmt::Return(Some(err_ret_expr), span)],
        };

        DesugaredExpr::new(
            DesugaredExprKind::Match {
                expr: Box::new(res_expr),
                arms: vec![ok_arm, err_arm],
            },
            ok_ty,
            span,
        )
    }

    fn desugar_pattern(&mut self, pat: &Spanned<Pattern>) -> DesugaredPattern {
        match &pat.node {
            Pattern::Wildcard => DesugaredPattern::Wildcard,
            Pattern::Ident(name) => DesugaredPattern::Ident(name.clone()),
            Pattern::Literal(lit) => DesugaredPattern::Literal(lit.clone()),
            Pattern::Variant {
                type_name,
                variant,
                patterns,
            } => DesugaredPattern::Variant {
                type_name: type_name.clone(),
                variant: variant.clone(),
                patterns: patterns.iter().map(|p| self.desugar_pattern(p)).collect(),
            },
            Pattern::Record(fields) => DesugaredPattern::Record(
                fields
                    .iter()
                    .map(|(n, p)| (n.clone(), p.as_ref().map(|sp| self.desugar_pattern(sp))))
                    .collect(),
            ),
            Pattern::Tuple(elems) => {
                DesugaredPattern::Tuple(elems.iter().map(|e| self.desugar_pattern(e)).collect())
            }
        }
    }

    fn get_record_fields(&self, ty: &Type) -> BTreeMap<String, Type> {
        match ty {
            Type::Record(fields) => fields.clone(),
            Type::Named { name, args } => {
                if let Some(expanded) = self.inferrer.env.expand_type_alias(name, args) {
                    self.get_record_fields(&expanded)
                } else {
                    BTreeMap::new()
                }
            }
            _ => BTreeMap::new(),
        }
    }
}
