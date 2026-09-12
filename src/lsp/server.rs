//! Tower-LSP LanguageServer implementation for Modus.

use super::code_actions::code_actions_at;
use super::completion::completions_at;
use super::definition::definition_at;
use super::diagnostics::compute_diagnostics;
use super::document::DocumentStore;
use super::hover::hover_at;
use super::symbols::document_symbols;
use tower_lsp::jsonrpc::Result;
use tower_lsp::lsp_types::*;
use tower_lsp::{Client, LanguageServer};

pub struct ModusBackend {
    pub client: Client,
    pub documents: DocumentStore,
}

impl ModusBackend {
    pub fn new(client: Client) -> Self {
        Self {
            client,
            documents: DocumentStore::new(),
        }
    }

    async fn analyze_and_publish(&self, uri: &Url, version: i32, text: &str) {
        let doc = self.documents.get(uri).await;
        if let Some(doc) = doc {
            let (program, env, diagnostics) = compute_diagnostics(text, &doc.line_index);
            self.documents
                .update_analysis(uri, program, env, diagnostics.clone())
                .await;
            self.client
                .publish_diagnostics(uri.clone(), diagnostics, Some(version))
                .await;
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
                    trigger_characters: Some(vec![".".to_string(), ":".to_string()]),
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

        self.documents
            .insert(uri.clone(), version, text.clone())
            .await;
        self.analyze_and_publish(&uri, version, &text).await;
    }

    async fn did_change(&self, params: DidChangeTextDocumentParams) {
        let uri = params.text_document.uri;
        let version = params.text_document.version;

        if let Some(change) = params.content_changes.into_iter().last() {
            self.documents
                .update(&uri, version, change.text.clone())
                .await;
            self.analyze_and_publish(&uri, version, &change.text).await;
        }
    }

    async fn did_save(&self, params: DidSaveTextDocumentParams) {
        let uri = params.text_document.uri;
        if let Some(doc) = self.documents.get(&uri).await {
            self.analyze_and_publish(&uri, doc.version, &doc.text).await;
        }
    }

    async fn did_close(&self, params: DidCloseTextDocumentParams) {
        let uri = params.text_document.uri;
        self.documents.remove(&uri).await;
        self.client.publish_diagnostics(uri, Vec::new(), None).await;
    }

    async fn hover(&self, params: HoverParams) -> Result<Option<Hover>> {
        let uri = params.text_document_position_params.text_document.uri;
        let position = params.text_document_position_params.position;

        if let Some(doc) = self.documents.get(&uri).await {
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

        if let Some(doc) = self.documents.get(&uri).await {
            Ok(definition_at(&doc, position))
        } else {
            Ok(None)
        }
    }

    async fn document_symbol(
        &self,
        params: DocumentSymbolParams,
    ) -> Result<Option<DocumentSymbolResponse>> {
        let uri = params.text_document.uri;

        if let Some(doc) = self.documents.get(&uri).await
            && let Some(program) = &doc.program
        {
            return Ok(document_symbols(program, &doc.line_index));
        }
        Ok(None)
    }

    async fn completion(&self, params: CompletionParams) -> Result<Option<CompletionResponse>> {
        let uri = params.text_document_position.text_document.uri;
        let position = params.text_document_position.position;

        if let Some(doc) = self.documents.get(&uri).await {
            Ok(completions_at(&doc, position))
        } else {
            Ok(None)
        }
    }

    async fn code_action(&self, params: CodeActionParams) -> Result<Option<CodeActionResponse>> {
        let uri = params.text_document.uri;
        let range = params.range;
        let context = params.context;

        if let Some(doc) = self.documents.get(&uri).await {
            Ok(code_actions_at(&doc, range, &context))
        } else {
            Ok(None)
        }
    }
}
