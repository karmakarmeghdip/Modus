//! Symbol Table & Lexical Scopes for Modus Semantic Analysis.

use crate::ast::{self, PrimitiveType, Span};
use crate::typechecker::error::{TypeError, TypeErrorKind};
use crate::typechecker::types::Type;
use std::collections::{BTreeMap, HashMap};

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
struct LexicalScope {
    variables: HashMap<String, (Type, Span)>,
}

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

    fn init_builtins(&mut self) {
        // Register primitive types
        let prims = [
            ("u8", PrimitiveType::U8),
            ("u16", PrimitiveType::U16),
            ("u32", PrimitiveType::U32),
            ("u64", PrimitiveType::U64),
            ("i8", PrimitiveType::I8),
            ("i16", PrimitiveType::I16),
            ("i32", PrimitiveType::I32),
            ("i64", PrimitiveType::I64),
            ("f32", PrimitiveType::F32),
            ("f64", PrimitiveType::F64),
            ("bool", PrimitiveType::Bool),
            ("String", PrimitiveType::String),
            ("void", PrimitiveType::Void),
        ];
        for (name, prim) in prims {
            self.types
                .insert(name.to_string(), TypeDefInfo::Primitive(prim));
        }

        // Register built-in IO type
        self.types.insert(
            "IO".to_string(),
            TypeDefInfo::Builtin {
                name: "IO".to_string(),
                type_params: vec![ast::TypeParam {
                    name: "T".to_string(),
                    bound: None,
                }],
            },
        );

        // Register IO.pure constructor: (T) => IO(T)
        self.constructors.insert(
            "IO.pure".to_string(),
            ConstructorInfo::Function {
                params: vec![Type::GenericParam("T".to_string())],
                return_type: Type::io(Type::GenericParam("T".to_string())),
                type_params: vec![ast::TypeParam {
                    name: "T".to_string(),
                    bound: None,
                }],
            },
        );

        // Register Option(T) = Some(T) | None
        self.register_option_builtin();

        // Register Result(T, E) = Ok(T) | Err(E)
        self.register_result_builtin();

        // Register ControlFlow(Residual, Output)
        self.types.insert(
            "ControlFlow".to_string(),
            TypeDefInfo::Builtin {
                name: "ControlFlow".to_string(),
                type_params: vec![
                    ast::TypeParam {
                        name: "Residual".to_string(),
                        bound: None,
                    },
                    ast::TypeParam {
                        name: "Output".to_string(),
                        bound: None,
                    },
                ],
            },
        );

        // Register built-in Drawable trait
        let mut drawable_methods = HashMap::new();
        drawable_methods.insert(
            "draw".to_string(),
            FunctionSig {
                name: "draw".to_string(),
                type_params: vec![ast::TypeParam {
                    name: "Self".to_string(),
                    bound: None,
                }],
                params: vec![("self".to_string(), Type::GenericParam("Self".to_string()))],
                return_type: Type::io(Type::void()),
                is_effectful: true,
                span: Span::default(),
            },
        );
        self.traits.insert(
            "Drawable".to_string(),
            TraitDef {
                name: "Drawable".to_string(),
                type_params: vec![ast::TypeParam {
                    name: "Self".to_string(),
                    bound: None,
                }],
                methods: drawable_methods,
            },
        );
        self.types.insert(
            "Drawable".to_string(),
            TypeDefInfo::Builtin {
                name: "Drawable".to_string(),
                type_params: vec![ast::TypeParam {
                    name: "Self".to_string(),
                    bound: None,
                }],
            },
        );
    }

    fn register_option_builtin(&mut self) {
        let t_param = ast::TypeParam {
            name: "T".to_string(),
            bound: None,
        };
        self.types.insert(
            "Option".to_string(),
            TypeDefInfo::Union {
                name: "Option".to_string(),
                type_params: vec![t_param.clone()],
                variants: vec![
                    VariantInfo {
                        name: "Some".to_string(),
                        fields: vec![Type::GenericParam("T".to_string())],
                    },
                    VariantInfo {
                        name: "None".to_string(),
                        fields: vec![],
                    },
                ],
            },
        );

        let some_ctor = ConstructorInfo::Function {
            params: vec![Type::GenericParam("T".to_string())],
            return_type: Type::option(Type::GenericParam("T".to_string())),
            type_params: vec![t_param.clone()],
        };
        self.constructors
            .insert("Some".to_string(), some_ctor.clone());
        self.constructors
            .insert("Option.Some".to_string(), some_ctor);

        let none_ctor = ConstructorInfo::Value {
            parent_type: Type::option(Type::GenericParam("T".to_string())),
            type_params: vec![t_param],
        };
        self.constructors
            .insert("None".to_string(), none_ctor.clone());
        self.constructors
            .insert("Option.None".to_string(), none_ctor);
    }

    fn register_result_builtin(&mut self) {
        let t_param = ast::TypeParam {
            name: "T".to_string(),
            bound: None,
        };
        let e_param = ast::TypeParam {
            name: "E".to_string(),
            bound: None,
        };
        self.types.insert(
            "Result".to_string(),
            TypeDefInfo::Union {
                name: "Result".to_string(),
                type_params: vec![t_param.clone(), e_param.clone()],
                variants: vec![
                    VariantInfo {
                        name: "Ok".to_string(),
                        fields: vec![Type::GenericParam("T".to_string())],
                    },
                    VariantInfo {
                        name: "Err".to_string(),
                        fields: vec![Type::GenericParam("E".to_string())],
                    },
                ],
            },
        );

        let ok_ctor = ConstructorInfo::Function {
            params: vec![Type::GenericParam("T".to_string())],
            return_type: Type::result(
                Type::GenericParam("T".to_string()),
                Type::GenericParam("E".to_string()),
            ),
            type_params: vec![t_param.clone(), e_param.clone()],
        };
        self.constructors.insert("Ok".to_string(), ok_ctor.clone());
        self.constructors.insert("Result.Ok".to_string(), ok_ctor);

        let err_ctor = ConstructorInfo::Function {
            params: vec![Type::GenericParam("E".to_string())],
            return_type: Type::result(
                Type::GenericParam("T".to_string()),
                Type::GenericParam("E".to_string()),
            ),
            type_params: vec![t_param, e_param],
        };
        self.constructors
            .insert("Err".to_string(), err_ctor.clone());
        self.constructors.insert("Result.Err".to_string(), err_ctor);
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

    /// Resolve an AST Type into a semantic Type
    pub fn resolve_ast_type(
        &self,
        ast_type: &ast::Type,
        generic_in_scope: &[String],
        span: Option<Span>,
    ) -> Result<Type, TypeError> {
        match ast_type {
            ast::Type::Primitive(p) => Ok(Type::Primitive(*p)),
            ast::Type::Unit => Ok(Type::Unit),
            ast::Type::Array(inner) => {
                let elem_type =
                    self.resolve_ast_type(&inner.node, generic_in_scope, Some(inner.span))?;
                Ok(Type::Array(Box::new(elem_type)))
            }
            ast::Type::Function {
                param_types,
                return_type,
            } => {
                let mut params = Vec::new();
                for p in param_types {
                    params.push(self.resolve_ast_type(&p.node, generic_in_scope, Some(p.span))?);
                }
                let ret = self.resolve_ast_type(
                    &return_type.node,
                    generic_in_scope,
                    Some(return_type.span),
                )?;
                Ok(Type::Function {
                    params,
                    ret: Box::new(ret),
                })
            }
            ast::Type::Record(fields) => {
                let mut rec_fields = BTreeMap::new();
                for (name, f_type) in fields {
                    let field_ty =
                        self.resolve_ast_type(&f_type.node, generic_in_scope, Some(f_type.span))?;
                    rec_fields.insert(name.clone(), field_ty);
                }
                Ok(Type::Record(rec_fields))
            }
            ast::Type::Tuple(elems) => {
                let mut tuple_elems = Vec::new();
                for e in elems {
                    tuple_elems.push(self.resolve_ast_type(
                        &e.node,
                        generic_in_scope,
                        Some(e.span),
                    )?);
                }
                Ok(Type::Tuple(tuple_elems))
            }
            ast::Type::Generic { name, type_args } => {
                if type_args.is_empty() {
                    if generic_in_scope.iter().any(|g| g == name) {
                        return Ok(Type::GenericParam(name.clone()));
                    }
                    if name == "Self" {
                        return Ok(Type::GenericParam("Self".to_string()));
                    }
                    if self.traits.contains_key(name) {
                        return Ok(Type::TraitObject(name.clone()));
                    }
                }
                let mut args = Vec::new();
                for arg in type_args {
                    args.push(self.resolve_ast_type(
                        &arg.node,
                        generic_in_scope,
                        Some(arg.span),
                    )?);
                }
                Ok(Type::Named {
                    name: name.clone(),
                    args,
                })
            }
            ast::Type::Path(segments) => {
                let name = segments.join(".");
                if segments.len() == 1 {
                    let seg = &segments[0];
                    if generic_in_scope.iter().any(|g| g == seg) {
                        return Ok(Type::GenericParam(seg.clone()));
                    }
                    if seg == "Self" {
                        return Ok(Type::GenericParam("Self".to_string()));
                    }
                    if self.traits.contains_key(seg) {
                        return Ok(Type::TraitObject(seg.clone()));
                    }
                    if self.types.contains_key(seg) {
                        return Ok(Type::Named {
                            name: seg.clone(),
                            args: Vec::new(),
                        });
                    }
                    if seg.chars().next().is_some_and(|c| c.is_uppercase()) {
                        return Ok(Type::Named {
                            name: seg.clone(),
                            args: Vec::new(),
                        });
                    }
                } else {
                    if segments[0] == "Self" || generic_in_scope.iter().any(|g| g == &segments[0]) {
                        return Ok(Type::GenericParam(name));
                    }
                }
                Err(TypeError::new(TypeErrorKind::UndeclaredType(name), span))
            }
        }
    }
}
