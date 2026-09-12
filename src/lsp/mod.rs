//! Language Server Protocol (LSP) implementation for Modus.

pub mod code_actions;
pub mod completion;
pub mod definition;
pub mod diagnostics;
pub mod document;
pub mod hover;
pub mod server;
pub mod symbols;

pub use document::{Document, DocumentStore, LineIndex};
pub use server::ModusBackend;

/// Runs the Modus LSP server on standard input/output.
pub fn run_server() -> Result<(), Box<dyn std::error::Error>> {
    let rt = tokio::runtime::Builder::new_multi_thread()
        .enable_all()
        .build()?;

    rt.block_on(async {
        let stdin = tokio::io::stdin();
        let stdout = tokio::io::stdout();

        let (service, socket) = tower_lsp::LspService::new(ModusBackend::new);
        tower_lsp::Server::new(stdin, stdout, socket)
            .serve(service)
            .await;
    });

    Ok(())
}
