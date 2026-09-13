//! Closure conversion for ANF IR.
//!
//! Performs:
//! 1. Free variable analysis for closures.
//! 2. Lambda lifting: extracting closures into top-level functions (`_lambda_N`).
//! 3. Environment record creation (`_Env_lambda_N`) and field unpacking.
//! 4. Replacing closure expressions with `MakeClosure(fn_name, env_atom)`.
//! 5. Tagging closure invocations as `CallClosure`.

use crate::ast::{self, Span, Spanned, TypeDecl, TypeDef};
use crate::desugar::DesugaredPattern;
use crate::ir::node::*;
use crate::typechecker::Type;
use std::collections::{BTreeMap, HashMap, HashSet};

/// Converts all closures in an `AnfProgram` into top-level functions and environment structs.
pub fn convert_closures(prog: &mut AnfProgram) {
    let mut ctx = ClosureConvertCtx::new(prog);
    ctx.convert_program(prog);
}

struct ClosureConvertCtx {
    lambda_counter: usize,
    global_names: HashSet<String>,
    new_functions: Vec<AnfFunction>,
    new_types: Vec<TypeDecl>,
}

impl ClosureConvertCtx {
    fn new(prog: &AnfProgram) -> Self {
        let mut global_names = HashSet::new();
        for f in &prog.functions {
            global_names.insert(f.name.clone());
        }
        for t in &prog.types {
            global_names.insert(t.name.clone());
        }
        for tr in &prog.traits {
            global_names.insert(tr.name.clone());
        }
        for ext in &prog.extern_functions {
            global_names.insert(ext.name.clone());
            global_names.insert(ext.symbol_name.clone());
        }
        // Builtins
        global_names.insert("IO".to_string());
        global_names.insert("Result".to_string());
        global_names.insert("Option".to_string());

        Self {
            lambda_counter: 0,
            global_names,
            new_functions: Vec::new(),
            new_types: Vec::new(),
        }
    }

    fn fresh_lambda_name(&mut self) -> String {
        let name = format!("_lambda_{}", self.lambda_counter);
        self.lambda_counter += 1;
        name
    }

    fn convert_program(&mut self, prog: &mut AnfProgram) {
        let mut converted_functions = Vec::new();
        for func in &prog.functions {
            converted_functions.push(self.convert_function(func));
        }

        let mut converted_impls = Vec::new();
        for im in &prog.impls {
            let mut converted_methods = Vec::new();
            for m in &im.methods {
                converted_methods.push(self.convert_function(m));
            }
            converted_impls.push(AnfImpl {
                trait_name: im.trait_name.clone(),
                target_type: im.target_type.clone(),
                methods: converted_methods,
                span: im.span,
            });
        }

        // Add converted functions and newly lifted lambda functions
        converted_functions.append(&mut self.new_functions);
        prog.functions = converted_functions;
        prog.impls = converted_impls;
        prog.types.append(&mut self.new_types);
    }

    fn convert_function(&mut self, func: &AnfFunction) -> AnfFunction {
        let mut var_types = HashMap::new();
        for (name, ty) in &func.params {
            var_types.insert(name.clone(), ty.clone());
        }

        let body = self.convert_block(&func.body, &mut var_types);

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

    fn convert_block(
        &mut self,
        block: &AnfBlock,
        var_types: &mut HashMap<String, Type>,
    ) -> AnfBlock {
        let mut new_stmts = Vec::new();

        for stmt in &block.stmts {
            match stmt {
                AnfStmt::Let {
                    var,
                    ty,
                    value,
                    span,
                } => {
                    var_types.insert(var.clone(), ty.clone());
                    let converted_val = self.convert_expr(value, &mut new_stmts, var_types, *span);
                    new_stmts.push(AnfStmt::Let {
                        var: var.clone(),
                        ty: ty.clone(),
                        value: converted_val,
                        span: *span,
                    });
                }
                AnfStmt::Expr(expr) => {
                    let converted_expr =
                        self.convert_expr(expr, &mut new_stmts, var_types, block.span);
                    new_stmts.push(AnfStmt::Expr(converted_expr));
                }
                _ => {
                    new_stmts.push(stmt.clone());
                }
            }
        }

        let new_tail = self.convert_tail(&block.tail, var_types);
        AnfBlock::new(new_stmts, new_tail, block.span)
    }

    fn convert_tail(&mut self, tail: &AnfTail, var_types: &mut HashMap<String, Type>) -> AnfTail {
        match tail {
            AnfTail::If {
                cond,
                then_branch,
                else_branch,
            } => {
                let mut then_vars = var_types.clone();
                let new_then = self.convert_block(then_branch, &mut then_vars);
                let new_else = else_branch.as_ref().map(|eb| {
                    let mut else_vars = var_types.clone();
                    Box::new(self.convert_block(eb, &mut else_vars))
                });
                AnfTail::If {
                    cond: cond.clone(),
                    then_branch: Box::new(new_then),
                    else_branch: new_else,
                }
            }
            AnfTail::Match { scrutinee, arms } => {
                let mut new_arms = Vec::new();
                for arm in arms {
                    let mut arm_vars = var_types.clone();
                    // Add pattern bound variables to scope
                    for p_var in pattern_bound_vars(&arm.pattern) {
                        arm_vars.insert(p_var, Type::void());
                    }
                    let new_body = self.convert_block(&arm.body, &mut arm_vars);
                    new_arms.push(AnfMatchArm {
                        pattern: arm.pattern.clone(),
                        body: new_body,
                    });
                }
                AnfTail::Match {
                    scrutinee: scrutinee.clone(),
                    arms: new_arms,
                }
            }
            AnfTail::TailCall { callee, args } => {
                // If callee is a closure variable, convert to CallClosure
                if let Atom::Var(name) = callee
                    && let Some(Type::Function { .. }) = var_types.get(name)
                {
                    return AnfTail::TailCall {
                        callee: callee.clone(),
                        args: args.clone(),
                    };
                }
                AnfTail::TailCall {
                    callee: callee.clone(),
                    args: args.clone(),
                }
            }
            _ => tail.clone(),
        }
    }

    fn convert_expr(
        &mut self,
        expr: &AnfExpr,
        pre_stmts: &mut Vec<AnfStmt>,
        var_types: &mut HashMap<String, Type>,
        span: Span,
    ) -> AnfExpr {
        match expr {
            AnfExpr::Closure {
                params,
                return_type,
                body,
            } => {
                // 1. Convert nested closures first
                let mut inner_var_types = var_types.clone();
                for (p, ty) in params {
                    inner_var_types.insert(p.clone(), ty.clone());
                }
                let mut converted_body = self.convert_block(body, &mut inner_var_types);

                // 2. Compute free variables
                let mut bound_vars = HashSet::new();
                for (p, _) in params {
                    bound_vars.insert(p.clone());
                }
                for v in converted_body.defined_vars() {
                    bound_vars.insert(v);
                }

                let mut free_vars_set = HashSet::new();
                for u in converted_body.used_vars() {
                    if !bound_vars.contains(&u) && !self.global_names.contains(&u) {
                        free_vars_set.insert(u);
                    }
                }

                let mut free_vars: Vec<String> = free_vars_set.into_iter().collect();
                free_vars.sort();

                let lambda_name = self.fresh_lambda_name();
                self.global_names.insert(lambda_name.clone());

                if free_vars.is_empty() {
                    // No captured variables: lifted function with unused _env pointer parameter
                    let mut lifted_params = vec![("_env".to_string(), Type::pointer(Type::u8()))];
                    lifted_params.extend(params.clone());

                    let lifted_fn = AnfFunction {
                        name: lambda_name.clone(),
                        type_params: Vec::new(),
                        params: lifted_params,
                        return_type: return_type.clone(),
                        body: converted_body,
                        is_effectful: return_type.is_io(),
                        span,
                    };
                    self.new_functions.push(lifted_fn);

                    AnfExpr::MakeClosure {
                        fn_name: lambda_name,
                        env: None,
                    }
                } else {
                    // Captured variables: construct environment struct and unpack in lambda
                    let env_type_name = format!("_Env_{lambda_name}");
                    let mut env_fields = BTreeMap::new();
                    let mut free_var_types = Vec::new();

                    for fv in &free_vars {
                        let fv_ty = var_types.get(fv).cloned().unwrap_or(Type::void());
                        env_fields.insert(fv.clone(), fv_ty.clone());
                        free_var_types.push((fv.clone(), fv_ty));
                    }

                    // Register environment type declaration
                    let env_record_type = Type::Record(env_fields.clone());
                    let env_type_decl = TypeDecl {
                        name: env_type_name.clone(),
                        type_params: Vec::new(),
                        definition: Spanned::new(
                            TypeDef::Alias(sem_type_to_ast_type(&env_record_type)),
                            span,
                        ),
                        is_exported: false,
                    };
                    self.new_types.push(env_type_decl);
                    self.global_names.insert(env_type_name.clone());

                    // Prepend field unpacking statements to lambda body:
                    // let fv = _env.fv;
                    let mut unpack_stmts = Vec::new();
                    for (fv, fv_ty) in &free_var_types {
                        unpack_stmts.push(AnfStmt::Let {
                            var: fv.clone(),
                            ty: fv_ty.clone(),
                            value: AnfExpr::FieldAccess {
                                receiver: Atom::Var("_env".to_string()),
                                field: fv.clone(),
                            },
                            span,
                        });
                    }
                    unpack_stmts.extend(converted_body.stmts);
                    converted_body.stmts = unpack_stmts;

                    // Lambda function parameters: (_env: _Env_lambda_N, params...)
                    let mut lifted_params = vec![(
                        "_env".to_string(),
                        Type::Named {
                            name: env_type_name.clone(),
                            args: Vec::new(),
                        },
                    )];
                    lifted_params.extend(params.clone());

                    let lifted_fn = AnfFunction {
                        name: lambda_name.clone(),
                        type_params: Vec::new(),
                        params: lifted_params,
                        return_type: return_type.clone(),
                        body: converted_body,
                        is_effectful: return_type.is_io(),
                        span,
                    };
                    self.new_functions.push(lifted_fn);

                    // At instantiation site:
                    // let _env_lambda_N = { fv1: fv1, fv2: fv2, ... };
                    let env_var = format!("_env_{lambda_name}");
                    let env_record_expr = AnfExpr::Record {
                        fields: free_vars
                            .iter()
                            .map(|fv| (fv.clone(), Atom::Var(fv.clone())))
                            .collect(),
                    };
                    pre_stmts.push(AnfStmt::Let {
                        var: env_var.clone(),
                        ty: Type::Named {
                            name: env_type_name,
                            args: Vec::new(),
                        },
                        value: env_record_expr,
                        span,
                    });

                    AnfExpr::MakeClosure {
                        fn_name: lambda_name,
                        env: Some(Atom::Var(env_var)),
                    }
                }
            }

            AnfExpr::Call { callee, args } => {
                if let Atom::Var(name) = callee
                    && let Some(Type::Function { .. }) = var_types.get(name)
                {
                    return AnfExpr::CallClosure {
                        closure: callee.clone(),
                        args: args.clone(),
                    };
                }
                AnfExpr::Call {
                    callee: callee.clone(),
                    args: args.clone(),
                }
            }

            AnfExpr::If {
                cond,
                then_branch,
                else_branch,
            } => {
                let mut then_vars = var_types.clone();
                let new_then = self.convert_block(then_branch, &mut then_vars);
                let mut else_vars = var_types.clone();
                let new_else = self.convert_block(else_branch, &mut else_vars);
                AnfExpr::If {
                    cond: cond.clone(),
                    then_branch: Box::new(new_then),
                    else_branch: Box::new(new_else),
                }
            }

            AnfExpr::Match { scrutinee, arms } => {
                let mut new_arms = Vec::new();
                for arm in arms {
                    let mut arm_vars = var_types.clone();
                    for p_var in pattern_bound_vars(&arm.pattern) {
                        arm_vars.insert(p_var, Type::void());
                    }
                    let new_body = self.convert_block(&arm.body, &mut arm_vars);
                    new_arms.push(AnfMatchArm {
                        pattern: arm.pattern.clone(),
                        body: new_body,
                    });
                }
                AnfExpr::Match {
                    scrutinee: scrutinee.clone(),
                    arms: new_arms,
                }
            }

            _ => expr.clone(),
        }
    }
}

/// Helper to collect variable names introduced by a pattern.
pub fn pattern_bound_vars(pat: &DesugaredPattern) -> HashSet<String> {
    let mut vars = HashSet::new();
    match pat {
        DesugaredPattern::Ident(name) => {
            vars.insert(name.clone());
        }
        DesugaredPattern::Variant { patterns, .. } => {
            for p in patterns {
                vars.extend(pattern_bound_vars(p));
            }
        }
        DesugaredPattern::Record(fields) => {
            for (name, sub_pat) in fields {
                if let Some(p) = sub_pat {
                    vars.extend(pattern_bound_vars(p));
                } else {
                    vars.insert(name.clone());
                }
            }
        }
        DesugaredPattern::Tuple(elems) => {
            for p in elems {
                vars.extend(pattern_bound_vars(p));
            }
        }
        DesugaredPattern::Wildcard | DesugaredPattern::Literal(_) => {}
    }
    vars
}

/// Converts a semantic `Type` to an AST `Type` for type declarations.
fn sem_type_to_ast_type(ty: &Type) -> ast::Type {
    match ty {
        Type::Primitive(p) => ast::Type::Primitive(*p),
        Type::Unit => ast::Type::Unit,
        Type::Array(inner) => ast::Type::Array(Box::new(Spanned::new(
            sem_type_to_ast_type(inner),
            Span::default(),
        ))),
        Type::Record(fields) => {
            let ast_fields = fields
                .iter()
                .map(|(n, t)| {
                    (
                        n.clone(),
                        Spanned::new(sem_type_to_ast_type(t), Span::default()),
                    )
                })
                .collect();
            ast::Type::Record(ast_fields)
        }
        Type::Tuple(elems) => {
            let ast_elems = elems
                .iter()
                .map(|t| Spanned::new(sem_type_to_ast_type(t), Span::default()))
                .collect();
            ast::Type::Tuple(ast_elems)
        }
        Type::Named { name, args } => {
            if args.is_empty() {
                ast::Type::Path(vec![name.clone()])
            } else {
                let type_args = args
                    .iter()
                    .map(|t| Spanned::new(sem_type_to_ast_type(t), Span::default()))
                    .collect();
                ast::Type::Generic {
                    name: name.clone(),
                    type_args,
                }
            }
        }
        Type::Function { params, ret } => {
            let param_types = params
                .iter()
                .map(|t| Spanned::new(sem_type_to_ast_type(t), Span::default()))
                .collect();
            ast::Type::Function {
                param_types,
                return_type: Box::new(Spanned::new(sem_type_to_ast_type(ret), Span::default())),
            }
        }
        _ => ast::Type::Primitive(ast::PrimitiveType::Void),
    }
}
