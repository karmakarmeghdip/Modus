//! Semantic Analysis & Typechecker for Modus.

pub mod error;
pub mod infer;
pub mod purity;
pub mod scope;
pub mod traits;
pub mod types;

pub use error::{TypeError, TypeErrorKind};
pub use infer::TypeInferrer;
pub use purity::EffectContext;
pub use scope::{
    ConstructorInfo, Environment, FunctionSig, ImplDef, TraitDef, TypeDefInfo, VariantInfo,
};
pub use traits::TraitResolver;
pub use types::{Substitution, Type, TypeVarGen, TypeVarId};

use crate::ast::{
    Declaration, FunctionBody, FunctionDecl, ImplDecl, Program, TraitDecl, TypeDecl, TypeDef,
};
use std::collections::HashMap;

/// Type-check a complete Modus AST program using a provided environment.
pub fn check_program_with_env(program: &Program, env: &mut Environment) -> Result<(), TypeError> {
    // Pass 0: Verify function body presence / absence invariant
    let is_header = program.library.is_some();
    for decl in &program.declarations {
        match &decl.node {
            Declaration::Function(func_decl) => {
                if is_header {
                    if func_decl.body.is_some() {
                        return Err(TypeError::new(
                            TypeErrorKind::UnexpectedFunctionBodyInHeader {
                                function_name: func_decl.name.clone(),
                            },
                            Some(decl.span),
                        ));
                    }
                } else if func_decl.body.is_none() {
                    return Err(TypeError::new(
                        TypeErrorKind::MissingFunctionBody {
                            function_name: func_decl.name.clone(),
                        },
                        Some(decl.span),
                    ));
                }
            }
            Declaration::Extern(ext) => {
                for f in &ext.functions {
                    if f.node.body.is_some() {
                        return Err(TypeError::new(
                            TypeErrorKind::General(format!(
                                "Extern function '{}' cannot have an implementation body. Remove the body and terminate the signature with ';'",
                                f.node.name
                            )),
                            Some(f.span),
                        ));
                    }
                }
            }
            _ => {}
        }
    }

    // Pass 1: Register all type declarations
    for decl in &program.declarations {
        if let Declaration::Type(type_decl) = &decl.node {
            register_type_decl(env, type_decl, decl.span)?;
        }
    }

    // Pass 2: Register all trait declarations
    for decl in &program.declarations {
        if let Declaration::Trait(trait_decl) = &decl.node {
            register_trait_decl(env, trait_decl, decl.span)?;
        }
    }

    // Pass 3: Register and verify all impl blocks
    for decl in &program.declarations {
        if let Declaration::Impl(impl_decl) = &decl.node {
            register_impl_decl(env, impl_decl, decl.span)?;
        }
    }

    // Pass 4: Register all function signatures and verify purity/dead-computation rules
    for decl in &program.declarations {
        match &decl.node {
            Declaration::Function(func_decl) => {
                register_function_sig(env, func_decl, decl.span)?;
            }
            Declaration::Extern(ext) => {
                for f in &ext.functions {
                    register_extern_sig(env, &f.node, f.span)?;
                }
            }
            _ => {}
        }
    }

    // Pass 5: Type check all function bodies
    for decl in &program.declarations {
        if let Declaration::Function(func_decl) = &decl.node {
            check_function_body(env, func_decl)?;
        }
    }

    // Pass 6: Type check all impl method bodies
    for decl in &program.declarations {
        if let Declaration::Impl(impl_decl) = &decl.node {
            check_impl_bodies(env, impl_decl)?;
        }
    }

    Ok(())
}

/// Type-check a complete Modus AST program
pub fn check_program(program: &Program) -> Result<Environment, TypeError> {
    let mut env = Environment::new();
    check_program_with_env(program, &mut env)?;
    Ok(env)
}

fn register_type_decl(
    env: &mut Environment,
    type_decl: &TypeDecl,
    span: crate::ast::Span,
) -> Result<(), TypeError> {
    let type_name = &type_decl.name;
    let generic_names: Vec<String> = type_decl
        .type_params
        .iter()
        .map(|p| p.name.clone())
        .collect();

    match &type_decl.definition.node {
        TypeDef::Alias(ast_ty) => {
            let expanded_type =
                env.resolve_ast_type(ast_ty, &generic_names, Some(type_decl.definition.span))?;
            env.types.insert(
                type_name.clone(),
                TypeDefInfo::Alias {
                    name: type_name.clone(),
                    type_params: type_decl.type_params.clone(),
                    expanded_type,
                },
            );
        }
        TypeDef::Union(variants) => {
            let mut variant_infos = Vec::new();
            for var in variants {
                let mut fields = Vec::new();
                for f in &var.fields {
                    let f_ty = env.resolve_ast_type(&f.node, &generic_names, Some(f.span))?;
                    fields.push(f_ty);
                }
                variant_infos.push(VariantInfo {
                    name: var.name.clone(),
                    fields: fields.clone(),
                });

                // Register constructors
                let parent_type = Type::Named {
                    name: type_name.clone(),
                    args: type_decl
                        .type_params
                        .iter()
                        .map(|p| Type::GenericParam(p.name.clone()))
                        .collect(),
                };

                let ctor_info = if fields.is_empty() {
                    ConstructorInfo::Value {
                        parent_type,
                        type_params: type_decl.type_params.clone(),
                    }
                } else {
                    ConstructorInfo::Function {
                        params: fields,
                        return_type: parent_type,
                        type_params: type_decl.type_params.clone(),
                    }
                };

                // Available as TypeName.Variant and Variant
                let qualified = format!("{type_name}.{}", var.name);
                env.constructors.insert(qualified, ctor_info.clone());
                env.constructors.insert(var.name.clone(), ctor_info);
            }

            env.types.insert(
                type_name.clone(),
                TypeDefInfo::Union {
                    name: type_name.clone(),
                    type_params: type_decl.type_params.clone(),
                    variants: variant_infos,
                },
            );
        }
    }
    let _ = span;
    Ok(())
}

fn register_trait_decl(
    env: &mut Environment,
    trait_decl: &TraitDecl,
    span: crate::ast::Span,
) -> Result<(), TypeError> {
    let mut methods = HashMap::new();
    let mut generic_names: Vec<String> = trait_decl
        .type_params
        .iter()
        .map(|p| p.name.clone())
        .collect();
    if !generic_names.iter().any(|g| g == "Self") {
        generic_names.push("Self".to_string());
    }

    for member in &trait_decl.members {
        let mut params = Vec::new();
        for p in &member.node.params {
            let p_ty = env.resolve_ast_type(&p.ty.node, &generic_names, Some(p.ty.span))?;
            params.push((p.name.clone(), p_ty));
        }
        let ret_ty = env.resolve_ast_type(
            &member.node.return_type.node,
            &generic_names,
            Some(member.node.return_type.span),
        )?;
        let is_effectful = ret_ty.is_io();

        methods.insert(
            member.node.name.clone(),
            FunctionSig {
                name: member.node.name.clone(),
                type_params: trait_decl.type_params.clone(),
                params,
                return_type: ret_ty,
                is_effectful,
                span: member.span,
                symbol_name: None,
                is_c_abi: false,
            },
        );
    }

    env.define_trait(
        TraitDef {
            name: trait_decl.name.clone(),
            type_params: trait_decl.type_params.clone(),
            methods,
        },
        span,
    )?;

    Ok(())
}

fn register_impl_decl(
    env: &mut Environment,
    impl_decl: &ImplDecl,
    span: crate::ast::Span,
) -> Result<(), TypeError> {
    let target_type = env.resolve_ast_type(
        &impl_decl.target_type.node,
        &[],
        Some(impl_decl.target_type.span),
    )?;
    let mut methods = HashMap::new();

    for m in &impl_decl.methods {
        let method_generics: Vec<String> =
            m.node.type_params.iter().map(|p| p.name.clone()).collect();
        let mut params = Vec::new();
        for p in &m.node.params {
            let p_ty = env.resolve_ast_type(&p.ty.node, &method_generics, Some(p.ty.span))?;
            params.push((p.name.clone(), p_ty));
        }
        let ret_ty = if let Some(ret) = &m.node.return_type {
            env.resolve_ast_type(&ret.node, &method_generics, Some(ret.span))?
        } else {
            Type::void()
        };
        let is_effectful = ret_ty.is_io();

        methods.insert(
            m.node.name.clone(),
            FunctionSig {
                name: m.node.name.clone(),
                type_params: m.node.type_params.clone(),
                params,
                return_type: ret_ty,
                is_effectful,
                span: m.span,
                symbol_name: None,
                is_c_abi: false,
            },
        );
    }

    let impl_def = ImplDef {
        trait_name: impl_decl.trait_name.clone(),
        target_type,
        methods,
    };

    let resolver = TraitResolver::new(env);
    resolver.verify_impl(&impl_def, span)?;
    env.register_impl(impl_def);

    Ok(())
}

fn register_function_sig(
    env: &mut Environment,
    func_decl: &FunctionDecl,
    span: crate::ast::Span,
) -> Result<(), TypeError> {
    let generic_names: Vec<String> = func_decl
        .type_params
        .iter()
        .map(|p| p.name.clone())
        .collect();
    let mut params = Vec::new();
    for p in &func_decl.params {
        let p_ty = env.resolve_ast_type(&p.ty.node, &generic_names, Some(p.ty.span))?;
        params.push((p.name.clone(), p_ty));
    }

    let return_type = if let Some(ret) = &func_decl.return_type {
        env.resolve_ast_type(&ret.node, &generic_names, Some(ret.span))?
    } else {
        Type::void()
    };

    let is_effectful = return_type.is_io();

    // Check purity & dead computation rule for pure function returning void
    if !is_effectful && return_type.is_void() {
        return Err(TypeError::new(
            TypeErrorKind::DeadComputation {
                function_name: func_decl.name.clone(),
            },
            Some(span),
        ));
    }

    let sig = FunctionSig {
        name: func_decl.name.clone(),
        type_params: func_decl.type_params.clone(),
        params,
        return_type,
        is_effectful,
        span,
        symbol_name: None,
        is_c_abi: false,
    };

    env.define_function(sig)?;
    Ok(())
}

fn register_extern_sig(
    env: &mut Environment,
    func_decl: &FunctionDecl,
    span: crate::ast::Span,
) -> Result<(), TypeError> {
    let generic_names: Vec<String> = func_decl
        .type_params
        .iter()
        .map(|p| p.name.clone())
        .collect();

    let mut params = Vec::new();
    for p in &func_decl.params {
        let p_ty = env.resolve_ast_type(&p.ty.node, &generic_names, Some(p.ty.span))?;
        params.push((p.name.clone(), p_ty));
    }

    let return_type = if let Some(ret) = &func_decl.return_type {
        env.resolve_ast_type(&ret.node, &generic_names, Some(ret.span))?
    } else {
        Type::void()
    };

    if !return_type.is_io() {
        return Err(TypeError::new(
            TypeErrorKind::ExternFunctionMustReturnIO {
                function_name: func_decl.name.clone(),
                found: return_type.to_string(),
            },
            Some(span),
        ));
    }

    let sig = FunctionSig {
        name: func_decl.name.clone(),
        type_params: func_decl.type_params.clone(),
        params,
        return_type,
        is_effectful: true,
        span,
        symbol_name: Some(
            func_decl
                .symbol_name
                .clone()
                .unwrap_or_else(|| func_decl.name.clone()),
        ),
        is_c_abi: true,
    };

    env.define_function(sig)?;
    Ok(())
}

fn check_function_body(env: &mut Environment, func_decl: &FunctionDecl) -> Result<(), TypeError> {
    let sig = env.lookup_function(&func_decl.name).unwrap().clone();
    let effect_ctx = EffectContext::new(func_decl.name.clone(), sig.return_type.clone(), sig.span)?;

    let mut inferrer = TypeInferrer::new(env, Some(effect_ctx));
    let generic_names: Vec<String> = func_decl
        .type_params
        .iter()
        .map(|p| p.name.clone())
        .collect();
    inferrer.set_generics_in_scope(generic_names);

    // Register generic bounds
    let mut bounds = HashMap::new();
    for tp in &func_decl.type_params {
        if let Some(bound) = &tp.bound {
            match &bound.node {
                crate::ast::Type::Generic { name, .. } => {
                    bounds.insert(tp.name.clone(), name.clone());
                }
                crate::ast::Type::Path(segments) => {
                    bounds.insert(tp.name.clone(), segments.join("."));
                }
                _ => {}
            }
        }
    }
    inferrer.set_generic_bounds(bounds);

    inferrer.env.enter_scope();
    for (name, ty) in &sig.params {
        inferrer
            .env
            .define_var(name.clone(), ty.clone(), sig.span)?;
    }

    let body = match &func_decl.body {
        Some(b) => b,
        None => return Ok(()),
    };

    match body {
        FunctionBody::Expr(expr) => {
            if let Some(inner) = sig.return_type.unwrap_io() {
                let inner_clone = inner.clone();
                let mut fork = inferrer.subst.clone();
                if fork
                    .unify(
                        &inferrer.synth_expr(expr).unwrap_or(Type::void()),
                        &sig.return_type,
                        None,
                        &|n, a| inferrer.env.expand_type_alias(n, a),
                    )
                    .is_ok()
                {
                    inferrer.check_expr(expr, &sig.return_type)?;
                } else {
                    inferrer.check_expr(expr, &inner_clone)?;
                }
            } else {
                inferrer.check_expr(expr, &sig.return_type)?;
            }
        }
        FunctionBody::Block(stmts) => {
            inferrer.check_block(stmts, &sig.return_type)?;
        }
    }

    inferrer.env.exit_scope();
    Ok(())
}

fn check_impl_bodies(env: &mut Environment, impl_decl: &ImplDecl) -> Result<(), TypeError> {
    for method in &impl_decl.methods {
        let generic_names: Vec<String> = method
            .node
            .type_params
            .iter()
            .map(|p| p.name.clone())
            .collect();
        let ret_ty = if let Some(ret) = &method.node.return_type {
            env.resolve_ast_type(&ret.node, &generic_names, Some(ret.span))?
        } else {
            Type::void()
        };

        let effect_ctx = EffectContext::new(method.node.name.clone(), ret_ty.clone(), method.span)?;
        let mut inferrer = TypeInferrer::new(env, Some(effect_ctx));
        inferrer.set_generics_in_scope(generic_names);

        inferrer.env.enter_scope();
        for param in &method.node.params {
            let p_ty = inferrer.env.resolve_ast_type(
                &param.ty.node,
                &inferrer.generics_in_scope,
                Some(param.ty.span),
            )?;
            inferrer
                .env
                .define_var(param.name.clone(), p_ty, method.span)?;
        }

        let body = match &method.node.body {
            Some(b) => b,
            None => {
                return Err(TypeError::new(
                    TypeErrorKind::MissingFunctionBody {
                        function_name: method.node.name.clone(),
                    },
                    Some(method.span),
                ));
            }
        };

        match body {
            FunctionBody::Expr(expr) => {
                inferrer.check_expr(expr, &ret_ty)?;
            }
            FunctionBody::Block(stmts) => {
                inferrer.check_block(stmts, &ret_ty)?;
            }
        }

        inferrer.env.exit_scope();
    }
    Ok(())
}
