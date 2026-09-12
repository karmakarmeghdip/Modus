//! Desugaring pass for Modus.
//!
//! Lowers syntactic sugar before ANF:
//! - `check` expressions to Result early-return branches
//! - Record updates `{ ...base, f: v }` to full record literals
//! - Function expression bodies `=> expr` to blocks with return
//! - Eliminates `Check` unary operations

pub mod node;
pub mod pass;

pub use node::*;
pub use pass::desugar_program;
