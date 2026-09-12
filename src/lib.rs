pub mod ast;
#[cfg(feature = "llvm")]
pub mod backend;
pub mod desugar;
pub mod ir;
#[cfg(feature = "lsp")]
pub mod lsp;
pub mod modules;
pub mod parser;
pub mod typechecker;

#[cfg(feature = "llvm")]
use backend::CodeGen;
#[cfg(feature = "llvm")]
use inkwell::context::Context;

/// Compiles Modus source code through the complete compiler pipeline into an LLVM CodeGen instance.
#[cfg(feature = "llvm")]
pub fn compile_source<'ctx>(
    context: &'ctx Context,
    source: &str,
    module_name: &str,
) -> Result<CodeGen<'ctx>, String> {
    let program = parser::parse_program(source).map_err(|e| format!("Parse error: {e:?}"))?;
    let env = typechecker::check_program(&program).map_err(|e| format!("Type error: {e:?}"))?;
    let desugared = desugar::desugar_program(&program, &env);
    let mut anf = ir::lower_program(&desugared);
    ir::convert_closures(&mut anf);
    ir::apply_perceus_and_fbip(&mut anf);
    let mut codegen = CodeGen::new(context, module_name);
    codegen.compile_program(&anf)?;
    Ok(codegen)
}
