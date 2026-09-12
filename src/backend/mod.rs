//! LLVM backend for Modus using Inkwell.
//!
//! Pipeline:
//! 1. Memory and type layout lowerer (`types`).
//! 2. Runtime memory primitives, Perceus RC helpers, and IO stubs (`runtime`).
//! 3. LLVM IR code generation with `musttail`, `fastcc`, and `-O3` (`codegen`).

pub mod codegen;
pub mod runtime;
pub mod types;

pub use codegen::{CodeGen, ExecutionResult};
pub use runtime::Runtime;
pub use types::TypeLowerer;
