use tower_lsp::jsonrpc::Result;
use tower_lsp::lsp_types::{
    CodeActionOptions, CodeActionProviderCapability, InitializeParams, InitializeResult,
    ServerCapabilities, ServerInfo, TextDocumentSyncCapability, TextDocumentSyncKind,
};
use tower_lsp::{Client, LanguageServer};

pub struct RephactorLanguageServer {
    client: Client,
}

impl RephactorLanguageServer {
    pub fn new(client: Client) -> Self {
        Self { client }
    }
}

#[tower_lsp::async_trait]
impl LanguageServer for RephactorLanguageServer {
    async fn initialize(&self, _params: InitializeParams) -> Result<InitializeResult> {
        Ok(InitializeResult {
            capabilities: ServerCapabilities {
                text_document_sync: Some(TextDocumentSyncCapability::Kind(
                    TextDocumentSyncKind::INCREMENTAL,
                )),
                code_action_provider: Some(CodeActionProviderCapability::Options(
                    CodeActionOptions {
                        resolve_provider: Some(false),
                        ..CodeActionOptions::default()
                    },
                )),
                ..ServerCapabilities::default()
            },
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
}
