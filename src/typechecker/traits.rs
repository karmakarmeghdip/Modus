//! Trait Resolution and Dispatch for Modus.
//!
//! Handles trait declarations, impl blocks, generic bounds (T: Drawable),
//! and fat-pointer dynamic trait objects (item: Drawable).

use crate::ast::Span;
use crate::typechecker::error::{TypeError, TypeErrorKind};
use crate::typechecker::scope::{Environment, FunctionSig, ImplDef};
use crate::typechecker::types::{Substitution, Type};
use std::collections::HashMap;

/// Trait checker and resolution engine
pub struct TraitResolver<'a> {
    env: &'a Environment,
}

impl<'a> TraitResolver<'a> {
    pub fn new(env: &'a Environment) -> Self {
        Self { env }
    }

    /// Check if a type implements a trait
    pub fn implements_trait(&self, ty: &Type, trait_name: &str) -> bool {
        // Dynamic trait object of the same trait name trivially implements it
        if let Type::TraitObject(name) = ty
            && name == trait_name
        {
            return true;
        }

        // Generic param with bound
        if let Type::GenericParam(_) = ty {
            // Check in function signatures if this generic param has the bound
            // (Caller provides bounded params or we check bounds)
            return true;
        }

        // Search impl blocks for this trait
        if let Some(impls) = self.env.lookup_impls(trait_name) {
            for im in impls {
                if self.types_match(&im.target_type, ty) {
                    return true;
                }
            }
        }

        false
    }

    /// Resolve a method call on a receiver type
    pub fn resolve_method(
        &self,
        receiver_ty: &Type,
        method_name: &str,
        generic_bounds: &HashMap<String, String>,
        span: Option<Span>,
    ) -> Result<FunctionSig, TypeError> {
        // Case 1: Trait object (item: Drawable)
        if let Type::TraitObject(trait_name) = receiver_ty {
            if let Some(trait_def) = self.env.lookup_trait(trait_name)
                && let Some(sig) = trait_def.methods.get(method_name)
            {
                return Ok(Self::substitute_self_in_sig(sig, receiver_ty));
            }
            return Err(TypeError::new(
                TypeErrorKind::MethodNotFound {
                    ty: receiver_ty.to_string(),
                    method: method_name.to_string(),
                },
                span,
            ));
        }

        // Case 2: Generic parameter with trait bound (e.g. T: Drawable)
        if let Type::GenericParam(param_name) = receiver_ty
            && let Some(bound_trait) = generic_bounds.get(param_name)
            && let Some(trait_def) = self.env.lookup_trait(bound_trait)
            && let Some(sig) = trait_def.methods.get(method_name)
        {
            return Ok(Self::substitute_self_in_sig(sig, receiver_ty));
        }

        // Case 3: Concrete type implementing trait(s)
        for (trait_name, impls) in &self.env.impls {
            for im in impls {
                if self.types_match(&im.target_type, receiver_ty)
                    && let Some(sig) = im.methods.get(method_name)
                {
                    return Ok(sig.clone());
                }
            }
            let _ = trait_name;
        }

        Err(TypeError::new(
            TypeErrorKind::MethodNotFound {
                ty: receiver_ty.to_string(),
                method: method_name.to_string(),
            },
            span,
        ))
    }

    /// Verify an impl block satisfies the trait definition
    pub fn verify_impl(&self, impl_def: &ImplDef, span: Span) -> Result<(), TypeError> {
        let trait_def = self.env.lookup_trait(&impl_def.trait_name).ok_or_else(|| {
            TypeError::new(
                TypeErrorKind::UndeclaredType(impl_def.trait_name.clone()),
                Some(span),
            )
        })?;

        // Verify each required method in the trait is implemented
        for (method_name, required_sig) in &trait_def.methods {
            let implemented_sig = impl_def.methods.get(method_name).ok_or_else(|| {
                TypeError::new(
                    TypeErrorKind::TraitNotImplemented {
                        ty: impl_def.target_type.to_string(),
                        trait_name: format!(
                            "missing method '{method_name}' of trait {}",
                            impl_def.trait_name
                        ),
                    },
                    Some(span),
                )
            })?;

            let expected_sig = Self::substitute_self_in_sig(required_sig, &impl_def.target_type);
            if implemented_sig.params.len() != expected_sig.params.len() {
                return Err(TypeError::new(
                    TypeErrorKind::ArgCountMismatch {
                        expected: expected_sig.params.len(),
                        found: implemented_sig.params.len(),
                    },
                    Some(implemented_sig.span),
                ));
            }

            // Check parameter types
            let mut subst = Substitution::new();
            for ((_, impl_param_ty), (_, exp_param_ty)) in implemented_sig
                .params
                .iter()
                .zip(expected_sig.params.iter())
            {
                subst
                    .unify(
                        impl_param_ty,
                        exp_param_ty,
                        Some(implemented_sig.span),
                        &|name, args| self.env.expand_type_alias(name, args),
                    )
                    .map_err(|_| {
                        TypeError::new(
                            TypeErrorKind::TypeMismatch {
                                expected: exp_param_ty.to_string(),
                                found: impl_param_ty.to_string(),
                            },
                            Some(implemented_sig.span),
                        )
                    })?;
            }

            // Check return type
            subst
                .unify(
                    &implemented_sig.return_type,
                    &expected_sig.return_type,
                    Some(implemented_sig.span),
                    &|name, args| self.env.expand_type_alias(name, args),
                )
                .map_err(|_| {
                    TypeError::new(
                        TypeErrorKind::TypeMismatch {
                            expected: expected_sig.return_type.to_string(),
                            found: implemented_sig.return_type.to_string(),
                        },
                        Some(implemented_sig.span),
                    )
                })?;
        }

        Ok(())
    }

    fn types_match(&self, target: &Type, actual: &Type) -> bool {
        if target == actual {
            return true;
        }
        // Expand aliases if needed
        let exp_target = match target {
            Type::Named { name, args } => self
                .env
                .expand_type_alias(name, args)
                .unwrap_or_else(|| target.clone()),
            _ => target.clone(),
        };
        let exp_actual = match actual {
            Type::Named { name, args } => self
                .env
                .expand_type_alias(name, args)
                .unwrap_or_else(|| actual.clone()),
            _ => actual.clone(),
        };
        exp_target == exp_actual
    }

    fn substitute_self_in_sig(sig: &FunctionSig, self_replacement: &Type) -> FunctionSig {
        let mut new_sig = sig.clone();
        for (_, ty) in &mut new_sig.params {
            *ty = Self::replace_self(ty, self_replacement);
        }
        new_sig.return_type = Self::replace_self(&new_sig.return_type, self_replacement);
        new_sig
    }

    fn replace_self(ty: &Type, replacement: &Type) -> Type {
        match ty {
            Type::GenericParam(name) if name == "Self" => replacement.clone(),
            Type::Named { name, args } if name == "Self" && args.is_empty() => replacement.clone(),
            Type::Array(inner) => Type::Array(Box::new(Self::replace_self(inner, replacement))),
            Type::Function { params, ret } => Type::Function {
                params: params
                    .iter()
                    .map(|p| Self::replace_self(p, replacement))
                    .collect(),
                ret: Box::new(Self::replace_self(ret, replacement)),
            },
            Type::Named { name, args } => Type::Named {
                name: name.clone(),
                args: args
                    .iter()
                    .map(|a| Self::replace_self(a, replacement))
                    .collect(),
            },
            _ => ty.clone(),
        }
    }
}
