//! Symbol Table & Lexical Scopes for Modus Semantic Analysis.

pub mod builtins;
pub mod defs;
pub mod resolve;

pub use defs::*;

use crate::ast::Span;
use crate::typechecker::error::{TypeError, TypeErrorKind};
use crate::typechecker::types::Type;
use std::collections::{BTreeMap, HashMap};

/// Lexical Environment and Symbol Table for Modus
#[derive(Debug, Clone)]
pub struct Environment {
    scopes: Vec<LexicalScope>,
    pub functions: HashMap<String, FunctionSig>,
    pub types: HashMap<String, TypeDefInfo>,
    pub constructors: HashMap<String, ConstructorInfo>,
    pub traits: HashMap<String, TraitDef>,
    pub impls: HashMap<String, Vec<ImplDef>>,
}

impl Default for Environment {
    fn default() -> Self {
        Self::new()
    }
}

impl Environment {
    pub fn new() -> Self {
        let mut env = Self {
            scopes: vec![LexicalScope::default()],
            functions: HashMap::new(),
            types: HashMap::new(),
            constructors: HashMap::new(),
            traits: HashMap::new(),
            impls: HashMap::new(),
        };
        env.init_builtins();
        env
    }

    // Lexical Scope Management
    pub fn enter_scope(&mut self) {
        self.scopes.push(LexicalScope::default());
    }

    pub fn exit_scope(&mut self) {
        if self.scopes.len() > 1 {
            self.scopes.pop();
        }
    }

    pub fn define_var(&mut self, name: String, ty: Type, span: Span) -> Result<(), TypeError> {
        // Disallow duplicate variable in the current scope
        if let Some(current_scope) = self.scopes.last_mut() {
            if current_scope.variables.contains_key(&name) {
                return Err(TypeError::new(
                    TypeErrorKind::DuplicateVariable(name),
                    Some(span),
                ));
            }
            current_scope.variables.insert(name, (ty, span));
            Ok(())
        } else {
            Err(TypeError::new(
                TypeErrorKind::General("No active lexical scope".to_string()),
                Some(span),
            ))
        }
    }

    pub fn lookup_var(&self, name: &str) -> Option<&(Type, Span)> {
        for scope in self.scopes.iter().rev() {
            if let Some(entry) = scope.variables.get(name) {
                return Some(entry);
            }
        }
        None
    }

    pub fn define_function(&mut self, sig: FunctionSig) -> Result<(), TypeError> {
        if self.functions.contains_key(&sig.name) {
            return Err(TypeError::new(
                TypeErrorKind::DuplicateFunction(sig.name.clone()),
                Some(sig.span),
            ));
        }
        self.functions.insert(sig.name.clone(), sig);
        Ok(())
    }

    pub fn lookup_function(&self, name: &str) -> Option<&FunctionSig> {
        self.functions.get(name)
    }

    pub fn define_trait(&mut self, trait_def: TraitDef, _span: Span) -> Result<(), TypeError> {
        // Also register trait name as a type so fat-pointer Drawable can be referenced
        self.types.insert(
            trait_def.name.clone(),
            TypeDefInfo::Builtin {
                name: trait_def.name.clone(),
                type_params: trait_def.type_params.clone(),
            },
        );
        self.traits.insert(trait_def.name.clone(), trait_def);
        Ok(())
    }

    pub fn lookup_trait(&self, name: &str) -> Option<&TraitDef> {
        self.traits.get(name)
    }

    pub fn register_impl(&mut self, impl_def: ImplDef) {
        self.impls
            .entry(impl_def.trait_name.clone())
            .or_default()
            .push(impl_def);
    }

    pub fn lookup_impls(&self, trait_name: &str) -> Option<&Vec<ImplDef>> {
        self.impls.get(trait_name)
    }

    pub fn expand_type_alias(&self, name: &str, args: &[Type]) -> Option<Type> {
        match self.types.get(name)? {
            TypeDefInfo::Alias {
                type_params,
                expanded_type,
                ..
            } => {
                let mut param_map = HashMap::new();
                for (param, arg) in type_params.iter().zip(args.iter()) {
                    param_map.insert(param.name.clone(), arg.clone());
                }
                Some(Self::substitute_params(expanded_type, &param_map))
            }
            _ => None,
        }
    }

    fn substitute_params(ty: &Type, param_map: &HashMap<String, Type>) -> Type {
        match ty {
            Type::GenericParam(name) => {
                if let Some(target) = param_map.get(name) {
                    target.clone()
                } else {
                    ty.clone()
                }
            }
            Type::Array(inner) => Type::Array(Box::new(Self::substitute_params(inner, param_map))),
            Type::Function { params, ret } => Type::Function {
                params: params
                    .iter()
                    .map(|p| Self::substitute_params(p, param_map))
                    .collect(),
                ret: Box::new(Self::substitute_params(ret, param_map)),
            },
            Type::Record(fields) => {
                let mut new_fields = BTreeMap::new();
                for (k, v) in fields {
                    new_fields.insert(k.clone(), Self::substitute_params(v, param_map));
                }
                Type::Record(new_fields)
            }
            Type::Tuple(elems) => Type::Tuple(
                elems
                    .iter()
                    .map(|e| Self::substitute_params(e, param_map))
                    .collect(),
            ),
            Type::Named { name, args } => Type::Named {
                name: name.clone(),
                args: args
                    .iter()
                    .map(|a| Self::substitute_params(a, param_map))
                    .collect(),
            },
            _ => ty.clone(),
        }
    }
}
