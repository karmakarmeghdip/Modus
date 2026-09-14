//! AST Type to Semantic Type resolution for Modus.

use crate::ast::{self, Span};
use crate::typechecker::error::{TypeError, TypeErrorKind};
use crate::typechecker::scope::Environment;
use crate::typechecker::types::Type;
use std::collections::BTreeMap;

impl Environment {
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
                if (name == "ArrayBuilder" || name == "Array") && args.len() == 1 {
                    return Ok(Type::Array(Box::new(args.into_iter().next().unwrap())));
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
                    if self.types.contains_key(&name) {
                        return Ok(Type::Named {
                            name,
                            args: Vec::new(),
                        });
                    }
                    if self.traits.contains_key(&name) {
                        return Ok(Type::TraitObject(name));
                    }
                }
                Err(TypeError::new(TypeErrorKind::UndeclaredType(name), span))
            }
        }
    }
}
