use std::sync::{Arc, RwLock};
use std::time::Instant;

use crate::document::DocumentStore;
use crate::php::{
    ProjectIndexCache, analyze_code_actions_for_position_with_cache,
    analyze_completion_for_position_with_cache, analyze_definition_for_position_with_cache,
    analyze_document_highlights, analyze_document_links, analyze_document_symbols,
    analyze_folding_ranges, analyze_hover_for_position_with_cache,
    analyze_inlay_hints_for_range_with_cache, analyze_parse_diagnostics,
    analyze_references_for_position_with_cache, analyze_signature_help_for_position_with_cache,
    analyze_workspace_symbols,
};
use tower_lsp::jsonrpc::Result;
use tower_lsp::lsp_types::{
    CodeActionKind, CodeActionOptions, CodeActionParams, CodeActionProviderCapability,
    CodeActionResponse, CompletionOptions, CompletionParams, CompletionResponse,
    DidChangeTextDocumentParams, DidCloseTextDocumentParams, DidOpenTextDocumentParams,
    DocumentHighlight, DocumentHighlightParams, DocumentLink, DocumentLinkOptions,
    DocumentLinkParams, DocumentSymbolParams, DocumentSymbolResponse, FoldingRange,
    FoldingRangeParams, FoldingRangeProviderCapability, GotoDefinitionParams,
    GotoDefinitionResponse, Hover, HoverParams, HoverProviderCapability, InitializeParams,
    InitializeResult, InlayHint, InlayHintOptions, InlayHintParams, InlayHintServerCapabilities,
    Location, MessageType, OneOf, ReferenceParams, ServerCapabilities, ServerInfo, SignatureHelp,
    SignatureHelpOptions, SignatureHelpParams, SymbolInformation, TextDocumentSyncCapability,
    TextDocumentSyncKind, Url, WorkspaceSymbolParams,
};
use tower_lsp::{Client, LanguageServer};

pub struct RephactorLanguageServer {
    client: Client,
    documents: Arc<RwLock<DocumentStore>>,
    index_cache: Arc<RwLock<ProjectIndexCache>>,
    root_uri: Arc<RwLock<Option<Url>>>,
}

impl RephactorLanguageServer {
    pub fn new(client: Client) -> Self {
        Self {
            client,
            documents: Arc::new(RwLock::new(DocumentStore::default())),
            index_cache: Arc::new(RwLock::new(ProjectIndexCache::default())),
            root_uri: Arc::new(RwLock::new(None)),
        }
    }

    async fn publish_diagnostics_for_open_document(&self, uri: Url) {
        let document = {
            let documents = self.documents.read().expect("document lock poisoned");
            documents.get(&uri).cloned()
        };
        let Some(document) = document else {
            self.client.publish_diagnostics(uri, Vec::new(), None).await;
            return;
        };

        let diagnostics = analyze_parse_diagnostics(&document.text);
        self.client
            .publish_diagnostics(uri, diagnostics, Some(document.version))
            .await;
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
        signature_help_provider: Some(SignatureHelpOptions {
            trigger_characters: Some(vec!["(".to_string(), ",".to_string(), ":".to_string()]),
            retrigger_characters: Some(vec![",".to_string(), ":".to_string()]),
            work_done_progress_options: Default::default(),
        }),
        definition_provider: Some(OneOf::Left(true)),
        hover_provider: Some(HoverProviderCapability::Simple(true)),
        completion_provider: Some(CompletionOptions {
            trigger_characters: Some(vec![
                "\\".to_string(),
                ":".to_string(),
                ">".to_string(),
                "$".to_string(),
            ]),
            resolve_provider: Some(false),
            ..CompletionOptions::default()
        }),
        document_symbol_provider: Some(OneOf::Left(true)),
        workspace_symbol_provider: Some(OneOf::Left(true)),
        references_provider: Some(OneOf::Left(true)),
        document_highlight_provider: Some(OneOf::Left(true)),
        folding_range_provider: Some(FoldingRangeProviderCapability::Simple(true)),
        inlay_hint_provider: Some(OneOf::Right(InlayHintServerCapabilities::Options(
            InlayHintOptions {
                resolve_provider: Some(false),
                work_done_progress_options: Default::default(),
            },
        ))),
        document_link_provider: Some(DocumentLinkOptions {
            resolve_provider: Some(false),
            work_done_progress_options: Default::default(),
        }),
        ..ServerCapabilities::default()
    }
}

#[tower_lsp::async_trait]
impl LanguageServer for RephactorLanguageServer {
    async fn initialize(&self, params: InitializeParams) -> Result<InitializeResult> {
        *self.root_uri.write().expect("root uri lock poisoned") = params.root_uri;
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
        let uri = params.text_document.uri.clone();
        self.documents
            .write()
            .expect("document lock poisoned")
            .open(params);
        self.publish_diagnostics_for_open_document(uri).await;
    }

    async fn did_change(&self, params: DidChangeTextDocumentParams) {
        let uri = params.text_document.uri.clone();
        self.documents
            .write()
            .expect("document lock poisoned")
            .change(params);
        self.publish_diagnostics_for_open_document(uri).await;
    }

    async fn did_close(&self, params: DidCloseTextDocumentParams) {
        let uri = params.text_document.uri.clone();
        self.documents
            .write()
            .expect("document lock poisoned")
            .close(params);
        self.client.publish_diagnostics(uri, Vec::new(), None).await;
    }

    async fn code_action(&self, params: CodeActionParams) -> Result<Option<CodeActionResponse>> {
        let uri = params.text_document.uri;
        let position = params.range.start;
        let started_at = Instant::now();
        let document_and_open_documents = {
            let documents = self.documents.read().expect("document lock poisoned");
            let open_documents = documents.texts();
            documents
                .get(&uri)
                .map(|document| (document.text.clone(), open_documents))
        };

        let analysis = if let Some((document_text, open_documents)) = document_and_open_documents {
            let mut index_cache = self.index_cache.write().expect("index cache lock poisoned");
            analyze_code_actions_for_position_with_cache(
                &uri,
                &document_text,
                position,
                &open_documents,
                &mut index_cache,
            )
        } else {
            crate::php::CodeActionAnalysis {
                actions: Vec::new(),
                skip_reason: Some(crate::php::SkipReason::NoSupportedCall),
                index_cache_status: crate::php::IndexCacheStatus::NoProject,
            }
        };
        let elapsed = started_at.elapsed();

        let mut log_message = format!(
            "Rephactor codeAction {}:{}:{} -> {} action(s) in {}ms ({})",
            uri,
            position.line,
            position.character,
            analysis.actions.len(),
            elapsed.as_millis(),
            analysis.index_cache_status
        );
        if analysis.actions.is_empty()
            && let Some(reason) = &analysis.skip_reason
        {
            log_message.push_str(": ");
            log_message.push_str(&reason.to_string());
        }

        self.client
            .log_message(MessageType::INFO, log_message)
            .await;

        Ok(Some(analysis.actions))
    }

    async fn signature_help(&self, params: SignatureHelpParams) -> Result<Option<SignatureHelp>> {
        let uri = params.text_document_position_params.text_document.uri;
        let position = params.text_document_position_params.position;
        let started_at = Instant::now();
        let document_and_open_documents = {
            let documents = self.documents.read().expect("document lock poisoned");
            let open_documents = documents.texts();
            documents
                .get(&uri)
                .map(|document| (document.text.clone(), open_documents))
        };

        let analysis = if let Some((document_text, open_documents)) = document_and_open_documents {
            let mut index_cache = self.index_cache.write().expect("index cache lock poisoned");
            analyze_signature_help_for_position_with_cache(
                &uri,
                &document_text,
                position,
                &open_documents,
                &mut index_cache,
            )
        } else {
            crate::php::SignatureHelpAnalysis {
                signature_help: None,
                skip_reason: Some(crate::php::SkipReason::NoSupportedCall),
                index_cache_status: crate::php::IndexCacheStatus::NoProject,
            }
        };
        let elapsed = started_at.elapsed();
        let signature_count = analysis
            .signature_help
            .as_ref()
            .map(|signature_help| signature_help.signatures.len())
            .unwrap_or_default();

        let mut log_message = format!(
            "Rephactor signatureHelp {}:{}:{} -> {} signature(s) in {}ms ({})",
            uri,
            position.line,
            position.character,
            signature_count,
            elapsed.as_millis(),
            analysis.index_cache_status
        );
        if signature_count == 0
            && let Some(reason) = &analysis.skip_reason
        {
            log_message.push_str(": ");
            log_message.push_str(&reason.to_string());
        }

        self.client
            .log_message(MessageType::INFO, log_message)
            .await;

        Ok(analysis.signature_help)
    }

    async fn goto_definition(
        &self,
        params: GotoDefinitionParams,
    ) -> Result<Option<GotoDefinitionResponse>> {
        let uri = params.text_document_position_params.text_document.uri;
        let position = params.text_document_position_params.position;
        let started_at = Instant::now();
        let document_and_open_documents = {
            let documents = self.documents.read().expect("document lock poisoned");
            let open_documents = documents.texts();
            documents
                .get(&uri)
                .map(|document| (document.text.clone(), open_documents))
        };

        let analysis = if let Some((document_text, open_documents)) = document_and_open_documents {
            let mut index_cache = self.index_cache.write().expect("index cache lock poisoned");
            analyze_definition_for_position_with_cache(
                &uri,
                &document_text,
                position,
                &open_documents,
                &mut index_cache,
            )
        } else {
            crate::php::DefinitionAnalysis {
                definition: None,
                skip_reason: Some(crate::php::SkipReason::NoSupportedCall),
                index_cache_status: crate::php::IndexCacheStatus::NoProject,
            }
        };
        let elapsed = started_at.elapsed();
        let definition_count = usize::from(analysis.definition.is_some());

        let mut log_message = format!(
            "Rephactor definition {}:{}:{} -> {} location(s) in {}ms ({})",
            uri,
            position.line,
            position.character,
            definition_count,
            elapsed.as_millis(),
            analysis.index_cache_status
        );
        if definition_count == 0
            && let Some(reason) = &analysis.skip_reason
        {
            log_message.push_str(": ");
            log_message.push_str(&reason.to_string());
        }

        self.client
            .log_message(MessageType::INFO, log_message)
            .await;

        Ok(analysis.definition)
    }

    async fn hover(&self, params: HoverParams) -> Result<Option<Hover>> {
        let uri = params.text_document_position_params.text_document.uri;
        let position = params.text_document_position_params.position;
        let started_at = Instant::now();
        let document_and_open_documents = {
            let documents = self.documents.read().expect("document lock poisoned");
            let open_documents = documents.texts();
            documents
                .get(&uri)
                .map(|document| (document.text.clone(), open_documents))
        };

        let analysis = if let Some((document_text, open_documents)) = document_and_open_documents {
            let mut index_cache = self.index_cache.write().expect("index cache lock poisoned");
            analyze_hover_for_position_with_cache(
                &uri,
                &document_text,
                position,
                &open_documents,
                &mut index_cache,
            )
        } else {
            crate::php::HoverAnalysis {
                hover: None,
                skip_reason: Some(crate::php::SkipReason::NoSupportedCall),
                index_cache_status: crate::php::IndexCacheStatus::NoProject,
            }
        };
        let elapsed = started_at.elapsed();
        let hover_count = usize::from(analysis.hover.is_some());

        let mut log_message = format!(
            "Rephactor hover {}:{}:{} -> {} hover(s) in {}ms ({})",
            uri,
            position.line,
            position.character,
            hover_count,
            elapsed.as_millis(),
            analysis.index_cache_status
        );
        if hover_count == 0
            && let Some(reason) = &analysis.skip_reason
        {
            log_message.push_str(": ");
            log_message.push_str(&reason.to_string());
        }

        self.client
            .log_message(MessageType::INFO, log_message)
            .await;

        Ok(analysis.hover)
    }

    async fn completion(&self, params: CompletionParams) -> Result<Option<CompletionResponse>> {
        let uri = params.text_document_position.text_document.uri;
        let position = params.text_document_position.position;
        let started_at = Instant::now();
        let document_and_open_documents = {
            let documents = self.documents.read().expect("document lock poisoned");
            let open_documents = documents.texts();
            documents
                .get(&uri)
                .map(|document| (document.text.clone(), open_documents))
        };

        let analysis = if let Some((document_text, open_documents)) = document_and_open_documents {
            let mut index_cache = self.index_cache.write().expect("index cache lock poisoned");
            analyze_completion_for_position_with_cache(
                &uri,
                &document_text,
                position,
                &open_documents,
                &mut index_cache,
            )
        } else {
            crate::php::CompletionAnalysis {
                completion: None,
                skip_reason: Some(crate::php::SkipReason::NoSupportedCall),
                index_cache_status: crate::php::IndexCacheStatus::NoProject,
            }
        };
        let elapsed = started_at.elapsed();
        let completion_count = analysis
            .completion
            .as_ref()
            .map(|completion| match completion {
                CompletionResponse::Array(items) => items.len(),
                CompletionResponse::List(list) => list.items.len(),
            })
            .unwrap_or_default();

        let mut log_message = format!(
            "Rephactor completion {}:{}:{} -> {} item(s) in {}ms ({})",
            uri,
            position.line,
            position.character,
            completion_count,
            elapsed.as_millis(),
            analysis.index_cache_status
        );
        if completion_count == 0
            && let Some(reason) = &analysis.skip_reason
        {
            log_message.push_str(": ");
            log_message.push_str(&reason.to_string());
        }

        self.client
            .log_message(MessageType::INFO, log_message)
            .await;

        Ok(analysis.completion)
    }

    async fn document_symbol(
        &self,
        params: DocumentSymbolParams,
    ) -> Result<Option<DocumentSymbolResponse>> {
        let uri = params.text_document.uri;
        let started_at = Instant::now();
        let document_text = {
            let documents = self.documents.read().expect("document lock poisoned");
            documents.get(&uri).map(|document| document.text.clone())
        };

        let analysis = if let Some(document_text) = document_text {
            analyze_document_symbols(&document_text)
        } else {
            crate::php::DocumentSymbolAnalysis {
                symbols: None,
                skip_reason: Some(crate::php::SkipReason::NoSupportedCall),
            }
        };
        let elapsed = started_at.elapsed();
        let symbol_count = analysis
            .symbols
            .as_ref()
            .map(|symbols| match symbols {
                DocumentSymbolResponse::Flat(symbols) => symbols.len(),
                DocumentSymbolResponse::Nested(symbols) => symbols.len(),
            })
            .unwrap_or_default();

        let mut log_message = format!(
            "Rephactor documentSymbol {} -> {} symbol(s) in {}ms",
            uri,
            symbol_count,
            elapsed.as_millis(),
        );
        if symbol_count == 0
            && let Some(reason) = &analysis.skip_reason
        {
            log_message.push_str(": ");
            log_message.push_str(&reason.to_string());
        }

        self.client
            .log_message(MessageType::INFO, log_message)
            .await;

        Ok(analysis.symbols)
    }

    async fn symbol(
        &self,
        params: WorkspaceSymbolParams,
    ) -> Result<Option<Vec<SymbolInformation>>> {
        let started_at = Instant::now();
        let root_uri = self
            .root_uri
            .read()
            .expect("root uri lock poisoned")
            .clone();
        let open_documents = {
            let documents = self.documents.read().expect("document lock poisoned");
            documents.texts()
        };
        let analysis = {
            let mut index_cache = self.index_cache.write().expect("index cache lock poisoned");
            analyze_workspace_symbols(
                root_uri.as_ref(),
                &params.query,
                &open_documents,
                &mut index_cache,
            )
        };
        let elapsed = started_at.elapsed();

        let mut log_message = format!(
            "Rephactor workspaceSymbol '{}' -> {} symbol(s) in {}ms ({})",
            params.query,
            analysis.symbols.len(),
            elapsed.as_millis(),
            analysis.index_cache_status
        );
        if analysis.symbols.is_empty()
            && let Some(reason) = &analysis.skip_reason
        {
            log_message.push_str(": ");
            log_message.push_str(&reason.to_string());
        }

        self.client
            .log_message(MessageType::INFO, log_message)
            .await;

        Ok(Some(analysis.symbols))
    }

    async fn references(&self, params: ReferenceParams) -> Result<Option<Vec<Location>>> {
        let uri = params.text_document_position.text_document.uri;
        let position = params.text_document_position.position;
        let started_at = Instant::now();
        let document_and_open_documents = {
            let documents = self.documents.read().expect("document lock poisoned");
            let open_documents = documents.texts();
            documents
                .get(&uri)
                .map(|document| (document.text.clone(), open_documents))
        };

        let analysis = if let Some((document_text, open_documents)) = document_and_open_documents {
            let mut index_cache = self.index_cache.write().expect("index cache lock poisoned");
            analyze_references_for_position_with_cache(
                &uri,
                &document_text,
                position,
                params.context.include_declaration,
                &open_documents,
                &mut index_cache,
            )
        } else {
            crate::php::ReferencesAnalysis {
                locations: Vec::new(),
                skip_reason: Some(crate::php::SkipReason::NoSupportedCall),
                index_cache_status: crate::php::IndexCacheStatus::NoProject,
            }
        };
        let elapsed = started_at.elapsed();

        let mut log_message = format!(
            "Rephactor references {}:{}:{} -> {} location(s) in {}ms ({})",
            uri,
            position.line,
            position.character,
            analysis.locations.len(),
            elapsed.as_millis(),
            analysis.index_cache_status
        );
        if analysis.locations.is_empty()
            && let Some(reason) = &analysis.skip_reason
        {
            log_message.push_str(": ");
            log_message.push_str(&reason.to_string());
        }

        self.client
            .log_message(MessageType::INFO, log_message)
            .await;

        Ok(Some(analysis.locations))
    }

    async fn document_highlight(
        &self,
        params: DocumentHighlightParams,
    ) -> Result<Option<Vec<DocumentHighlight>>> {
        let uri = params.text_document_position_params.text_document.uri;
        let position = params.text_document_position_params.position;
        let started_at = Instant::now();
        let document_text = {
            let documents = self.documents.read().expect("document lock poisoned");
            documents.get(&uri).map(|document| document.text.clone())
        };

        let analysis = if let Some(document_text) = document_text {
            analyze_document_highlights(&document_text, position)
        } else {
            crate::php::DocumentHighlightAnalysis {
                highlights: Vec::new(),
                skip_reason: Some(crate::php::SkipReason::NoSupportedCall),
            }
        };
        let elapsed = started_at.elapsed();

        let mut log_message = format!(
            "Rephactor documentHighlight {}:{}:{} -> {} highlight(s) in {}ms",
            uri,
            position.line,
            position.character,
            analysis.highlights.len(),
            elapsed.as_millis()
        );
        if analysis.highlights.is_empty()
            && let Some(reason) = &analysis.skip_reason
        {
            log_message.push_str(": ");
            log_message.push_str(&reason.to_string());
        }

        self.client
            .log_message(MessageType::INFO, log_message)
            .await;

        Ok(Some(analysis.highlights))
    }

    async fn folding_range(&self, params: FoldingRangeParams) -> Result<Option<Vec<FoldingRange>>> {
        let uri = params.text_document.uri;
        let started_at = Instant::now();
        let document_text = {
            let documents = self.documents.read().expect("document lock poisoned");
            documents.get(&uri).map(|document| document.text.clone())
        };

        let analysis = if let Some(document_text) = document_text {
            analyze_folding_ranges(&document_text)
        } else {
            crate::php::FoldingRangeAnalysis {
                ranges: Vec::new(),
                skip_reason: Some(crate::php::SkipReason::NoSupportedCall),
            }
        };
        let elapsed = started_at.elapsed();

        let mut log_message = format!(
            "Rephactor foldingRange {} -> {} range(s) in {}ms",
            uri,
            analysis.ranges.len(),
            elapsed.as_millis()
        );
        if analysis.ranges.is_empty()
            && let Some(reason) = &analysis.skip_reason
        {
            log_message.push_str(": ");
            log_message.push_str(&reason.to_string());
        }

        self.client
            .log_message(MessageType::INFO, log_message)
            .await;

        Ok(Some(analysis.ranges))
    }

    async fn inlay_hint(&self, params: InlayHintParams) -> Result<Option<Vec<InlayHint>>> {
        let uri = params.text_document.uri;
        let range = params.range;
        let started_at = Instant::now();
        let document_and_open_documents = {
            let documents = self.documents.read().expect("document lock poisoned");
            let open_documents = documents.texts();
            documents
                .get(&uri)
                .map(|document| (document.text.clone(), open_documents))
        };

        let analysis = if let Some((document_text, open_documents)) = document_and_open_documents {
            let mut index_cache = self.index_cache.write().expect("index cache lock poisoned");
            analyze_inlay_hints_for_range_with_cache(
                &uri,
                &document_text,
                range,
                &open_documents,
                &mut index_cache,
            )
        } else {
            crate::php::InlayHintAnalysis {
                hints: Vec::new(),
                skip_reason: Some(crate::php::SkipReason::NoSupportedCall),
                index_cache_status: crate::php::IndexCacheStatus::NoProject,
            }
        };
        let elapsed = started_at.elapsed();

        let mut log_message = format!(
            "Rephactor inlayHint {} -> {} hint(s) in {}ms ({})",
            uri,
            analysis.hints.len(),
            elapsed.as_millis(),
            analysis.index_cache_status
        );
        if analysis.hints.is_empty()
            && let Some(reason) = &analysis.skip_reason
        {
            log_message.push_str(": ");
            log_message.push_str(&reason.to_string());
        }

        self.client
            .log_message(MessageType::INFO, log_message)
            .await;

        Ok(Some(analysis.hints))
    }

    async fn document_link(&self, params: DocumentLinkParams) -> Result<Option<Vec<DocumentLink>>> {
        let uri = params.text_document.uri;
        let started_at = Instant::now();
        let document_text = {
            let documents = self.documents.read().expect("document lock poisoned");
            documents.get(&uri).map(|document| document.text.clone())
        };

        let analysis = if let Some(document_text) = document_text {
            analyze_document_links(&uri, &document_text)
        } else {
            crate::php::DocumentLinkAnalysis {
                links: Vec::new(),
                skip_reason: Some(crate::php::SkipReason::NoSupportedCall),
            }
        };
        let elapsed = started_at.elapsed();

        let mut log_message = format!(
            "Rephactor documentLink {} -> {} link(s) in {}ms",
            uri,
            analysis.links.len(),
            elapsed.as_millis()
        );
        if analysis.links.is_empty()
            && let Some(reason) = &analysis.skip_reason
        {
            log_message.push_str(": ");
            log_message.push_str(&reason.to_string());
        }

        self.client
            .log_message(MessageType::INFO, log_message)
            .await;

        Ok(Some(analysis.links))
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
        assert!(capabilities.signature_help_provider.is_some());
        assert_eq!(capabilities.definition_provider, Some(OneOf::Left(true)));
        assert_eq!(
            capabilities.hover_provider,
            Some(HoverProviderCapability::Simple(true))
        );
        assert!(capabilities.completion_provider.is_some());
        assert_eq!(
            capabilities.document_symbol_provider,
            Some(OneOf::Left(true))
        );
        assert_eq!(
            capabilities.workspace_symbol_provider,
            Some(OneOf::Left(true))
        );
        assert_eq!(capabilities.references_provider, Some(OneOf::Left(true)));
        assert_eq!(
            capabilities.document_highlight_provider,
            Some(OneOf::Left(true))
        );
        assert_eq!(
            capabilities.folding_range_provider,
            Some(FoldingRangeProviderCapability::Simple(true))
        );
        assert!(capabilities.inlay_hint_provider.is_some());
        assert!(capabilities.document_link_provider.is_some());
    }
}
