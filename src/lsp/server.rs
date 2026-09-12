//! Tower-LSP backend implementing language server capabilities for Modus.

use super::code_actions::code_actions_at;
use super::completion::completions_with_store;
use super::definition::definition_with_store;
use super::diagnostics::{ResolvedImport, compute_diagnostics_with_imports, resolve_import};
use super::document::DocumentStore;
use super::hover::hover_at;
use super::symbols::document_symbols;
use tower_lsp::jsonrpc::Result;
use tower_lsp::lsp_types::*;
use tower_lsp::{Client, LanguageServer};

/// Modus language server backend managing open documents, diagnostics, and protocol RPCs.
#[derive(Debug, Clone)]
pub struct ModusBackend {
    client: Client,
    documents: DocumentStore,
}

impl ModusBackend {
    pub fn new(client: Client) -> Self {
        Self {
            client,
            documents: DocumentStore::new(),
        }
    }

    async fn analyze_and_publish(&self, uri: &Url, version: i32, text: &str) {
        let doc = self.documents.get(uri);
        if let Some(doc) = doc {
            let (program, env, interface, diagnostics) = compute_diagnostics_with_imports(
                text,
                &doc.line_index,
                Some(uri),
                Some(&self.documents),
            );
            self.documents
                .update_analysis(uri, program, env, interface, diagnostics.clone());
            self.client
                .publish_diagnostics(uri.clone(), diagnostics, Some(version))
                .await;
        }
    }

    async fn reanalyze_dependents(&self, changed_uri: &Url) {
        let open_docs = self.documents.get_all_open_docs();
        for other_doc in open_docs {
            if &other_doc.uri != changed_uri {
                let imports_changed = if let Some(program) = &other_doc.program {
                    let current_file_path = other_doc.uri.to_file_path().ok();
                    program.imports.iter().any(|imp| {
                        if let Ok(resolved) = resolve_import(
                            &imp.node.source,
                            current_file_path.as_deref(),
                            Some(&self.documents),
                        ) {
                            match resolved {
                                ResolvedImport::File { url, .. } => &url == changed_uri,
                                ResolvedImport::Std(_) => false,
                            }
                        } else {
                            false
                        }
                    })
                } else {
                    false
                };

                if imports_changed {
                    self.analyze_and_publish(&other_doc.uri, other_doc.version, &other_doc.text)
                        .await;
                }
            }
        }
    }
}

#[tower_lsp::async_trait]
impl LanguageServer for ModusBackend {
    async fn initialize(&self, _: InitializeParams) -> Result<InitializeResult> {
        Ok(InitializeResult {
            server_info: Some(ServerInfo {
                name: "modus-lsp".to_string(),
                version: Some(env!("CARGO_PKG_VERSION").to_string()),
            }),
            capabilities: ServerCapabilities {
                text_document_sync: Some(TextDocumentSyncCapability::Kind(
                    TextDocumentSyncKind::FULL,
                )),
                hover_provider: Some(HoverProviderCapability::Simple(true)),
                definition_provider: Some(OneOf::Left(true)),
                document_symbol_provider: Some(OneOf::Left(true)),
                completion_provider: Some(CompletionOptions {
                    trigger_characters: Some(vec![
                        ".".to_string(),
                        ":".to_string(),
                        "{".to_string(),
                    ]),
                    ..Default::default()
                }),
                code_action_provider: Some(CodeActionProviderCapability::Simple(true)),
                ..Default::default()
            },
        })
    }

    async fn initialized(&self, _: InitializedParams) {
        self.client
            .log_message(MessageType::INFO, "Modus Language Server initialized!")
            .await;
    }

    async fn shutdown(&self) -> Result<()> {
        Ok(())
    }

    async fn did_open(&self, params: DidOpenTextDocumentParams) {
        let uri = params.text_document.uri;
        let version = params.text_document.version;
        let text = params.text_document.text;

        self.documents.insert(uri.clone(), version, text.clone());
        self.analyze_and_publish(&uri, version, &text).await;
        self.reanalyze_dependents(&uri).await;
    }

    async fn did_change(&self, params: DidChangeTextDocumentParams) {
        let uri = params.text_document.uri;
        let version = params.text_document.version;

        if let Some(change) = params.content_changes.into_iter().last() {
            self.documents.update(&uri, version, change.text.clone());
            self.analyze_and_publish(&uri, version, &change.text).await;
            self.reanalyze_dependents(&uri).await;
        }
    }

    async fn did_save(&self, params: DidSaveTextDocumentParams) {
        let uri = params.text_document.uri;
        if let Some(doc) = self.documents.get(&uri) {
            self.analyze_and_publish(&uri, doc.version, &doc.text).await;
            self.reanalyze_dependents(&uri).await;
        }
    }

    async fn did_close(&self, params: DidCloseTextDocumentParams) {
        let uri = params.text_document.uri;
        self.documents.remove(&uri);
        self.client.publish_diagnostics(uri, Vec::new(), None).await;
    }

    async fn hover(&self, params: HoverParams) -> Result<Option<Hover>> {
        let uri = params.text_document_position_params.text_document.uri;
        let position = params.text_document_position_params.position;

        if let Some(doc) = self.documents.get(&uri) {
            Ok(hover_at(&doc, position))
        } else {
            Ok(None)
        }
    }

    async fn goto_definition(
        &self,
        params: GotoDefinitionParams,
    ) -> Result<Option<GotoDefinitionResponse>> {
        let uri = params.text_document_position_params.text_document.uri;
        let position = params.text_document_position_params.position;

        if let Some(doc) = self.documents.get(&uri) {
            Ok(definition_with_store(&doc, position, Some(&self.documents)))
        } else {
            Ok(None)
        }
    }

    async fn document_symbol(
        &self,
        params: DocumentSymbolParams,
    ) -> Result<Option<DocumentSymbolResponse>> {
        let uri = params.text_document.uri;

        if let Some(doc) = self.documents.get(&uri)
            && let Some(program) = &doc.program
        {
            return Ok(document_symbols(program, &doc.line_index));
        }
        Ok(None)
    }

    async fn completion(&self, params: CompletionParams) -> Result<Option<CompletionResponse>> {
        let uri = params.text_document_position.text_document.uri;
        let position = params.text_document_position.position;

        if let Some(doc) = self.documents.get(&uri) {
            Ok(completions_with_store(
                &doc,
                position,
                Some(&self.documents),
            ))
        } else {
            Ok(None)
        }
    }

    async fn code_action(&self, params: CodeActionParams) -> Result<Option<CodeActionResponse>> {
        let uri = params.text_document.uri;
        let range = params.range;
        let context = params.context;

        if let Some(doc) = self.documents.get(&uri) {
            Ok(code_actions_at(&doc, range, &context))
        } else {
            Ok(None)
        }
    }
}
