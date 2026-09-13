//! Bidirectional Type Inference for Modus.

pub mod check;
pub mod patterns;
pub mod stmts;
pub mod synth;

use crate::ast::{self, Span};
use crate::typechecker::error::TypeError;
use crate::typechecker::purity::EffectContext;
use crate::typechecker::scope::{ConstructorInfo, Environment, FunctionSig};
use crate::typechecker::types::{Substitution, Type, TypeVarGen};
use std::collections::{BTreeMap, HashMap};

pub struct TypeInferrer<'a> {
    pub env: &'a mut Environment,
    pub var_gen: TypeVarGen,
    pub subst: Substitution,
    pub effect_ctx: Option<EffectContext>,
    pub generic_bounds: HashMap<String, String>,
    pub generics_in_scope: Vec<String>,
}

impl<'a> TypeInferrer<'a> {
    pub fn new(env: &'a mut Environment, effect_ctx: Option<EffectContext>) -> Self {
        Self {
            env,
            var_gen: TypeVarGen::new(),
            subst: Substitution::new(),
            effect_ctx,
            generic_bounds: HashMap::new(),
            generics_in_scope: Vec::new(),
        }
    }

    pub fn set_generic_bounds(&mut self, bounds: HashMap<String, String>) {
        self.generic_bounds = bounds;
    }

    pub fn set_generics_in_scope(&mut self, generics: Vec<String>) {
        self.generics_in_scope = generics;
    }

    pub(crate) fn expand_type(&self, ty: &Type) -> Type {
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

    pub(crate) fn unify(
        &mut self,
        t1: &Type,
        t2: &Type,
        span: Option<Span>,
    ) -> Result<(), TypeError> {
        self.subst.unify(t1, t2, span, &|name, args| {
            self.env.expand_type_alias(name, args)
        })
    }

    pub(crate) fn instantiate_function(&mut self, sig: &FunctionSig) -> Type {
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

    pub(crate) fn instantiate_constructor(&mut self, ctor: &ConstructorInfo) -> Type {
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

    pub(crate) fn instantiate_parent_type(
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

    pub(crate) fn instantiate_ctor_fn(
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

    pub(crate) fn subst_generic_params(
        &self,
        ty: &Type,
        param_map: &HashMap<String, Type>,
    ) -> Type {
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
