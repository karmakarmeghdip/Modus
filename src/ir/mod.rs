//! Intermediate Representation (IR) pipeline for Modus.
//!
//! Pipeline:
//! 1. Lower desugared AST into A-Normal Form (`lower_program`).
//! 2. Convert closures via lambda lifting and environment packing (`convert_closures`).
//! 3. Analyze variable lifecycles and last-use points (`analyze_function_liveness`).
//! 4. Apply Perceus reference counting (`inc_ref`/`dec_ref`) and FBIP in-place reuse (`apply_perceus_and_fbip`).

pub mod closure;
pub mod display;
pub mod liveness;
pub mod lower;
pub mod node;
pub mod perceus;

pub use closure::convert_closures;
pub use liveness::{
    BlockLiveness, FunctionLiveness, StmtLiveness, TailLiveness, analyze_block_liveness,
    analyze_function_liveness, is_heap_type,
};
pub use lower::lower_program;
pub use node::*;
pub use perceus::apply_perceus_and_fbip;
