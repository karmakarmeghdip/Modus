use crate::ast::{Literal, Pattern, Spanned};
use crate::typechecker::error::{TypeError, TypeErrorKind};
use crate::typechecker::infer::TypeInferrer;
use crate::typechecker::scope::ConstructorInfo;
use crate::typechecker::types::Type;

impl<'a> TypeInferrer<'a> {
    /// Check a pattern against a target type, binding variables into the current scope
    pub fn check_pattern(
        &mut self,
        pat: &Spanned<Pattern>,
        target_ty: &Type,
    ) -> Result<(), TypeError> {
        let expanded = self.expand_type(&self.subst.apply(target_ty));

        match (&pat.node, &expanded) {
            (Pattern::Wildcard, _) => Ok(()),

            (Pattern::Ident(name), _) => {
                // If name is a 0-argument variant like None or PointShape
                if let Some(ctor) = self.env.constructors.get(name).cloned() {
                    let ctor_ty = self.instantiate_constructor(&ctor);
                    if self.unify(&ctor_ty, target_ty, Some(pat.span)).is_ok() {
                        return Ok(());
                    }
                }
                // Otherwise binds variable in arm scope
                self.env
                    .define_var(name.clone(), target_ty.clone(), pat.span)
            }

            (Pattern::Literal(lit), _) => match lit {
                Literal::Int(_) if target_ty.is_integer() => Ok(()),
                Literal::UInt(_) if target_ty.is_integer() => Ok(()),
                Literal::Float(_) if target_ty.is_float() => Ok(()),
                Literal::String(_) if *target_ty == Type::string() => Ok(()),
                Literal::Bool(_) if target_ty.is_bool() => Ok(()),
                _ => Err(TypeError::new(
                    TypeErrorKind::TypeMismatch {
                        expected: target_ty.to_string(),
                        found: format!("{lit:?}"),
                    },
                    Some(pat.span),
                )),
            },

            (
                Pattern::Variant {
                    type_name,
                    variant,
                    patterns,
                },
                _,
            ) => {
                let qualified = if let Some(t_name) = type_name {
                    format!("{t_name}.{variant}")
                } else {
                    variant.clone()
                };

                let ctor = self
                    .env
                    .constructors
                    .get(&qualified)
                    .or_else(|| self.env.constructors.get(variant))
                    .cloned()
                    .ok_or_else(|| {
                        TypeError::new(
                            TypeErrorKind::General(format!(
                                "Unknown variant or constructor '{qualified}'"
                            )),
                            Some(pat.span),
                        )
                    })?;

                match ctor {
                    ConstructorInfo::Value {
                        parent_type,
                        type_params,
                    } => {
                        if !patterns.is_empty() {
                            return Err(TypeError::new(
                                TypeErrorKind::ArgCountMismatch {
                                    expected: 0,
                                    found: patterns.len(),
                                },
                                Some(pat.span),
                            ));
                        }
                        let instantiated = self.instantiate_parent_type(&parent_type, &type_params);
                        self.unify(&instantiated, target_ty, Some(pat.span))?;
                        Ok(())
                    }
                    ConstructorInfo::Function {
                        params,
                        return_type,
                        type_params,
                    } => {
                        if params.len() != patterns.len() {
                            return Err(TypeError::new(
                                TypeErrorKind::ArgCountMismatch {
                                    expected: params.len(),
                                    found: patterns.len(),
                                },
                                Some(pat.span),
                            ));
                        }
                        let (inst_params, inst_ret) =
                            self.instantiate_ctor_fn(&params, &return_type, &type_params);
                        self.unify(&inst_ret, target_ty, Some(pat.span))?;
                        for (sub_pat, param_ty) in patterns.iter().zip(inst_params.iter()) {
                            self.check_pattern(sub_pat, param_ty)?;
                        }
                        Ok(())
                    }
                }
            }

            (Pattern::Record(fields), Type::Record(expected_fields)) => {
                for (name, sub_pat) in fields {
                    if let Some(expected_field_ty) = expected_fields.get(name) {
                        if let Some(p) = sub_pat {
                            self.check_pattern(p, expected_field_ty)?;
                        } else {
                            self.env.define_var(
                                name.clone(),
                                expected_field_ty.clone(),
                                pat.span,
                            )?;
                        }
                    } else {
                        return Err(TypeError::new(
                            TypeErrorKind::MissingRecordField {
                                field: name.clone(),
                                record_type: target_ty.to_string(),
                            },
                            Some(pat.span),
                        ));
                    }
                }
                Ok(())
            }

            (Pattern::Tuple(sub_pats), Type::Tuple(elem_types)) => {
                if sub_pats.len() != elem_types.len() {
                    return Err(TypeError::new(
                        TypeErrorKind::TypeMismatch {
                            expected: target_ty.to_string(),
                            found: format!("tuple of length {}", sub_pats.len()),
                        },
                        Some(pat.span),
                    ));
                }
                for (sub_pat, elem_ty) in sub_pats.iter().zip(elem_types.iter()) {
                    self.check_pattern(sub_pat, elem_ty)?;
                }
                Ok(())
            }

            _ => Err(TypeError::new(
                TypeErrorKind::InvalidPattern(format!(
                    "Pattern does not match expected type '{}'",
                    target_ty
                )),
                Some(pat.span),
            )),
        }
    }
}
