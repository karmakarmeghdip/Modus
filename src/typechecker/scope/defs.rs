//! Symbol definitions for Modus symbol table and scopes.

use crate::ast::{self, PrimitiveType, Span};
use crate::typechecker::types::Type;
use std::collections::HashMap;

/// Represents a function signature in the symbol table
#[derive(Debug, Clone, PartialEq)]
pub struct FunctionSig {
    pub name: String,
    pub type_params: Vec<ast::TypeParam>,
    pub params: Vec<(String, Type)>,
    pub return_type: Type,
    pub is_effectful: bool,
    pub span: Span,
}

/// Represents information about a type definition (alias or union)
#[derive(Debug, Clone, PartialEq)]
pub enum TypeDefInfo {
    Alias {
        name: String,
        type_params: Vec<ast::TypeParam>,
        expanded_type: Type,
    },
    Union {
        name: String,
        type_params: Vec<ast::TypeParam>,
        variants: Vec<VariantInfo>,
    },
    Primitive(PrimitiveType),
    Builtin {
        name: String,
        type_params: Vec<ast::TypeParam>,
    },
}

#[derive(Debug, Clone, PartialEq)]
pub struct VariantInfo {
    pub name: String,
    pub fields: Vec<Type>,
}

#[derive(Debug, Clone, PartialEq)]
pub enum ConstructorInfo {
    Value {
        parent_type: Type,
        type_params: Vec<ast::TypeParam>,
    },
    Function {
        params: Vec<Type>,
        return_type: Type,
        type_params: Vec<ast::TypeParam>,
    },
}

#[derive(Debug, Clone, PartialEq)]
pub struct TraitDef {
    pub name: String,
    pub type_params: Vec<ast::TypeParam>,
    pub methods: HashMap<String, FunctionSig>,
}

#[derive(Debug, Clone, PartialEq)]
pub struct ImplDef {
    pub trait_name: String,
    pub target_type: Type,
    pub methods: HashMap<String, FunctionSig>,
}

#[derive(Debug, Clone, Default)]
pub(crate) struct LexicalScope {
    pub(crate) variables: HashMap<String, (Type, Span)>,
}
