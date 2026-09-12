//! Perceus Reference Counting and FBIP (Functional But In-Place) pass for Modus ANF IR.
//!
//! Implements:
//! 1. Zero-RC moves for variables at their last use (`last_uses`).
//! 2. Selective `inc_ref` insertion for shared uses of heap objects.
//! 3. Precise `dec_ref` insertion when heap variables go out of scope / die unconsumed.
//! 4. FBIP optimization: converting record constructions that project from a last-use base
//!    into `ReuseRecord` (which reuses the buffer in-place when `rc == 1`).

use crate::ir::liveness::{analyze_function_liveness, is_heap_type};
use crate::ir::node::*;
use crate::typechecker::Type;
use std::collections::{HashMap, HashSet};

/// Applies Perceus reference counting and FBIP optimization to all functions in an `AnfProgram`.
pub fn apply_perceus_and_fbip(prog: &mut AnfProgram) {
    for func in &mut prog.functions {
        optimize_function(func);
    }

    for im in &mut prog.impls {
        for m in &mut im.methods {
            optimize_function(m);
        }
    }
}

fn optimize_function(func: &mut AnfFunction) {
    let liveness = analyze_function_liveness(func);
    let mut ctx = PerceusCtx {
        var_types: liveness.var_types.clone(),
        consumed_vars: HashSet::new(),
    };

    func.body = ctx.optimize_block(&func.body, &liveness.block);
}

struct PerceusCtx {
    var_types: HashMap<String, Type>,
    consumed_vars: HashSet<String>,
}

impl PerceusCtx {
    fn is_heap_var(&self, var: &str) -> bool {
        self.var_types.get(var).map(is_heap_type).unwrap_or(false)
    }

    fn optimize_block(
        &mut self,
        block: &AnfBlock,
        liveness: &crate::ir::liveness::BlockLiveness,
    ) -> AnfBlock {
        let mut new_stmts = Vec::new();

        // Map field projections to find base records: field_var -> (base_record_var, field_name)
        let mut projections: HashMap<String, (String, String)> = HashMap::new();

        for (i, stmt) in block.stmts.iter().enumerate() {
            let stmt_liveness = &liveness.stmts[i];

            match stmt {
                AnfStmt::Let {
                    var,
                    ty,
                    value,
                    span,
                } => {
                    // Record field access projections for FBIP reuse detection:
                    // let _t = pt.x;
                    if let AnfExpr::FieldAccess { receiver, field } = value
                        && let Some(base_var) = receiver.as_var()
                    {
                        projections.insert(var.clone(), (base_var.to_string(), field.clone()));
                    }

                    // Check for FBIP reuse opportunity on record construction
                    let optimized_value =
                        self.try_optimize_fbip(value, &projections, &stmt_liveness.live_out);

                    // For each variable used by this expression:
                    // If the expression consumes it (e.g. into record, array, or call),
                    // track whether each occurrence is a move or shared copy.
                    if self.expr_consumes_operands(&optimized_value) {
                        let mut occurrences: HashMap<String, usize> = HashMap::new();
                        for v in optimized_value.var_occurrences() {
                            *occurrences.entry(v).or_insert(0) += 1;
                        }

                        for (used, count) in occurrences {
                            if self.is_heap_var(&used) {
                                if stmt_liveness.live_out.contains(&used) {
                                    // All occurrences are shared since `used` remains live after this stmt
                                    for _ in 0..count {
                                        new_stmts.push(AnfStmt::IncRef { var: used.clone() });
                                    }
                                } else {
                                    // First occurrence moves the variable (zero RC overhead).
                                    // Any additional occurrences (count - 1) require inc_ref.
                                    for _ in 1..count {
                                        new_stmts.push(AnfStmt::IncRef { var: used.clone() });
                                    }
                                    self.consumed_vars.insert(used.clone());
                                }
                            }
                        }
                    }

                    new_stmts.push(AnfStmt::Let {
                        var: var.clone(),
                        ty: ty.clone(),
                        value: optimized_value,
                        span: *span,
                    });

                    // Insert dec_ref for any heap variables that died at this statement unconsumed
                    for died_var in &stmt_liveness.died {
                        if died_var != var
                            && self.is_heap_var(died_var)
                            && !self.consumed_vars.contains(died_var)
                        {
                            new_stmts.push(AnfStmt::DecRef {
                                var: died_var.clone(),
                            });
                        }
                    }
                }

                AnfStmt::Expr(expr) => {
                    if self.expr_consumes_operands(expr) {
                        let mut occurrences: HashMap<String, usize> = HashMap::new();
                        for v in expr.var_occurrences() {
                            *occurrences.entry(v).or_insert(0) += 1;
                        }

                        for (used, count) in occurrences {
                            if self.is_heap_var(&used) {
                                if stmt_liveness.live_out.contains(&used) {
                                    for _ in 0..count {
                                        new_stmts.push(AnfStmt::IncRef { var: used.clone() });
                                    }
                                } else {
                                    for _ in 1..count {
                                        new_stmts.push(AnfStmt::IncRef { var: used.clone() });
                                    }
                                    self.consumed_vars.insert(used.clone());
                                }
                            }
                        }
                    }
                    new_stmts.push(stmt.clone());

                    for died_var in &stmt_liveness.died {
                        if self.is_heap_var(died_var) && !self.consumed_vars.contains(died_var) {
                            new_stmts.push(AnfStmt::DecRef {
                                var: died_var.clone(),
                            });
                        }
                    }
                }

                _ => {
                    new_stmts.push(stmt.clone());
                }
            }
        }

        // Optimize tail
        let new_tail = match &block.tail {
            AnfTail::Return(Some(atom)) => {
                if let Some(v) = atom.as_var() {
                    // If returning a shared variable that is still in scope, inc_ref
                    if self.is_heap_var(v) && !liveness.tail.last_uses.contains(v) {
                        new_stmts.push(AnfStmt::IncRef { var: v.to_string() });
                    }
                }
                block.tail.clone()
            }
            AnfTail::TailCall { callee, args } => {
                for a in args {
                    if let Some(v) = a.as_var()
                        && self.is_heap_var(v)
                        && !liveness.tail.last_uses.contains(v)
                    {
                        new_stmts.push(AnfStmt::IncRef { var: v.to_string() });
                    }
                }
                if let Some(v) = callee.as_var()
                    && self.is_heap_var(v)
                    && !liveness.tail.last_uses.contains(v)
                {
                    new_stmts.push(AnfStmt::IncRef { var: v.to_string() });
                }
                block.tail.clone()
            }
            AnfTail::If {
                cond,
                then_branch,
                else_branch,
            } => {
                // Recursively optimize branches
                let then_l = crate::ir::liveness::analyze_block_liveness(
                    then_branch,
                    &HashSet::new(),
                    &self.var_types,
                );
                let opt_then = self.optimize_block(then_branch, &then_l);

                let opt_else = else_branch.as_ref().map(|eb| {
                    let else_l = crate::ir::liveness::analyze_block_liveness(
                        eb,
                        &HashSet::new(),
                        &self.var_types,
                    );
                    Box::new(self.optimize_block(eb, &else_l))
                });

                AnfTail::If {
                    cond: cond.clone(),
                    then_branch: Box::new(opt_then),
                    else_branch: opt_else,
                }
            }
            AnfTail::Match { scrutinee, arms } => {
                let mut opt_arms = Vec::new();
                for arm in arms {
                    let arm_l = crate::ir::liveness::analyze_block_liveness(
                        &arm.body,
                        &HashSet::new(),
                        &self.var_types,
                    );
                    let opt_body = self.optimize_block(&arm.body, &arm_l);
                    opt_arms.push(AnfMatchArm {
                        pattern: arm.pattern.clone(),
                        body: opt_body,
                    });
                }
                AnfTail::Match {
                    scrutinee: scrutinee.clone(),
                    arms: opt_arms,
                }
            }
            _ => block.tail.clone(),
        };

        AnfBlock::new(new_stmts, new_tail, block.span)
    }

    /// Determines if an expression consumes ownership of its operand values.
    fn expr_consumes_operands(&self, expr: &AnfExpr) -> bool {
        matches!(
            expr,
            AnfExpr::Record { .. }
                | AnfExpr::Array { .. }
                | AnfExpr::Tuple { .. }
                | AnfExpr::Variant { .. }
                | AnfExpr::MakeClosure { .. }
                | AnfExpr::Call { .. }
                | AnfExpr::MethodCall { .. }
                | AnfExpr::CallClosure { .. }
                | AnfExpr::Atom(Atom::Var(_))
                | AnfExpr::Unary {
                    op: crate::desugar::DesugaredUnaryOp::Perform,
                    ..
                }
        )
    }

    /// Tries to apply FBIP (Functional But In-Place) optimization to a record constructor.
    ///
    /// If a record constructor `{ f1: v1, f2: v2, ... }` has fields that are mostly projections
    /// from a base record `b`, and `b` is at its last use (not in `live_out`),
    /// we convert this into `AnfExpr::ReuseRecord { base: Atom::Var(b), fields: updated_fields }`.
    fn try_optimize_fbip(
        &mut self,
        expr: &AnfExpr,
        projections: &HashMap<String, (String, String)>,
        live_out: &HashSet<String>,
    ) -> AnfExpr {
        if let AnfExpr::Record { fields } = expr {
            // Count projections from candidate base variables
            let mut base_counts: HashMap<String, usize> = HashMap::new();
            let mut updated_fields = Vec::new();

            for (f_name, atom) in fields {
                if let Some(var_name) = atom.as_var()
                    && let Some((base_var, orig_field)) = projections.get(var_name)
                    && orig_field == f_name
                {
                    *base_counts.entry(base_var.clone()).or_insert(0) += 1;
                    continue;
                }
                updated_fields.push((f_name.clone(), atom.clone()));
            }

            // Find base candidate that has at least one projected field and is NOT live afterwards
            for (base_var, projected_count) in base_counts {
                if projected_count > 0 && !live_out.contains(&base_var) {
                    // Candidate base record can be reused in-place!
                    self.consumed_vars.insert(base_var.clone());
                    return AnfExpr::ReuseRecord {
                        base: Atom::Var(base_var),
                        fields: updated_fields,
                    };
                }
            }
        }

        expr.clone()
    }
}
