//! Lowering pass from Desugared AST to A-Normal Form (ANF) IR.
//!
//! Linearizes nested expressions into flat sequences of let-bindings with atomic operands,
//! canonicalizes blocks, and identifies tail calls for LLVM `musttail`.

use crate::ast::{Literal, Span};
use crate::desugar::*;
use crate::ir::node::*;
use crate::typechecker::Type;
use std::collections::HashMap;

/// Lowers a `DesugaredProgram` into an `AnfProgram`.
pub fn lower_program(desugared: &DesugaredProgram) -> AnfProgram {
    let mut ctx = AnfLowerCtx::new();
    let mut functions = Vec::new();
    let mut types = Vec::new();
    let mut traits = Vec::new();
    let mut impls = Vec::new();

    for decl in &desugared.declarations {
        match decl {
            DesugaredDecl::Function(func) => {
                functions.push(ctx.lower_function(func));
            }
            DesugaredDecl::Type(ty_decl) => {
                types.push(ty_decl.clone());
            }
            DesugaredDecl::Trait(tr_decl) => {
                traits.push(tr_decl.clone());
            }
            DesugaredDecl::Impl(impl_decl) => {
                let lowered_methods = impl_decl
                    .methods
                    .iter()
                    .map(|m| ctx.lower_function(m))
                    .collect();
                impls.push(AnfImpl {
                    trait_name: impl_decl.trait_name.clone(),
                    target_type: impl_decl.target_type.clone(),
                    methods: lowered_methods,
                    span: impl_decl.span,
                });
            }
        }
    }

    let extern_functions = desugared
        .extern_functions
        .iter()
        .map(|e| AnfExternFunction {
            name: e.name.clone(),
            symbol_name: e.symbol_name.clone(),
            param_types: e.param_types.clone(),
            return_type: e.return_type.clone(),
            is_effectful: e.is_effectful,
            is_c_abi: e.is_c_abi,
        })
        .collect();

    AnfProgram {
        functions,
        extern_functions,
        types,
        traits,
        impls,
    }
}

/// Context for lowering expressions and statements into ANF.
pub struct AnfLowerCtx {
    tmp_counter: usize,
    current_fn_name: String,
    var_types: HashMap<String, Type>,
}

impl Default for AnfLowerCtx {
    fn default() -> Self {
        Self::new()
    }
}

impl AnfLowerCtx {
    pub fn new() -> Self {
        Self {
            tmp_counter: 0,
            current_fn_name: String::new(),
            var_types: HashMap::new(),
        }
    }

    /// Generates a fresh temporary variable name.
    pub fn fresh_var(&mut self, prefix: &str) -> String {
        let name = format!("_{prefix}_{}", self.tmp_counter);
        self.tmp_counter += 1;
        name
    }

    /// Lowers a desugared function into an ANF function.
    pub fn lower_function(&mut self, func: &DesugaredFunction) -> AnfFunction {
        self.current_fn_name = func.name.clone();
        self.var_types.clear();

        for (name, ty) in &func.params {
            self.var_types.insert(name.clone(), ty.clone());
        }

        let body = self.lower_stmts_to_block(&func.body, func.span, true);

        AnfFunction {
            name: func.name.clone(),
            type_params: func.type_params.clone(),
            params: func.params.clone(),
            return_type: func.return_type.clone(),
            body,
            is_effectful: func.is_effectful,
            span: func.span,
        }
    }

    /// Lowers a sequence of desugared statements into an `AnfBlock`.
    /// `is_fn_level` indicates if this block is the root body of a function.
    pub fn lower_stmts_to_block(
        &mut self,
        stmts: &[DesugaredStmt],
        span: Span,
        is_fn_level: bool,
    ) -> AnfBlock {
        let mut anf_stmts = Vec::new();

        if stmts.is_empty() {
            let tail = if is_fn_level {
                AnfTail::Return(None)
            } else {
                AnfTail::Atom(Atom::Literal(Literal::Unit))
            };
            return AnfBlock::new(anf_stmts, tail, span);
        }

        for (i, stmt) in stmts.iter().enumerate() {
            let is_last = i == stmts.len() - 1;

            match stmt {
                DesugaredStmt::Let {
                    name,
                    ty,
                    initializer,
                    span: let_span,
                } => {
                    self.var_types.insert(name.clone(), ty.clone());
                    self.lower_expr_into_var(initializer, name, ty.clone(), &mut anf_stmts);
                    if is_last {
                        let tail = if is_fn_level {
                            AnfTail::Return(None)
                        } else {
                            AnfTail::Atom(Atom::Var(name.clone()))
                        };
                        return AnfBlock::new(anf_stmts, tail, *let_span);
                    }
                }

                DesugaredStmt::Expr(expr) => {
                    if is_last {
                        return self.lower_terminal_expr(expr, anf_stmts, is_fn_level);
                    } else if let DesugaredExprKind::If {
                        condition,
                        then_branch,
                        else_branch,
                    } = &expr.kind
                    {
                        let remaining = &stmts[i + 1..];
                        let mut full_then = then_branch.clone();
                        if !block_always_returns(then_branch) {
                            full_then.extend_from_slice(remaining);
                        }

                        let full_else = match else_branch {
                            Some(eb) => {
                                let mut full_e = eb.clone();
                                if !block_always_returns(eb) {
                                    full_e.extend_from_slice(remaining);
                                }
                                Some(full_e)
                            }
                            None => Some(remaining.to_vec()),
                        };

                        let new_if = DesugaredExpr::new(
                            DesugaredExprKind::If {
                                condition: condition.clone(),
                                then_branch: full_then,
                                else_branch: full_else,
                            },
                            expr.ty.clone(),
                            expr.span,
                        );
                        return self.lower_terminal_expr(&new_if, anf_stmts, is_fn_level);
                    } else if let DesugaredExprKind::Match { expr: scrut, arms } = &expr.kind {
                        let remaining = &stmts[i + 1..];
                        let mut new_arms = Vec::new();
                        for arm in arms {
                            let mut arm_body = arm.body.clone();
                            if !block_always_returns(&arm.body) {
                                arm_body.extend_from_slice(remaining);
                            }
                            new_arms.push(DesugaredMatchArm {
                                pattern: arm.pattern.clone(),
                                body: arm_body,
                            });
                        }
                        let new_match = DesugaredExpr::new(
                            DesugaredExprKind::Match {
                                expr: scrut.clone(),
                                arms: new_arms,
                            },
                            expr.ty.clone(),
                            expr.span,
                        );
                        return self.lower_terminal_expr(&new_match, anf_stmts, is_fn_level);
                    } else {
                        // Intermediate expression statement
                        let _ = self.lower_expr_to_atom(expr, &mut anf_stmts);
                    }
                }

                DesugaredStmt::Return(opt_expr, ret_span) => {
                    if let Some(ret_expr) = opt_expr {
                        // Check for direct tail call
                        if let DesugaredExprKind::Call { callee, args } = &ret_expr.kind {
                            let callee_atom = self.lower_expr_to_atom(callee, &mut anf_stmts);
                            let arg_atoms = args
                                .iter()
                                .map(|a| self.lower_expr_to_atom(a, &mut anf_stmts))
                                .collect();

                            let tail = AnfTail::TailCall {
                                callee: callee_atom,
                                args: arg_atoms,
                            };
                            return AnfBlock::new(anf_stmts, tail, *ret_span);
                        } else if let DesugaredExprKind::If {
                            condition,
                            then_branch,
                            else_branch,
                        } = &ret_expr.kind
                        {
                            let cond_atom = self.lower_expr_to_atom(condition, &mut anf_stmts);
                            let then_block =
                                self.lower_stmts_to_block(then_branch, ret_expr.span, true);
                            let else_block = else_branch.as_ref().map(|eb| {
                                Box::new(self.lower_stmts_to_block(eb, ret_expr.span, true))
                            });
                            let tail = AnfTail::If {
                                cond: cond_atom,
                                then_branch: Box::new(then_block),
                                else_branch: else_block,
                            };
                            return AnfBlock::new(anf_stmts, tail, *ret_span);
                        } else if let DesugaredExprKind::Match { expr: scrut, arms } =
                            &ret_expr.kind
                        {
                            let scrut_atom = self.lower_expr_to_atom(scrut, &mut anf_stmts);
                            let anf_arms = arms
                                .iter()
                                .map(|arm| AnfMatchArm {
                                    pattern: arm.pattern.clone(),
                                    body: self.lower_stmts_to_block(&arm.body, ret_expr.span, true),
                                })
                                .collect();
                            let tail = AnfTail::Match {
                                scrutinee: scrut_atom,
                                arms: anf_arms,
                            };
                            return AnfBlock::new(anf_stmts, tail, *ret_span);
                        } else {
                            let ret_atom = self.lower_expr_to_atom(ret_expr, &mut anf_stmts);
                            let tail = AnfTail::Return(Some(ret_atom));
                            return AnfBlock::new(anf_stmts, tail, *ret_span);
                        }
                    } else {
                        return AnfBlock::new(anf_stmts, AnfTail::Return(None), *ret_span);
                    }
                }
            }
        }

        AnfBlock::new(anf_stmts, AnfTail::Return(None), span)
    }

    /// Lowers an expression in terminal position of a block.
    fn lower_terminal_expr(
        &mut self,
        expr: &DesugaredExpr,
        mut stmts: Vec<AnfStmt>,
        is_fn_level: bool,
    ) -> AnfBlock {
        match &expr.kind {
            DesugaredExprKind::If {
                condition,
                then_branch,
                else_branch,
            } => {
                let cond_atom = self.lower_expr_to_atom(condition, &mut stmts);
                let then_block = self.lower_stmts_to_block(then_branch, expr.span, is_fn_level);
                let else_block = else_branch
                    .as_ref()
                    .map(|eb| Box::new(self.lower_stmts_to_block(eb, expr.span, is_fn_level)));
                let tail = AnfTail::If {
                    cond: cond_atom,
                    then_branch: Box::new(then_block),
                    else_branch: else_block,
                };
                AnfBlock::new(stmts, tail, expr.span)
            }

            DesugaredExprKind::Match { expr: scrut, arms } => {
                let scrut_atom = self.lower_expr_to_atom(scrut, &mut stmts);
                let anf_arms = arms
                    .iter()
                    .map(|arm| AnfMatchArm {
                        pattern: arm.pattern.clone(),
                        body: self.lower_stmts_to_block(&arm.body, expr.span, is_fn_level),
                    })
                    .collect();
                let tail = AnfTail::Match {
                    scrutinee: scrut_atom,
                    arms: anf_arms,
                };
                AnfBlock::new(stmts, tail, expr.span)
            }

            DesugaredExprKind::Call { callee, args } if is_fn_level => {
                // Potential tail call at function level
                let callee_atom = self.lower_expr_to_atom(callee, &mut stmts);
                let arg_atoms = args
                    .iter()
                    .map(|a| self.lower_expr_to_atom(a, &mut stmts))
                    .collect();
                let tail = AnfTail::TailCall {
                    callee: callee_atom,
                    args: arg_atoms,
                };
                AnfBlock::new(stmts, tail, expr.span)
            }

            _ => {
                let atom = self.lower_expr_to_atom(expr, &mut stmts);
                let tail = if is_fn_level {
                    AnfTail::Return(Some(atom))
                } else {
                    AnfTail::Atom(atom)
                };
                AnfBlock::new(stmts, tail, expr.span)
            }
        }
    }

    /// Lowers an arbitrary desugared expression into an `Atom`, appending any intermediate
    /// let-bindings to `stmts`.
    pub fn lower_expr_to_atom(&mut self, expr: &DesugaredExpr, stmts: &mut Vec<AnfStmt>) -> Atom {
        match &expr.kind {
            DesugaredExprKind::Literal(lit) => Atom::Literal(lit.clone()),

            DesugaredExprKind::Ident(name) => Atom::Var(name.clone()),

            _ => {
                let tmp = self.fresh_var("t");
                self.var_types.insert(tmp.clone(), expr.ty.clone());
                self.lower_expr_into_var(expr, &tmp, expr.ty.clone(), stmts);
                Atom::Var(tmp)
            }
        }
    }

    /// Lowers an expression directly into a designated variable binding: `let var: ty = ...`.
    pub fn lower_expr_into_var(
        &mut self,
        expr: &DesugaredExpr,
        var: &str,
        ty: Type,
        stmts: &mut Vec<AnfStmt>,
    ) {
        let anf_expr = match &expr.kind {
            DesugaredExprKind::Literal(lit) => AnfExpr::Atom(Atom::Literal(lit.clone())),

            DesugaredExprKind::Ident(name) => AnfExpr::Atom(Atom::Var(name.clone())),

            DesugaredExprKind::Binary { lhs, op, rhs } => {
                let lhs_atom = self.lower_expr_to_atom(lhs, stmts);
                let rhs_atom = self.lower_expr_to_atom(rhs, stmts);
                AnfExpr::Binary {
                    op: *op,
                    lhs: lhs_atom,
                    rhs: rhs_atom,
                }
            }

            DesugaredExprKind::Unary { op, expr: sub } => {
                let sub_atom = self.lower_expr_to_atom(sub, stmts);
                AnfExpr::Unary {
                    op: *op,
                    operand: sub_atom,
                }
            }

            DesugaredExprKind::Call { callee, args } => {
                let callee_atom = self.lower_expr_to_atom(callee, stmts);
                let arg_atoms = args
                    .iter()
                    .map(|a| self.lower_expr_to_atom(a, stmts))
                    .collect();
                AnfExpr::Call {
                    callee: callee_atom,
                    args: arg_atoms,
                }
            }

            DesugaredExprKind::MethodCall {
                receiver,
                method,
                args,
            } => {
                let recv_atom = self.lower_expr_to_atom(receiver, stmts);
                let arg_atoms = args
                    .iter()
                    .map(|a| self.lower_expr_to_atom(a, stmts))
                    .collect();
                AnfExpr::MethodCall {
                    receiver: recv_atom,
                    method: method.clone(),
                    args: arg_atoms,
                }
            }

            DesugaredExprKind::FieldAccess { receiver, field } => {
                let recv_atom = self.lower_expr_to_atom(receiver, stmts);
                AnfExpr::FieldAccess {
                    receiver: recv_atom,
                    field: field.clone(),
                }
            }

            DesugaredExprKind::Index { receiver, index } => {
                let recv_atom = self.lower_expr_to_atom(receiver, stmts);
                let idx_atom = self.lower_expr_to_atom(index, stmts);
                AnfExpr::Index {
                    receiver: recv_atom,
                    index: idx_atom,
                }
            }

            DesugaredExprKind::Record(fields) => {
                let field_atoms = fields
                    .iter()
                    .map(|(n, e)| (n.clone(), self.lower_expr_to_atom(e, stmts)))
                    .collect();
                AnfExpr::Record {
                    fields: field_atoms,
                }
            }

            DesugaredExprKind::Array(elements) => {
                let elem_atoms = elements
                    .iter()
                    .map(|e| self.lower_expr_to_atom(e, stmts))
                    .collect();
                AnfExpr::Array {
                    elements: elem_atoms,
                }
            }

            DesugaredExprKind::Closure {
                params,
                return_type,
                body,
            } => {
                let block = self.lower_stmts_to_block(body, expr.span, true);
                AnfExpr::Closure {
                    params: params.clone(),
                    return_type: return_type.clone(),
                    body: block,
                }
            }

            DesugaredExprKind::If {
                condition,
                then_branch,
                else_branch,
            } => {
                let cond_atom = self.lower_expr_to_atom(condition, stmts);
                let then_block = self.lower_stmts_to_block(then_branch, expr.span, false);
                let else_block = match else_branch {
                    Some(eb) => self.lower_stmts_to_block(eb, expr.span, false),
                    None => AnfBlock::new(
                        Vec::new(),
                        AnfTail::Atom(Atom::Literal(Literal::Unit)),
                        expr.span,
                    ),
                };
                AnfExpr::If {
                    cond: cond_atom,
                    then_branch: Box::new(then_block),
                    else_branch: Box::new(else_block),
                }
            }

            DesugaredExprKind::Match { expr: scrut, arms } => {
                let scrut_atom = self.lower_expr_to_atom(scrut, stmts);
                let anf_arms = arms
                    .iter()
                    .map(|arm| AnfMatchArm {
                        pattern: arm.pattern.clone(),
                        body: self.lower_stmts_to_block(&arm.body, expr.span, false),
                    })
                    .collect();
                AnfExpr::Match {
                    scrutinee: scrut_atom,
                    arms: anf_arms,
                }
            }

            DesugaredExprKind::Block(block_stmts) => {
                let block = self.lower_stmts_to_block(block_stmts, expr.span, false);
                // In ANF, inline block statements into outer stmts and bind tail atom
                stmts.extend(block.stmts);
                match block.tail {
                    AnfTail::Atom(a) => AnfExpr::Atom(a),
                    AnfTail::Return(Some(a)) => AnfExpr::Atom(a),
                    AnfTail::Return(None) => AnfExpr::Atom(Atom::Literal(Literal::Unit)),
                    AnfTail::TailCall { callee, args } => AnfExpr::Call { callee, args },
                    AnfTail::If {
                        cond,
                        then_branch,
                        else_branch,
                    } => AnfExpr::If {
                        cond,
                        then_branch,
                        else_branch: else_branch.unwrap_or_else(|| {
                            Box::new(AnfBlock::new(
                                Vec::new(),
                                AnfTail::Atom(Atom::Literal(Literal::Unit)),
                                expr.span,
                            ))
                        }),
                    },
                    AnfTail::Match { scrutinee, arms } => AnfExpr::Match { scrutinee, arms },
                }
            }

            DesugaredExprKind::Cast {
                expr: sub_expr,
                target_type,
            } => {
                let atom = self.lower_expr_to_atom(sub_expr, stmts);
                AnfExpr::Cast {
                    expr: atom,
                    target_type: target_type.clone(),
                }
            }
        };

        stmts.push(AnfStmt::Let {
            var: var.to_string(),
            ty,
            value: anf_expr,
            span: expr.span,
        });
    }
}

fn block_always_returns(stmts: &[DesugaredStmt]) -> bool {
    stmts.iter().any(stmt_always_returns)
}

fn stmt_always_returns(stmt: &DesugaredStmt) -> bool {
    match stmt {
        DesugaredStmt::Return(..) => true,
        DesugaredStmt::Expr(expr) => expr_always_returns(expr),
        DesugaredStmt::Let { .. } => false,
    }
}

fn expr_always_returns(expr: &DesugaredExpr) -> bool {
    match &expr.kind {
        DesugaredExprKind::If {
            then_branch,
            else_branch: Some(eb),
            ..
        } => block_always_returns(then_branch) && block_always_returns(eb),
        DesugaredExprKind::Match { arms, .. } => {
            !arms.is_empty() && arms.iter().all(|arm| block_always_returns(&arm.body))
        }
        DesugaredExprKind::Block(stmts) => block_always_returns(stmts),
        _ => false,
    }
}
