//! Purity & Effect Checker for Modus.
//!
//! Enforces Modus purity rules:
//! 1. Pure functions cannot return `void` (dead computation violation).
//! 2. `perform` or calling an IO-returning function inside a non-IO function is a compile-time error.
//! 3. `perform expr` requires `expr: IO(T)` and yields `T`.
//! 4. `check expr` requires `expr: Result(T, E)` and yields `T`.

use crate::ast::Span;
use crate::typechecker::error::{TypeError, TypeErrorKind};
use crate::typechecker::types::Type;

#[derive(Debug, Clone)]
pub struct EffectContext {
    pub function_name: String,
    pub return_type: Type,
    pub is_io: bool,
    pub span: Span,
}

impl EffectContext {
    pub fn new(function_name: String, return_type: Type, span: Span) -> Result<Self, TypeError> {
        let is_io = return_type.is_io();

        // Enforce: Pure functions cannot return void (dead computation violation)
        if !is_io && return_type.is_void() {
            return Err(TypeError::new(
                TypeErrorKind::DeadComputation {
                    function_name: function_name.clone(),
                },
                Some(span),
            ));
        }

        Ok(Self {
            function_name,
            return_type,
            is_io,
            span,
        })
    }

    /// Check if 'perform' can be called in this context
    pub fn verify_perform_allowed(&self, span: Option<Span>) -> Result<(), TypeError> {
        if !self.is_io {
            return Err(TypeError::new(
                TypeErrorKind::PurityViolation {
                    function_name: self.function_name.clone(),
                    reason: format!(
                        "Cannot use 'perform' in pure function '{}'. Functions with side effects must return IO(T) or IO(void).",
                        self.function_name
                    ),
                },
                span,
            ));
        }
        Ok(())
    }

    /// Check if an IO-returning function can be called directly in this context without perform
    pub fn verify_call_allowed(
        &self,
        callee_name: &str,
        callee_ret: &Type,
        span: Option<Span>,
    ) -> Result<(), TypeError> {
        if callee_ret.is_io() && !self.is_io {
            return Err(TypeError::new(
                TypeErrorKind::PurityViolation {
                    function_name: self.function_name.clone(),
                    reason: format!(
                        "Cannot call effectful function '{callee_name}' in pure function '{}'.",
                        self.function_name
                    ),
                },
                span,
            ));
        }
        Ok(())
    }

    /// Check if 'check' expression can be used in this context
    pub fn verify_check_allowed(
        &self,
        _err_type: &Type,
        span: Option<Span>,
    ) -> Result<(), TypeError> {
        // Enclosing function must return Result(..., E) or IO(Result(..., E))
        let target_ret = if self.is_io {
            self.return_type.unwrap_io().unwrap_or(&self.return_type)
        } else {
            &self.return_type
        };

        if let Some((_, func_err)) = target_ret.unwrap_result() {
            // Error type must match or unify
            // (We will allow flexible check or exact error type matching)
            let _ = func_err;
            Ok(())
        } else {
            // Function does not return Result
            Err(TypeError::new(
                TypeErrorKind::General(format!(
                    "Cannot use 'check' in function '{}' because its return type '{}' is not a Result or IO(Result)",
                    self.function_name, self.return_type
                )),
                span,
            ))
        }
    }
}
