use std::sync::{Arc, RwLock};

use crate::document::DocumentStore;
use crate::php::code_actions_for_position;
use tower_lsp::jsonrpc::Result;
use tower_lsp::lsp_types::{
    CodeActionKind, CodeActionOptions, CodeActionParams, CodeActionProviderCapability,
    CodeActionResponse, DidChangeTextDocumentParams, DidCloseTextDocumentParams,
    DidOpenTextDocumentParams, InitializeParams, InitializeResult, MessageType, ServerCapabilities,
    ServerInfo, TextDocumentSyncCapability, TextDocumentSyncKind,
};
use tower_lsp::{Client, LanguageServer};

pub struct RephactorLanguageServer {
    client: Client,
    documents: Arc<RwLock<DocumentStore>>,
}

impl RephactorLanguageServer {
    pub fn new(client: Client) -> Self {
        Self {
            client,
            documents: Arc::new(RwLock::new(DocumentStore::default())),
        }
    }
}

fn server_capabilities() -> ServerCapabilities {
    ServerCapabilities {
        text_document_sync: Some(TextDocumentSyncCapability::Kind(
            TextDocumentSyncKind::INCREMENTAL,
        )),
        code_action_provider: Some(CodeActionProviderCapability::Options(CodeActionOptions {
            code_action_kinds: Some(vec![CodeActionKind::REFACTOR_REWRITE]),
            resolve_provider: Some(false),
            ..CodeActionOptions::default()
        })),
        ..ServerCapabilities::default()
    }
}

#[tower_lsp::async_trait]
impl LanguageServer for RephactorLanguageServer {
    async fn initialize(&self, _params: InitializeParams) -> Result<InitializeResult> {
        Ok(InitializeResult {
            capabilities: server_capabilities(),
            server_info: Some(ServerInfo {
                name: "rephactor".to_string(),
                version: Some(env!("CARGO_PKG_VERSION").to_string()),
            }),
        })
    }

    async fn initialized(&self, _params: tower_lsp::lsp_types::InitializedParams) {
        self.client
            .log_message(
                tower_lsp::lsp_types::MessageType::INFO,
                "Rephactor initialized",
            )
            .await;
    }

    async fn shutdown(&self) -> Result<()> {
        Ok(())
    }

    async fn did_open(&self, params: DidOpenTextDocumentParams) {
        self.documents
            .write()
            .expect("document lock poisoned")
            .open(params);
    }

    async fn did_change(&self, params: DidChangeTextDocumentParams) {
        self.documents
            .write()
            .expect("document lock poisoned")
            .change(params);
    }

    async fn did_close(&self, params: DidCloseTextDocumentParams) {
        self.documents
            .write()
            .expect("document lock poisoned")
            .close(params);
    }

    async fn code_action(&self, params: CodeActionParams) -> Result<Option<CodeActionResponse>> {
        let uri = params.text_document.uri;
        let position = params.range.start;
        let actions = {
            let documents = self.documents.read().expect("document lock poisoned");
            documents
                .get(&uri)
                .map(|document| code_actions_for_position(&uri, &document.text, position))
                .unwrap_or_default()
        };

        self.client
            .log_message(
                MessageType::INFO,
                format!(
                    "Rephactor codeAction {}:{}:{} -> {} action(s)",
                    uri,
                    position.line,
                    position.character,
                    actions.len()
                ),
            )
            .await;

        Ok(Some(actions))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn capabilities_advertise_incremental_sync_and_code_actions() {
        let capabilities = server_capabilities();

        assert_eq!(
            capabilities.text_document_sync,
            Some(TextDocumentSyncCapability::Kind(
                TextDocumentSyncKind::INCREMENTAL
            ))
        );
        assert!(matches!(
            capabilities.code_action_provider,
            Some(CodeActionProviderCapability::Options(CodeActionOptions {
                code_action_kinds: Some(kinds),
                resolve_provider: Some(false),
                ..
            })) if kinds == vec![CodeActionKind::REFACTOR_REWRITE]
        ));
    }
}
