//! Liveness and Borrow Analysis for Modus ANF IR.
//!
//! Analyzes variable lifecycles across basic blocks and control-flow branches:
//! - Computes `live_in` and `live_out` sets for each statement and block.
//! - Detects `last_uses` (variables whose last use is at this point, enabling zero-RC moves).
//! - Identifies shared uses requiring `inc_ref`.
//! - Identifies dead heap variables requiring `dec_ref`.
//! - Distinguishes unboxed primitive types from heap-allocated RC types.

use crate::ir::closure::pattern_bound_vars;
use crate::ir::node::*;
use crate::typechecker::Type;
use std::collections::{HashMap, HashSet};

/// Checks if a type is heap-allocated and managed by Perceus reference counting.
/// Primitives (numbers, bool, void, unit) are unboxed and require no RC.
pub fn is_heap_type(ty: &Type) -> bool {
    match ty {
        Type::Primitive(_) | Type::Unit => false,
        Type::Named { name, args } => {
            if name == "Pointer" || name == "CString" {
                return false;
            }
            if name == "IO" {
                return args.first().map(is_heap_type).unwrap_or(false);
            }
            true
        }
        Type::Array(_)
        | Type::Record(_)
        | Type::Tuple(_)
        | Type::Function { .. }
        | Type::TraitObject(_) => true,
        // Conservatively treat type variables and generic bounds as potential heap objects
        Type::Var(_) | Type::GenericParam(_) => true,
    }
}

/// Liveness information for an individual ANF statement.
#[derive(Debug, Clone, PartialEq)]
pub struct StmtLiveness {
    /// Variables live immediately before executing this statement
    pub live_in: HashSet<String>,
    /// Variables live immediately after executing this statement
    pub live_out: HashSet<String>,
    /// Variables defined by this statement
    pub defs: HashSet<String>,
    /// Variables read/used by this statement
    pub uses: HashSet<String>,
    /// Variables whose last use occurs in this statement (eligible for move / zero-RC traffic)
    pub last_uses: HashSet<String>,
    /// Heap variables that die after this statement without being consumed
    pub died: HashSet<String>,
}

/// Liveness information for a block terminator.
#[derive(Debug, Clone, PartialEq)]
pub struct TailLiveness {
    /// Variables live at entry to this terminator
    pub live_in: HashSet<String>,
    /// Variables read/used by this terminator
    pub uses: HashSet<String>,
    /// Variables whose last use occurs in this terminator
    pub last_uses: HashSet<String>,
}

/// Liveness analysis result for an `AnfBlock`.
#[derive(Debug, Clone, PartialEq)]
pub struct BlockLiveness {
    pub stmts: Vec<StmtLiveness>,
    pub tail: TailLiveness,
}

impl BlockLiveness {
    pub fn live_in(&self) -> HashSet<String> {
        if let Some(first) = self.stmts.first() {
            first.live_in.clone()
        } else {
            self.tail.live_in.clone()
        }
    }
}

/// Complete liveness analysis for an `AnfFunction`.
#[derive(Debug, Clone, PartialEq)]
pub struct FunctionLiveness {
    pub fn_name: String,
    pub var_types: HashMap<String, Type>,
    pub block: BlockLiveness,
}

/// Analyzes variable liveness across a function.
pub fn analyze_function_liveness(func: &AnfFunction) -> FunctionLiveness {
    let mut var_types = HashMap::new();
    for (p, ty) in &func.params {
        var_types.insert(p.clone(), ty.clone());
    }
    collect_var_types_block(&func.body, &mut var_types);

    let initial_live_out = HashSet::new();
    let block = analyze_block_liveness(&func.body, &initial_live_out, &var_types);

    FunctionLiveness {
        fn_name: func.name.clone(),
        var_types,
        block,
    }
}

fn collect_var_types_block(block: &AnfBlock, var_types: &mut HashMap<String, Type>) {
    for stmt in &block.stmts {
        if let AnfStmt::Let { var, ty, value, .. } = stmt {
            var_types.insert(var.clone(), ty.clone());
            match value {
                AnfExpr::If {
                    then_branch,
                    else_branch,
                    ..
                } => {
                    collect_var_types_block(then_branch, var_types);
                    collect_var_types_block(else_branch, var_types);
                }
                AnfExpr::Match { arms, .. } => {
                    for arm in arms {
                        collect_var_types_block(&arm.body, var_types);
                    }
                }
                AnfExpr::Closure { body, .. } => {
                    collect_var_types_block(body, var_types);
                }
                _ => {}
            }
        }
    }

    match &block.tail {
        AnfTail::If {
            then_branch,
            else_branch,
            ..
        } => {
            collect_var_types_block(then_branch, var_types);
            if let Some(eb) = else_branch {
                collect_var_types_block(eb, var_types);
            }
        }
        AnfTail::Match { arms, .. } => {
            for arm in arms {
                collect_var_types_block(&arm.body, var_types);
            }
        }
        _ => {}
    }
}

/// Backward liveness analysis for a block.
pub fn analyze_block_liveness(
    block: &AnfBlock,
    exit_live_out: &HashSet<String>,
    var_types: &HashMap<String, Type>,
) -> BlockLiveness {
    // 1. Analyze tail
    let (tail_liveness, mut current_live) =
        analyze_tail_liveness(&block.tail, exit_live_out, var_types);

    // 2. Backward propagation through statements
    let mut stmt_liveness = Vec::with_capacity(block.stmts.len());

    for stmt in block.stmts.iter().rev() {
        let live_out = current_live.clone();

        let mut defs = HashSet::new();
        if let Some(d) = stmt.defined_var() {
            defs.insert(d.to_string());
        }

        let uses = match stmt {
            AnfStmt::Let { value, .. } => match value {
                AnfExpr::If {
                    cond,
                    then_branch,
                    else_branch,
                } => {
                    let mut u = HashSet::new();
                    if let Some(v) = cond.as_var() {
                        u.insert(v.to_string());
                    }
                    let then_l = analyze_block_liveness(then_branch, &live_out, var_types);
                    let else_l = analyze_block_liveness(else_branch, &live_out, var_types);
                    u.extend(then_l.live_in());
                    u.extend(else_l.live_in());
                    u
                }
                AnfExpr::Match { scrutinee, arms } => {
                    let mut u = HashSet::new();
                    if let Some(v) = scrutinee.as_var() {
                        u.insert(v.to_string());
                    }
                    for arm in arms {
                        let arm_l = analyze_block_liveness(&arm.body, &live_out, var_types);
                        let bound = pattern_bound_vars(&arm.pattern);
                        for v in arm_l.live_in() {
                            if !bound.contains(&v) {
                                u.insert(v);
                            }
                        }
                    }
                    u
                }
                _ => stmt.used_vars(),
            },
            _ => stmt.used_vars(),
        };

        // live_in = uses ∪ (live_out \ defs)
        let mut live_in = uses.clone();
        for v in &live_out {
            if !defs.contains(v) {
                live_in.insert(v.clone());
            }
        }

        // last_uses = { u ∈ uses | u ∉ live_out }
        let mut last_uses = HashSet::new();
        for u in &uses {
            if !live_out.contains(u) {
                last_uses.insert(u.clone());
            }
        }

        // Variables that died: in (live_in ∪ defs) but not in live_out
        let mut died = HashSet::new();
        for v in live_in.union(&defs) {
            if !live_out.contains(v) {
                // Only track heap-allocated types
                let is_heap = var_types.get(v).map(is_heap_type).unwrap_or(false);
                if is_heap {
                    died.insert(v.clone());
                }
            }
        }

        stmt_liveness.push(StmtLiveness {
            live_in: live_in.clone(),
            live_out,
            defs,
            uses,
            last_uses,
            died,
        });

        current_live = live_in;
    }

    stmt_liveness.reverse();

    BlockLiveness {
        stmts: stmt_liveness,
        tail: tail_liveness,
    }
}

fn analyze_tail_liveness(
    tail: &AnfTail,
    exit_live_out: &HashSet<String>,
    var_types: &HashMap<String, Type>,
) -> (TailLiveness, HashSet<String>) {
    match tail {
        AnfTail::Return(opt_atom) => {
            let mut uses = HashSet::new();
            if let Some(a) = opt_atom
                && let Some(v) = a.as_var()
            {
                uses.insert(v.to_string());
            }
            let live_in = uses.clone();
            let last_uses = uses.clone();
            (
                TailLiveness {
                    live_in: live_in.clone(),
                    uses,
                    last_uses,
                },
                live_in,
            )
        }

        AnfTail::TailCall { callee, args } => {
            let mut uses = HashSet::new();
            if let Some(v) = callee.as_var() {
                uses.insert(v.to_string());
            }
            for a in args {
                if let Some(v) = a.as_var() {
                    uses.insert(v.to_string());
                }
            }
            let live_in = uses.clone();
            let last_uses = uses.clone();
            (
                TailLiveness {
                    live_in: live_in.clone(),
                    uses,
                    last_uses,
                },
                live_in,
            )
        }

        AnfTail::Atom(atom) => {
            let mut uses = HashSet::new();
            if let Some(v) = atom.as_var() {
                uses.insert(v.to_string());
            }
            let mut live_in = uses.clone();
            live_in.extend(exit_live_out.clone());
            let mut last_uses = HashSet::new();
            for u in &uses {
                if !exit_live_out.contains(u) {
                    last_uses.insert(u.clone());
                }
            }
            (
                TailLiveness {
                    live_in: live_in.clone(),
                    uses,
                    last_uses,
                },
                live_in,
            )
        }

        AnfTail::If {
            cond,
            then_branch,
            else_branch,
        } => {
            let mut uses = HashSet::new();
            if let Some(v) = cond.as_var() {
                uses.insert(v.to_string());
            }

            let then_l = analyze_block_liveness(then_branch, exit_live_out, var_types);
            let else_l = else_branch
                .as_ref()
                .map(|eb| analyze_block_liveness(eb, exit_live_out, var_types));

            let mut live_in = uses.clone();
            live_in.extend(then_l.live_in());
            if let Some(el) = &else_l {
                live_in.extend(el.live_in());
            }

            let mut last_uses = HashSet::new();
            for u in &uses {
                if !exit_live_out.contains(u) {
                    last_uses.insert(u.clone());
                }
            }

            (
                TailLiveness {
                    live_in: live_in.clone(),
                    uses,
                    last_uses,
                },
                live_in,
            )
        }

        AnfTail::Match { scrutinee, arms } => {
            let mut uses = HashSet::new();
            if let Some(v) = scrutinee.as_var() {
                uses.insert(v.to_string());
            }

            let mut live_in = uses.clone();
            for arm in arms {
                let arm_l = analyze_block_liveness(&arm.body, exit_live_out, var_types);
                let bound = pattern_bound_vars(&arm.pattern);
                for v in arm_l.live_in() {
                    if !bound.contains(&v) {
                        live_in.insert(v);
                    }
                }
            }

            let mut last_uses = HashSet::new();
            for u in &uses {
                if !exit_live_out.contains(u) {
                    last_uses.insert(u.clone());
                }
            }

            (
                TailLiveness {
                    live_in: live_in.clone(),
                    uses,
                    last_uses,
                },
                live_in,
            )
        }
    }
}
