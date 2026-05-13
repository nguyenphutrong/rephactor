use std::collections::{HashMap, HashSet};
use std::fmt;
use std::fs;
use std::path::{Path, PathBuf};

use tower_lsp::lsp_types::{
    CodeAction, CodeActionKind, CodeActionOrCommand, CodeLens, Command, CompletionItem,
    CompletionItemKind, CompletionResponse, Diagnostic, DiagnosticSeverity, DiagnosticTag,
    DocumentHighlight, DocumentHighlightKind, DocumentLink, DocumentSymbol, DocumentSymbolResponse,
    FoldingRange, FoldingRangeKind, GotoDefinitionResponse, Hover, HoverContents, InlayHint,
    InlayHintKind, InlayHintLabel, InlineValue, InlineValueVariableLookup, Location, MarkupContent,
    MarkupKind, ParameterInformation, ParameterLabel, Position, Range, SelectionRange,
    SignatureHelp, SignatureInformation, SymbolInformation, SymbolKind, TextEdit, Url,
    WorkspaceEdit,
};
use tree_sitter::{Node, Parser};

use crate::document::{byte_offset_for_lsp_position, lsp_position_for_byte_offset};

const ACTION_TITLE: &str = "[Rephactor] Add names to arguments";

#[derive(Debug, Clone, PartialEq, Eq)]
struct Signature {
    name: String,
    parameters: Vec<String>,
    parameter_types: Vec<Option<ComparableReturnType>>,
    return_type: Option<ComparableReturnType>,
    is_variadic: bool,
    is_abstract: bool,
    location: Option<SourceLocation>,
    doc_summary: Option<String>,
}

#[derive(Debug, Clone, Default, PartialEq, Eq)]
struct ClassInfo {
    fqn: String,
    location: Option<SourceLocation>,
    doc_summary: Option<String>,
    methods: HashMap<String, Signature>,
    constructor: Option<Signature>,
    parents: Vec<String>,
    interfaces: Vec<String>,
    traits: Vec<String>,
    mixins: Vec<String>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
struct SourceLocation {
    path: PathBuf,
    byte_offset: usize,
}

#[derive(Debug, Default)]
struct ImportMap {
    classes: HashMap<String, String>,
}

#[derive(Debug, Clone, Default)]
struct SymbolIndex {
    functions: HashMap<String, Vec<Signature>>,
    classes: HashMap<String, ClassInfo>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
struct CallInfo {
    target: CallTarget,
    arguments: Vec<ArgumentInfo>,
    arguments_start_byte: usize,
    arguments_end_byte: usize,
}

#[derive(Debug, Clone, PartialEq, Eq)]
enum CallTarget {
    Function(String),
    StaticMethod { class_name: String, method: String },
    Constructor { class_name: String },
    InstanceMethod { variable: String, method: String },
}

#[derive(Debug, Clone, PartialEq, Eq)]
struct ArgumentInfo {
    start_byte: usize,
    end_byte: usize,
    insert_byte: usize,
    name: Option<String>,
    is_unpacking: bool,
}

#[derive(Debug, Clone, PartialEq, Eq)]
struct ImportDeclaration {
    fqn: String,
    alias: String,
    start_byte: usize,
    end_byte: usize,
    is_grouped: bool,
    has_alias: bool,
}

#[derive(Debug, Clone, Default)]
pub struct ProjectIndexCache {
    indexes: HashMap<PathBuf, SymbolIndex>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum IndexCacheStatus {
    Hit(PathBuf),
    Miss(PathBuf),
    NoProject,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum SkipReason {
    PhpVersionBelow8,
    InvalidCursorPosition,
    ParseError,
    NoSupportedCall,
    UnsupportedDynamicCall,
    UnpackingArgument,
    UnsafeNamedArguments,
    UnresolvedCallable(String),
    AmbiguousCallable(String),
    NoEdits,
}

#[derive(Debug)]
pub struct CodeActionAnalysis {
    pub actions: Vec<CodeActionOrCommand>,
    pub skip_reason: Option<SkipReason>,
    pub index_cache_status: IndexCacheStatus,
}

#[derive(Debug)]
pub struct SignatureHelpAnalysis {
    pub signature_help: Option<SignatureHelp>,
    pub skip_reason: Option<SkipReason>,
    pub index_cache_status: IndexCacheStatus,
}

#[derive(Debug)]
pub struct DefinitionAnalysis {
    pub definition: Option<GotoDefinitionResponse>,
    pub skip_reason: Option<SkipReason>,
    pub index_cache_status: IndexCacheStatus,
}

#[derive(Debug)]
pub struct HoverAnalysis {
    pub hover: Option<Hover>,
    pub skip_reason: Option<SkipReason>,
    pub index_cache_status: IndexCacheStatus,
}

#[derive(Debug)]
pub struct CompletionAnalysis {
    pub completion: Option<CompletionResponse>,
    pub skip_reason: Option<SkipReason>,
    pub index_cache_status: IndexCacheStatus,
}

#[derive(Debug)]
pub struct DocumentSymbolAnalysis {
    pub symbols: Option<DocumentSymbolResponse>,
    pub skip_reason: Option<SkipReason>,
}

#[derive(Debug)]
pub struct WorkspaceSymbolAnalysis {
    pub symbols: Vec<SymbolInformation>,
    pub skip_reason: Option<SkipReason>,
    pub index_cache_status: IndexCacheStatus,
}

#[derive(Debug)]
pub struct ReferencesAnalysis {
    pub locations: Vec<Location>,
    pub skip_reason: Option<SkipReason>,
    pub index_cache_status: IndexCacheStatus,
}

#[derive(Debug)]
pub struct CodeLensAnalysis {
    pub lenses: Vec<CodeLens>,
    pub skip_reason: Option<SkipReason>,
    pub index_cache_status: IndexCacheStatus,
}

#[derive(Debug)]
pub struct RenameAnalysis {
    pub edit: Option<WorkspaceEdit>,
    pub skip_reason: Option<SkipReason>,
    pub index_cache_status: IndexCacheStatus,
}

#[derive(Debug)]
pub struct DocumentHighlightAnalysis {
    pub highlights: Vec<DocumentHighlight>,
    pub skip_reason: Option<SkipReason>,
}

#[derive(Debug)]
pub struct FoldingRangeAnalysis {
    pub ranges: Vec<FoldingRange>,
    pub skip_reason: Option<SkipReason>,
}

#[derive(Debug)]
pub struct InlayHintAnalysis {
    pub hints: Vec<InlayHint>,
    pub skip_reason: Option<SkipReason>,
    pub index_cache_status: IndexCacheStatus,
}

#[derive(Debug)]
pub struct DocumentLinkAnalysis {
    pub links: Vec<DocumentLink>,
    pub skip_reason: Option<SkipReason>,
}

pub fn analyze_parse_diagnostics(text: &str) -> Vec<Diagnostic> {
    let Some(tree) = parse_php_allowing_errors(text) else {
        return vec![Diagnostic::new_simple(
            Range::default(),
            "Unable to parse PHP document".to_string(),
        )];
    };
    let mut diagnostics = Vec::new();
    collect_parse_error_diagnostics(tree.root_node(), text, &mut diagnostics);
    diagnostics
}

pub fn analyze_diagnostics_for_document_with_cache(
    uri: &Url,
    text: &str,
    open_documents: &HashMap<Url, String>,
    cache: &mut ProjectIndexCache,
) -> Vec<Diagnostic> {
    let mut diagnostics = analyze_parse_diagnostics(text);
    if !diagnostics.is_empty() {
        return diagnostics;
    }

    let Some(tree) = parse_php(text) else {
        return diagnostics;
    };
    let root = tree.root_node();
    let imports = ImportMap::from_root(root, text);
    let index = cache.index_for_document(uri, text, open_documents);
    let mut call_nodes = Vec::new();
    collect_supported_call_nodes(root, 0, text.len(), &mut call_nodes);

    for call_node in call_nodes {
        let Ok(call) = call_info(call_node, text) else {
            continue;
        };
        let namespace = namespace_at_byte(root, text, call_node.start_byte());
        match index.resolve(
            &call.target,
            root,
            text,
            call_node.start_byte(),
            namespace.as_deref(),
            &imports,
        ) {
            Ok(signature) => {
                diagnostics.extend(duplicate_named_argument_diagnostics(text, &call));
                diagnostics.extend(unknown_named_argument_diagnostics(text, &call, &signature));
                diagnostics.extend(too_many_argument_diagnostics(text, &call, &signature));
                diagnostics.extend(argument_type_mismatch_diagnostics(
                    root, text, &imports, &index, call_node, &call, &signature,
                ));
            }
            Err(
                reason @ (SkipReason::UnresolvedCallable(_) | SkipReason::AmbiguousCallable(_)),
            ) => {
                diagnostics.push(Diagnostic {
                    range: call_target_range(text, call_node).unwrap_or_else(|_| Range::default()),
                    severity: Some(DiagnosticSeverity::ERROR),
                    code: None,
                    code_description: None,
                    source: Some("rephactor".to_string()),
                    message: reason.to_string(),
                    related_information: None,
                    tags: None,
                    data: None,
                });
            }
            Err(_) => {}
        }
    }

    let mut reported_type_ranges = HashSet::new();
    let mut type_nodes = Vec::new();
    collect_type_reference_nodes(root, &mut type_nodes);
    for type_node in type_nodes {
        if !reported_type_ranges.insert((type_node.start_byte(), type_node.end_byte())) {
            continue;
        }

        let type_name = clean_name_text(node_text(type_node, text));
        if is_builtin_type_name(&type_name) {
            continue;
        }

        let namespace = namespace_at_byte(root, text, type_node.start_byte());
        if index
            .resolve_class(&type_name, namespace.as_deref(), &imports)
            .is_some()
        {
            continue;
        }

        diagnostics.push(Diagnostic {
            range: range_for_bytes(text, type_node.start_byte(), type_node.end_byte())
                .unwrap_or_else(|_| Range::default()),
            severity: Some(DiagnosticSeverity::ERROR),
            code: None,
            code_description: None,
            source: Some("rephactor".to_string()),
            message: format!("unresolved type {type_name}"),
            related_information: None,
            tags: None,
            data: None,
        });
    }

    diagnostics.extend(duplicate_declaration_diagnostics(root, text));
    diagnostics.extend(duplicate_parameter_diagnostics(root, text));
    diagnostics.extend(return_type_mismatch_diagnostics(
        root, text, &imports, &index,
    ));
    diagnostics.extend(assignment_type_mismatch_diagnostics(
        root, text, &imports, &index,
    ));
    diagnostics.extend(unused_import_diagnostics(
        root,
        text,
        &import_declarations(root, text),
    ));

    diagnostics
}

pub fn analyze_selection_ranges(text: &str, positions: &[Position]) -> Vec<SelectionRange> {
    selection_ranges_for_text(text, positions).unwrap_or_default()
}

enum CodeActionOutcome {
    Action(Box<CodeAction>),
    NoAction(SkipReason),
}

impl fmt::Display for SkipReason {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::PhpVersionBelow8 => write!(formatter, "project allows PHP below 8.0"),
            Self::InvalidCursorPosition => write!(formatter, "invalid cursor position"),
            Self::ParseError => write!(formatter, "PHP parse error"),
            Self::NoSupportedCall => write!(formatter, "no supported call at cursor"),
            Self::UnsupportedDynamicCall => write!(formatter, "unsupported dynamic call target"),
            Self::UnpackingArgument => write!(formatter, "call contains unpacking argument"),
            Self::UnsafeNamedArguments => write!(formatter, "existing named arguments are unsafe"),
            Self::UnresolvedCallable(callable) => {
                write!(formatter, "unresolved callable {callable}")
            }
            Self::AmbiguousCallable(callable) => {
                write!(formatter, "ambiguous callable {callable}")
            }
            Self::NoEdits => write!(formatter, "no positional arguments need names"),
        }
    }
}

#[cfg(test)]
pub fn analyze_code_actions_for_position(
    uri: &Url,
    text: &str,
    position: Position,
    open_documents: &HashMap<Url, String>,
) -> CodeActionAnalysis {
    let mut cache = ProjectIndexCache::default();
    analyze_code_actions_for_position_with_cache(uri, text, position, open_documents, &mut cache)
}

pub fn analyze_code_actions_for_position_with_cache(
    uri: &Url,
    text: &str,
    position: Position,
    open_documents: &HashMap<Url, String>,
    cache: &mut ProjectIndexCache,
) -> CodeActionAnalysis {
    let index_cache_status = cache.status_for_document(uri);
    let mut actions = Vec::new();
    let mut skip_reason = None;

    match named_argument_code_action_with_cache(uri, text, position, open_documents, cache) {
        CodeActionOutcome::Action(action) => actions.push(CodeActionOrCommand::CodeAction(*action)),
        CodeActionOutcome::NoAction(reason) => skip_reason = Some(reason),
    }

    match import_code_actions_with_cache(uri, text, position, open_documents, cache) {
        Ok(import_actions) => {
            actions.extend(
                import_actions
                    .into_iter()
                    .map(CodeActionOrCommand::CodeAction),
            );
        }
        Err(reason) if actions.is_empty() && skip_reason.is_none() => skip_reason = Some(reason),
        Err(_) => {}
    }

    match phpdoc_code_action(uri, text, position) {
        Ok(Some(action)) => actions.push(CodeActionOrCommand::CodeAction(action)),
        Ok(None) => {}
        Err(reason) if actions.is_empty() && skip_reason.is_none() => skip_reason = Some(reason),
        Err(_) => {}
    }

    match implement_interface_methods_action_with_cache(uri, text, position, open_documents, cache)
    {
        Ok(Some(action)) => actions.push(CodeActionOrCommand::CodeAction(action)),
        Ok(None) => {}
        Err(reason) if actions.is_empty() && skip_reason.is_none() => skip_reason = Some(reason),
        Err(_) => {}
    }
    match implement_abstract_methods_action_with_cache(uri, text, position, open_documents, cache) {
        Ok(Some(action)) => actions.push(CodeActionOrCommand::CodeAction(action)),
        Ok(None) => {}
        Err(reason) if actions.is_empty() && skip_reason.is_none() => skip_reason = Some(reason),
        Err(_) => {}
    }

    CodeActionAnalysis {
        skip_reason: actions.is_empty().then_some(skip_reason).flatten(),
        actions,
        index_cache_status,
    }
}

pub fn analyze_signature_help_for_position_with_cache(
    uri: &Url,
    text: &str,
    position: Position,
    open_documents: &HashMap<Url, String>,
    cache: &mut ProjectIndexCache,
) -> SignatureHelpAnalysis {
    let index_cache_status = cache.status_for_document(uri);
    match signature_help_for_position_with_cache(uri, text, position, open_documents, cache) {
        Ok(signature_help) => SignatureHelpAnalysis {
            signature_help: Some(signature_help),
            skip_reason: None,
            index_cache_status,
        },
        Err(reason) => SignatureHelpAnalysis {
            signature_help: None,
            skip_reason: Some(reason),
            index_cache_status,
        },
    }
}

pub fn analyze_definition_for_position_with_cache(
    uri: &Url,
    text: &str,
    position: Position,
    open_documents: &HashMap<Url, String>,
    cache: &mut ProjectIndexCache,
) -> DefinitionAnalysis {
    let index_cache_status = cache.status_for_document(uri);
    match definition_for_position_with_cache(uri, text, position, open_documents, cache) {
        Ok(definition) => DefinitionAnalysis {
            definition: Some(definition),
            skip_reason: None,
            index_cache_status,
        },
        Err(reason) => DefinitionAnalysis {
            definition: None,
            skip_reason: Some(reason),
            index_cache_status,
        },
    }
}

pub fn analyze_declaration_for_position_with_cache(
    uri: &Url,
    text: &str,
    position: Position,
    open_documents: &HashMap<Url, String>,
    cache: &mut ProjectIndexCache,
) -> DefinitionAnalysis {
    let index_cache_status = cache.status_for_document(uri);
    match declaration_for_position_with_cache(uri, text, position, open_documents, cache) {
        Ok(declaration) => DefinitionAnalysis {
            definition: Some(declaration),
            skip_reason: None,
            index_cache_status,
        },
        Err(reason) => DefinitionAnalysis {
            definition: None,
            skip_reason: Some(reason),
            index_cache_status,
        },
    }
}

pub fn analyze_type_definition_for_position_with_cache(
    uri: &Url,
    text: &str,
    position: Position,
    open_documents: &HashMap<Url, String>,
    cache: &mut ProjectIndexCache,
) -> DefinitionAnalysis {
    let index_cache_status = cache.status_for_document(uri);
    match type_definition_for_position_with_cache(uri, text, position, open_documents, cache) {
        Ok(definition) => DefinitionAnalysis {
            definition: Some(definition),
            skip_reason: None,
            index_cache_status,
        },
        Err(reason) => DefinitionAnalysis {
            definition: None,
            skip_reason: Some(reason),
            index_cache_status,
        },
    }
}

pub fn analyze_implementation_for_position_with_cache(
    uri: &Url,
    text: &str,
    position: Position,
    open_documents: &HashMap<Url, String>,
    cache: &mut ProjectIndexCache,
) -> DefinitionAnalysis {
    let index_cache_status = cache.status_for_document(uri);
    match implementation_for_position_with_cache(uri, text, position, open_documents, cache) {
        Ok(definition) => DefinitionAnalysis {
            definition: Some(definition),
            skip_reason: None,
            index_cache_status,
        },
        Err(reason) => DefinitionAnalysis {
            definition: None,
            skip_reason: Some(reason),
            index_cache_status,
        },
    }
}

pub fn analyze_hover_for_position_with_cache(
    uri: &Url,
    text: &str,
    position: Position,
    open_documents: &HashMap<Url, String>,
    cache: &mut ProjectIndexCache,
) -> HoverAnalysis {
    let index_cache_status = cache.status_for_document(uri);
    match hover_for_position_with_cache(uri, text, position, open_documents, cache) {
        Ok(hover) => HoverAnalysis {
            hover: Some(hover),
            skip_reason: None,
            index_cache_status,
        },
        Err(reason) => HoverAnalysis {
            hover: None,
            skip_reason: Some(reason),
            index_cache_status,
        },
    }
}

pub fn analyze_completion_for_position_with_cache(
    uri: &Url,
    text: &str,
    position: Position,
    open_documents: &HashMap<Url, String>,
    cache: &mut ProjectIndexCache,
) -> CompletionAnalysis {
    let index_cache_status = cache.status_for_document(uri);
    match completion_for_position_with_cache(uri, text, position, open_documents, cache) {
        Ok(completion) => CompletionAnalysis {
            completion: Some(completion),
            skip_reason: None,
            index_cache_status,
        },
        Err(reason) => CompletionAnalysis {
            completion: None,
            skip_reason: Some(reason),
            index_cache_status,
        },
    }
}

pub fn analyze_document_symbols(text: &str) -> DocumentSymbolAnalysis {
    match document_symbols_for_text(text) {
        Ok(symbols) => DocumentSymbolAnalysis {
            symbols: Some(symbols),
            skip_reason: None,
        },
        Err(reason) => DocumentSymbolAnalysis {
            symbols: None,
            skip_reason: Some(reason),
        },
    }
}

pub fn analyze_workspace_symbols(
    root_uri: Option<&Url>,
    query: &str,
    open_documents: &HashMap<Url, String>,
    cache: &mut ProjectIndexCache,
) -> WorkspaceSymbolAnalysis {
    let Some(project_root) = root_uri.and_then(project_root_from_workspace_uri) else {
        return WorkspaceSymbolAnalysis {
            symbols: Vec::new(),
            skip_reason: Some(SkipReason::NoSupportedCall),
            index_cache_status: IndexCacheStatus::NoProject,
        };
    };
    let index_cache_status = cache.status_for_project_root(&project_root);
    let index = cache.index_for_project_root(&project_root, open_documents);
    let open_paths = open_project_documents(open_documents);
    let symbols = workspace_symbols_for_index(&index, query, &open_paths);

    WorkspaceSymbolAnalysis {
        skip_reason: symbols.is_empty().then_some(SkipReason::NoEdits),
        symbols,
        index_cache_status,
    }
}

pub fn analyze_references_for_position_with_cache(
    uri: &Url,
    text: &str,
    position: Position,
    include_declaration: bool,
    open_documents: &HashMap<Url, String>,
    cache: &mut ProjectIndexCache,
) -> ReferencesAnalysis {
    let index_cache_status = cache.status_for_document(uri);
    match references_for_position(uri, text, position, include_declaration, open_documents) {
        Ok(locations) => ReferencesAnalysis {
            skip_reason: locations.is_empty().then_some(SkipReason::NoEdits),
            locations,
            index_cache_status,
        },
        Err(reason) => ReferencesAnalysis {
            locations: Vec::new(),
            skip_reason: Some(reason),
            index_cache_status,
        },
    }
}

pub fn analyze_code_lenses_for_document_with_cache(
    uri: &Url,
    text: &str,
    open_documents: &HashMap<Url, String>,
    cache: &mut ProjectIndexCache,
) -> CodeLensAnalysis {
    let index_cache_status = cache.status_for_document(uri);
    match code_lenses_for_document(uri, text, open_documents) {
        Ok(lenses) => CodeLensAnalysis {
            skip_reason: lenses.is_empty().then_some(SkipReason::NoEdits),
            lenses,
            index_cache_status,
        },
        Err(reason) => CodeLensAnalysis {
            lenses: Vec::new(),
            skip_reason: Some(reason),
            index_cache_status,
        },
    }
}

pub fn analyze_rename_for_position_with_cache(
    uri: &Url,
    text: &str,
    position: Position,
    new_name: &str,
    open_documents: &HashMap<Url, String>,
    cache: &mut ProjectIndexCache,
) -> RenameAnalysis {
    let index_cache_status = cache.status_for_document(uri);
    match rename_for_position(uri, text, position, new_name, open_documents) {
        Ok(edit) => RenameAnalysis {
            edit: Some(edit),
            skip_reason: None,
            index_cache_status,
        },
        Err(reason) => RenameAnalysis {
            edit: None,
            skip_reason: Some(reason),
            index_cache_status,
        },
    }
}

pub fn analyze_document_highlights(text: &str, position: Position) -> DocumentHighlightAnalysis {
    match document_highlights_for_position(text, position) {
        Ok(highlights) => DocumentHighlightAnalysis {
            skip_reason: highlights.is_empty().then_some(SkipReason::NoEdits),
            highlights,
        },
        Err(reason) => DocumentHighlightAnalysis {
            highlights: Vec::new(),
            skip_reason: Some(reason),
        },
    }
}

pub fn analyze_folding_ranges(text: &str) -> FoldingRangeAnalysis {
    match folding_ranges_for_text(text) {
        Ok(ranges) => FoldingRangeAnalysis {
            skip_reason: ranges.is_empty().then_some(SkipReason::NoEdits),
            ranges,
        },
        Err(reason) => FoldingRangeAnalysis {
            ranges: Vec::new(),
            skip_reason: Some(reason),
        },
    }
}

pub fn analyze_inlay_hints_for_range_with_cache(
    uri: &Url,
    text: &str,
    range: Range,
    open_documents: &HashMap<Url, String>,
    cache: &mut ProjectIndexCache,
) -> InlayHintAnalysis {
    let index_cache_status = cache.status_for_document(uri);
    match inlay_hints_for_range(uri, text, range, open_documents, cache) {
        Ok(hints) => InlayHintAnalysis {
            skip_reason: hints.is_empty().then_some(SkipReason::NoEdits),
            hints,
            index_cache_status,
        },
        Err(reason) => InlayHintAnalysis {
            hints: Vec::new(),
            skip_reason: Some(reason),
            index_cache_status,
        },
    }
}

pub fn analyze_document_links(uri: &Url, text: &str) -> DocumentLinkAnalysis {
    match document_links_for_text(uri, text) {
        Ok(links) => DocumentLinkAnalysis {
            skip_reason: links.is_empty().then_some(SkipReason::NoEdits),
            links,
        },
        Err(reason) => DocumentLinkAnalysis {
            links: Vec::new(),
            skip_reason: Some(reason),
        },
    }
}

pub fn formatting_edits_for_text(text: &str) -> Vec<TextEdit> {
    if text.is_empty() {
        return Vec::new();
    }

    let formatted = trim_trailing_whitespace(text, true);
    if formatted == text {
        return Vec::new();
    }

    let Some(end) = lsp_position_for_byte_offset(text, text.len()) else {
        return Vec::new();
    };

    vec![TextEdit::new(
        Range {
            start: Position::new(0, 0),
            end,
        },
        formatted,
    )]
}

pub fn range_formatting_edits_for_text(text: &str, range: Range) -> Vec<TextEdit> {
    if text.is_empty() {
        return Vec::new();
    }

    let Some(start_byte) = byte_offset_for_lsp_position(text, range.start) else {
        return Vec::new();
    };
    let Some(end_byte) = byte_offset_for_lsp_position(text, range.end) else {
        return Vec::new();
    };
    let Some(selected_text) = text.get(start_byte..end_byte) else {
        return Vec::new();
    };

    let formatted = trim_trailing_whitespace(selected_text, false);
    if formatted == selected_text {
        return Vec::new();
    }

    vec![TextEdit::new(range, formatted)]
}

pub fn inline_values_for_range(text: &str, range: Range) -> Vec<InlineValue> {
    let Some(start_byte) = byte_offset_for_lsp_position(text, range.start) else {
        return Vec::new();
    };
    let Some(end_byte) = byte_offset_for_lsp_position(text, range.end) else {
        return Vec::new();
    };
    let Some(tree) = parse_php(text) else {
        return Vec::new();
    };

    let mut variables = Vec::new();
    collect_variable_inline_values(tree.root_node(), text, start_byte, end_byte, &mut variables);
    variables
}

fn named_argument_code_action_with_cache(
    uri: &Url,
    text: &str,
    position: Position,
    open_documents: &HashMap<Url, String>,
    cache: &mut ProjectIndexCache,
) -> CodeActionOutcome {
    if !document_supports_named_arguments(uri) {
        return CodeActionOutcome::NoAction(SkipReason::PhpVersionBelow8);
    }

    let Some(byte_offset) = byte_offset_for_lsp_position(text, position) else {
        return CodeActionOutcome::NoAction(SkipReason::InvalidCursorPosition);
    };

    let Some(tree) = parse_php(text) else {
        return CodeActionOutcome::NoAction(SkipReason::ParseError);
    };
    let root = tree.root_node();
    let namespace = namespace_at_byte(root, text, byte_offset);
    let imports = ImportMap::from_root(root, text);
    let call = match find_call_at_byte(root, text, byte_offset) {
        Ok(call) => call,
        Err(reason) => return CodeActionOutcome::NoAction(reason),
    };
    let index = cache.index_for_document(uri, text, open_documents);
    let signature = match index.resolve(
        &call.target,
        root,
        text,
        byte_offset,
        namespace.as_deref(),
        &imports,
    ) {
        Ok(signature) => signature,
        Err(reason) => return CodeActionOutcome::NoAction(reason),
    };
    let edits = match edits_for_call(text, &call, &signature) {
        Ok(edits) => edits,
        Err(reason) => return CodeActionOutcome::NoAction(reason),
    };
    let title = action_title_for_edits(&edits);

    let mut changes = HashMap::new();
    changes.insert(uri.clone(), edits);

    CodeActionOutcome::Action(Box::new(CodeAction {
        title,
        kind: Some(CodeActionKind::REFACTOR_REWRITE),
        diagnostics: None,
        edit: Some(WorkspaceEdit::new(changes)),
        command: None,
        is_preferred: Some(true),
        disabled: None,
        data: None,
    }))
}

fn import_code_actions_with_cache(
    uri: &Url,
    text: &str,
    position: Position,
    open_documents: &HashMap<Url, String>,
    cache: &mut ProjectIndexCache,
) -> Result<Vec<CodeAction>, SkipReason> {
    let Some(byte_offset) = byte_offset_for_lsp_position(text, position) else {
        return Err(SkipReason::InvalidCursorPosition);
    };
    let Some(tree) = parse_php(text) else {
        return Err(SkipReason::ParseError);
    };

    let root = tree.root_node();
    let imports = import_declarations(root, text);
    let index = cache.index_for_document(uri, text, open_documents);
    let mut actions = Vec::new();

    if let Some(action) =
        replace_fqcn_with_import_action(uri, text, root, byte_offset, &imports, &index)?
    {
        actions.push(action);
    }
    if let Some(action) = sort_imports_action(uri, text, &imports)? {
        actions.push(action);
    }
    actions.extend(remove_unused_import_actions(uri, text, root, &imports)?);

    if actions.is_empty() {
        Err(SkipReason::NoEdits)
    } else {
        Ok(actions)
    }
}

fn phpdoc_code_action(
    uri: &Url,
    text: &str,
    position: Position,
) -> Result<Option<CodeAction>, SkipReason> {
    let Some(byte_offset) = byte_offset_for_lsp_position(text, position) else {
        return Err(SkipReason::InvalidCursorPosition);
    };
    let Some(tree) = parse_php(text) else {
        return Err(SkipReason::ParseError);
    };
    let root = tree.root_node();
    let Some(declaration) = find_function_like_declaration_at_byte(root, byte_offset) else {
        return Ok(None);
    };
    if phpdoc_summary_before(text, declaration.start_byte()).is_some() {
        return Ok(None);
    }

    let docblock = phpdoc_for_declaration(text, declaration);
    if docblock.is_empty() {
        return Ok(None);
    }
    let Some(position) = lsp_position_for_byte_offset(text, declaration.start_byte()) else {
        return Err(SkipReason::InvalidCursorPosition);
    };
    let edit = TextEdit::new(
        Range {
            start: position,
            end: position,
        },
        docblock,
    );

    Ok(Some(code_action("[Rephactor] Add PHPDoc", uri, vec![edit])))
}

fn implement_interface_methods_action_with_cache(
    uri: &Url,
    text: &str,
    position: Position,
    open_documents: &HashMap<Url, String>,
    cache: &mut ProjectIndexCache,
) -> Result<Option<CodeAction>, SkipReason> {
    let Some(byte_offset) = byte_offset_for_lsp_position(text, position) else {
        return Err(SkipReason::InvalidCursorPosition);
    };
    let Some(tree) = parse_php(text) else {
        return Err(SkipReason::ParseError);
    };
    let root = tree.root_node();
    let Some(class_node) = find_class_declaration_at_byte(root, byte_offset) else {
        return Ok(None);
    };
    let Some(name_node) = class_node.child_by_field_name("name") else {
        return Ok(None);
    };
    let Some(body) = class_node.child_by_field_name("body") else {
        return Ok(None);
    };

    let namespace = namespace_at_byte(root, text, class_node.start_byte());
    let imports = ImportMap::from_root(root, text);
    let index = cache.index_for_document(uri, text, open_documents);
    let class_name = clean_name_text(node_text(name_node, text));
    let Some(class_info) = index.resolve_class(&class_name, namespace.as_deref(), &imports) else {
        return Ok(None);
    };

    let missing = missing_interface_methods(&index, class_info);
    if missing.is_empty() {
        return Ok(None);
    }

    let insert_byte = body.end_byte().saturating_sub(1);
    let Some(position) = lsp_position_for_byte_offset(text, insert_byte) else {
        return Err(SkipReason::InvalidCursorPosition);
    };
    let indent = format!("{}    ", line_indent_before(text, class_node.start_byte()));
    let edit = TextEdit::new(
        Range {
            start: position,
            end: position,
        },
        method_stubs(&missing, &indent),
    );

    Ok(Some(code_action(
        "[Rephactor] Implement interface methods",
        uri,
        vec![edit],
    )))
}

fn implement_abstract_methods_action_with_cache(
    uri: &Url,
    text: &str,
    position: Position,
    open_documents: &HashMap<Url, String>,
    cache: &mut ProjectIndexCache,
) -> Result<Option<CodeAction>, SkipReason> {
    let Some(byte_offset) = byte_offset_for_lsp_position(text, position) else {
        return Err(SkipReason::InvalidCursorPosition);
    };
    let Some(tree) = parse_php(text) else {
        return Err(SkipReason::ParseError);
    };
    let root = tree.root_node();
    let Some(class_node) = find_class_declaration_at_byte(root, byte_offset) else {
        return Ok(None);
    };
    let Some(name_node) = class_node.child_by_field_name("name") else {
        return Ok(None);
    };
    let Some(body) = class_node.child_by_field_name("body") else {
        return Ok(None);
    };

    let namespace = namespace_at_byte(root, text, class_node.start_byte());
    let imports = ImportMap::from_root(root, text);
    let index = cache.index_for_document(uri, text, open_documents);
    let class_name = clean_name_text(node_text(name_node, text));
    let Some(class_info) = index.resolve_class(&class_name, namespace.as_deref(), &imports) else {
        return Ok(None);
    };

    let missing = missing_abstract_parent_methods(&index, class_info);
    if missing.is_empty() {
        return Ok(None);
    }

    let insert_byte = body.end_byte().saturating_sub(1);
    let Some(position) = lsp_position_for_byte_offset(text, insert_byte) else {
        return Err(SkipReason::InvalidCursorPosition);
    };
    let indent = format!("{}    ", line_indent_before(text, class_node.start_byte()));
    let edit = TextEdit::new(
        Range {
            start: position,
            end: position,
        },
        method_stubs(&missing, &indent),
    );

    Ok(Some(code_action(
        "[Rephactor] Implement abstract methods",
        uri,
        vec![edit],
    )))
}

fn signature_help_for_position_with_cache(
    uri: &Url,
    text: &str,
    position: Position,
    open_documents: &HashMap<Url, String>,
    cache: &mut ProjectIndexCache,
) -> Result<SignatureHelp, SkipReason> {
    if !document_supports_named_arguments(uri) {
        return Err(SkipReason::PhpVersionBelow8);
    }

    let Some(byte_offset) = byte_offset_for_lsp_position(text, position) else {
        return Err(SkipReason::InvalidCursorPosition);
    };

    let Some(tree) = parse_php(text) else {
        return Err(SkipReason::ParseError);
    };
    let root = tree.root_node();
    let namespace = namespace_at_byte(root, text, byte_offset);
    let imports = ImportMap::from_root(root, text);
    let call = find_call_at_byte(root, text, byte_offset)?;

    if call.arguments.iter().any(|argument| argument.is_unpacking) {
        return Err(SkipReason::UnpackingArgument);
    }

    let index = cache.index_for_document(uri, text, open_documents);
    let signature = index.resolve(
        &call.target,
        root,
        text,
        byte_offset,
        namespace.as_deref(),
        &imports,
    )?;
    let active_parameter = active_parameter_for_call(byte_offset, &call, &signature)?;

    Ok(signature_help_for_call(
        &call.target,
        &signature,
        active_parameter,
    ))
}

fn definition_for_position_with_cache(
    uri: &Url,
    text: &str,
    position: Position,
    open_documents: &HashMap<Url, String>,
    cache: &mut ProjectIndexCache,
) -> Result<GotoDefinitionResponse, SkipReason> {
    let Some(byte_offset) = byte_offset_for_lsp_position(text, position) else {
        return Err(SkipReason::InvalidCursorPosition);
    };

    let Some(tree) = parse_php(text) else {
        return Err(SkipReason::ParseError);
    };
    let root = tree.root_node();
    let namespace = namespace_at_byte(root, text, byte_offset);
    let imports = ImportMap::from_root(root, text);
    let index = cache.index_for_document(uri, text, open_documents);
    let open_paths = open_project_documents(open_documents);

    if let Ok(call) = find_call_at_byte(root, text, byte_offset) {
        let signature = index.resolve(
            &call.target,
            root,
            text,
            byte_offset,
            namespace.as_deref(),
            &imports,
        )?;
        return location_response(signature.location.as_ref(), &open_paths);
    }

    let Some(name_node) = find_name_reference_at_byte(root, text, byte_offset) else {
        return Err(SkipReason::NoSupportedCall);
    };
    let class_name = clean_name_text(node_text(name_node, text));
    let Some(class_info) = index.resolve_class(&class_name, namespace.as_deref(), &imports) else {
        return Err(SkipReason::UnresolvedCallable(class_name));
    };

    location_response(class_info.location.as_ref(), &open_paths)
}

fn declaration_for_position_with_cache(
    uri: &Url,
    text: &str,
    position: Position,
    open_documents: &HashMap<Url, String>,
    cache: &mut ProjectIndexCache,
) -> Result<GotoDefinitionResponse, SkipReason> {
    let Some(byte_offset) = byte_offset_for_lsp_position(text, position) else {
        return Err(SkipReason::InvalidCursorPosition);
    };

    let Some(tree) = parse_php(text) else {
        return Err(SkipReason::ParseError);
    };
    let root = tree.root_node();
    let Some(method) = find_method_declaration_at_byte(root, byte_offset) else {
        return Err(SkipReason::NoSupportedCall);
    };
    let Some(method_name) = method.child_by_field_name("name") else {
        return Err(SkipReason::NoSupportedCall);
    };
    let Some(class_node) = containing_class_like_declaration(method) else {
        return Err(SkipReason::NoSupportedCall);
    };
    let Some(class_name) = class_node.child_by_field_name("name") else {
        return Err(SkipReason::NoSupportedCall);
    };

    let namespace = namespace_at_byte(root, text, class_node.start_byte());
    let imports = ImportMap::from_root(root, text);
    let index = cache.index_for_document(uri, text, open_documents);
    let class_name = clean_name_text(node_text(class_name, text));
    let Some(class_info) = index.resolve_class(&class_name, namespace.as_deref(), &imports) else {
        return Err(SkipReason::UnresolvedCallable(class_name));
    };

    let method_key = normalize_method_key(node_text(method_name, text));
    let mut declarations = Vec::new();
    let mut visited = Vec::new();
    for related_name in class_info
        .parents
        .iter()
        .chain(class_info.interfaces.iter())
    {
        index.collect_related_method_signatures(
            related_name,
            &method_key,
            &mut visited,
            &mut declarations,
        );
    }

    match declarations.len() {
        0 => Err(SkipReason::NoEdits),
        1 => {
            let open_paths = open_project_documents(open_documents);
            location_response(declarations[0].location.as_ref(), &open_paths)
        }
        _ => Err(SkipReason::AmbiguousCallable(format!(
            "{}::{}",
            class_info.fqn,
            node_text(method_name, text)
        ))),
    }
}

fn type_definition_for_position_with_cache(
    uri: &Url,
    text: &str,
    position: Position,
    open_documents: &HashMap<Url, String>,
    cache: &mut ProjectIndexCache,
) -> Result<GotoDefinitionResponse, SkipReason> {
    let Some(byte_offset) = byte_offset_for_lsp_position(text, position) else {
        return Err(SkipReason::InvalidCursorPosition);
    };

    let Some(tree) = parse_php(text) else {
        return Err(SkipReason::ParseError);
    };
    let root = tree.root_node();
    let namespace = namespace_at_byte(root, text, byte_offset);
    let imports = ImportMap::from_root(root, text);
    let index = cache.index_for_document(uri, text, open_documents);
    let open_paths = open_project_documents(open_documents);

    if let Some(variable) = find_variable_name_at_byte(root, text, byte_offset) {
        let variable_types =
            variable_types_at_byte(root, text, byte_offset, namespace.as_deref(), &imports);
        let Some(class_name) = variable_types.get(&variable) else {
            return Err(SkipReason::UnresolvedCallable(variable));
        };
        let Some(class_info) = index.resolve_class(class_name, namespace.as_deref(), &imports)
        else {
            return Err(SkipReason::UnresolvedCallable(class_name.clone()));
        };
        return location_response(class_info.location.as_ref(), &open_paths);
    }

    let Some(name_node) = find_name_reference_at_byte(root, text, byte_offset) else {
        return Err(SkipReason::NoSupportedCall);
    };
    let class_name = clean_name_text(node_text(name_node, text));
    let Some(class_info) = index.resolve_class(&class_name, namespace.as_deref(), &imports) else {
        return Err(SkipReason::UnresolvedCallable(class_name));
    };

    location_response(class_info.location.as_ref(), &open_paths)
}

fn implementation_for_position_with_cache(
    uri: &Url,
    text: &str,
    position: Position,
    open_documents: &HashMap<Url, String>,
    cache: &mut ProjectIndexCache,
) -> Result<GotoDefinitionResponse, SkipReason> {
    let Some(byte_offset) = byte_offset_for_lsp_position(text, position) else {
        return Err(SkipReason::InvalidCursorPosition);
    };

    let Some(tree) = parse_php(text) else {
        return Err(SkipReason::ParseError);
    };
    let root = tree.root_node();
    let namespace = namespace_at_byte(root, text, byte_offset);
    let imports = ImportMap::from_root(root, text);
    let index = cache.index_for_document(uri, text, open_documents);
    let open_paths = open_project_documents(open_documents);

    if let Some(method) = find_method_declaration_at_byte(root, byte_offset) {
        let Some(method_name) = method.child_by_field_name("name") else {
            return Err(SkipReason::NoSupportedCall);
        };
        let Some(class_node) = containing_class_like_declaration(method) else {
            return Err(SkipReason::NoSupportedCall);
        };
        let Some(class_name) = class_node.child_by_field_name("name") else {
            return Err(SkipReason::NoSupportedCall);
        };
        let namespace = namespace_at_byte(root, text, class_node.start_byte());
        let class_name = clean_name_text(node_text(class_name, text));
        let Some(target) = index.resolve_class(&class_name, namespace.as_deref(), &imports) else {
            return Err(SkipReason::UnresolvedCallable(class_name));
        };
        let method_key = normalize_method_key(node_text(method_name, text));
        return implementation_locations_for_method(&index, target, &method_key, &open_paths);
    }

    let Some(name_node) = find_name_reference_at_byte(root, text, byte_offset) else {
        return Err(SkipReason::NoSupportedCall);
    };
    let class_name = clean_name_text(node_text(name_node, text));
    let Some(target) = index.resolve_class(&class_name, namespace.as_deref(), &imports) else {
        return Err(SkipReason::UnresolvedCallable(class_name));
    };

    let mut locations = index
        .classes
        .values()
        .filter(|class_info| {
            normalize_symbol_key(&class_info.fqn) != normalize_symbol_key(&target.fqn)
        })
        .filter(|class_info| {
            class_derives_from(&index, class_info, &target.fqn, &mut HashSet::new())
        })
        .filter_map(|class_info| location_for_source(class_info.location.as_ref()?, &open_paths))
        .collect::<Vec<_>>();
    locations.sort_by_key(|location| location.uri.to_string());

    if locations.is_empty() {
        Err(SkipReason::NoEdits)
    } else {
        Ok(GotoDefinitionResponse::Array(locations))
    }
}

fn implementation_locations_for_method(
    index: &SymbolIndex,
    target: &ClassInfo,
    method_key: &str,
    open_paths: &HashMap<PathBuf, String>,
) -> Result<GotoDefinitionResponse, SkipReason> {
    let mut locations = index
        .classes
        .values()
        .filter(|class_info| {
            normalize_symbol_key(&class_info.fqn) != normalize_symbol_key(&target.fqn)
        })
        .filter(|class_info| {
            class_derives_from(index, class_info, &target.fqn, &mut HashSet::new())
        })
        .filter_map(|class_info| class_info.methods.get(method_key))
        .filter_map(|signature| location_for_source(signature.location.as_ref()?, open_paths))
        .collect::<Vec<_>>();
    locations.sort_by_key(|location| location.uri.to_string());

    if locations.is_empty() {
        Err(SkipReason::NoEdits)
    } else {
        Ok(GotoDefinitionResponse::Array(locations))
    }
}

fn missing_interface_methods(index: &SymbolIndex, class_info: &ClassInfo) -> Vec<Signature> {
    let mut missing = Vec::new();
    let mut seen = HashSet::new();

    for interface_name in &class_info.interfaces {
        let Some(interface_info) = index.classes.get(&normalize_symbol_key(interface_name)) else {
            continue;
        };

        for (method_key, signature) in &interface_info.methods {
            if class_info.methods.contains_key(method_key) || !seen.insert(method_key.clone()) {
                continue;
            }
            missing.push(signature.clone());
        }
    }

    missing.sort_by_key(|signature| signature.name.to_ascii_lowercase());
    missing
}

fn missing_abstract_parent_methods(index: &SymbolIndex, class_info: &ClassInfo) -> Vec<Signature> {
    let mut missing = Vec::new();
    let mut seen = HashSet::new();

    for parent_name in &class_info.parents {
        let Some(parent_info) = index.classes.get(&normalize_symbol_key(parent_name)) else {
            continue;
        };

        for (method_key, signature) in &parent_info.methods {
            if !signature.is_abstract
                || class_info.methods.contains_key(method_key)
                || !seen.insert(method_key.clone())
            {
                continue;
            }
            missing.push(signature.clone());
        }
    }

    missing.sort_by_key(|signature| signature.name.to_ascii_lowercase());
    missing
}

fn method_stubs(signatures: &[Signature], indent: &str) -> String {
    let body_indent = format!("{indent}    ");
    signatures
        .iter()
        .map(|signature| {
            let parameters = signature
                .parameters
                .iter()
                .map(|parameter| format!("${parameter}"))
                .collect::<Vec<_>>()
                .join(", ");
            format!(
                "\n{indent}public function {}({parameters}) {{\n{body_indent}throw new \\BadMethodCallException('Not implemented');\n{indent}}}\n",
                signature.name
            )
        })
        .collect::<String>()
}

fn class_derives_from(
    index: &SymbolIndex,
    class_info: &ClassInfo,
    target_fqn: &str,
    visited: &mut HashSet<String>,
) -> bool {
    if !visited.insert(normalize_symbol_key(&class_info.fqn)) {
        return false;
    }

    class_info
        .parents
        .iter()
        .chain(class_info.interfaces.iter())
        .any(|related| normalize_symbol_key(related) == normalize_symbol_key(target_fqn))
        || class_info
            .parents
            .iter()
            .chain(class_info.interfaces.iter())
            .filter_map(|related| index.classes.get(&normalize_symbol_key(related)))
            .any(|related| class_derives_from(index, related, target_fqn, visited))
}

fn hover_for_position_with_cache(
    uri: &Url,
    text: &str,
    position: Position,
    open_documents: &HashMap<Url, String>,
    cache: &mut ProjectIndexCache,
) -> Result<Hover, SkipReason> {
    let Some(byte_offset) = byte_offset_for_lsp_position(text, position) else {
        return Err(SkipReason::InvalidCursorPosition);
    };
    let Some(tree) = parse_php(text) else {
        return Err(SkipReason::ParseError);
    };
    let root = tree.root_node();
    let namespace = namespace_at_byte(root, text, byte_offset);
    let imports = ImportMap::from_root(root, text);
    let index = cache.index_for_document(uri, text, open_documents);

    if let Ok(call) = find_call_at_byte(root, text, byte_offset) {
        let signature = index.resolve(
            &call.target,
            root,
            text,
            byte_offset,
            namespace.as_deref(),
            &imports,
        )?;
        return Ok(hover_from_parts(
            signature_label(&call.target, &signature),
            signature.location.as_ref(),
            signature.doc_summary.as_deref(),
        ));
    }

    let Some(name_node) = find_name_reference_at_byte(root, text, byte_offset) else {
        return Err(SkipReason::NoSupportedCall);
    };
    let class_name = clean_name_text(node_text(name_node, text));
    let Some(class_info) = index.resolve_class(&class_name, namespace.as_deref(), &imports) else {
        return Err(SkipReason::UnresolvedCallable(class_name));
    };

    Ok(hover_from_parts(
        format!("class {}", class_info.fqn),
        class_info.location.as_ref(),
        class_info.doc_summary.as_deref(),
    ))
}

fn completion_for_position_with_cache(
    uri: &Url,
    text: &str,
    position: Position,
    open_documents: &HashMap<Url, String>,
    cache: &mut ProjectIndexCache,
) -> Result<CompletionResponse, SkipReason> {
    let Some(byte_offset) = byte_offset_for_lsp_position(text, position) else {
        return Err(SkipReason::InvalidCursorPosition);
    };
    let Some(tree) = parse_php(text) else {
        return Err(SkipReason::ParseError);
    };
    let root = tree.root_node();
    let namespace = namespace_at_byte(root, text, byte_offset);
    let imports = ImportMap::from_root(root, text);
    let index = cache.index_for_document(uri, text, open_documents);
    let prefix = completion_prefix(text, byte_offset);

    let items = if let Some((class_name, method_prefix)) =
        static_method_completion_context(text, byte_offset)
    {
        let Some(class_info) = index.resolve_class(&class_name, namespace.as_deref(), &imports)
        else {
            return Err(SkipReason::UnresolvedCallable(class_name));
        };
        method_completion_items(&index, class_info, &method_prefix)
    } else if let Some((variable, method_prefix)) =
        instance_method_completion_context(text, byte_offset)
    {
        let variable_types =
            variable_types_at_byte(root, text, byte_offset, namespace.as_deref(), &imports);
        let Some(class_name) = variable_types.get(&variable) else {
            return Err(SkipReason::UnresolvedCallable(variable));
        };
        let Some(class_info) = index.resolve_class(class_name, namespace.as_deref(), &imports)
        else {
            return Err(SkipReason::UnresolvedCallable(class_name.clone()));
        };
        method_completion_items(&index, class_info, &method_prefix)
    } else {
        let import_declarations = import_declarations(root, text);
        let mut items = class_completion_items(
            text,
            root,
            namespace.as_deref(),
            &import_declarations,
            &index,
            &prefix,
        );
        items.extend(function_completion_items(&index, &prefix));
        items.extend(keyword_completion_items(&prefix));
        items.sort_by_key(|item| item.label.to_ascii_lowercase());
        items
    };

    if items.is_empty() {
        Err(SkipReason::NoEdits)
    } else {
        Ok(CompletionResponse::Array(items))
    }
}

fn document_symbols_for_text(text: &str) -> Result<DocumentSymbolResponse, SkipReason> {
    let Some(tree) = parse_php(text) else {
        return Err(SkipReason::ParseError);
    };

    let symbols = collect_document_symbols(tree.root_node(), text)?;
    Ok(DocumentSymbolResponse::Nested(symbols))
}

fn document_links_for_text(uri: &Url, text: &str) -> Result<Vec<DocumentLink>, SkipReason> {
    let Some(tree) = parse_php_allowing_errors(text) else {
        return Err(SkipReason::ParseError);
    };
    let Some(document_path) = uri.to_file_path().ok() else {
        return Err(SkipReason::InvalidCursorPosition);
    };
    let base_dir = document_path.parent().unwrap_or_else(|| Path::new(""));
    let mut links = Vec::new();
    collect_document_links(tree.root_node(), text, base_dir, &mut links);
    Ok(links)
}

fn collect_document_links(node: Node, text: &str, base_dir: &Path, links: &mut Vec<DocumentLink>) {
    if is_include_or_require_node(node)
        && let Some((target_path, start_byte, end_byte)) =
            include_literal_target(node, text, base_dir)
        && target_path.is_file()
        && let Some(target) = Url::from_file_path(target_path).ok()
        && let Ok(range) = range_for_bytes(text, start_byte, end_byte)
    {
        links.push(DocumentLink {
            range,
            target: Some(target),
            tooltip: None,
            data: None,
        });
    }

    let mut cursor = node.walk();
    for child in node.named_children(&mut cursor) {
        collect_document_links(child, text, base_dir, links);
    }
}

fn is_include_or_require_node(node: Node) -> bool {
    matches!(
        node.kind(),
        "include_expression"
            | "include_once_expression"
            | "require_expression"
            | "require_once_expression"
    )
}

fn include_literal_target(
    node: Node,
    text: &str,
    base_dir: &Path,
) -> Option<(PathBuf, usize, usize)> {
    let node_text = node_text(node, text);
    let quote_index = node_text.find(['"', '\''])?;
    let quote = node_text.as_bytes()[quote_index] as char;
    let rest = node_text.get(quote_index + 1..)?;
    let end_quote = rest.find(quote)?;
    let relative = rest.get(..end_quote)?;
    if relative.contains("://") {
        return None;
    }

    let start_byte = node.start_byte() + quote_index + 1;
    let end_byte = start_byte + relative.len();
    Some((base_dir.join(relative), start_byte, end_byte))
}

fn folding_ranges_for_text(text: &str) -> Result<Vec<FoldingRange>, SkipReason> {
    let Some(tree) = parse_php_allowing_errors(text) else {
        return Err(SkipReason::ParseError);
    };
    let mut ranges = Vec::new();
    collect_folding_ranges(tree.root_node(), text, &mut ranges);
    ranges.sort_by_key(|range| (range.start_line, range.end_line));
    Ok(ranges)
}

fn inlay_hints_for_range(
    uri: &Url,
    text: &str,
    range: Range,
    open_documents: &HashMap<Url, String>,
    cache: &mut ProjectIndexCache,
) -> Result<Vec<InlayHint>, SkipReason> {
    let Some(start_byte) = byte_offset_for_lsp_position(text, range.start) else {
        return Err(SkipReason::InvalidCursorPosition);
    };
    let Some(end_byte) = byte_offset_for_lsp_position(text, range.end) else {
        return Err(SkipReason::InvalidCursorPosition);
    };
    let Some(tree) = parse_php(text) else {
        return Err(SkipReason::ParseError);
    };
    let root = tree.root_node();
    let imports = ImportMap::from_root(root, text);
    let index = cache.index_for_document(uri, text, open_documents);
    let mut call_nodes = Vec::new();
    collect_supported_call_nodes(root, start_byte, end_byte, &mut call_nodes);
    let mut hints = Vec::new();
    collect_return_type_inlay_hints(root, text, start_byte, end_byte, &mut hints);

    for call_node in call_nodes {
        let byte_offset = call_node.start_byte();
        let namespace = namespace_at_byte(root, text, byte_offset);
        let Ok(call) = call_info(call_node, text) else {
            continue;
        };
        if call.arguments.iter().any(|argument| argument.is_unpacking) {
            continue;
        }
        let Ok(signature) = index.resolve(
            &call.target,
            root,
            text,
            byte_offset,
            namespace.as_deref(),
            &imports,
        ) else {
            continue;
        };

        for (argument, parameter_name) in call.arguments.iter().zip(signature.parameters.iter()) {
            if argument.name.is_some() {
                continue;
            }
            let Some(position) = lsp_position_for_byte_offset(text, argument.start_byte) else {
                continue;
            };
            hints.push(InlayHint {
                position,
                label: InlayHintLabel::String(format!("{parameter_name}:")),
                kind: Some(InlayHintKind::PARAMETER),
                text_edits: None,
                tooltip: None,
                padding_left: None,
                padding_right: Some(true),
                data: None,
            });
        }
    }

    Ok(hints)
}

fn collect_return_type_inlay_hints(
    node: Node,
    text: &str,
    start_byte: usize,
    end_byte: usize,
    hints: &mut Vec<InlayHint>,
) {
    if node.end_byte() < start_byte || node.start_byte() > end_byte {
        return;
    }

    if matches!(node.kind(), "function_definition" | "method_declaration")
        && node.child_by_field_name("return_type").is_none()
        && let Some(parameters) = node.child_by_field_name("parameters")
        && let Some(return_type) = inferred_new_return_type(node, text)
        && let Some(position) = lsp_position_for_byte_offset(text, parameters.end_byte())
    {
        hints.push(InlayHint {
            position,
            label: InlayHintLabel::String(format!(": {return_type}")),
            kind: Some(InlayHintKind::TYPE),
            text_edits: None,
            tooltip: None,
            padding_left: None,
            padding_right: None,
            data: None,
        });
    }

    let mut cursor = node.walk();
    for child in node.named_children(&mut cursor) {
        collect_return_type_inlay_hints(child, text, start_byte, end_byte, hints);
    }
}

fn inferred_new_return_type(node: Node, text: &str) -> Option<String> {
    let mut names = Vec::new();
    collect_new_return_type_names(node, text, &mut names);
    names.sort_by_key(|name| name.to_ascii_lowercase());
    names.dedup_by(|left, right| left.eq_ignore_ascii_case(right));
    (names.len() == 1).then(|| names.remove(0))
}

fn collect_new_return_type_names(node: Node, text: &str, names: &mut Vec<String>) {
    if node.kind() == "return_statement"
        && let Some(object_creation) = find_descendant_kind(node, "object_creation_expression")
        && let Some(class_node) = class_name_for_object_creation(object_creation)
        && is_name_node(class_node)
    {
        names.push(clean_name_text(node_text(class_node, text)));
    }

    let mut cursor = node.walk();
    for child in node.named_children(&mut cursor) {
        collect_new_return_type_names(child, text, names);
    }
}

fn collect_variable_inline_values(
    node: Node,
    text: &str,
    start_byte: usize,
    end_byte: usize,
    values: &mut Vec<InlineValue>,
) {
    if node.end_byte() < start_byte || node.start_byte() > end_byte {
        return;
    }

    if node.kind() == "variable_name"
        && let Ok(range) = range_for_bytes(text, node.start_byte(), node.end_byte())
    {
        values.push(InlineValue::VariableLookup(InlineValueVariableLookup {
            range,
            variable_name: Some(node_text(node, text).to_string()),
            case_sensitive_lookup: true,
        }));
        return;
    }

    let mut cursor = node.walk();
    for child in node.named_children(&mut cursor) {
        collect_variable_inline_values(child, text, start_byte, end_byte, values);
    }
}

fn collect_supported_call_nodes<'tree>(
    node: Node<'tree>,
    start_byte: usize,
    end_byte: usize,
    calls: &mut Vec<Node<'tree>>,
) {
    if node.end_byte() < start_byte || node.start_byte() > end_byte {
        return;
    }
    if is_supported_call_kind(node.kind()) {
        calls.push(node);
        return;
    }

    let mut cursor = node.walk();
    for child in node.named_children(&mut cursor) {
        collect_supported_call_nodes(child, start_byte, end_byte, calls);
    }
}

fn collect_folding_ranges(node: Node, text: &str, ranges: &mut Vec<FoldingRange>) {
    if let Some(kind) = folding_kind_for_node(node)
        && let Some(range) = folding_range_for_node(node, text, kind)
    {
        ranges.push(range);
    }

    let mut cursor = node.walk();
    for child in node.children(&mut cursor) {
        collect_folding_ranges(child, text, ranges);
    }
}

fn folding_kind_for_node(node: Node) -> Option<FoldingRangeKind> {
    match node.kind() {
        "comment" => Some(FoldingRangeKind::Comment),
        "namespace_use_declaration" => Some(FoldingRangeKind::Imports),
        "compound_statement" | "declaration_list" => Some(FoldingRangeKind::Region),
        _ => None,
    }
}

fn folding_range_for_node(node: Node, text: &str, kind: FoldingRangeKind) -> Option<FoldingRange> {
    let start = lsp_position_for_byte_offset(text, node.start_byte())?;
    let end = lsp_position_for_byte_offset(text, node.end_byte())?;
    if start.line >= end.line {
        return None;
    }

    Some(FoldingRange {
        start_line: start.line,
        start_character: Some(start.character),
        end_line: end.line,
        end_character: Some(end.character),
        kind: Some(kind),
        collapsed_text: None,
    })
}

fn collect_parse_error_diagnostics(node: Node, text: &str, diagnostics: &mut Vec<Diagnostic>) {
    if node.is_error() || node.is_missing() {
        diagnostics.push(Diagnostic {
            range: range_for_bytes(text, node.start_byte(), node.end_byte())
                .unwrap_or_else(|_| Range::default()),
            severity: Some(DiagnosticSeverity::ERROR),
            code: None,
            code_description: None,
            source: Some("rephactor".to_string()),
            message: "PHP parse error".to_string(),
            related_information: None,
            tags: None,
            data: None,
        });
        return;
    }

    if !node.has_error() {
        return;
    }

    let mut cursor = node.walk();
    for child in node.children(&mut cursor) {
        collect_parse_error_diagnostics(child, text, diagnostics);
    }
}

fn duplicate_declaration_diagnostics(root: Node, text: &str) -> Vec<Diagnostic> {
    let mut declarations = Vec::new();
    collect_duplicate_checked_declarations(root, &mut declarations);
    let mut seen = HashMap::new();
    let mut diagnostics = Vec::new();

    for declaration in declarations {
        let Some(name_node) = declaration.child_by_field_name("name") else {
            continue;
        };
        let namespace = namespace_at_byte(root, text, declaration.start_byte());
        let name = qualify_name(node_text(name_node, text), namespace.as_deref());
        let duplicate_key = match declaration.kind() {
            "function_definition" => format!("function:{}", normalize_symbol_key(&name)),
            "class_declaration" | "interface_declaration" | "trait_declaration" => {
                format!("type:{}", normalize_symbol_key(&name))
            }
            _ => continue,
        };

        if seen.insert(duplicate_key, name_node.start_byte()).is_some() {
            let label = if declaration.kind() == "function_definition" {
                "function"
            } else {
                "type"
            };
            diagnostics.push(Diagnostic {
                range: range_for_bytes(text, name_node.start_byte(), name_node.end_byte())
                    .unwrap_or_else(|_| Range::default()),
                severity: Some(DiagnosticSeverity::ERROR),
                code: None,
                code_description: None,
                source: Some("rephactor".to_string()),
                message: format!("duplicate {label} declaration {name}"),
                related_information: None,
                tags: None,
                data: None,
            });
        }
    }

    diagnostics
}

fn collect_duplicate_checked_declarations<'tree>(
    node: Node<'tree>,
    declarations: &mut Vec<Node<'tree>>,
) {
    if matches!(
        node.kind(),
        "function_definition" | "class_declaration" | "interface_declaration" | "trait_declaration"
    ) {
        declarations.push(node);
    }

    let mut cursor = node.walk();
    for child in node.named_children(&mut cursor) {
        collect_duplicate_checked_declarations(child, declarations);
    }
}

fn duplicate_parameter_diagnostics(root: Node, text: &str) -> Vec<Diagnostic> {
    let mut declarations = Vec::new();
    collect_function_like_declarations(root, &mut declarations);
    let mut diagnostics = Vec::new();

    for declaration in declarations {
        let Some(parameters) = declaration.child_by_field_name("parameters") else {
            continue;
        };
        let mut seen = HashSet::new();
        let mut cursor = parameters.walk();

        for parameter in parameters.named_children(&mut cursor) {
            if !matches!(
                parameter.kind(),
                "simple_parameter" | "variadic_parameter" | "property_promotion_parameter"
            ) {
                continue;
            }
            let Some(name_node) = parameter.child_by_field_name("name") else {
                continue;
            };
            let parameter_name = node_text(name_node, text);
            if seen.insert(parameter_name.to_string()) {
                continue;
            }

            diagnostics.push(Diagnostic {
                range: range_for_bytes(text, name_node.start_byte(), name_node.end_byte())
                    .unwrap_or_else(|_| Range::default()),
                severity: Some(DiagnosticSeverity::ERROR),
                code: None,
                code_description: None,
                source: Some("rephactor".to_string()),
                message: format!("duplicate parameter {parameter_name}"),
                related_information: None,
                tags: None,
                data: None,
            });
        }
    }

    diagnostics
}

fn unknown_named_argument_diagnostics(
    text: &str,
    call: &CallInfo,
    signature: &Signature,
) -> Vec<Diagnostic> {
    call.arguments
        .iter()
        .filter_map(|argument| {
            let argument_name = argument.name.as_ref()?;
            if signature
                .parameters
                .iter()
                .any(|parameter| parameter.eq_ignore_ascii_case(argument_name))
            {
                return None;
            }

            Some(Diagnostic {
                range: range_for_bytes(text, argument.start_byte, argument.end_byte)
                    .unwrap_or_else(|_| Range::default()),
                severity: Some(DiagnosticSeverity::ERROR),
                code: None,
                code_description: None,
                source: Some("rephactor".to_string()),
                message: format!("unknown named argument {argument_name}"),
                related_information: None,
                tags: None,
                data: None,
            })
        })
        .collect()
}

fn duplicate_named_argument_diagnostics(text: &str, call: &CallInfo) -> Vec<Diagnostic> {
    let mut seen = HashSet::new();
    let mut diagnostics = Vec::new();

    for argument in &call.arguments {
        let Some(argument_name) = &argument.name else {
            continue;
        };
        let argument_key = argument_name.to_ascii_lowercase();
        if seen.insert(argument_key) {
            continue;
        }

        diagnostics.push(Diagnostic {
            range: range_for_bytes(text, argument.start_byte, argument.end_byte)
                .unwrap_or_else(|_| Range::default()),
            severity: Some(DiagnosticSeverity::ERROR),
            code: None,
            code_description: None,
            source: Some("rephactor".to_string()),
            message: format!("duplicate named argument {argument_name}"),
            related_information: None,
            tags: None,
            data: None,
        });
    }

    diagnostics
}

fn too_many_argument_diagnostics(
    text: &str,
    call: &CallInfo,
    signature: &Signature,
) -> Vec<Diagnostic> {
    if signature.is_variadic
        || call.arguments.iter().any(|argument| argument.is_unpacking)
        || call.arguments.len() <= signature.parameters.len()
    {
        return Vec::new();
    }

    let Some(argument) = call.arguments.get(signature.parameters.len()) else {
        return Vec::new();
    };

    vec![Diagnostic {
        range: range_for_bytes(text, argument.start_byte, argument.end_byte)
            .unwrap_or_else(|_| Range::default()),
        severity: Some(DiagnosticSeverity::ERROR),
        code: None,
        code_description: None,
        source: Some("rephactor".to_string()),
        message: format!("too many arguments for {}", signature.name),
        related_information: None,
        tags: None,
        data: None,
    }]
}

fn argument_type_mismatch_diagnostics(
    root: Node,
    text: &str,
    imports: &ImportMap,
    index: &SymbolIndex,
    call_node: Node,
    call: &CallInfo,
    signature: &Signature,
) -> Vec<Diagnostic> {
    if signature.parameter_types.iter().all(Option::is_none) {
        return Vec::new();
    }

    let namespace = namespace_at_byte(root, text, call_node.start_byte());
    call.arguments
        .iter()
        .enumerate()
        .filter_map(|(position, argument)| {
            if argument.is_unpacking {
                return None;
            }
            let parameter_index = parameter_index_for_argument(argument, position, signature)?;
            let expected = signature.parameter_types.get(parameter_index)?.as_ref()?;
            let argument_node =
                find_argument_node_by_range(call_node, argument.start_byte, argument.end_byte)?;
            let value_node = argument_value_node(argument_node)?;
            let actual = inferred_argument_expression_type(
                root,
                value_node,
                text,
                namespace.as_deref(),
                imports,
                index,
            )?;
            (!types_compatible(expected, &actual)).then(|| Diagnostic {
                range: range_for_bytes(text, value_node.start_byte(), value_node.end_byte())
                    .unwrap_or_else(|_| Range::default()),
                severity: Some(DiagnosticSeverity::ERROR),
                code: None,
                code_description: None,
                source: Some("rephactor".to_string()),
                message: format!(
                    "argument type mismatch for {}: expected {}, got {}",
                    signature.parameters[parameter_index], expected.display, actual.display
                ),
                related_information: None,
                tags: None,
                data: None,
            })
        })
        .collect()
}

fn parameter_index_for_argument(
    argument: &ArgumentInfo,
    position: usize,
    signature: &Signature,
) -> Option<usize> {
    if let Some(name) = argument.name.as_deref() {
        return signature
            .parameters
            .iter()
            .position(|parameter| parameter == name);
    }

    (position < signature.parameters.len()).then_some(position)
}

fn find_argument_node_by_range<'tree>(
    node: Node<'tree>,
    start_byte: usize,
    end_byte: usize,
) -> Option<Node<'tree>> {
    if node.start_byte() == start_byte && node.end_byte() == end_byte && node.kind() == "argument" {
        return Some(node);
    }
    if start_byte < node.start_byte() || end_byte > node.end_byte() {
        return None;
    }

    let mut cursor = node.walk();
    for child in node.named_children(&mut cursor) {
        if let Some(argument) = find_argument_node_by_range(child, start_byte, end_byte) {
            return Some(argument);
        }
    }

    None
}

fn argument_value_node(argument_node: Node) -> Option<Node> {
    let mut cursor = argument_node.walk();
    let children = argument_node
        .named_children(&mut cursor)
        .collect::<Vec<_>>();
    if children.is_empty() {
        return None;
    }
    if children.first().is_some_and(|child| child.kind() == "name") {
        return children.get(1).copied();
    }

    children.first().copied()
}

fn collect_function_like_declarations<'tree>(
    node: Node<'tree>,
    declarations: &mut Vec<Node<'tree>>,
) {
    if matches!(node.kind(), "function_definition" | "method_declaration") {
        declarations.push(node);
    }

    let mut cursor = node.walk();
    for child in node.named_children(&mut cursor) {
        collect_function_like_declarations(child, declarations);
    }
}

fn return_type_mismatch_diagnostics(
    root: Node,
    text: &str,
    imports: &ImportMap,
    index: &SymbolIndex,
) -> Vec<Diagnostic> {
    let mut diagnostics = Vec::new();
    collect_return_type_mismatch_diagnostics(root, root, text, imports, index, &mut diagnostics);
    diagnostics
}

fn collect_return_type_mismatch_diagnostics(
    root: Node,
    node: Node,
    text: &str,
    imports: &ImportMap,
    index: &SymbolIndex,
    diagnostics: &mut Vec<Diagnostic>,
) {
    if matches!(node.kind(), "function_definition" | "method_declaration") {
        diagnostics.extend(return_type_mismatches_for_declaration(
            root, node, text, imports, index,
        ));
    }

    let mut cursor = node.walk();
    for child in node.named_children(&mut cursor) {
        collect_return_type_mismatch_diagnostics(root, child, text, imports, index, diagnostics);
    }
}

fn return_type_mismatches_for_declaration(
    root: Node,
    declaration: Node,
    text: &str,
    imports: &ImportMap,
    index: &SymbolIndex,
) -> Vec<Diagnostic> {
    let namespace = namespace_at_byte(root, text, declaration.start_byte());
    let declared = declaration
        .child_by_field_name("return_type")
        .and_then(|type_node| {
            let return_type = single_named_type(type_node, text)?;
            let (normalized_type_name, _) = nullable_type_name(&return_type);
            let mut declared = (!matches!(
                normalize_return_type_name(normalized_type_name).as_str(),
                "mixed" | "never"
            ))
            .then(|| comparable_return_type(&return_type, namespace.as_deref(), imports))?;
            declared.allows_null = declared.allows_null || type_node_allows_null(type_node, text);
            Some(declared)
        })
        .or_else(|| {
            phpdoc_return_type_before(
                text,
                declaration.start_byte(),
                namespace.as_deref(),
                imports,
            )
        });
    let Some(declared) = declared else {
        return Vec::new();
    };

    let mut returned = Vec::new();
    collect_return_expressions(declaration, declaration, &mut returned);

    returned
        .into_iter()
        .filter_map(|expression| {
            let actual = inferred_return_expression_type(
                root,
                declaration,
                expression,
                text,
                namespace.as_deref(),
                imports,
                Some(index),
            )?;
            (!types_compatible(&declared, &actual)).then(|| Diagnostic {
                range: range_for_bytes(text, expression.start_byte(), expression.end_byte())
                    .unwrap_or_else(|_| Range::default()),
                severity: Some(DiagnosticSeverity::ERROR),
                code: None,
                code_description: None,
                source: Some("rephactor".to_string()),
                message: format!(
                    "return type mismatch: declared {}, returned {}",
                    declared.display, actual.display
                ),
                related_information: None,
                tags: None,
                data: None,
            })
        })
        .collect()
}

#[derive(Clone, Debug, Eq, PartialEq)]
struct ComparableReturnType {
    key: String,
    display: String,
    allows_null: bool,
}

fn comparable_return_type(
    type_name: &str,
    namespace: Option<&str>,
    imports: &ImportMap,
) -> ComparableReturnType {
    let (type_name, allows_null) = nullable_type_name(type_name);
    let normalized = normalize_return_type_name(type_name);
    if let Some(key) = scalar_return_type_key(&normalized) {
        return ComparableReturnType {
            key: key.to_string(),
            display: normalized,
            allows_null,
        };
    }

    let qualified = qualify_type_name(type_name, namespace, imports);
    ComparableReturnType {
        key: format!("class:{}", normalize_symbol_key(&qualified)),
        display: qualified,
        allows_null,
    }
}

fn types_compatible(expected: &ComparableReturnType, actual: &ComparableReturnType) -> bool {
    expected.key == actual.key || (expected.allows_null && actual.key == "scalar:null")
}

fn nullable_type_name(type_name: &str) -> (&str, bool) {
    let type_name = type_name.trim();
    type_name
        .strip_prefix('?')
        .map(|inner| (inner.trim(), true))
        .unwrap_or((type_name, false))
}

fn inferred_return_expression_type(
    root: Node,
    declaration: Node,
    expression: Node,
    text: &str,
    namespace: Option<&str>,
    imports: &ImportMap,
    index: Option<&SymbolIndex>,
) -> Option<ComparableReturnType> {
    let kind = expression.kind();
    if kind == "expression" {
        let mut cursor = expression.walk();
        let inner = expression.named_children(&mut cursor).next()?;
        return inferred_return_expression_type(
            root,
            declaration,
            inner,
            text,
            namespace,
            imports,
            index,
        );
    }
    if matches!(
        kind,
        "integer"
            | "float"
            | "string"
            | "encapsed_string"
            | "string_content"
            | "nowdoc_string"
            | "boolean"
            | "null"
    ) {
        let key = match kind {
            "integer" => "int",
            "float" => "float",
            "string" | "encapsed_string" | "string_content" | "nowdoc_string" => "string",
            "boolean" => "bool",
            "null" => "null",
            _ => return None,
        };
        return Some(ComparableReturnType {
            key: format!("scalar:{key}"),
            display: key.to_string(),
            allows_null: false,
        });
    }
    if kind == "array_creation_expression" {
        return Some(ComparableReturnType {
            key: "scalar:array".to_string(),
            display: "array".to_string(),
            allows_null: false,
        });
    }
    if kind == "object_creation_expression"
        && let Some(class_node) = class_name_for_object_creation(expression)
        && is_name_node(class_node)
    {
        return Some(comparable_return_type(
            &clean_name_text(node_text(class_node, text)),
            namespace,
            imports,
        ));
    }
    if kind == "variable_name" {
        if let Some(return_type) = local_variable_return_type_at_byte(
            declaration,
            text,
            expression.start_byte(),
            namespace,
            imports,
            node_text(expression, text),
        ) {
            return Some(return_type);
        }
        let call_assignment_context = CallAssignmentInference {
            root,
            text,
            byte_offset: expression.start_byte(),
            namespace,
            imports,
            index: index?,
        };
        return local_variable_call_return_type_at_byte(
            declaration,
            node_text(expression, text),
            &call_assignment_context,
        );
    }
    if matches!(
        kind,
        "function_call_expression" | "scoped_call_expression" | "member_call_expression"
    ) {
        let target = call_target_for_call_node(expression, text).ok()?;
        return index?
            .resolve(
                &target,
                root,
                text,
                expression.start_byte(),
                namespace,
                imports,
            )
            .ok()?
            .return_type;
    }

    None
}

fn local_variable_return_type_at_byte(
    declaration: Node,
    text: &str,
    byte_offset: usize,
    namespace: Option<&str>,
    imports: &ImportMap,
    variable_name: &str,
) -> Option<ComparableReturnType> {
    let mut types = HashMap::new();
    collect_local_assignment_return_types(
        declaration,
        declaration,
        text,
        byte_offset,
        namespace,
        imports,
        &mut types,
    );
    types.get(variable_name).cloned()
}

fn local_variable_call_return_type_at_byte(
    declaration: Node,
    variable_name: &str,
    context: &CallAssignmentInference<'_, '_>,
) -> Option<ComparableReturnType> {
    let mut types = HashMap::new();
    collect_local_call_assignment_return_types(declaration, declaration, &mut types, context);
    types.get(variable_name).cloned()
}

struct CallAssignmentInference<'a, 'tree> {
    root: Node<'tree>,
    text: &'a str,
    byte_offset: usize,
    namespace: Option<&'a str>,
    imports: &'a ImportMap,
    index: &'a SymbolIndex,
}

fn collect_local_call_assignment_return_types(
    declaration: Node,
    node: Node,
    types: &mut HashMap<String, ComparableReturnType>,
    context: &CallAssignmentInference<'_, '_>,
) {
    if node.start_byte() >= context.byte_offset {
        return;
    }
    if node != declaration
        && matches!(
            node.kind(),
            "function_definition"
                | "method_declaration"
                | "anonymous_function_creation_expression"
                | "arrow_function"
                | "class_declaration"
                | "interface_declaration"
                | "trait_declaration"
        )
    {
        return;
    }

    if node.kind() == "assignment_expression"
        && let (Some(left), Some(right)) = (
            node.child_by_field_name("left"),
            node.child_by_field_name("right"),
        )
        && left.kind() == "variable_name"
        && let Some(return_type) = inferred_call_return_type(right, context)
            .or_else(|| assigned_variable_return_type(right, context.text, types))
    {
        types.insert(node_text(left, context.text).to_string(), return_type);
    }

    let mut cursor = node.walk();
    for child in node.named_children(&mut cursor) {
        collect_local_call_assignment_return_types(declaration, child, types, context);
    }
}

fn inferred_call_return_type(
    expression: Node,
    context: &CallAssignmentInference<'_, '_>,
) -> Option<ComparableReturnType> {
    let kind = expression.kind();
    if kind == "expression" {
        let mut cursor = expression.walk();
        let inner = expression.named_children(&mut cursor).next()?;
        return inferred_call_return_type(inner, context);
    }
    if !matches!(
        kind,
        "function_call_expression" | "scoped_call_expression" | "member_call_expression"
    ) {
        return None;
    }

    let target = call_target_for_call_node(expression, context.text).ok()?;
    context
        .index
        .resolve(
            &target,
            context.root,
            context.text,
            expression.start_byte(),
            context.namespace,
            context.imports,
        )
        .ok()?
        .return_type
}

fn collect_local_assignment_return_types(
    declaration: Node,
    node: Node,
    text: &str,
    byte_offset: usize,
    namespace: Option<&str>,
    imports: &ImportMap,
    types: &mut HashMap<String, ComparableReturnType>,
) {
    if node.start_byte() >= byte_offset {
        return;
    }
    if node != declaration
        && matches!(
            node.kind(),
            "function_definition"
                | "method_declaration"
                | "anonymous_function_creation_expression"
                | "arrow_function"
                | "class_declaration"
                | "interface_declaration"
                | "trait_declaration"
        )
    {
        return;
    }

    if node.kind() == "assignment_expression"
        && let (Some(left), Some(right)) = (
            node.child_by_field_name("left"),
            node.child_by_field_name("right"),
        )
        && left.kind() == "variable_name"
        && let Some(return_type) = inferred_assigned_return_type(right, text, namespace, imports)
            .or_else(|| assigned_variable_return_type(right, text, types))
    {
        types.insert(node_text(left, text).to_string(), return_type);
    }

    let mut cursor = node.walk();
    for child in node.named_children(&mut cursor) {
        collect_local_assignment_return_types(
            declaration,
            child,
            text,
            byte_offset,
            namespace,
            imports,
            types,
        );
    }
}

fn inferred_assigned_return_type(
    expression: Node,
    text: &str,
    namespace: Option<&str>,
    imports: &ImportMap,
) -> Option<ComparableReturnType> {
    let kind = expression.kind();
    if kind == "expression" {
        let mut cursor = expression.walk();
        let inner = expression.named_children(&mut cursor).next()?;
        return inferred_assigned_return_type(inner, text, namespace, imports);
    }
    if matches!(kind, "variable_name" | "assignment_expression") {
        return None;
    }

    inferred_return_expression_type(
        expression, expression, expression, text, namespace, imports, None,
    )
}

fn assigned_variable_return_type(
    expression: Node,
    text: &str,
    types: &HashMap<String, ComparableReturnType>,
) -> Option<ComparableReturnType> {
    let kind = expression.kind();
    if kind == "expression" {
        let mut cursor = expression.walk();
        let inner = expression.named_children(&mut cursor).next()?;
        return assigned_variable_return_type(inner, text, types);
    }
    (kind == "variable_name")
        .then(|| types.get(node_text(expression, text)).cloned())
        .flatten()
}

fn inferred_argument_expression_type(
    root: Node,
    expression: Node,
    text: &str,
    namespace: Option<&str>,
    imports: &ImportMap,
    index: &SymbolIndex,
) -> Option<ComparableReturnType> {
    let kind = expression.kind();
    if kind == "expression" {
        let mut cursor = expression.walk();
        let inner = expression.named_children(&mut cursor).next()?;
        return inferred_argument_expression_type(root, inner, text, namespace, imports, index);
    }
    if kind == "variable_name" {
        let scope =
            find_function_like_declaration_at_byte(root, expression.start_byte()).unwrap_or(root);
        if let Some(return_type) = local_variable_return_type_at_byte(
            scope,
            text,
            expression.start_byte(),
            namespace,
            imports,
            node_text(expression, text),
        ) {
            return Some(return_type);
        }
        let call_assignment_context = CallAssignmentInference {
            root,
            text,
            byte_offset: expression.start_byte(),
            namespace,
            imports,
            index,
        };
        if let Some(return_type) = local_variable_call_return_type_at_byte(
            scope,
            node_text(expression, text),
            &call_assignment_context,
        ) {
            return Some(return_type);
        }
        return None;
    }

    inferred_return_expression_type(
        root,
        expression,
        expression,
        text,
        namespace,
        imports,
        Some(index),
    )
}

fn assignment_type_mismatch_diagnostics(
    root: Node,
    text: &str,
    imports: &ImportMap,
    index: &SymbolIndex,
) -> Vec<Diagnostic> {
    let mut diagnostics = Vec::new();
    let empty_expected_types = HashMap::new();
    let context = AssignmentTypeMismatchContext {
        root,
        text,
        imports,
        index,
        expected_types: &empty_expected_types,
    };
    collect_assignment_type_mismatches(root, root, &context, &mut diagnostics);
    collect_assignment_type_mismatch_diagnostics(
        root,
        root,
        text,
        imports,
        index,
        &mut diagnostics,
    );
    diagnostics
}

fn collect_assignment_type_mismatch_diagnostics(
    root: Node,
    node: Node,
    text: &str,
    imports: &ImportMap,
    index: &SymbolIndex,
    diagnostics: &mut Vec<Diagnostic>,
) {
    if matches!(node.kind(), "function_definition" | "method_declaration") {
        diagnostics.extend(assignment_type_mismatches_for_declaration(
            root, node, text, imports, index,
        ));
    }

    let mut cursor = node.walk();
    for child in node.named_children(&mut cursor) {
        collect_assignment_type_mismatch_diagnostics(
            root,
            child,
            text,
            imports,
            index,
            diagnostics,
        );
    }
}

fn assignment_type_mismatches_for_declaration(
    root: Node,
    declaration: Node,
    text: &str,
    imports: &ImportMap,
    index: &SymbolIndex,
) -> Vec<Diagnostic> {
    let Some(parameters_node) = declaration.child_by_field_name("parameters") else {
        return Vec::new();
    };
    let namespace = namespace_at_byte(root, text, declaration.start_byte());
    let parameters = parameter_names(parameters_node, text);
    let parameter_types = declaration_signature_parameter_types(
        declaration,
        parameters_node,
        text,
        namespace.as_deref(),
        imports,
    );
    let expected_types = parameters
        .into_iter()
        .zip(parameter_types)
        .filter_map(|(parameter, parameter_type)| Some((format!("${parameter}"), parameter_type?)))
        .collect::<HashMap<_, _>>();
    if expected_types.is_empty() {
        return Vec::new();
    }

    let mut diagnostics = Vec::new();
    let context = AssignmentTypeMismatchContext {
        root,
        text,
        imports,
        index,
        expected_types: &expected_types,
    };
    collect_assignment_type_mismatches(declaration, declaration, &context, &mut diagnostics);
    diagnostics
}

struct AssignmentTypeMismatchContext<'a, 'tree> {
    root: Node<'tree>,
    text: &'a str,
    imports: &'a ImportMap,
    index: &'a SymbolIndex,
    expected_types: &'a HashMap<String, ComparableReturnType>,
}

fn collect_assignment_type_mismatches(
    declaration: Node,
    node: Node,
    context: &AssignmentTypeMismatchContext<'_, '_>,
    diagnostics: &mut Vec<Diagnostic>,
) {
    if node != declaration
        && matches!(
            node.kind(),
            "function_definition"
                | "method_declaration"
                | "anonymous_function_creation_expression"
                | "arrow_function"
                | "class_declaration"
                | "interface_declaration"
                | "trait_declaration"
        )
    {
        return;
    }

    if node.kind() == "assignment_expression"
        && let (Some(left), Some(right)) = (
            node.child_by_field_name("left"),
            node.child_by_field_name("right"),
        )
        && left.kind() == "variable_name"
    {
        let assignment_namespace = namespace_at_byte(context.root, context.text, node.start_byte());
        let assignment_namespace = assignment_namespace.as_deref();
        let expected = context
            .expected_types
            .get(node_text(left, context.text))
            .cloned()
            .or_else(|| {
                phpdoc_variable_type_at_byte(
                    context.text,
                    left.start_byte(),
                    assignment_namespace,
                    context.imports,
                    node_text(left, context.text),
                )
            });
        let actual = inferred_assigned_return_type(
            right,
            context.text,
            assignment_namespace,
            context.imports,
        )
        .or_else(|| {
            let call_context = CallAssignmentInference {
                root: context.root,
                text: context.text,
                byte_offset: right.end_byte(),
                namespace: assignment_namespace,
                imports: context.imports,
                index: context.index,
            };
            inferred_call_return_type(right, &call_context)
        })
        .or_else(|| {
            inferred_assigned_variable_type(declaration, right, context, assignment_namespace)
        });
        if let Some(expected) = expected
            && let Some(actual) = actual
            && !types_compatible(&expected, &actual)
        {
            diagnostics.push(Diagnostic {
                range: range_for_bytes(context.text, right.start_byte(), right.end_byte())
                    .unwrap_or_else(|_| Range::default()),
                severity: Some(DiagnosticSeverity::ERROR),
                code: None,
                code_description: None,
                source: Some("rephactor".to_string()),
                message: format!(
                    "assignment type mismatch for {}: expected {}, got {}",
                    node_text(left, context.text),
                    expected.display,
                    actual.display
                ),
                related_information: None,
                tags: None,
                data: None,
            });
        }
    }

    let mut cursor = node.walk();
    for child in node.named_children(&mut cursor) {
        collect_assignment_type_mismatches(declaration, child, context, diagnostics);
    }
}

fn inferred_assigned_variable_type(
    declaration: Node,
    expression: Node,
    context: &AssignmentTypeMismatchContext<'_, '_>,
    namespace: Option<&str>,
) -> Option<ComparableReturnType> {
    let kind = expression.kind();
    if kind == "expression" {
        let mut cursor = expression.walk();
        let inner = expression.named_children(&mut cursor).next()?;
        return inferred_assigned_variable_type(declaration, inner, context, namespace);
    }
    if kind != "variable_name" {
        return None;
    }

    let variable_name = node_text(expression, context.text);
    local_variable_return_type_at_byte(
        declaration,
        context.text,
        expression.start_byte(),
        namespace,
        context.imports,
        variable_name,
    )
    .or_else(|| {
        let call_context = CallAssignmentInference {
            root: context.root,
            text: context.text,
            byte_offset: expression.start_byte(),
            namespace,
            imports: context.imports,
            index: context.index,
        };
        local_variable_call_return_type_at_byte(declaration, variable_name, &call_context)
    })
}

fn phpdoc_variable_type_at_byte(
    text: &str,
    byte_offset: usize,
    namespace: Option<&str>,
    imports: &ImportMap,
    variable_name: &str,
) -> Option<ComparableReturnType> {
    let mut types = HashMap::new();
    collect_phpdoc_var_types(text, byte_offset, namespace, imports, &mut types);
    types
        .get(variable_name)
        .and_then(|type_name| comparable_parameter_type(type_name, namespace, imports))
}

fn collect_return_expressions<'tree>(
    declaration: Node<'tree>,
    node: Node<'tree>,
    expressions: &mut Vec<Node<'tree>>,
) {
    if node != declaration
        && matches!(
            node.kind(),
            "function_definition"
                | "method_declaration"
                | "anonymous_function_creation_expression"
                | "arrow_function"
                | "class_declaration"
                | "interface_declaration"
                | "trait_declaration"
        )
    {
        return;
    }

    if node.kind() == "return_statement" {
        let mut cursor = node.walk();
        if let Some(expression) = node.named_children(&mut cursor).next() {
            expressions.push(expression);
        }
        return;
    }

    let mut cursor = node.walk();
    for child in node.named_children(&mut cursor) {
        collect_return_expressions(declaration, child, expressions);
    }
}

fn single_named_type(type_node: Node, text: &str) -> Option<String> {
    let mut type_names = Vec::new();
    collect_named_type_texts(type_node, text, &mut type_names);
    (type_names.len() == 1).then(|| type_names.remove(0))
}

fn collect_named_type_texts(node: Node, text: &str, type_names: &mut Vec<String>) {
    if matches!(
        node.kind(),
        "primitive_type" | "named_type" | "name" | "qualified_name" | "relative_name"
    ) {
        type_names.push(clean_name_text(node_text(node, text)));
        return;
    }

    let mut cursor = node.walk();
    for child in node.named_children(&mut cursor) {
        collect_named_type_texts(child, text, type_names);
    }
}

fn normalize_return_type_name(type_name: &str) -> String {
    normalize_symbol_key(type_name)
}

fn scalar_return_type_key(type_name: &str) -> Option<&'static str> {
    match type_name {
        "array" => Some("scalar:array"),
        "bool" | "boolean" | "false" | "true" => Some("scalar:bool"),
        "float" | "double" => Some("scalar:float"),
        "int" | "integer" => Some("scalar:int"),
        "null" => Some("scalar:null"),
        "string" => Some("scalar:string"),
        "void" => Some("scalar:void"),
        _ => None,
    }
}

fn unused_import_diagnostics(
    root: Node,
    text: &str,
    imports: &[ImportDeclaration],
) -> Vec<Diagnostic> {
    imports
        .iter()
        .filter(|import| !import.is_grouped && !import.has_alias)
        .filter(|import| {
            !class_name_is_used(
                root,
                text,
                &import.alias,
                import.start_byte,
                import.end_byte,
            )
        })
        .filter_map(|import| {
            Some(Diagnostic {
                range: range_for_bytes(text, import.start_byte, import.end_byte).ok()?,
                severity: Some(DiagnosticSeverity::WARNING),
                code: None,
                code_description: None,
                source: Some("rephactor".to_string()),
                message: format!("unused import {}", import.alias),
                related_information: None,
                tags: Some(vec![DiagnosticTag::UNNECESSARY]),
                data: None,
            })
        })
        .collect()
}

fn selection_ranges_for_text(
    text: &str,
    positions: &[Position],
) -> Result<Vec<SelectionRange>, SkipReason> {
    let Some(tree) = parse_php(text) else {
        return Err(SkipReason::ParseError);
    };
    let root = tree.root_node();
    positions
        .iter()
        .map(|position| {
            let Some(byte_offset) = byte_offset_for_lsp_position(text, *position) else {
                return Err(SkipReason::InvalidCursorPosition);
            };
            selection_range_at_byte(root, text, byte_offset).ok_or(SkipReason::NoSupportedCall)
        })
        .collect()
}

fn selection_range_at_byte(root: Node, text: &str, byte_offset: usize) -> Option<SelectionRange> {
    let mut current = find_smallest_named_node_at_byte(root, byte_offset)?;
    let mut ranges = Vec::new();
    let mut last_range = None;

    loop {
        if let Ok(range) = range_for_bytes(text, current.start_byte(), current.end_byte())
            && Some(range) != last_range
        {
            ranges.push(range);
            last_range = Some(range);
        }

        let Some(parent) = current.parent() else {
            break;
        };
        current = parent;
    }

    let mut parent = None;
    for range in ranges.into_iter().rev() {
        parent = Some(Box::new(SelectionRange { range, parent }));
    }
    parent.map(|selection_range| *selection_range)
}

fn find_smallest_named_node_at_byte<'tree>(
    node: Node<'tree>,
    byte_offset: usize,
) -> Option<Node<'tree>> {
    if byte_offset < node.start_byte() || byte_offset > node.end_byte() {
        return None;
    }

    let mut cursor = node.walk();
    for child in node.named_children(&mut cursor) {
        if let Some(found) = find_smallest_named_node_at_byte(child, byte_offset) {
            return Some(found);
        }
    }

    Some(node)
}

fn collect_type_reference_nodes<'tree>(node: Node<'tree>, type_nodes: &mut Vec<Node<'tree>>) {
    if let Some(type_node) = node.child_by_field_name("type") {
        collect_named_type_nodes(type_node, type_nodes);
    }
    if let Some(type_node) = node.child_by_field_name("return_type") {
        collect_named_type_nodes(type_node, type_nodes);
    }

    let mut cursor = node.walk();
    for child in node.named_children(&mut cursor) {
        collect_type_reference_nodes(child, type_nodes);
    }
}

fn collect_named_type_nodes<'tree>(node: Node<'tree>, type_nodes: &mut Vec<Node<'tree>>) {
    if matches!(
        node.kind(),
        "named_type" | "name" | "qualified_name" | "relative_name"
    ) {
        type_nodes.push(node);
        return;
    }

    let mut cursor = node.walk();
    for child in node.named_children(&mut cursor) {
        collect_named_type_nodes(child, type_nodes);
    }
}

fn is_builtin_type_name(name: &str) -> bool {
    matches!(
        normalize_symbol_key(name).as_str(),
        "array"
            | "bool"
            | "callable"
            | "false"
            | "float"
            | "int"
            | "iterable"
            | "mixed"
            | "never"
            | "null"
            | "object"
            | "parent"
            | "self"
            | "static"
            | "string"
            | "true"
            | "void"
    )
}

fn references_for_position(
    uri: &Url,
    text: &str,
    position: Position,
    include_declaration: bool,
    open_documents: &HashMap<Url, String>,
) -> Result<Vec<Location>, SkipReason> {
    let Some(byte_offset) = byte_offset_for_lsp_position(text, position) else {
        return Err(SkipReason::InvalidCursorPosition);
    };
    let Some(tree) = parse_php(text) else {
        return Err(SkipReason::ParseError);
    };
    let Some(name_node) = find_reference_name_at_byte(tree.root_node(), text, byte_offset) else {
        return Err(SkipReason::NoSupportedCall);
    };
    let search_name = clean_reference_name(node_text(name_node, text));
    if search_name.is_empty() {
        return Err(SkipReason::NoSupportedCall);
    }

    let documents = reference_documents(uri, text, open_documents);
    let mut locations = Vec::new();

    for (path, document_text) in documents {
        let Some(tree) = parse_php(&document_text) else {
            continue;
        };
        let mut names = Vec::new();
        collect_name_nodes(tree.root_node(), &mut names);

        for name in names {
            if !include_declaration && is_declaration_name(name) {
                continue;
            }
            if clean_reference_name(node_text(name, &document_text))
                .eq_ignore_ascii_case(&search_name)
                && let Some(location) = location_for_path_range(
                    &path,
                    &document_text,
                    name.start_byte(),
                    name.end_byte(),
                )
            {
                locations.push(location);
            }
        }
    }

    locations.sort_by_key(|location| {
        (
            location.uri.to_string(),
            location.range.start.line,
            location.range.start.character,
        )
    });
    Ok(locations)
}

fn rename_for_position(
    uri: &Url,
    text: &str,
    position: Position,
    new_name: &str,
    open_documents: &HashMap<Url, String>,
) -> Result<WorkspaceEdit, SkipReason> {
    if !is_valid_rename_identifier(new_name) {
        return Err(SkipReason::NoSupportedCall);
    }

    let locations = references_for_position(uri, text, position, true, open_documents)?;
    if locations.is_empty() {
        return Err(SkipReason::NoEdits);
    }

    let mut changes: HashMap<Url, Vec<TextEdit>> = HashMap::new();
    for location in locations {
        changes.entry(location.uri).or_default().push(TextEdit {
            range: location.range,
            new_text: new_name.to_string(),
        });
    }

    for edits in changes.values_mut() {
        edits.sort_by_key(|edit| (edit.range.start.line, edit.range.start.character));
    }

    Ok(WorkspaceEdit {
        changes: Some(changes),
        document_changes: None,
        change_annotations: None,
    })
}

fn is_valid_rename_identifier(name: &str) -> bool {
    let mut characters = name.chars();
    let Some(first) = characters.next() else {
        return false;
    };
    (first.is_ascii_alphabetic() || first == '_')
        && characters.all(|character| character.is_ascii_alphanumeric() || character == '_')
}

fn code_lenses_for_document(
    uri: &Url,
    text: &str,
    open_documents: &HashMap<Url, String>,
) -> Result<Vec<CodeLens>, SkipReason> {
    let Some(tree) = parse_php(text) else {
        return Err(SkipReason::ParseError);
    };

    let mut declaration_names = Vec::new();
    collect_declaration_name_nodes(tree.root_node(), &mut declaration_names);
    let mut lenses = Vec::new();

    for name in declaration_names {
        let Some(position) = lsp_position_for_byte_offset(text, name.start_byte()) else {
            continue;
        };
        let Ok(locations) = references_for_position(uri, text, position, true, open_documents)
        else {
            continue;
        };
        let reference_count = locations
            .iter()
            .filter(|location| {
                !(location.uri == *uri && location.range == range_for_node(text, name))
            })
            .count();
        let range = range_for_bytes(text, name.start_byte(), name.end_byte())?;

        lenses.push(CodeLens {
            range,
            command: Some(Command::new(
                format!(
                    "{} reference{}",
                    reference_count,
                    if reference_count == 1 { "" } else { "s" }
                ),
                "editor.action.showReferences".to_string(),
                Some(vec![
                    serde_json::to_value(uri).unwrap_or(serde_json::Value::Null),
                    serde_json::to_value(position).unwrap_or(serde_json::Value::Null),
                    serde_json::to_value(locations).unwrap_or(serde_json::Value::Null),
                ]),
            )),
            data: None,
        });
    }

    Ok(lenses)
}

fn range_for_node(text: &str, node: Node) -> Range {
    range_for_bytes(text, node.start_byte(), node.end_byte()).unwrap_or_else(|_| Range::default())
}

fn collect_declaration_name_nodes<'tree>(node: Node<'tree>, names: &mut Vec<Node<'tree>>) {
    if is_declaration_name(node) {
        names.push(node);
        return;
    }

    let mut cursor = node.walk();
    for child in node.named_children(&mut cursor) {
        collect_declaration_name_nodes(child, names);
    }
}

fn document_highlights_for_position(
    text: &str,
    position: Position,
) -> Result<Vec<DocumentHighlight>, SkipReason> {
    let Some(byte_offset) = byte_offset_for_lsp_position(text, position) else {
        return Err(SkipReason::InvalidCursorPosition);
    };
    let Some(tree) = parse_php(text) else {
        return Err(SkipReason::ParseError);
    };
    let root = tree.root_node();
    let Some(name_node) = find_reference_name_at_byte(root, text, byte_offset) else {
        return Err(SkipReason::NoSupportedCall);
    };
    let search_name = clean_reference_name(node_text(name_node, text));
    if search_name.is_empty() {
        return Err(SkipReason::NoSupportedCall);
    }

    let mut names = Vec::new();
    collect_name_nodes(root, &mut names);
    let mut highlights = Vec::new();

    for name in names {
        if clean_reference_name(node_text(name, text)).eq_ignore_ascii_case(&search_name) {
            highlights.push(DocumentHighlight {
                range: range_for_bytes(text, name.start_byte(), name.end_byte())?,
                kind: Some(DocumentHighlightKind::TEXT),
            });
        }
    }

    Ok(highlights)
}

fn parse_php(text: &str) -> Option<tree_sitter::Tree> {
    let tree = parse_php_allowing_errors(text)?;
    (!tree.root_node().has_error()).then_some(tree)
}

fn parse_php_allowing_errors(text: &str) -> Option<tree_sitter::Tree> {
    let mut parser = Parser::new();
    parser
        .set_language(&tree_sitter_php::LANGUAGE_PHP.into())
        .ok()?;
    parser.parse(text, None)
}

impl ImportMap {
    fn from_root(root: Node, text: &str) -> Self {
        let mut imports = Self::default();
        collect_imports(root, text, &mut imports);
        imports
    }

    fn insert_class(&mut self, alias: String, fqn: String) {
        self.classes
            .insert(normalize_symbol_key(&alias), clean_name_text(&fqn));
    }

    fn resolve_class_name(&self, name: &str, namespace: Option<&str>) -> Vec<String> {
        let name = clean_name_text(name);
        if name.starts_with('\\') {
            return vec![name.trim_start_matches('\\').to_string()];
        }

        let mut segments = name.split('\\');
        let first_segment = segments.next().unwrap_or_default();
        let rest = segments.collect::<Vec<_>>();

        if let Some(imported) = self.classes.get(&normalize_symbol_key(first_segment)) {
            let mut resolved = imported.clone();
            if !rest.is_empty() {
                resolved.push('\\');
                resolved.push_str(&rest.join("\\"));
            }
            return vec![resolved];
        }

        name_candidates(&name, namespace)
    }
}

impl SymbolIndex {
    fn for_project(project_root: &Path) -> Self {
        let mut index = Self::default();
        index.index_project(project_root, &HashMap::new());
        index
    }

    fn index_project(&mut self, project_root: &Path, open_documents: &HashMap<PathBuf, String>) {
        let Some(paths) = composer_autoload_paths(project_root) else {
            return;
        };

        for path in paths {
            self.index_php_path(&path, open_documents);
        }
    }

    fn index_php_path(&mut self, path: &Path, open_documents: &HashMap<PathBuf, String>) {
        if path.is_dir() {
            self.index_php_files(path, open_documents);
            return;
        }

        if path.extension().and_then(|extension| extension.to_str()) != Some("php") {
            return;
        }

        if let Some(open_text) = open_documents.get(path) {
            self.index_text_at_path(open_text, Some(path));
            return;
        }

        let Ok(text) = fs::read_to_string(path) else {
            return;
        };
        self.index_text_at_path(&text, Some(path));
    }

    fn index_php_files(&mut self, root: &Path, open_documents: &HashMap<PathBuf, String>) {
        let Ok(entries) = fs::read_dir(root) else {
            return;
        };

        for entry in entries.flatten() {
            let path = entry.path();

            if path.is_dir() {
                self.index_php_path(&path, open_documents);
                continue;
            }

            self.index_php_path(&path, open_documents);
        }
    }

    #[cfg(test)]
    fn index_text(&mut self, text: &str) {
        self.index_text_at_path(text, None);
    }

    fn index_text_at_path(&mut self, text: &str, path: Option<&Path>) {
        let Some(tree) = parse_php(text) else {
            return;
        };
        let root = tree.root_node();
        let imports = ImportMap::from_root(root, text);

        index_children(self, root, text, path, None, &imports);
    }

    fn add_function(&mut self, fqn: String, signature: Signature) {
        self.functions
            .insert(normalize_symbol_key(&fqn), vec![signature]);
    }

    fn add_class(&mut self, fqn: String, class_info: ClassInfo) {
        self.classes.insert(normalize_symbol_key(&fqn), class_info);
    }

    fn resolve(
        &self,
        target: &CallTarget,
        root: Node,
        text: &str,
        byte_offset: usize,
        namespace: Option<&str>,
        imports: &ImportMap,
    ) -> Result<Signature, SkipReason> {
        match target {
            CallTarget::Function(name) => self.resolve_function(name, namespace),
            CallTarget::StaticMethod { class_name, method } => self
                .resolve_class(class_name, namespace, imports)
                .ok_or_else(|| SkipReason::UnresolvedCallable(target.describe()))
                .and_then(|class_info| self.resolve_method(class_info, method, target)),
            CallTarget::Constructor { class_name } => self
                .resolve_class(class_name, namespace, imports)
                .ok_or_else(|| SkipReason::UnresolvedCallable(target.describe()))
                .and_then(|class_info| {
                    class_info
                        .constructor
                        .clone()
                        .ok_or_else(|| SkipReason::UnresolvedCallable(target.describe()))
                }),
            CallTarget::InstanceMethod { variable, method } => {
                let variable_types =
                    variable_types_at_byte(root, text, byte_offset, namespace, imports);
                let class_name = variable_types
                    .get(variable)
                    .ok_or_else(|| SkipReason::UnresolvedCallable(target.describe()))?;
                self.resolve_class(class_name, namespace, imports)
                    .ok_or_else(|| SkipReason::UnresolvedCallable(target.describe()))
                    .and_then(|class_info| self.resolve_method(class_info, method, target))
            }
        }
    }

    fn resolve_function(
        &self,
        name: &str,
        namespace: Option<&str>,
    ) -> Result<Signature, SkipReason> {
        for candidate in name_candidates(name, namespace) {
            if let Some(signatures) = self.functions.get(&normalize_symbol_key(&candidate)) {
                return if signatures.len() == 1 {
                    Ok(signatures.first().expect("signature exists").clone())
                } else {
                    Err(SkipReason::AmbiguousCallable(name.to_string()))
                };
            }
        }

        internal_function_signature(name)
            .ok_or_else(|| SkipReason::UnresolvedCallable(name.to_string()))
    }

    fn resolve_class(
        &self,
        class_name: &str,
        namespace: Option<&str>,
        imports: &ImportMap,
    ) -> Option<&ClassInfo> {
        for candidate in imports.resolve_class_name(class_name, namespace) {
            if let Some(class_info) = self.classes.get(&normalize_symbol_key(&candidate)) {
                return Some(class_info);
            }
        }

        None
    }

    fn resolve_method(
        &self,
        class_info: &ClassInfo,
        method: &str,
        target: &CallTarget,
    ) -> Result<Signature, SkipReason> {
        let method_key = normalize_method_key(method);
        if let Some(signature) = class_info.methods.get(&method_key) {
            return Ok(signature.clone());
        }

        let mut signatures = Vec::new();
        let mut visited = Vec::new();

        for related_name in class_info
            .parents
            .iter()
            .chain(class_info.interfaces.iter())
            .chain(class_info.traits.iter())
            .chain(class_info.mixins.iter())
        {
            self.collect_related_method_signatures(
                related_name,
                &method_key,
                &mut visited,
                &mut signatures,
            );
        }

        match signatures.len() {
            0 => Err(SkipReason::UnresolvedCallable(target.describe())),
            1 => Ok(signatures.remove(0)),
            _ => Err(SkipReason::AmbiguousCallable(target.describe())),
        }
    }

    fn collect_related_method_signatures(
        &self,
        class_name: &str,
        method_key: &str,
        visited: &mut Vec<String>,
        signatures: &mut Vec<Signature>,
    ) {
        let class_key = normalize_symbol_key(class_name);
        if visited.contains(&class_key) {
            return;
        }
        visited.push(class_key.clone());

        let Some(class_info) = self.classes.get(&class_key) else {
            return;
        };

        if let Some(signature) = class_info.methods.get(method_key)
            && !signatures.contains(signature)
        {
            signatures.push(signature.clone());
        }

        for related_name in class_info
            .parents
            .iter()
            .chain(class_info.interfaces.iter())
            .chain(class_info.traits.iter())
            .chain(class_info.mixins.iter())
        {
            self.collect_related_method_signatures(related_name, method_key, visited, signatures);
        }
    }

    fn collect_related_method_names(
        &self,
        class_name: &str,
        visited: &mut Vec<String>,
        names: &mut Vec<String>,
    ) {
        let class_key = normalize_symbol_key(class_name);
        if visited.contains(&class_key) {
            return;
        }
        visited.push(class_key.clone());

        let Some(class_info) = self.classes.get(&class_key) else {
            return;
        };

        names.extend(
            class_info
                .methods
                .values()
                .map(|signature| signature.name.clone()),
        );

        for related_name in class_info
            .parents
            .iter()
            .chain(class_info.interfaces.iter())
            .chain(class_info.traits.iter())
            .chain(class_info.mixins.iter())
        {
            self.collect_related_method_names(related_name, visited, names);
        }
    }
}

impl ProjectIndexCache {
    pub fn invalidate_document(&mut self, uri: &Url) -> bool {
        let Some(project_root) = project_root_for_uri(uri) else {
            return false;
        };

        self.indexes.remove(&project_root).is_some()
    }

    fn status_for_project_root(&self, project_root: &Path) -> IndexCacheStatus {
        if self.indexes.contains_key(project_root) {
            IndexCacheStatus::Hit(project_root.to_path_buf())
        } else {
            IndexCacheStatus::Miss(project_root.to_path_buf())
        }
    }

    fn status_for_document(&self, uri: &Url) -> IndexCacheStatus {
        let Some(project_root) = project_root_for_uri(uri) else {
            return IndexCacheStatus::NoProject;
        };

        self.status_for_project_root(&project_root)
    }

    fn index_for_project_root(
        &mut self,
        project_root: &Path,
        open_documents: &HashMap<Url, String>,
    ) -> SymbolIndex {
        let disk_index = self
            .indexes
            .entry(project_root.to_path_buf())
            .or_insert_with(|| SymbolIndex::for_project(project_root));
        let mut index = disk_index.clone();

        for (path, open_text) in open_project_documents(open_documents) {
            index.index_text_at_path(&open_text, Some(&path));
        }

        index
    }

    fn index_for_document(
        &mut self,
        uri: &Url,
        text: &str,
        open_documents: &HashMap<Url, String>,
    ) -> SymbolIndex {
        let Some(project_root) = project_root_for_uri(uri) else {
            let mut index = SymbolIndex::default();
            let path = uri.to_file_path().ok();
            index.index_text_at_path(text, path.as_deref());
            return index;
        };

        let mut index = self.index_for_project_root(&project_root, open_documents);

        let path = uri.to_file_path().ok();
        index.index_text_at_path(text, path.as_deref());
        index
    }
}

impl fmt::Display for IndexCacheStatus {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Hit(path) => write!(formatter, "index cache hit {}", path.display()),
            Self::Miss(path) => write!(formatter, "index cache miss {}", path.display()),
            Self::NoProject => write!(formatter, "no project index"),
        }
    }
}

impl CallTarget {
    fn describe(&self) -> String {
        match self {
            Self::Function(name) => name.clone(),
            Self::StaticMethod { class_name, method } => format!("{class_name}::{method}"),
            Self::Constructor { class_name } => format!("new {class_name}"),
            Self::InstanceMethod { variable, method } => format!("{variable}->{method}"),
        }
    }
}

fn open_project_documents(open_documents: &HashMap<Url, String>) -> HashMap<PathBuf, String> {
    open_documents
        .iter()
        .filter_map(|(uri, text)| {
            let path = uri.to_file_path().ok()?;
            (path.extension().and_then(|extension| extension.to_str()) == Some("php"))
                .then(|| (path, text.clone()))
        })
        .collect()
}

fn collect_imports(node: Node, text: &str, imports: &mut ImportMap) {
    let mut cursor = node.walk();

    for child in node.named_children(&mut cursor) {
        if child.kind() == "namespace_use_declaration" {
            index_use_declaration(child, text, imports);
            continue;
        }

        if child.kind() == "class_declaration" || child.kind() == "function_definition" {
            continue;
        }

        collect_imports(child, text, imports);
    }
}

fn import_declarations(root: Node, text: &str) -> Vec<ImportDeclaration> {
    let mut declarations = Vec::new();
    collect_import_declarations(root, text, &mut declarations);
    declarations
}

fn collect_import_declarations(node: Node, text: &str, declarations: &mut Vec<ImportDeclaration>) {
    let mut cursor = node.walk();

    for child in node.named_children(&mut cursor) {
        if child.kind() == "namespace_use_declaration" {
            declarations.extend(import_declarations_from_node(child, text));
            continue;
        }

        if child.kind() == "class_declaration" || child.kind() == "function_definition" {
            continue;
        }

        collect_import_declarations(child, text, declarations);
    }
}

fn import_declarations_from_node(node: Node, text: &str) -> Vec<ImportDeclaration> {
    let declaration_text = node_text(node, text).trim_start();
    if starts_with_use_kind(declaration_text, "function")
        || starts_with_use_kind(declaration_text, "const")
    {
        return Vec::new();
    }

    if let Some(group) = direct_child_kind(node, "namespace_use_group") {
        let Some(prefix) = direct_child_kind(node, "namespace_name") else {
            return Vec::new();
        };
        let prefix = clean_name_text(node_text(prefix, text));
        let mut cursor = group.walk();

        return group
            .named_children(&mut cursor)
            .filter(|child| child.kind() == "namespace_use_clause")
            .filter_map(|clause| {
                let (alias, target) = use_clause_names(clause, text)?;
                Some(ImportDeclaration {
                    fqn: qualify_name(&target, Some(&prefix)),
                    alias,
                    start_byte: node.start_byte(),
                    end_byte: node.end_byte(),
                    is_grouped: true,
                    has_alias: use_clause_has_alias(clause, text),
                })
            })
            .collect();
    }

    let mut cursor = node.walk();
    node.named_children(&mut cursor)
        .filter(|child| child.kind() == "namespace_use_clause")
        .filter_map(|clause| {
            let (alias, target) = use_clause_names(clause, text)?;
            Some(ImportDeclaration {
                fqn: target,
                alias,
                start_byte: node.start_byte(),
                end_byte: node.end_byte(),
                is_grouped: false,
                has_alias: use_clause_has_alias(clause, text),
            })
        })
        .collect()
}

fn index_use_declaration(node: Node, text: &str, imports: &mut ImportMap) {
    let declaration_text = node_text(node, text).trim_start();
    if starts_with_use_kind(declaration_text, "function")
        || starts_with_use_kind(declaration_text, "const")
    {
        return;
    }

    if let Some(group) = direct_child_kind(node, "namespace_use_group") {
        let Some(prefix) = direct_child_kind(node, "namespace_name") else {
            return;
        };
        let prefix = clean_name_text(node_text(prefix, text));
        let mut cursor = group.walk();

        for clause in group
            .named_children(&mut cursor)
            .filter(|child| child.kind() == "namespace_use_clause")
        {
            if let Some((alias, target)) = use_clause_names(clause, text) {
                imports.insert_class(alias, qualify_name(&target, Some(&prefix)));
            }
        }

        return;
    }

    let mut cursor = node.walk();
    for clause in node
        .named_children(&mut cursor)
        .filter(|child| child.kind() == "namespace_use_clause")
    {
        if let Some((alias, target)) = use_clause_names(clause, text) {
            imports.insert_class(alias, target);
        }
    }
}

fn starts_with_use_kind(text: &str, kind: &str) -> bool {
    let Some(rest) = text.strip_prefix("use") else {
        return false;
    };

    rest.trim_start()
        .get(..kind.len())
        .is_some_and(|prefix| prefix.eq_ignore_ascii_case(kind))
}

fn direct_child_kind<'tree>(node: Node<'tree>, kind: &str) -> Option<Node<'tree>> {
    let mut cursor = node.walk();
    node.named_children(&mut cursor)
        .find(|child| child.kind() == kind)
}

fn use_clause_names(clause: Node, text: &str) -> Option<(String, String)> {
    let children = direct_name_children(clause);
    let target_node = children.first().copied()?;
    let target = clean_name_text(node_text(target_node, text));
    if target.is_empty() {
        return None;
    }

    let alias = if use_clause_has_alias(clause, text) {
        children
            .last()
            .copied()
            .filter(|node| node.kind() == "name")
            .map(|node| clean_name_text(node_text(node, text)))?
    } else {
        last_name_segment(&target).to_string()
    };

    Some((alias, target))
}

fn direct_name_children(node: Node) -> Vec<Node> {
    let mut cursor = node.walk();
    node.named_children(&mut cursor)
        .filter(|child| is_name_node(*child))
        .collect()
}

fn use_clause_has_alias(clause: Node, text: &str) -> bool {
    node_text(clause, text)
        .split_whitespace()
        .any(|part| part.eq_ignore_ascii_case("as"))
}

fn index_children(
    index: &mut SymbolIndex,
    node: Node,
    text: &str,
    path: Option<&Path>,
    namespace: Option<String>,
    imports: &ImportMap,
) {
    let mut cursor = node.walk();
    let mut active_namespace = namespace;

    for child in node.named_children(&mut cursor) {
        match child.kind() {
            "namespace_definition" => {
                let namespace_name = child
                    .child_by_field_name("name")
                    .map(|name| clean_name_text(node_text(name, text)))
                    .filter(|name| !name.is_empty());

                if let Some(body) = child.child_by_field_name("body") {
                    index_children(index, body, text, path, namespace_name, imports);
                } else {
                    active_namespace = namespace_name;
                }
            }
            "function_definition" => {
                index_function(
                    index,
                    child,
                    text,
                    path,
                    active_namespace.as_deref(),
                    imports,
                );
            }
            "class_declaration" | "interface_declaration" | "trait_declaration" => {
                index_class(
                    index,
                    child,
                    text,
                    path,
                    active_namespace.as_deref(),
                    imports,
                );
            }
            _ => index_children(index, child, text, path, active_namespace.clone(), imports),
        }
    }
}

fn index_function(
    index: &mut SymbolIndex,
    node: Node,
    text: &str,
    path: Option<&Path>,
    namespace: Option<&str>,
    imports: &ImportMap,
) {
    let Some(name_node) = node.child_by_field_name("name") else {
        return;
    };
    let Some(parameters_node) = node.child_by_field_name("parameters") else {
        return;
    };

    let name = qualify_name(node_text(name_node, text), namespace);
    let signature = Signature {
        name: name.clone(),
        parameters: parameter_names(parameters_node, text),
        parameter_types: declaration_signature_parameter_types(
            node,
            parameters_node,
            text,
            namespace,
            imports,
        ),
        return_type: declaration_signature_return_type(node, text, namespace, imports),
        is_variadic: parameters_node_has_variadic(parameters_node),
        is_abstract: false,
        location: source_location(path, name_node.start_byte()),
        doc_summary: phpdoc_summary_before(text, node.start_byte()),
    };
    index.add_function(name, signature);
}

fn index_class(
    index: &mut SymbolIndex,
    node: Node,
    text: &str,
    path: Option<&Path>,
    namespace: Option<&str>,
    imports: &ImportMap,
) {
    let Some(name_node) = node.child_by_field_name("name") else {
        return;
    };
    let Some(body) = node.child_by_field_name("body") else {
        return;
    };

    let fqn = qualify_name(node_text(name_node, text), namespace);
    let mut class_info = ClassInfo {
        fqn: fqn.clone(),
        location: source_location(path, name_node.start_byte()),
        doc_summary: phpdoc_summary_before(text, node.start_byte()),
        parents: class_like_names_from_direct_child(node, "base_clause", text, namespace),
        interfaces: class_like_names_from_direct_child(
            node,
            "class_interface_clause",
            text,
            namespace,
        ),
        mixins: phpdoc_mixins_before(text, node.start_byte(), namespace),
        ..ClassInfo::default()
    };
    for signature in phpdoc_methods_before(text, node.start_byte()) {
        class_info
            .methods
            .insert(normalize_method_key(&signature.name), signature);
    }
    let mut cursor = body.walk();

    for child in body.named_children(&mut cursor) {
        if child.kind() == "use_declaration" {
            class_info
                .traits
                .extend(class_like_names(child, text, namespace));
            continue;
        }

        if child.kind() != "method_declaration" {
            continue;
        }

        index_method(&mut class_info, child, text, path, namespace, imports);
    }

    index.add_class(fqn, class_info);
}

fn index_method(
    class_info: &mut ClassInfo,
    node: Node,
    text: &str,
    path: Option<&Path>,
    namespace: Option<&str>,
    imports: &ImportMap,
) {
    let Some(name_node) = node.child_by_field_name("name") else {
        return;
    };
    let Some(parameters_node) = node.child_by_field_name("parameters") else {
        return;
    };

    let method_name = node_text(name_node, text).to_string();
    let signature = Signature {
        name: method_name.clone(),
        parameters: parameter_names(parameters_node, text),
        parameter_types: declaration_signature_parameter_types(
            node,
            parameters_node,
            text,
            namespace,
            imports,
        ),
        return_type: declaration_signature_return_type(node, text, namespace, imports),
        is_variadic: parameters_node_has_variadic(parameters_node),
        is_abstract: method_is_abstract(node, text),
        location: source_location(path, name_node.start_byte()),
        doc_summary: phpdoc_summary_before(text, node.start_byte()),
    };

    if method_name.eq_ignore_ascii_case("__construct") {
        class_info.constructor = Some(signature);
    } else {
        class_info
            .methods
            .insert(normalize_method_key(&method_name), signature);
    }
}

fn method_is_abstract(node: Node, text: &str) -> bool {
    node.child_by_field_name("body").is_none() || node_text(node, text).contains("abstract")
}

fn collect_document_symbols(node: Node, text: &str) -> Result<Vec<DocumentSymbol>, SkipReason> {
    let mut symbols = Vec::new();
    let mut cursor = node.walk();

    for child in node.named_children(&mut cursor) {
        match child.kind() {
            "namespace_definition" => {
                if let Some(body) = child.child_by_field_name("body") {
                    symbols.extend(collect_document_symbols(body, text)?);
                } else {
                    symbols.extend(collect_document_symbols(child, text)?);
                }
            }
            "function_definition" => {
                if let Some(symbol) = document_symbol(child, text, SymbolKind::FUNCTION, None)? {
                    symbols.push(symbol);
                }
            }
            "class_declaration" => {
                if let Some(symbol) = document_symbol(
                    child,
                    text,
                    SymbolKind::CLASS,
                    class_member_symbols(child, text)?,
                )? {
                    symbols.push(symbol);
                }
            }
            "interface_declaration" => {
                if let Some(symbol) = document_symbol(
                    child,
                    text,
                    SymbolKind::INTERFACE,
                    class_member_symbols(child, text)?,
                )? {
                    symbols.push(symbol);
                }
            }
            "trait_declaration" => {
                if let Some(symbol) = document_symbol(
                    child,
                    text,
                    SymbolKind::CLASS,
                    class_member_symbols(child, text)?,
                )? {
                    symbols.push(symbol);
                }
            }
            _ => symbols.extend(collect_document_symbols(child, text)?),
        }
    }

    Ok(symbols)
}

fn class_member_symbols(
    class_node: Node,
    text: &str,
) -> Result<Option<Vec<DocumentSymbol>>, SkipReason> {
    let Some(body) = class_node.child_by_field_name("body") else {
        return Ok(None);
    };
    let mut members = Vec::new();
    let mut cursor = body.walk();

    for child in body.named_children(&mut cursor) {
        if child.kind() == "method_declaration"
            && let Some(symbol) = document_symbol(child, text, SymbolKind::METHOD, None)?
        {
            members.push(symbol);
        }
    }

    Ok((!members.is_empty()).then_some(members))
}

#[allow(deprecated)]
fn document_symbol(
    node: Node,
    text: &str,
    kind: SymbolKind,
    children: Option<Vec<DocumentSymbol>>,
) -> Result<Option<DocumentSymbol>, SkipReason> {
    let Some(name_node) = node.child_by_field_name("name") else {
        return Ok(None);
    };

    Ok(Some(DocumentSymbol {
        name: node_text(name_node, text).to_string(),
        detail: None,
        kind,
        tags: None,
        deprecated: None,
        range: range_for_bytes(text, node.start_byte(), node.end_byte())?,
        selection_range: range_for_bytes(text, name_node.start_byte(), name_node.end_byte())?,
        children,
    }))
}

fn workspace_symbols_for_index(
    index: &SymbolIndex,
    query: &str,
    open_documents: &HashMap<PathBuf, String>,
) -> Vec<SymbolInformation> {
    let mut symbols = Vec::new();

    for signatures in index.functions.values() {
        for signature in signatures {
            if workspace_query_matches(&signature.name, query)
                && let Some(symbol) = symbol_information(
                    signature.name.clone(),
                    SymbolKind::FUNCTION,
                    signature.location.as_ref(),
                    None,
                    open_documents,
                )
            {
                symbols.push(symbol);
            }
        }
    }

    for class_info in index.classes.values() {
        if workspace_query_matches(&class_info.fqn, query)
            && let Some(symbol) = symbol_information(
                class_info.fqn.clone(),
                SymbolKind::CLASS,
                class_info.location.as_ref(),
                None,
                open_documents,
            )
        {
            symbols.push(symbol);
        }

        for method in class_info.methods.values() {
            let method_label = format!("{}::{}", class_info.fqn, method.name);
            if workspace_query_matches(&method_label, query)
                && let Some(symbol) = symbol_information(
                    method_label,
                    SymbolKind::METHOD,
                    method.location.as_ref(),
                    Some(class_info.fqn.clone()),
                    open_documents,
                )
            {
                symbols.push(symbol);
            }
        }
    }

    symbols.sort_by_key(|symbol| symbol.name.to_ascii_lowercase());
    symbols
}

#[allow(deprecated)]
fn symbol_information(
    name: String,
    kind: SymbolKind,
    location: Option<&SourceLocation>,
    container_name: Option<String>,
    open_documents: &HashMap<PathBuf, String>,
) -> Option<SymbolInformation> {
    Some(SymbolInformation {
        name,
        kind,
        tags: None,
        deprecated: None,
        location: location_for_source(location?, open_documents)?,
        container_name,
    })
}

fn location_for_source(
    location: &SourceLocation,
    open_documents: &HashMap<PathBuf, String>,
) -> Option<Location> {
    let uri = Url::from_file_path(&location.path).ok()?;
    let text = open_documents
        .get(&location.path)
        .cloned()
        .or_else(|| fs::read_to_string(&location.path).ok())?;
    let position = lsp_position_for_byte_offset(&text, location.byte_offset)?;

    Some(Location::new(
        uri,
        Range {
            start: position,
            end: position,
        },
    ))
}

fn location_for_path_range(
    path: &Path,
    text: &str,
    start_byte: usize,
    end_byte: usize,
) -> Option<Location> {
    Some(Location::new(
        Url::from_file_path(path).ok()?,
        range_for_bytes(text, start_byte, end_byte).ok()?,
    ))
}

fn workspace_query_matches(name: &str, query: &str) -> bool {
    let query = query.trim();
    query.is_empty()
        || name_contains_case_insensitive(name, query)
        || abbreviation_matches(name, query, false)
        || compact_subsequence_matches(name, query, false)
}

fn class_like_names_from_direct_child(
    node: Node,
    child_kind: &str,
    text: &str,
    namespace: Option<&str>,
) -> Vec<String> {
    direct_child_kind(node, child_kind)
        .map(|child| class_like_names(child, text, namespace))
        .unwrap_or_default()
}

fn class_like_names(node: Node, text: &str, namespace: Option<&str>) -> Vec<String> {
    let mut cursor = node.walk();
    node.named_children(&mut cursor)
        .filter(|child| is_name_node(*child))
        .map(|child| qualify_name(node_text(child, text), namespace))
        .collect()
}

fn parameter_names(parameters_node: Node, text: &str) -> Vec<String> {
    let mut parameters = Vec::new();
    let mut cursor = parameters_node.walk();

    for child in parameters_node.named_children(&mut cursor) {
        if !matches!(
            child.kind(),
            "simple_parameter" | "variadic_parameter" | "property_promotion_parameter"
        ) {
            continue;
        }

        let Some(name_node) = child.child_by_field_name("name") else {
            continue;
        };

        parameters.push(
            node_text(name_node, text)
                .trim_start_matches('$')
                .to_string(),
        );
    }

    parameters
}

fn parameter_types(
    parameters_node: Node,
    text: &str,
    namespace: Option<&str>,
    imports: &ImportMap,
) -> Vec<Option<ComparableReturnType>> {
    let mut types = Vec::new();
    let mut cursor = parameters_node.walk();

    for child in parameters_node.named_children(&mut cursor) {
        if !matches!(
            child.kind(),
            "simple_parameter" | "variadic_parameter" | "property_promotion_parameter"
        ) {
            continue;
        }

        let declared_type = child.child_by_field_name("type").and_then(|type_node| {
            comparable_parameter_type_node(type_node, text, namespace, imports)
        });
        types.push(declared_type);
    }

    types
}

fn declaration_signature_parameter_types(
    declaration: Node,
    parameters_node: Node,
    text: &str,
    namespace: Option<&str>,
    imports: &ImportMap,
) -> Vec<Option<ComparableReturnType>> {
    let native_types = parameter_types(parameters_node, text, namespace, imports);
    let phpdoc_types =
        phpdoc_param_types_before(text, declaration.start_byte(), namespace, imports)
            .into_iter()
            .filter_map(|(variable_name, type_name)| {
                comparable_parameter_type(&type_name, namespace, imports)
                    .map(|return_type| (variable_name, return_type))
            })
            .collect::<HashMap<_, _>>();

    parameter_names(parameters_node, text)
        .into_iter()
        .zip(native_types)
        .map(|(parameter_name, native_type)| {
            native_type.or_else(|| phpdoc_types.get(&format!("${parameter_name}")).cloned())
        })
        .collect()
}

fn comparable_parameter_type(
    type_name: &str,
    namespace: Option<&str>,
    imports: &ImportMap,
) -> Option<ComparableReturnType> {
    let (normalized_type_name, _) = nullable_type_name(type_name);
    let normalized = normalize_return_type_name(normalized_type_name);
    if matches!(
        normalized.as_str(),
        "callable"
            | "iterable"
            | "mixed"
            | "never"
            | "object"
            | "parent"
            | "self"
            | "static"
            | "void"
    ) {
        return None;
    }

    Some(comparable_return_type(type_name, namespace, imports))
}

fn comparable_parameter_type_node(
    type_node: Node,
    text: &str,
    namespace: Option<&str>,
    imports: &ImportMap,
) -> Option<ComparableReturnType> {
    let type_name = single_named_type(type_node, text)?;
    let mut comparable = comparable_parameter_type(&type_name, namespace, imports)?;
    comparable.allows_null = comparable.allows_null || type_node_allows_null(type_node, text);
    Some(comparable)
}

fn type_node_allows_null(type_node: Node, text: &str) -> bool {
    node_text(type_node, text).trim_start().starts_with('?')
}

fn declaration_signature_return_type(
    declaration: Node,
    text: &str,
    namespace: Option<&str>,
    imports: &ImportMap,
) -> Option<ComparableReturnType> {
    declaration
        .child_by_field_name("return_type")
        .and_then(|type_node| comparable_parameter_type_node(type_node, text, namespace, imports))
        .or_else(|| phpdoc_return_type_before(text, declaration.start_byte(), namespace, imports))
}

fn phpdoc_return_type_before(
    text: &str,
    byte_offset: usize,
    namespace: Option<&str>,
    imports: &ImportMap,
) -> Option<ComparableReturnType> {
    let return_line = phpdoc_tag_lines_before(text, byte_offset, "@return")
        .into_iter()
        .next()?;
    let return_type = return_line.split_whitespace().next()?.trim();
    comparable_parameter_type(return_type, namespace, imports)
}

fn parameters_node_has_variadic(parameters_node: Node) -> bool {
    let mut cursor = parameters_node.walk();
    parameters_node
        .named_children(&mut cursor)
        .any(|child| child.kind() == "variadic_parameter")
}

fn namespace_at_byte(root: Node, text: &str, byte_offset: usize) -> Option<String> {
    let mut cursor = root.walk();
    let mut active_namespace = None;

    for child in root.named_children(&mut cursor) {
        if child.kind() != "namespace_definition" {
            continue;
        }

        if child.start_byte() > byte_offset {
            break;
        }

        let namespace_name = child
            .child_by_field_name("name")
            .map(|name| clean_name_text(node_text(name, text)))
            .filter(|name| !name.is_empty());

        if child.child_by_field_name("body").is_some() {
            if child.start_byte() <= byte_offset && byte_offset <= child.end_byte() {
                return namespace_name;
            }
        } else {
            active_namespace = namespace_name;
        }
    }

    active_namespace
}

fn find_call_at_byte(root: Node, text: &str, byte_offset: usize) -> Result<CallInfo, SkipReason> {
    let Some(node) = find_smallest_call(root, byte_offset) else {
        return Err(SkipReason::NoSupportedCall);
    };

    call_info(node, text)
}

fn find_smallest_name<'tree>(node: Node<'tree>, byte_offset: usize) -> Option<Node<'tree>> {
    if byte_offset < node.start_byte() || byte_offset > node.end_byte() {
        return None;
    }

    let mut cursor = node.walk();
    for child in node.named_children(&mut cursor) {
        if let Some(found) = find_smallest_name(child, byte_offset) {
            return Some(found);
        }
    }

    is_name_node(node).then_some(node)
}

fn find_name_reference_at_byte<'tree>(
    node: Node<'tree>,
    text: &str,
    byte_offset: usize,
) -> Option<Node<'tree>> {
    let name = find_smallest_name(node, byte_offset)?;
    let mut current = name;
    let mut best = name;

    while let Some(parent) = current.parent() {
        if parent.start_byte() > byte_offset || parent.end_byte() < byte_offset {
            break;
        }
        if is_name_node(parent) {
            best = parent;
            if clean_name_text(node_text(parent, text)).contains('\\') {
                return Some(parent);
            }
        }
        current = parent;
    }

    Some(best)
}

fn find_variable_name_at_byte(node: Node, text: &str, byte_offset: usize) -> Option<String> {
    if byte_offset < node.start_byte() || byte_offset > node.end_byte() {
        return None;
    }

    let mut cursor = node.walk();
    for child in node.named_children(&mut cursor) {
        if let Some(variable) = find_variable_name_at_byte(child, text, byte_offset) {
            return Some(variable);
        }
    }

    (node.kind() == "variable_name").then(|| node_text(node, text).to_string())
}

fn find_function_like_declaration_at_byte<'tree>(
    node: Node<'tree>,
    byte_offset: usize,
) -> Option<Node<'tree>> {
    if byte_offset < node.start_byte() || byte_offset > node.end_byte() {
        return None;
    }

    let mut cursor = node.walk();
    for child in node.named_children(&mut cursor) {
        if let Some(found) = find_function_like_declaration_at_byte(child, byte_offset) {
            return Some(found);
        }
    }

    matches!(node.kind(), "function_definition" | "method_declaration").then_some(node)
}

fn find_method_declaration_at_byte<'tree>(
    node: Node<'tree>,
    byte_offset: usize,
) -> Option<Node<'tree>> {
    let declaration = find_function_like_declaration_at_byte(node, byte_offset)?;
    (declaration.kind() == "method_declaration").then_some(declaration)
}

fn find_class_declaration_at_byte<'tree>(
    node: Node<'tree>,
    byte_offset: usize,
) -> Option<Node<'tree>> {
    if node.kind() == "class_declaration"
        && node.start_byte() <= byte_offset
        && byte_offset <= node.end_byte()
    {
        return Some(node);
    }

    let mut cursor = node.walk();
    for child in node.named_children(&mut cursor) {
        if let Some(found) = find_class_declaration_at_byte(child, byte_offset) {
            return Some(found);
        }
    }

    None
}

fn containing_class_like_declaration<'tree>(node: Node<'tree>) -> Option<Node<'tree>> {
    let mut current = node;
    while let Some(parent) = current.parent() {
        if matches!(
            parent.kind(),
            "class_declaration" | "interface_declaration" | "trait_declaration"
        ) {
            return Some(parent);
        }
        current = parent;
    }

    None
}

fn find_smallest_call<'tree>(node: Node<'tree>, byte_offset: usize) -> Option<Node<'tree>> {
    if byte_offset < node.start_byte() || byte_offset > node.end_byte() {
        return None;
    }

    let mut cursor = node.walk();
    for child in node.named_children(&mut cursor) {
        if let Some(found) = find_smallest_call(child, byte_offset) {
            return Some(found);
        }
    }

    is_supported_call_kind(node.kind()).then_some(node)
}

fn is_supported_call_kind(kind: &str) -> bool {
    matches!(
        kind,
        "function_call_expression"
            | "scoped_call_expression"
            | "member_call_expression"
            | "object_creation_expression"
    )
}

fn call_info(node: Node, text: &str) -> Result<CallInfo, SkipReason> {
    let Some(arguments_node) = find_arguments_node(node) else {
        return Err(SkipReason::NoSupportedCall);
    };
    let arguments = argument_infos(arguments_node, text);

    if arguments.is_empty() {
        return Err(SkipReason::NoEdits);
    }

    let target = call_target_for_call_node(node, text)?;

    Ok(CallInfo {
        target,
        arguments,
        arguments_start_byte: arguments_node.start_byte(),
        arguments_end_byte: arguments_node.end_byte(),
    })
}

fn call_target_for_call_node(node: Node, text: &str) -> Result<CallTarget, SkipReason> {
    match node.kind() {
        "function_call_expression" => {
            let Some(function_node) = node.child_by_field_name("function") else {
                return Err(SkipReason::UnsupportedDynamicCall);
            };
            if !is_name_node(function_node) {
                return Err(SkipReason::UnsupportedDynamicCall);
            }
            Ok(CallTarget::Function(clean_name_text(node_text(
                function_node,
                text,
            ))))
        }
        "scoped_call_expression" => {
            let Some(scope_node) = node.child_by_field_name("scope") else {
                return Err(SkipReason::UnsupportedDynamicCall);
            };
            if !is_name_node(scope_node) {
                return Err(SkipReason::UnsupportedDynamicCall);
            }
            let Some(method) = member_name_for_call(node, text) else {
                return Err(SkipReason::UnsupportedDynamicCall);
            };
            Ok(CallTarget::StaticMethod {
                class_name: clean_name_text(node_text(scope_node, text)),
                method,
            })
        }
        "member_call_expression" => {
            let Some(object_node) = node.child_by_field_name("object") else {
                return Err(SkipReason::UnsupportedDynamicCall);
            };
            if object_node.kind() != "variable_name" {
                return Err(SkipReason::UnsupportedDynamicCall);
            }
            let Some(method) = member_name_for_call(node, text) else {
                return Err(SkipReason::UnsupportedDynamicCall);
            };
            Ok(CallTarget::InstanceMethod {
                variable: node_text(object_node, text).to_string(),
                method,
            })
        }
        "object_creation_expression" => {
            let Some(class_node) = class_name_for_object_creation(node) else {
                return Err(SkipReason::UnsupportedDynamicCall);
            };
            if !is_name_node(class_node) {
                return Err(SkipReason::UnsupportedDynamicCall);
            }
            Ok(CallTarget::Constructor {
                class_name: clean_name_text(node_text(class_node, text)),
            })
        }
        _ => Err(SkipReason::NoSupportedCall),
    }
}

fn call_target_range(text: &str, node: Node) -> Result<Range, SkipReason> {
    let target_node = match node.kind() {
        "function_call_expression" => node.child_by_field_name("function"),
        "scoped_call_expression" => member_name_node_for_call(node),
        "member_call_expression" => member_name_node_for_call(node),
        "object_creation_expression" => class_name_for_object_creation(node),
        _ => None,
    }
    .ok_or(SkipReason::NoSupportedCall)?;

    range_for_bytes(text, target_node.start_byte(), target_node.end_byte())
}

fn find_arguments_node(node: Node) -> Option<Node> {
    if let Some(arguments) = node.child_by_field_name("arguments") {
        return Some(arguments);
    }

    let mut cursor = node.walk();
    node.named_children(&mut cursor)
        .find(|child| child.kind() == "arguments")
}

fn class_name_for_object_creation(node: Node) -> Option<Node> {
    let mut cursor = node.walk();
    node.named_children(&mut cursor)
        .find(|child| is_name_node(*child))
}

fn member_name_for_call(node: Node, text: &str) -> Option<String> {
    member_name_node_for_call(node).map(|node| node_text(node, text).to_string())
}

fn member_name_node_for_call(node: Node) -> Option<Node> {
    let mut cursor = node.walk();

    for child in node.named_children(&mut cursor) {
        if child.kind() == "arguments" {
            continue;
        }
        if node.child_by_field_name("scope") == Some(child)
            || node.child_by_field_name("object") == Some(child)
        {
            continue;
        }
        if child.kind() == "name" {
            return Some(child);
        }
    }

    None
}

fn argument_infos(arguments_node: Node, text: &str) -> Vec<ArgumentInfo> {
    let mut arguments = Vec::new();
    let mut cursor = arguments_node.walk();

    for child in arguments_node.named_children(&mut cursor) {
        if child.kind() != "argument" {
            continue;
        }

        let argument_text = node_text(child, text);
        arguments.push(ArgumentInfo {
            start_byte: child.start_byte(),
            end_byte: child.end_byte(),
            insert_byte: child.start_byte(),
            name: named_argument_name(child, text),
            is_unpacking: argument_text.trim_start().starts_with("..."),
        });
    }

    arguments
}

fn named_argument_name(argument_node: Node, text: &str) -> Option<String> {
    let mut cursor = argument_node.walk();
    let mut children = argument_node.named_children(&mut cursor);
    let first_child = children.next()?;

    if first_child.kind() != "name" {
        return None;
    }

    let after_name = &text[first_child.end_byte()..argument_node.end_byte()];
    after_name
        .trim_start()
        .starts_with(':')
        .then(|| clean_name_text(node_text(first_child, text)))
}

fn edits_for_call(
    text: &str,
    call: &CallInfo,
    signature: &Signature,
) -> Result<Vec<TextEdit>, SkipReason> {
    if call.arguments.iter().any(|argument| argument.is_unpacking) {
        return Err(SkipReason::UnpackingArgument);
    }

    if call.arguments.len() > signature.parameters.len() || call.arguments.is_empty() {
        return Err(SkipReason::UnsafeNamedArguments);
    }

    let mut edits = Vec::new();

    for (argument, parameter_name) in call.arguments.iter().zip(signature.parameters.iter()) {
        if let Some(argument_name) = &argument.name {
            if !argument_name.eq_ignore_ascii_case(parameter_name) {
                return Err(SkipReason::UnsafeNamedArguments);
            }
            continue;
        }

        let Some(position) = lsp_position_for_byte_offset(text, argument.insert_byte) else {
            return Err(SkipReason::InvalidCursorPosition);
        };
        edits.push(TextEdit::new(
            Range {
                start: position,
                end: position,
            },
            format!("{parameter_name}: "),
        ));
    }

    if edits.is_empty() {
        Err(SkipReason::NoEdits)
    } else {
        Ok(edits)
    }
}

fn action_title_for_edits(edits: &[TextEdit]) -> String {
    if edits.len() == 1
        && let Some(parameter_name) = edits[0].new_text.strip_suffix(": ")
    {
        return format!("[Rephactor] Add name identifier '{parameter_name}'");
    }

    ACTION_TITLE.to_string()
}

fn replace_fqcn_with_import_action(
    uri: &Url,
    text: &str,
    root: Node,
    byte_offset: usize,
    imports: &[ImportDeclaration],
    index: &SymbolIndex,
) -> Result<Option<CodeAction>, SkipReason> {
    let Some(name_node) = find_name_reference_at_byte(root, text, byte_offset) else {
        return Ok(None);
    };
    if is_inside_import(name_node, root, byte_offset) {
        return Ok(None);
    }

    let fqn = clean_name_text(node_text(name_node, text))
        .trim_start_matches('\\')
        .to_string();
    if !fqn.contains('\\') {
        return Ok(None);
    }

    let Some(class_info) = index.classes.get(&normalize_symbol_key(&fqn)) else {
        return Err(SkipReason::UnresolvedCallable(fqn));
    };
    if class_info.fqn != fqn {
        return Err(SkipReason::AmbiguousCallable(fqn));
    }

    let short_name = last_name_segment(&fqn);
    if imports.iter().any(|import| {
        normalize_symbol_key(&import.alias) == normalize_symbol_key(short_name)
            && normalize_symbol_key(&import.fqn) != normalize_symbol_key(&fqn)
    }) {
        return Err(SkipReason::AmbiguousCallable(short_name.to_string()));
    }

    let mut edits = Vec::new();
    if !imports
        .iter()
        .any(|import| normalize_symbol_key(&import.fqn) == normalize_symbol_key(&fqn))
    {
        edits.push(insert_import_edit(text, root, imports, &fqn)?);
    }

    edits.push(TextEdit::new(
        range_for_bytes(text, name_node.start_byte(), name_node.end_byte())?,
        short_name.to_string(),
    ));

    Ok(Some(code_action(
        format!("[Rephactor] Add import for '{fqn}'"),
        uri,
        edits,
    )))
}

fn sort_imports_action(
    uri: &Url,
    text: &str,
    imports: &[ImportDeclaration],
) -> Result<Option<CodeAction>, SkipReason> {
    let normal_imports = imports
        .iter()
        .filter(|import| !import.is_grouped && !import.has_alias)
        .collect::<Vec<_>>();
    if normal_imports.len() < 2 {
        return Ok(None);
    }

    let start_byte = normal_imports
        .iter()
        .map(|import| import.start_byte)
        .min()
        .expect("normal imports");
    let end_byte = normal_imports
        .iter()
        .map(|import| line_end_including_newline(text, import.end_byte))
        .max()
        .expect("normal imports");
    let import_block = &text[start_byte..end_byte];
    if import_block.contains("//") || import_block.contains("/*") {
        return Ok(None);
    }

    let sorted = {
        let mut imports = normal_imports
            .iter()
            .map(|import| import.fqn.clone())
            .collect::<Vec<_>>();
        imports.sort_by_key(|import| normalize_symbol_key(import));
        imports
    };
    if sorted
        .iter()
        .map(|fqn| normalize_symbol_key(fqn))
        .eq(normal_imports
            .iter()
            .map(|import| normalize_symbol_key(&import.fqn)))
    {
        return Ok(None);
    }

    let new_text = sorted
        .iter()
        .map(|fqn| format!("use {fqn};\n"))
        .collect::<String>();
    let edit = TextEdit::new(range_for_bytes(text, start_byte, end_byte)?, new_text);

    Ok(Some(code_action(
        "[Rephactor] Sort imports",
        uri,
        vec![edit],
    )))
}

fn remove_unused_import_actions(
    uri: &Url,
    text: &str,
    root: Node,
    imports: &[ImportDeclaration],
) -> Result<Vec<CodeAction>, SkipReason> {
    let mut actions = Vec::new();

    for import in imports
        .iter()
        .filter(|import| !import.is_grouped && !import.has_alias)
    {
        if class_name_is_used(
            root,
            text,
            &import.alias,
            import.start_byte,
            import.end_byte,
        ) {
            continue;
        }

        let edit = TextEdit::new(
            range_for_bytes(
                text,
                import.start_byte,
                line_end_including_newline(text, import.end_byte),
            )?,
            String::new(),
        );
        actions.push(code_action(
            format!("[Rephactor] Remove unused import '{}'", import.alias),
            uri,
            vec![edit],
        ));
    }

    Ok(actions)
}

fn insert_import_edit(
    text: &str,
    root: Node,
    imports: &[ImportDeclaration],
    fqn: &str,
) -> Result<TextEdit, SkipReason> {
    let insert_byte = import_insert_byte(text, root, imports);
    let Some(position) = lsp_position_for_byte_offset(text, insert_byte) else {
        return Err(SkipReason::InvalidCursorPosition);
    };

    Ok(TextEdit::new(
        Range {
            start: position,
            end: position,
        },
        format!("use {fqn};\n"),
    ))
}

fn import_insert_byte(text: &str, root: Node, imports: &[ImportDeclaration]) -> usize {
    if let Some(last_import_end) = imports
        .iter()
        .filter(|import| !import.is_grouped)
        .map(|import| line_end_including_newline(text, import.end_byte))
        .max()
    {
        return last_import_end;
    }

    if let Some(namespace) = first_namespace_definition(root) {
        return line_end_including_newline(text, namespace.end_byte());
    }

    text.find('\n').map(|index| index + 1).unwrap_or(text.len())
}

fn first_namespace_definition(root: Node) -> Option<Node> {
    let mut cursor = root.walk();
    root.named_children(&mut cursor)
        .find(|child| child.kind() == "namespace_definition")
}

fn class_name_is_used(
    root: Node,
    text: &str,
    alias: &str,
    start_byte: usize,
    end_byte: usize,
) -> bool {
    let mut names = Vec::new();
    collect_name_nodes(root, &mut names);
    names.into_iter().any(|name| {
        (name.end_byte() <= start_byte || name.start_byte() >= end_byte)
            && node_text(name, text).eq_ignore_ascii_case(alias)
    })
}

fn collect_name_nodes<'tree>(node: Node<'tree>, names: &mut Vec<Node<'tree>>) {
    if is_name_node(node) {
        names.push(node);
        return;
    }

    let mut cursor = node.walk();
    for child in node.named_children(&mut cursor) {
        collect_name_nodes(child, names);
    }
}

fn find_reference_name_at_byte<'tree>(
    node: Node<'tree>,
    text: &str,
    byte_offset: usize,
) -> Option<Node<'tree>> {
    let name = find_name_reference_at_byte(node, text, byte_offset)?;
    if name.kind() == "qualified_name" {
        let mut cursor = name.walk();
        return name
            .named_children(&mut cursor)
            .filter(|child| child.kind() == "name")
            .find(|child| child.start_byte() <= byte_offset && byte_offset <= child.end_byte())
            .or(Some(name));
    }

    Some(name)
}

fn clean_reference_name(name: &str) -> String {
    last_name_segment(clean_name_text(name).trim_start_matches('\\')).to_string()
}

fn is_declaration_name(node: Node) -> bool {
    node.parent().is_some_and(|parent| {
        matches!(
            parent.kind(),
            "function_definition"
                | "class_declaration"
                | "interface_declaration"
                | "trait_declaration"
                | "method_declaration"
        ) && parent.child_by_field_name("name") == Some(node)
    })
}

fn reference_documents(
    uri: &Url,
    text: &str,
    open_documents: &HashMap<Url, String>,
) -> HashMap<PathBuf, String> {
    let mut documents = HashMap::new();

    if let Some(project_root) = project_root_for_uri(uri)
        && let Some(paths) = composer_autoload_paths(&project_root)
    {
        for path in paths {
            collect_reference_documents_from_path(&path, open_documents, &mut documents);
        }
    }

    for (path, open_text) in open_project_documents(open_documents) {
        documents.insert(path, open_text);
    }

    if let Ok(path) = uri.to_file_path() {
        documents.insert(path, text.to_string());
    }

    documents
}

fn collect_reference_documents_from_path(
    path: &Path,
    open_documents: &HashMap<Url, String>,
    documents: &mut HashMap<PathBuf, String>,
) {
    if path.is_dir() {
        let Ok(entries) = fs::read_dir(path) else {
            return;
        };
        for entry in entries.flatten() {
            collect_reference_documents_from_path(&entry.path(), open_documents, documents);
        }
        return;
    }

    if path.extension().and_then(|extension| extension.to_str()) != Some("php") {
        return;
    }

    if let Some(open_text) = open_documents.iter().find_map(|(uri, text)| {
        uri.to_file_path()
            .ok()
            .as_deref()
            .is_some_and(|open_path| open_path == path)
            .then(|| text.clone())
    }) {
        documents.insert(path.to_path_buf(), open_text);
        return;
    }

    if let Ok(text) = fs::read_to_string(path) {
        documents.insert(path.to_path_buf(), text);
    }
}

fn is_inside_import(node: Node, root: Node, byte_offset: usize) -> bool {
    let mut imports = Vec::new();
    collect_import_nodes(root, &mut imports);
    imports
        .into_iter()
        .any(|import| import.start_byte() <= byte_offset && byte_offset <= import.end_byte())
        || node.kind() == "namespace_use_declaration"
}

fn collect_import_nodes<'tree>(node: Node<'tree>, imports: &mut Vec<Node<'tree>>) {
    if node.kind() == "namespace_use_declaration" {
        imports.push(node);
        return;
    }

    let mut cursor = node.walk();
    for child in node.named_children(&mut cursor) {
        collect_import_nodes(child, imports);
    }
}

fn code_action(title: impl Into<String>, uri: &Url, edits: Vec<TextEdit>) -> CodeAction {
    let mut changes = HashMap::new();
    changes.insert(uri.clone(), edits);

    CodeAction {
        title: title.into(),
        kind: Some(CodeActionKind::REFACTOR_REWRITE),
        diagnostics: None,
        edit: Some(WorkspaceEdit::new(changes)),
        command: None,
        is_preferred: Some(false),
        disabled: None,
        data: None,
    }
}

fn range_for_bytes(text: &str, start_byte: usize, end_byte: usize) -> Result<Range, SkipReason> {
    let Some(start) = lsp_position_for_byte_offset(text, start_byte) else {
        return Err(SkipReason::InvalidCursorPosition);
    };
    let Some(end) = lsp_position_for_byte_offset(text, end_byte) else {
        return Err(SkipReason::InvalidCursorPosition);
    };

    Ok(Range { start, end })
}

fn line_end_including_newline(text: &str, byte_offset: usize) -> usize {
    let Some(relative_newline) = text[byte_offset..].find('\n') else {
        return text.len();
    };

    byte_offset + relative_newline + 1
}

fn location_response(
    location: Option<&SourceLocation>,
    open_documents: &HashMap<PathBuf, String>,
) -> Result<GotoDefinitionResponse, SkipReason> {
    let Some(location) = location else {
        return Err(SkipReason::UnresolvedCallable("definition".to_string()));
    };
    let Some(uri) = Url::from_file_path(&location.path).ok() else {
        return Err(SkipReason::InvalidCursorPosition);
    };
    let text = open_documents
        .get(&location.path)
        .cloned()
        .or_else(|| fs::read_to_string(&location.path).ok())
        .ok_or(SkipReason::InvalidCursorPosition)?;
    let Some(position) = lsp_position_for_byte_offset(&text, location.byte_offset) else {
        return Err(SkipReason::InvalidCursorPosition);
    };

    Ok(GotoDefinitionResponse::Scalar(Location::new(
        uri,
        Range {
            start: position,
            end: position,
        },
    )))
}

fn active_parameter_for_call(
    byte_offset: usize,
    call: &CallInfo,
    signature: &Signature,
) -> Result<u32, SkipReason> {
    if signature.parameters.is_empty()
        || byte_offset < call.arguments_start_byte
        || byte_offset > call.arguments_end_byte
    {
        return Err(SkipReason::NoSupportedCall);
    }

    let Some(argument_index) = active_argument_index(byte_offset, call) else {
        return Err(SkipReason::NoSupportedCall);
    };
    let Some(argument) = call.arguments.get(argument_index) else {
        return Err(SkipReason::NoSupportedCall);
    };

    if let Some(name) = &argument.name {
        return signature
            .parameters
            .iter()
            .position(|parameter| parameter.eq_ignore_ascii_case(name))
            .map(|index| index as u32)
            .ok_or(SkipReason::UnsafeNamedArguments);
    }

    if argument_index >= signature.parameters.len() {
        return Err(SkipReason::UnsafeNamedArguments);
    }

    Ok(argument_index as u32)
}

fn active_argument_index(byte_offset: usize, call: &CallInfo) -> Option<usize> {
    if call.arguments.is_empty() {
        return None;
    }

    for (index, argument) in call.arguments.iter().enumerate() {
        if argument.start_byte <= byte_offset && byte_offset <= argument.end_byte {
            return Some(index);
        }

        if byte_offset < argument.start_byte {
            return Some(index);
        }
    }

    Some(call.arguments.len().saturating_sub(1))
}

fn signature_help_for_call(
    target: &CallTarget,
    signature: &Signature,
    active_parameter: u32,
) -> SignatureHelp {
    SignatureHelp {
        signatures: vec![SignatureInformation {
            label: signature_label(target, signature),
            documentation: None,
            parameters: Some(
                signature
                    .parameters
                    .iter()
                    .map(|parameter| ParameterInformation {
                        label: ParameterLabel::Simple(format!("${parameter}")),
                        documentation: None,
                    })
                    .collect(),
            ),
            active_parameter: Some(active_parameter),
        }],
        active_signature: Some(0),
        active_parameter: Some(active_parameter),
    }
}

fn signature_label(target: &CallTarget, signature: &Signature) -> String {
    let parameters = signature
        .parameters
        .iter()
        .map(|parameter| format!("${parameter}"))
        .collect::<Vec<_>>()
        .join(", ");

    match target {
        CallTarget::Function(name) => format!("{name}({parameters})"),
        CallTarget::StaticMethod { class_name, method } => {
            format!("{class_name}::{method}({parameters})")
        }
        CallTarget::Constructor { class_name } => {
            format!("{class_name}::__construct({parameters})")
        }
        CallTarget::InstanceMethod { variable, method } => {
            format!("{variable}->{method}({parameters})")
        }
    }
}

fn hover_from_parts(
    label: String,
    location: Option<&SourceLocation>,
    doc_summary: Option<&str>,
) -> Hover {
    let mut value = format!("```php\n{label}\n```");
    if let Some(location) = location {
        value.push_str("\n\n");
        value.push_str(&format!("Defined in {}", location.path.display()));
    }
    if let Some(summary) = doc_summary.filter(|summary| !summary.is_empty()) {
        value.push_str("\n\n");
        value.push_str(summary);
    }

    Hover {
        contents: HoverContents::Markup(MarkupContent {
            kind: MarkupKind::Markdown,
            value,
        }),
        range: None,
    }
}

fn phpdoc_summary_before(text: &str, byte_offset: usize) -> Option<String> {
    let before = text.get(..byte_offset)?;
    let comment_start = before.rfind("/**")?;
    let between = before.get(comment_start..)?;
    let comment_end = between.rfind("*/")?;
    if comment_start + comment_end + 2 < before.trim_end().len() {
        return None;
    }

    between
        .lines()
        .map(|line| {
            line.trim()
                .trim_start_matches("/**")
                .trim_start_matches('*')
                .trim_end_matches("*/")
                .trim()
        })
        .find(|line| !line.is_empty())
        .map(str::to_string)
}

fn phpdoc_mixins_before(text: &str, byte_offset: usize, namespace: Option<&str>) -> Vec<String> {
    let Some(before) = text.get(..byte_offset) else {
        return Vec::new();
    };
    let Some(comment_start) = before.rfind("/**") else {
        return Vec::new();
    };
    let Some(between) = before.get(comment_start..) else {
        return Vec::new();
    };
    let Some(comment_end) = between.rfind("*/") else {
        return Vec::new();
    };
    if comment_start + comment_end + 2 < before.trim_end().len() {
        return Vec::new();
    }

    between
        .lines()
        .filter_map(|line| {
            let line = line
                .trim()
                .trim_start_matches("/**")
                .trim_start_matches('*')
                .trim_end_matches("*/")
                .trim();
            let rest = line.strip_prefix("@mixin")?.trim();
            let mixin = rest.split_whitespace().next()?.split('<').next()?.trim();
            if mixin.is_empty() {
                None
            } else {
                Some(qualify_name(mixin, namespace))
            }
        })
        .collect()
}

fn phpdoc_methods_before(text: &str, byte_offset: usize) -> Vec<Signature> {
    phpdoc_tag_lines_before(text, byte_offset, "@method")
        .into_iter()
        .filter_map(|method_text| phpdoc_method_signature(&method_text))
        .collect()
}

fn phpdoc_method_signature(method_text: &str) -> Option<Signature> {
    let open = method_text.find('(')?;
    let close = method_text.rfind(')')?;
    if close < open {
        return None;
    }

    let name = method_text[..open]
        .split_whitespace()
        .rev()
        .find(|token| *token != "static")?
        .trim();
    if name.is_empty() {
        return None;
    }

    let parameter_text = &method_text[open + 1..close];
    let is_variadic = parameter_text.contains("...");
    let parameters = parameter_text
        .split(',')
        .filter_map(|parameter| {
            let parameter = parameter.trim();
            if parameter.is_empty() {
                return None;
            }
            parameter
                .split_whitespace()
                .last()
                .map(|name| {
                    name.trim_start_matches("...")
                        .trim_start_matches('$')
                        .to_string()
                })
                .filter(|name| !name.is_empty())
        })
        .collect::<Vec<_>>();

    Some(Signature {
        name: name.to_string(),
        parameters,
        parameter_types: Vec::new(),
        return_type: None,
        is_variadic,
        is_abstract: false,
        location: None,
        doc_summary: None,
    })
}

fn phpdoc_tag_lines_before(text: &str, byte_offset: usize, tag: &str) -> Vec<String> {
    let Some(before) = text.get(..byte_offset) else {
        return Vec::new();
    };
    let Some(comment_start) = before.rfind("/**") else {
        return Vec::new();
    };
    let Some(between) = before.get(comment_start..) else {
        return Vec::new();
    };
    let Some(comment_end) = between.rfind("*/") else {
        return Vec::new();
    };
    if comment_start + comment_end + 2 < before.trim_end().len() {
        return Vec::new();
    }

    between
        .lines()
        .filter_map(|line| {
            let line = line
                .trim()
                .trim_start_matches("/**")
                .trim_start_matches('*')
                .trim_end_matches("*/")
                .trim();
            let tag_offset = line.find(tag)?;
            Some(line[tag_offset + tag.len()..].trim().to_string())
        })
        .collect()
}

fn phpdoc_for_declaration(text: &str, declaration: Node) -> String {
    let indent = line_indent_before(text, declaration.start_byte());
    let mut lines = Vec::new();

    if let Some(parameters) = declaration.child_by_field_name("parameters") {
        let mut cursor = parameters.walk();
        for parameter in parameters.named_children(&mut cursor) {
            if parameter.kind() != "simple_parameter" {
                continue;
            }
            let Some(name_node) = parameter.child_by_field_name("name") else {
                continue;
            };
            let parameter_name = node_text(name_node, text);
            let parameter_type = parameter
                .child_by_field_name("type")
                .map(|type_node| phpdoc_type_text(type_node, text))
                .filter(|type_name| !type_name.is_empty())
                .unwrap_or_else(|| "mixed".to_string());
            lines.push(format!(
                "{indent} * @param {parameter_type} {parameter_name}"
            ));
        }
    }

    if let Some(return_type) = declaration.child_by_field_name("return_type") {
        let return_type = phpdoc_type_text(return_type, text);
        if !return_type.is_empty() && return_type != "void" {
            lines.push(format!("{indent} * @return {return_type}"));
        }
    }

    for thrown_type in thrown_type_names(declaration, text) {
        lines.push(format!("{indent} * @throws {thrown_type}"));
    }

    if lines.is_empty() {
        return String::new();
    }

    let mut docblock = format!("{indent}/**\n");
    docblock.push_str(&lines.join("\n"));
    docblock.push('\n');
    docblock.push_str(&format!("{indent} */\n"));
    docblock
}

fn thrown_type_names(declaration: Node, text: &str) -> Vec<String> {
    let mut names = Vec::new();
    collect_thrown_type_names(declaration, text, &mut names);
    names.sort_by_key(|name| name.to_ascii_lowercase());
    names.dedup_by(|left, right| left.eq_ignore_ascii_case(right));
    names
}

fn collect_thrown_type_names(node: Node, text: &str, names: &mut Vec<String>) {
    if node.kind() == "throw_expression"
        && let Some(object_creation) = find_descendant_kind(node, "object_creation_expression")
        && let Some(class_node) = class_name_for_object_creation(object_creation)
        && is_name_node(class_node)
    {
        names.push(clean_name_text(node_text(class_node, text)));
    }

    let mut cursor = node.walk();
    for child in node.named_children(&mut cursor) {
        collect_thrown_type_names(child, text, names);
    }
}

fn find_descendant_kind<'tree>(node: Node<'tree>, kind: &str) -> Option<Node<'tree>> {
    if node.kind() == kind {
        return Some(node);
    }

    let mut cursor = node.walk();
    for child in node.named_children(&mut cursor) {
        if let Some(found) = find_descendant_kind(child, kind) {
            return Some(found);
        }
    }

    None
}

fn line_indent_before(text: &str, byte_offset: usize) -> String {
    let line_start = text
        .get(..byte_offset)
        .and_then(|before| before.rfind('\n').map(|index| index + 1))
        .unwrap_or_default();
    text[line_start..byte_offset]
        .chars()
        .take_while(|character| character.is_whitespace() && *character != '\n')
        .collect()
}

fn phpdoc_type_text(type_node: Node, text: &str) -> String {
    clean_name_text(node_text(type_node, text))
        .trim_start_matches(':')
        .trim_start_matches('?')
        .trim()
        .to_string()
}

fn completion_prefix(text: &str, byte_offset: usize) -> String {
    text.get(..byte_offset)
        .unwrap_or_default()
        .chars()
        .rev()
        .take_while(|character| {
            character.is_alphanumeric() || *character == '_' || *character == '\\'
        })
        .collect::<String>()
        .chars()
        .rev()
        .collect()
}

fn static_method_completion_context(text: &str, byte_offset: usize) -> Option<(String, String)> {
    let before = text.get(..byte_offset)?;
    let colon = before.rfind("::")?;
    if before[colon + 2..]
        .chars()
        .any(|character| !(character.is_alphanumeric() || character == '_'))
    {
        return None;
    }

    let class_name = before[..colon]
        .chars()
        .rev()
        .take_while(|character| {
            character.is_alphanumeric() || *character == '_' || *character == '\\'
        })
        .collect::<String>()
        .chars()
        .rev()
        .collect::<String>();
    (!class_name.is_empty()).then(|| (class_name, completion_prefix(text, byte_offset)))
}

fn instance_method_completion_context(text: &str, byte_offset: usize) -> Option<(String, String)> {
    let before = text.get(..byte_offset)?;
    let arrow = before.rfind("->")?;
    if before[arrow + 2..]
        .chars()
        .any(|character| !(character.is_alphanumeric() || character == '_'))
    {
        return None;
    }

    let variable = before[..arrow]
        .chars()
        .rev()
        .take_while(|character| {
            character.is_alphanumeric() || *character == '_' || *character == '$'
        })
        .collect::<String>()
        .chars()
        .rev()
        .collect::<String>();
    variable
        .starts_with('$')
        .then(|| (variable, completion_prefix(text, byte_offset)))
}

fn class_completion_items(
    text: &str,
    root: Node,
    namespace: Option<&str>,
    imports: &[ImportDeclaration],
    index: &SymbolIndex,
    prefix: &str,
) -> Vec<CompletionItem> {
    let mut items = index
        .classes
        .values()
        .filter(|class_info| prefix_matches(last_name_segment(&class_info.fqn), prefix))
        .map(|class_info| {
            let mut item = CompletionItem {
                label: last_name_segment(&class_info.fqn).to_string(),
                kind: Some(CompletionItemKind::CLASS),
                detail: Some(class_info.fqn.clone()),
                ..CompletionItem::default()
            };
            if let Some(edit) =
                completion_import_edit(text, root, namespace, imports, &class_info.fqn)
            {
                item.additional_text_edits = Some(vec![edit]);
            }
            item
        })
        .collect::<Vec<_>>();
    items.sort_by_key(|item| item.label.to_ascii_lowercase());
    items
}

fn completion_import_edit(
    text: &str,
    root: Node,
    namespace: Option<&str>,
    imports: &[ImportDeclaration],
    fqn: &str,
) -> Option<TextEdit> {
    if !fqn.contains('\\') {
        return None;
    }

    let short_name = last_name_segment(fqn);
    if namespace
        .filter(|namespace| !namespace.is_empty())
        .is_some_and(|namespace| fqn == format!("{namespace}\\{short_name}"))
    {
        return None;
    }

    let normalized_fqn = normalize_symbol_key(fqn);
    if imports
        .iter()
        .any(|import| normalize_symbol_key(&import.fqn) == normalized_fqn)
    {
        return None;
    }

    let normalized_short_name = normalize_symbol_key(short_name);
    if imports
        .iter()
        .any(|import| normalize_symbol_key(&import.alias) == normalized_short_name)
    {
        return None;
    }

    insert_import_edit(text, root, imports, fqn).ok()
}

fn function_completion_items(index: &SymbolIndex, prefix: &str) -> Vec<CompletionItem> {
    let mut labels = index
        .functions
        .values()
        .flat_map(|signatures| signatures.iter())
        .map(|signature| last_name_segment(&signature.name).to_string())
        .chain(internal_function_names().into_iter().map(str::to_string))
        .filter(|name| prefix_matches(name, prefix))
        .collect::<Vec<_>>();
    labels.sort_by_key(|label| label.to_ascii_lowercase());
    labels.dedup_by(|left, right| left.eq_ignore_ascii_case(right));
    labels
        .into_iter()
        .map(|label| CompletionItem {
            label,
            kind: Some(CompletionItemKind::FUNCTION),
            ..CompletionItem::default()
        })
        .collect()
}

fn method_completion_items(
    index: &SymbolIndex,
    class_info: &ClassInfo,
    prefix: &str,
) -> Vec<CompletionItem> {
    let mut labels = class_info
        .methods
        .values()
        .map(|signature| signature.name.clone())
        .collect::<Vec<_>>();

    let mut visited = Vec::new();
    for related_name in class_info
        .parents
        .iter()
        .chain(class_info.interfaces.iter())
        .chain(class_info.traits.iter())
        .chain(class_info.mixins.iter())
    {
        index.collect_related_method_names(related_name, &mut visited, &mut labels);
    }

    let mut labels = labels
        .into_iter()
        .filter(|name| prefix_matches(name, prefix))
        .collect::<Vec<_>>();
    labels.sort_by_key(|label| label.to_ascii_lowercase());
    labels.dedup_by(|left, right| left.eq_ignore_ascii_case(right));
    labels
        .into_iter()
        .map(|label| CompletionItem {
            label,
            kind: Some(CompletionItemKind::METHOD),
            ..CompletionItem::default()
        })
        .collect()
}

fn keyword_completion_items(prefix: &str) -> Vec<CompletionItem> {
    php_keywords()
        .into_iter()
        .filter(|keyword| prefix_matches(keyword, prefix))
        .map(|keyword| CompletionItem {
            label: keyword.to_string(),
            kind: Some(CompletionItemKind::KEYWORD),
            ..CompletionItem::default()
        })
        .collect()
}

fn prefix_matches(name: &str, prefix: &str) -> bool {
    let prefix = prefix.trim();
    prefix.is_empty()
        || name_starts_with_case_insensitive(name, prefix)
        || abbreviation_matches(name, prefix, true)
        || compact_subsequence_matches(name, prefix, true)
}

fn name_starts_with_case_insensitive(name: &str, prefix: &str) -> bool {
    name.to_ascii_lowercase()
        .starts_with(&prefix.to_ascii_lowercase())
}

fn name_contains_case_insensitive(name: &str, query: &str) -> bool {
    name.to_ascii_lowercase()
        .contains(&query.to_ascii_lowercase())
}

fn abbreviation_matches(name: &str, query: &str, anchored: bool) -> bool {
    let abbreviation = word_abbreviation(name);
    let query = compact_identifier(query);
    if query.is_empty() || abbreviation.is_empty() {
        return false;
    }

    if anchored {
        abbreviation.starts_with(&query)
    } else {
        abbreviation.contains(&query)
    }
}

fn compact_subsequence_matches(name: &str, query: &str, anchored: bool) -> bool {
    let compact_name = compact_identifier(name);
    let compact_query = compact_identifier(query);
    if compact_name.is_empty() || compact_query.is_empty() {
        return false;
    }
    if anchored && compact_name.chars().next() != compact_query.chars().next() {
        return false;
    }

    compact_query
        .chars()
        .try_fold(0, |start, query_char| {
            compact_name[start..]
                .find(query_char)
                .map(|offset| start + offset + query_char.len_utf8())
        })
        .is_some()
}

fn word_abbreviation(name: &str) -> String {
    let mut abbreviation = String::new();
    let mut previous = None;

    for character in name.chars() {
        if !character.is_alphanumeric() {
            previous = Some(character);
            continue;
        }

        let starts_word = previous.is_none_or(|previous| !previous.is_alphanumeric())
            || previous.is_some_and(|previous| {
                previous == '_' || previous == '-' || previous == '\\' || previous == ':'
            })
            || previous.is_some_and(|previous| previous.is_lowercase() && character.is_uppercase());

        if starts_word {
            abbreviation.push(character.to_ascii_lowercase());
        }

        previous = Some(character);
    }

    abbreviation
}

fn compact_identifier(value: &str) -> String {
    value
        .chars()
        .filter(|character| character.is_alphanumeric())
        .map(|character| character.to_ascii_lowercase())
        .collect()
}

fn trim_trailing_whitespace(text: &str, ensure_final_newline: bool) -> String {
    let mut formatted = String::with_capacity(text.len() + 1);

    for segment in text.split_inclusive('\n') {
        let (line, newline) = if let Some(line) = segment.strip_suffix("\r\n") {
            (line, "\r\n")
        } else if let Some(line) = segment.strip_suffix('\n') {
            (line, "\n")
        } else {
            (segment, "")
        };
        formatted.push_str(line.trim_end_matches([' ', '\t']));
        formatted.push_str(newline);
    }

    if ensure_final_newline && !text.ends_with('\n') {
        formatted.push('\n');
    }

    formatted
}

fn variable_types_at_byte(
    root: Node,
    text: &str,
    byte_offset: usize,
    namespace: Option<&str>,
    imports: &ImportMap,
) -> HashMap<String, String> {
    let mut types = HashMap::new();
    collect_parameter_types(root, text, byte_offset, namespace, imports, &mut types);
    collect_phpdoc_param_types(root, text, byte_offset, namespace, imports, &mut types);
    collect_assignment_types(root, text, byte_offset, namespace, imports, &mut types);
    collect_phpdoc_var_types(text, byte_offset, namespace, imports, &mut types);
    types
}

fn collect_parameter_types(
    node: Node,
    text: &str,
    byte_offset: usize,
    namespace: Option<&str>,
    imports: &ImportMap,
    types: &mut HashMap<String, String>,
) {
    if byte_offset < node.start_byte() || byte_offset > node.end_byte() {
        return;
    }

    if matches!(node.kind(), "function_definition" | "method_declaration")
        && let Some(parameters) = node.child_by_field_name("parameters")
    {
        let mut cursor = parameters.walk();
        for parameter in parameters.named_children(&mut cursor) {
            if parameter.kind() != "simple_parameter" {
                continue;
            }

            let Some(name_node) = parameter.child_by_field_name("name") else {
                continue;
            };
            let Some(type_node) = parameter.child_by_field_name("type") else {
                continue;
            };

            let type_name = first_named_type(type_node, text);
            if let Some(type_name) = type_name {
                types.insert(
                    node_text(name_node, text).to_string(),
                    qualify_type_name(&type_name, namespace, imports),
                );
            }
        }
    }

    let mut cursor = node.walk();
    for child in node.named_children(&mut cursor) {
        collect_parameter_types(child, text, byte_offset, namespace, imports, types);
    }
}

fn collect_phpdoc_param_types(
    node: Node,
    text: &str,
    byte_offset: usize,
    namespace: Option<&str>,
    imports: &ImportMap,
    types: &mut HashMap<String, String>,
) {
    if byte_offset < node.start_byte() || byte_offset > node.end_byte() {
        return;
    }

    if matches!(node.kind(), "function_definition" | "method_declaration") {
        for (variable_name, type_name) in
            phpdoc_param_types_before(text, node.start_byte(), namespace, imports)
        {
            types.entry(variable_name).or_insert(type_name);
        }
    }

    let mut cursor = node.walk();
    for child in node.named_children(&mut cursor) {
        collect_phpdoc_param_types(child, text, byte_offset, namespace, imports, types);
    }
}

fn phpdoc_param_types_before(
    text: &str,
    byte_offset: usize,
    namespace: Option<&str>,
    imports: &ImportMap,
) -> Vec<(String, String)> {
    let Some(before) = text.get(..byte_offset) else {
        return Vec::new();
    };
    let Some(comment_start) = before.rfind("/**") else {
        return Vec::new();
    };
    let Some(between) = before.get(comment_start..) else {
        return Vec::new();
    };
    let Some(comment_end) = between.rfind("*/") else {
        return Vec::new();
    };
    if comment_start + comment_end + 2 < before.trim_end().len() {
        return Vec::new();
    }

    between
        .lines()
        .filter_map(|line| {
            let line = line
                .trim()
                .trim_start_matches("/**")
                .trim_start_matches('*')
                .trim_end_matches("*/")
                .trim();
            let param_offset = line.find("@param")?;
            let tokens = line[param_offset + 6..]
                .split_whitespace()
                .collect::<Vec<_>>();
            let (type_name, variable_name) = phpdoc_var_tokens(&tokens)?;
            Some((
                variable_name.to_string(),
                qualify_type_name(type_name, namespace, imports),
            ))
        })
        .collect()
}

fn collect_assignment_types(
    node: Node,
    text: &str,
    byte_offset: usize,
    namespace: Option<&str>,
    imports: &ImportMap,
    types: &mut HashMap<String, String>,
) {
    if node.start_byte() >= byte_offset {
        return;
    }

    if node.kind() == "assignment_expression"
        && let (Some(left), Some(right)) = (
            node.child_by_field_name("left"),
            node.child_by_field_name("right"),
        )
        && left.kind() == "variable_name"
        && right.kind() == "object_creation_expression"
        && let Some(class_node) = class_name_for_object_creation(right)
        && is_name_node(class_node)
    {
        types.insert(
            node_text(left, text).to_string(),
            qualify_type_name(
                &clean_name_text(node_text(class_node, text)),
                namespace,
                imports,
            ),
        );
    }

    let mut cursor = node.walk();
    for child in node.named_children(&mut cursor) {
        collect_assignment_types(child, text, byte_offset, namespace, imports, types);
    }
}

fn collect_phpdoc_var_types(
    text: &str,
    byte_offset: usize,
    namespace: Option<&str>,
    imports: &ImportMap,
    types: &mut HashMap<String, String>,
) {
    let Some(before) = text.get(..byte_offset) else {
        return;
    };

    for line in before.lines() {
        let line = line
            .trim()
            .trim_start_matches("/**")
            .trim_start_matches('*')
            .trim_end_matches("*/")
            .trim();
        let Some(var_offset) = line.find("@var") else {
            continue;
        };
        let tokens = line[var_offset + 4..]
            .split_whitespace()
            .collect::<Vec<_>>();
        let Some((type_name, variable_name)) = phpdoc_var_tokens(&tokens) else {
            continue;
        };

        types.insert(
            variable_name.to_string(),
            qualify_type_name(type_name, namespace, imports),
        );
    }
}

fn phpdoc_var_tokens<'a>(tokens: &'a [&str]) -> Option<(&'a str, &'a str)> {
    let first = tokens.first()?.trim();
    let second = tokens.get(1)?.trim();
    if first.starts_with('$') {
        Some((second, first))
    } else if second.starts_with('$') {
        Some((first, second))
    } else {
        None
    }
}

fn first_named_type(type_node: Node, text: &str) -> Option<String> {
    if matches!(
        type_node.kind(),
        "named_type" | "name" | "qualified_name" | "relative_name"
    ) {
        return Some(clean_name_text(node_text(type_node, text)));
    }

    let mut cursor = type_node.walk();
    for child in type_node.named_children(&mut cursor) {
        if let Some(type_name) = first_named_type(child, text) {
            return Some(type_name);
        }
    }

    None
}

fn find_project_root(document_path: &Path) -> Option<PathBuf> {
    let mut current = document_path.parent();

    while let Some(path) = current {
        if path.join("composer.json").is_file() {
            return Some(path.to_path_buf());
        }
        current = path.parent();
    }

    None
}

fn project_root_for_uri(uri: &Url) -> Option<PathBuf> {
    uri.to_file_path()
        .ok()
        .and_then(|document_path| find_project_root(&document_path))
}

fn project_root_from_workspace_uri(uri: &Url) -> Option<PathBuf> {
    let path = uri.to_file_path().ok()?;
    if path.join("composer.json").is_file() {
        Some(path)
    } else {
        find_project_root(&path)
    }
}

fn document_supports_named_arguments(uri: &Url) -> bool {
    let Some(project_root) = project_root_for_uri(uri) else {
        return true;
    };

    project_supports_named_arguments(&project_root)
}

fn project_supports_named_arguments(project_root: &Path) -> bool {
    composer_php_constraint(project_root)
        .map(|constraint| php_constraint_requires_at_least_8(&constraint))
        .unwrap_or(true)
}

fn composer_php_constraint(project_root: &Path) -> Option<String> {
    let composer_text = fs::read_to_string(project_root.join("composer.json")).ok()?;
    let composer_json: serde_json::Value = serde_json::from_str(&composer_text).ok()?;
    composer_json
        .get("require")
        .and_then(|require| require.get("php"))
        .and_then(|php| php.as_str())
        .map(str::to_string)
}

fn php_constraint_requires_at_least_8(constraint: &str) -> bool {
    constraint
        .split("||")
        .map(str::trim)
        .filter(|alternative| !alternative.is_empty())
        .all(php_constraint_alternative_requires_at_least_8)
}

fn php_constraint_alternative_requires_at_least_8(alternative: &str) -> bool {
    alternative
        .split([',', ' '])
        .map(str::trim)
        .filter(|token| !token.is_empty())
        .any(php_constraint_token_requires_at_least_8)
}

fn php_constraint_token_requires_at_least_8(token: &str) -> bool {
    let token = token.trim_start_matches('=');
    let token = token.strip_prefix(">=").unwrap_or(token);
    let token = token.strip_prefix('^').unwrap_or(token);
    let token = token.strip_prefix('~').unwrap_or(token);
    let token = token.strip_prefix('v').unwrap_or(token);

    token == "8" || token.starts_with("8.") || token.starts_with("8.*") || token.starts_with("9")
}

fn composer_autoload_paths(project_root: &Path) -> Option<Vec<PathBuf>> {
    let composer_text = fs::read_to_string(project_root.join("composer.json")).ok()?;
    let composer_json: serde_json::Value = serde_json::from_str(&composer_text).ok()?;
    let autoload = composer_json.get("autoload")?;
    let mut roots = Vec::new();

    if let Some(psr4) = autoload.get("psr-4").and_then(|psr4| psr4.as_object()) {
        for value in psr4.values() {
            collect_composer_paths(project_root, value, &mut roots);
        }
    }

    if let Some(classmap) = autoload.get("classmap") {
        collect_composer_paths(project_root, classmap, &mut roots);
    }

    (!roots.is_empty()).then_some(roots)
}

fn collect_composer_paths(
    project_root: &Path,
    value: &serde_json::Value,
    paths: &mut Vec<PathBuf>,
) {
    if let Some(path) = value.as_str() {
        paths.push(project_root.join(path));
    } else if let Some(values) = value.as_array() {
        for value in values {
            collect_composer_paths(project_root, value, paths);
        }
    }
}

fn is_name_node(node: Node) -> bool {
    matches!(node.kind(), "name" | "qualified_name" | "relative_name")
}

fn node_text<'a>(node: Node, text: &'a str) -> &'a str {
    node.utf8_text(text.as_bytes()).unwrap_or("")
}

fn clean_name_text(name: &str) -> String {
    name.chars()
        .filter(|character| !character.is_whitespace())
        .collect()
}

fn qualify_name(name: &str, namespace: Option<&str>) -> String {
    let name = clean_name_text(name);
    if name.starts_with('\\') || name.contains('\\') || namespace.unwrap_or("").is_empty() {
        name.trim_start_matches('\\').to_string()
    } else {
        format!("{}\\{}", namespace.unwrap_or(""), name)
    }
}

fn qualify_type_name(name: &str, namespace: Option<&str>, imports: &ImportMap) -> String {
    imports
        .resolve_class_name(name, namespace)
        .into_iter()
        .next()
        .unwrap_or_else(|| qualify_name(name, namespace))
}

fn name_candidates(name: &str, namespace: Option<&str>) -> Vec<String> {
    let name = clean_name_text(name);
    if name.starts_with('\\') {
        return vec![name.trim_start_matches('\\').to_string()];
    }
    if name.contains('\\') {
        return vec![qualify_name(&name, namespace)];
    }

    let mut candidates = Vec::new();
    if let Some(namespace) = namespace.filter(|namespace| !namespace.is_empty()) {
        candidates.push(format!("{namespace}\\{name}"));
    }
    candidates.push(name);
    candidates
}

fn last_name_segment(name: &str) -> &str {
    name.rsplit('\\').next().unwrap_or(name)
}

fn normalize_symbol_key(name: &str) -> String {
    clean_name_text(name)
        .trim_start_matches('\\')
        .to_ascii_lowercase()
}

fn normalize_method_key(name: &str) -> String {
    name.to_ascii_lowercase()
}

fn internal_function_signature(name: &str) -> Option<Signature> {
    let normalized_name = normalize_symbol_key(name);
    let parameters = match normalized_name.as_str() {
        "array_filter" => &["array", "callback", "mode"][..],
        "array_key_exists" => &["key", "array"],
        "array_map" => &["callback", "array", "arrays"],
        "array_merge" => &["arrays"],
        "count" => &["value", "mode"],
        "explode" => &["separator", "string", "limit"],
        "implode" => &["separator", "array"],
        "in_array" => &["needle", "haystack", "strict"],
        "is_array" => &["value"],
        "json_decode" => &["json", "associative", "depth", "flags"],
        "json_encode" => &["value", "flags", "depth"],
        "preg_match" => &["pattern", "subject", "matches", "flags", "offset"],
        "str_contains" => &["haystack", "needle"],
        "str_replace" => &["search", "replace", "subject", "count"],
        "strlen" => &["string"],
        "trim" => &["string", "characters"],
        _ => return None,
    };

    let parameters = parameters
        .iter()
        .map(|parameter| parameter.to_string())
        .collect::<Vec<_>>();
    Some(Signature {
        name: name.to_string(),
        parameter_types: vec![None; parameters.len()],
        parameters,
        return_type: None,
        is_variadic: matches!(normalized_name.as_str(), "array_merge"),
        is_abstract: false,
        location: None,
        doc_summary: Some(format!(
            "[PHP manual](https://www.php.net/{normalized_name})"
        )),
    })
}

fn internal_function_names() -> Vec<&'static str> {
    vec![
        "array_filter",
        "array_key_exists",
        "array_map",
        "array_merge",
        "count",
        "explode",
        "implode",
        "in_array",
        "is_array",
        "json_decode",
        "json_encode",
        "preg_match",
        "str_contains",
        "str_replace",
        "strlen",
        "trim",
    ]
}

fn php_keywords() -> Vec<&'static str> {
    vec![
        "abstract",
        "array",
        "as",
        "break",
        "callable",
        "case",
        "catch",
        "class",
        "clone",
        "const",
        "continue",
        "default",
        "do",
        "echo",
        "else",
        "elseif",
        "enum",
        "extends",
        "final",
        "finally",
        "for",
        "foreach",
        "function",
        "global",
        "if",
        "implements",
        "include",
        "include_once",
        "interface",
        "match",
        "namespace",
        "new",
        "private",
        "protected",
        "public",
        "readonly",
        "require",
        "require_once",
        "return",
        "static",
        "switch",
        "throw",
        "trait",
        "try",
        "use",
        "while",
        "yield",
    ]
}

fn source_location(path: Option<&Path>, byte_offset: usize) -> Option<SourceLocation> {
    path.map(|path| SourceLocation {
        path: path.to_path_buf(),
        byte_offset,
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::time::{SystemTime, UNIX_EPOCH};

    fn uri() -> Url {
        Url::parse("file:///tmp/project/src/Example.php").expect("valid uri")
    }

    fn named_argument_code_action(uri: &Url, text: &str, position: Position) -> Option<CodeAction> {
        analyze_code_actions_for_position(uri, text, position, &HashMap::new())
            .actions
            .into_iter()
            .find_map(|action| match action {
                CodeActionOrCommand::CodeAction(action) => Some(action),
                CodeActionOrCommand::Command(_) => None,
            })
    }

    fn skip_reason(uri: &Url, text: &str, position: Position) -> Option<SkipReason> {
        analyze_code_actions_for_position(uri, text, position, &HashMap::new()).skip_reason
    }

    fn signature_help(text: &str, line: u32, character: u32) -> Option<SignatureHelp> {
        let mut cache = ProjectIndexCache::default();
        analyze_signature_help_for_position_with_cache(
            &uri(),
            text,
            position(line, character),
            &HashMap::new(),
            &mut cache,
        )
        .signature_help
    }

    fn active_parameter(text: &str, line: u32, character: u32) -> Option<u32> {
        signature_help(text, line, character).and_then(|help| help.active_parameter)
    }

    fn position(line: u32, character: u32) -> Position {
        Position { line, character }
    }

    fn action_edits(text: &str, line: u32, character: u32) -> Vec<TextEdit> {
        let action = named_argument_code_action(&uri(), text, position(line, character))
            .expect("code action");
        action
            .edit
            .expect("workspace edit")
            .changes
            .expect("changes")
            .remove(&uri())
            .expect("edits")
    }

    fn action_by_title(text: &str, line: u32, character: u32, title: &str) -> CodeAction {
        analyze_code_actions_for_position(&uri(), text, position(line, character), &HashMap::new())
            .actions
            .into_iter()
            .find_map(|action| match action {
                CodeActionOrCommand::CodeAction(action) if action.title == title => Some(action),
                _ => None,
            })
            .expect("code action by title")
    }

    fn edits_from_action(action: CodeAction) -> Vec<TextEdit> {
        action
            .edit
            .expect("workspace edit")
            .changes
            .expect("changes")
            .remove(&uri())
            .expect("edits")
    }

    fn apply_edits(text: &str, edits: &[TextEdit]) -> String {
        let mut output = text.to_string();
        let mut byte_edits = edits
            .iter()
            .map(|edit| {
                let start = byte_offset_for_lsp_position(&output, edit.range.start)
                    .expect("valid edit start");
                let end =
                    byte_offset_for_lsp_position(&output, edit.range.end).expect("valid edit end");
                (start, end, edit.new_text.clone())
            })
            .collect::<Vec<_>>();

        byte_edits.sort_by_key(|(start, _, _)| *start);

        for (start, end, new_text) in byte_edits.into_iter().rev() {
            output.replace_range(start..end, &new_text);
        }

        output
    }

    fn unique_project_root() -> PathBuf {
        let nanos = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .expect("system time")
            .as_nanos();
        std::env::temp_dir().join(format!("rephactor-test-{nanos}"))
    }

    #[test]
    fn converts_same_file_function_call() {
        let text =
            "<?php\nfunction send_invoice($invoice, $notify) {}\nsend_invoice($invoice, true);\n";

        let edits = action_edits(text, 2, 5);

        assert_eq!(
            apply_edits(text, &edits),
            "<?php\nfunction send_invoice($invoice, $notify) {}\nsend_invoice(invoice: $invoice, notify: true);\n"
        );
    }

    #[test]
    fn returns_signature_help_for_same_file_function_call() {
        let text =
            "<?php\nfunction send_invoice($invoice, $notify) {}\nsend_invoice($invoice, true);\n";

        let help = signature_help(text, 2, 22).expect("signature help");

        assert_eq!(help.signatures.len(), 1);
        assert_eq!(help.signatures[0].label, "send_invoice($invoice, $notify)");
        assert_eq!(help.active_parameter, Some(1));
    }

    #[test]
    fn signature_help_uses_named_argument_parameter_index() {
        let text = "<?php\nfunction send_invoice($invoice, $notify) {}\nsend_invoice(notify: true, invoice: $invoice);\n";

        assert_eq!(active_parameter(text, 2, 17), Some(1));
        assert_eq!(active_parameter(text, 2, 31), Some(0));
    }

    #[test]
    fn signature_help_maps_partial_named_argument_positionally() {
        let text = "<?php\nclass customer_supplier { public static function accumulatePoints($shop_id, $grand_total, $exchange_gift = null) {} }\ncustomer_supplier::accumulatePoints(\n    shop_id: $shop_id,\n    grand_total: $request->grand_total,\n    $request->exchange_gift,\n);\n";

        let help = signature_help(text, 5, 14).expect("signature help");

        assert_eq!(help.active_parameter, Some(2));
        assert_eq!(
            help.signatures[0].label,
            "customer_supplier::accumulatePoints($shop_id, $grand_total, $exchange_gift)"
        );
    }

    #[test]
    fn converts_seeded_php_internal_function_call() {
        let text = "<?php\nstr_replace($search, $replace, $subject);\n";

        let edits = action_edits(text, 1, 5);

        assert_eq!(
            apply_edits(text, &edits),
            "<?php\nstr_replace(search: $search, replace: $replace, subject: $subject);\n"
        );
    }

    #[test]
    fn adds_import_and_shortens_fully_qualified_class_name() {
        let text = "<?php\nnamespace App\\Http;\nclass Controller { public function run() { \\App\\Models\\Customer::sync(); } }\nnamespace App\\Models;\nclass Customer { public static function sync() {} }\n";

        let action = action_by_title(
            text,
            2,
            60,
            "[Rephactor] Add import for 'App\\Models\\Customer'",
        );
        let edits = edits_from_action(action);

        assert_eq!(
            apply_edits(text, &edits),
            "<?php\nnamespace App\\Http;\nuse App\\Models\\Customer;\nclass Controller { public function run() { Customer::sync(); } }\nnamespace App\\Models;\nclass Customer { public static function sync() {} }\n"
        );
    }

    #[test]
    fn sorts_simple_imports_without_reformatting_usage() {
        let text = "<?php\nnamespace App\\Http;\nuse Zed\\B;\nuse App\\A;\nnew B();\nnew A();\n";

        let action = action_by_title(text, 4, 1, "[Rephactor] Sort imports");
        let edits = edits_from_action(action);

        assert_eq!(
            apply_edits(text, &edits),
            "<?php\nnamespace App\\Http;\nuse App\\A;\nuse Zed\\B;\nnew B();\nnew A();\n"
        );
    }

    #[test]
    fn removes_unused_simple_import() {
        let text = "<?php\nnamespace App\\Http;\nuse App\\A;\nuse App\\Unused;\nnew A();\n";

        let action = action_by_title(text, 4, 1, "[Rephactor] Remove unused import 'Unused'");
        let edits = edits_from_action(action);

        assert_eq!(
            apply_edits(text, &edits),
            "<?php\nnamespace App\\Http;\nuse App\\A;\nnew A();\n"
        );
    }

    #[test]
    fn skips_import_action_when_short_name_is_ambiguous() {
        let text = "<?php\nnamespace App\\Http;\nuse Other\\Customer;\nclass Controller { public function run() { \\App\\Models\\Customer::sync(); } }\nnamespace App\\Models;\nclass Customer { public static function sync() {} }\n";

        let actions =
            analyze_code_actions_for_position(&uri(), text, position(2, 78), &HashMap::new());

        assert!(actions.actions.into_iter().all(|action| match action {
            CodeActionOrCommand::CodeAction(action) => !action.title.contains("Add import"),
            CodeActionOrCommand::Command(_) => true,
        }));
    }

    #[test]
    fn converts_namespaced_same_file_function_call() {
        let text = "<?php\nnamespace App\\Billing;\nfunction send_invoice($invoice, $notify) {}\nsend_invoice($invoice, true);\n";

        let edits = action_edits(text, 3, 5);

        assert_eq!(
            apply_edits(text, &edits),
            "<?php\nnamespace App\\Billing;\nfunction send_invoice($invoice, $notify) {}\nsend_invoice(invoice: $invoice, notify: true);\n"
        );
    }

    #[test]
    fn converts_static_method_call() {
        let text = "<?php\nclass InvoiceSender { public static function dispatch($invoice, $notify) {} }\nInvoiceSender::dispatch($invoice, true);\n";

        let edits = action_edits(text, 2, 20);

        assert_eq!(
            apply_edits(text, &edits),
            "<?php\nclass InvoiceSender { public static function dispatch($invoice, $notify) {} }\nInvoiceSender::dispatch(invoice: $invoice, notify: true);\n"
        );
    }

    #[test]
    fn converts_static_method_call_through_import() {
        let text = "<?php\nnamespace App\\Http;\nuse App\\Models\\InvoiceSender;\nnamespace App\\Models;\nclass InvoiceSender { public static function dispatch($invoice, $notify) {} }\nnamespace App\\Http;\nInvoiceSender::dispatch($invoice, true);\n";

        let edits = action_edits(text, 6, 20);

        assert_eq!(
            apply_edits(text, &edits),
            "<?php\nnamespace App\\Http;\nuse App\\Models\\InvoiceSender;\nnamespace App\\Models;\nclass InvoiceSender { public static function dispatch($invoice, $notify) {} }\nnamespace App\\Http;\nInvoiceSender::dispatch(invoice: $invoice, notify: true);\n"
        );
    }

    #[test]
    fn converts_static_method_call_through_grouped_import() {
        let text = "<?php\nnamespace App\\Http;\nuse App\\Models\\{customer_supplier};\nnamespace App\\Models;\nclass customer_supplier { public static function accumulatePoints($shop_id, $promotion_id) {} }\nnamespace App\\Http;\ncustomer_supplier::accumulatePoints($shop_id, $promotion_id);\n";

        let edits = action_edits(text, 6, 35);

        assert_eq!(
            apply_edits(text, &edits),
            "<?php\nnamespace App\\Http;\nuse App\\Models\\{customer_supplier};\nnamespace App\\Models;\nclass customer_supplier { public static function accumulatePoints($shop_id, $promotion_id) {} }\nnamespace App\\Http;\ncustomer_supplier::accumulatePoints(shop_id: $shop_id, promotion_id: $promotion_id);\n"
        );
    }

    #[test]
    fn converts_static_method_call_through_aliased_import() {
        let text = "<?php\nnamespace App\\Http;\nuse App\\Models\\customer_supplier as CustomerSupplier;\nnamespace App\\Models;\nclass customer_supplier { public static function accumulatePoints($shop_id, $promotion_id) {} }\nnamespace App\\Http;\nCustomerSupplier::accumulatePoints($shop_id, $promotion_id);\n";

        let edits = action_edits(text, 6, 35);

        assert_eq!(
            apply_edits(text, &edits),
            "<?php\nnamespace App\\Http;\nuse App\\Models\\customer_supplier as CustomerSupplier;\nnamespace App\\Models;\nclass customer_supplier { public static function accumulatePoints($shop_id, $promotion_id) {} }\nnamespace App\\Http;\nCustomerSupplier::accumulatePoints(shop_id: $shop_id, promotion_id: $promotion_id);\n"
        );
    }

    #[test]
    fn converts_static_method_call_through_imported_namespace_alias() {
        let text = "<?php\nnamespace App\\Http;\nuse App\\Models as Models;\nnamespace App\\Models;\nclass Customer { public static function sync($shop_id, $customer_id) {} }\nnamespace App\\Http;\nModels\\Customer::sync($shop_id, $customer_id);\n";

        let edits = action_edits(text, 6, 25);

        assert_eq!(
            apply_edits(text, &edits),
            "<?php\nnamespace App\\Http;\nuse App\\Models as Models;\nnamespace App\\Models;\nclass Customer { public static function sync($shop_id, $customer_id) {} }\nnamespace App\\Http;\nModels\\Customer::sync(shop_id: $shop_id, customer_id: $customer_id);\n"
        );
    }

    #[test]
    fn converts_constructor_call() {
        let text = "<?php\nclass InvoiceJob { public function __construct($invoice, $notify) {} }\nnew InvoiceJob($invoice, true);\n";

        let edits = action_edits(text, 2, 6);

        assert_eq!(
            apply_edits(text, &edits),
            "<?php\nclass InvoiceJob { public function __construct($invoice, $notify) {} }\nnew InvoiceJob(invoice: $invoice, notify: true);\n"
        );
    }

    #[test]
    fn converts_constructor_call_through_import() {
        let text = "<?php\nnamespace App\\Http;\nuse App\\Jobs\\InvoiceJob;\nnamespace App\\Jobs;\nclass InvoiceJob { public function __construct($invoice, $notify) {} }\nnamespace App\\Http;\nnew InvoiceJob($invoice, true);\n";

        let edits = action_edits(text, 6, 6);

        assert_eq!(
            apply_edits(text, &edits),
            "<?php\nnamespace App\\Http;\nuse App\\Jobs\\InvoiceJob;\nnamespace App\\Jobs;\nclass InvoiceJob { public function __construct($invoice, $notify) {} }\nnamespace App\\Http;\nnew InvoiceJob(invoice: $invoice, notify: true);\n"
        );
    }

    #[test]
    fn converts_instance_method_when_variable_type_is_obvious() {
        let text = "<?php\nclass InvoiceSender { public function dispatch($invoice, $notify) {} }\n$sender = new InvoiceSender();\n$sender->dispatch($invoice, true);\n";

        let edits = action_edits(text, 3, 15);

        assert_eq!(
            apply_edits(text, &edits),
            "<?php\nclass InvoiceSender { public function dispatch($invoice, $notify) {} }\n$sender = new InvoiceSender();\n$sender->dispatch(invoice: $invoice, notify: true);\n"
        );
    }

    #[test]
    fn converts_instance_method_from_imported_typed_parameter() {
        let text = "<?php\nnamespace App\\Http;\nuse App\\Services\\InvoiceSender;\nnamespace App\\Services;\nclass InvoiceSender { public function dispatch($invoice, $notify) {} }\nnamespace App\\Http;\nfunction run(InvoiceSender $sender, $invoice) {\n    $sender->dispatch($invoice, true);\n}\n";

        let edits = action_edits(text, 7, 15);

        assert_eq!(
            apply_edits(text, &edits),
            "<?php\nnamespace App\\Http;\nuse App\\Services\\InvoiceSender;\nnamespace App\\Services;\nclass InvoiceSender { public function dispatch($invoice, $notify) {} }\nnamespace App\\Http;\nfunction run(InvoiceSender $sender, $invoice) {\n    $sender->dispatch(invoice: $invoice, notify: true);\n}\n"
        );
    }

    #[test]
    fn converts_instance_method_from_typed_parameter() {
        let text = "<?php\nclass InvoiceSender { public function dispatch($invoice, $notify) {} }\nfunction run(InvoiceSender $sender, $invoice) {\n    $sender->dispatch($invoice, true);\n}\n";

        let edits = action_edits(text, 3, 15);

        assert_eq!(
            apply_edits(text, &edits),
            "<?php\nclass InvoiceSender { public function dispatch($invoice, $notify) {} }\nfunction run(InvoiceSender $sender, $invoice) {\n    $sender->dispatch(invoice: $invoice, notify: true);\n}\n"
        );
    }

    #[test]
    fn resolves_static_method_from_parent_class() {
        let text = "<?php\nclass BaseSender { public static function dispatch($invoice, $notify) {} }\nclass InvoiceSender extends BaseSender {}\nInvoiceSender::dispatch($invoice, true);\n";

        let edits = action_edits(text, 3, 25);

        assert_eq!(
            apply_edits(text, &edits),
            "<?php\nclass BaseSender { public static function dispatch($invoice, $notify) {} }\nclass InvoiceSender extends BaseSender {}\nInvoiceSender::dispatch(invoice: $invoice, notify: true);\n"
        );
    }

    #[test]
    fn resolves_instance_method_from_implemented_interface() {
        let text = "<?php\ninterface Sender { public function dispatch($invoice, $notify); }\nclass InvoiceSender implements Sender {}\nfunction run(InvoiceSender $sender, $invoice) {\n    $sender->dispatch($invoice, true);\n}\n";

        let edits = action_edits(text, 4, 15);

        assert_eq!(
            apply_edits(text, &edits),
            "<?php\ninterface Sender { public function dispatch($invoice, $notify); }\nclass InvoiceSender implements Sender {}\nfunction run(InvoiceSender $sender, $invoice) {\n    $sender->dispatch(invoice: $invoice, notify: true);\n}\n"
        );
    }

    #[test]
    fn resolves_instance_method_from_used_trait() {
        let text = "<?php\ntrait Dispatchable { public function dispatch($invoice, $notify) {} }\nclass InvoiceSender { use Dispatchable; }\n$sender = new InvoiceSender();\n$sender->dispatch($invoice, true);\n";

        let edits = action_edits(text, 4, 15);

        assert_eq!(
            apply_edits(text, &edits),
            "<?php\ntrait Dispatchable { public function dispatch($invoice, $notify) {} }\nclass InvoiceSender { use Dispatchable; }\n$sender = new InvoiceSender();\n$sender->dispatch(invoice: $invoice, notify: true);\n"
        );
    }

    #[test]
    fn skips_inherited_method_when_signatures_conflict() {
        let text = "<?php\ninterface FirstSender { public function dispatch($invoice); }\ninterface SecondSender { public function dispatch($invoice, $notify); }\nclass InvoiceSender implements FirstSender, SecondSender {}\n$sender = new InvoiceSender();\n$sender->dispatch($invoice, true);\n";

        assert!(named_argument_code_action(&uri(), text, position(5, 15)).is_none());
        assert_eq!(
            skip_reason(&uri(), text, position(5, 15)),
            Some(SkipReason::AmbiguousCallable(
                "$sender->dispatch".to_string()
            ))
        );
    }

    #[test]
    fn resolves_project_functions_from_composer_psr4_roots() {
        let project_root = unique_project_root();
        let src_dir = project_root.join("src");
        fs::create_dir_all(&src_dir).expect("create source dir");
        fs::write(
            project_root.join("composer.json"),
            r#"{"autoload":{"psr-4":{"App\\":"src/"}}}"#,
        )
        .expect("write composer");
        fs::write(
            src_dir.join("Functions.php"),
            "<?php\nnamespace App;\nfunction send_invoice($invoice, $notify) {}\n",
        )
        .expect("write functions");

        let caller_path = src_dir.join("Caller.php");
        let caller_uri = Url::from_file_path(&caller_path).expect("file uri");
        let text = "<?php\nnamespace App;\nsend_invoice($invoice, true);\n";
        let action =
            named_argument_code_action(&caller_uri, text, position(2, 5)).expect("code action");
        let edits = action
            .edit
            .expect("workspace edit")
            .changes
            .expect("changes")
            .remove(&caller_uri)
            .expect("edits");

        assert_eq!(
            apply_edits(text, &edits),
            "<?php\nnamespace App;\nsend_invoice(invoice: $invoice, notify: true);\n"
        );

        fs::remove_dir_all(project_root).expect("remove project root");
    }

    #[test]
    fn resolves_project_classes_from_composer_classmap_file() {
        let project_root = unique_project_root();
        let legacy_dir = project_root.join("legacy");
        let app_dir = project_root.join("app");
        fs::create_dir_all(&legacy_dir).expect("create legacy dir");
        fs::create_dir_all(&app_dir).expect("create app dir");
        fs::write(
            project_root.join("composer.json"),
            r#"{"autoload":{"classmap":["legacy/CustomerSupplier.php"]}}"#,
        )
        .expect("write composer");
        fs::write(
            legacy_dir.join("CustomerSupplier.php"),
            "<?php\nnamespace Legacy;\nclass CustomerSupplier { public static function sync($shop_id, $customer_id) {} }\n",
        )
        .expect("write classmap class");

        let caller_path = app_dir.join("Caller.php");
        let caller_uri = Url::from_file_path(&caller_path).expect("file uri");
        let text = "<?php\nnamespace App;\nuse Legacy\\CustomerSupplier;\nCustomerSupplier::sync($shop_id, $customer_id);\n";
        let action =
            named_argument_code_action(&caller_uri, text, position(3, 25)).expect("code action");
        let edits = action
            .edit
            .expect("workspace edit")
            .changes
            .expect("changes")
            .remove(&caller_uri)
            .expect("edits");

        assert_eq!(
            apply_edits(text, &edits),
            "<?php\nnamespace App;\nuse Legacy\\CustomerSupplier;\nCustomerSupplier::sync(shop_id: $shop_id, customer_id: $customer_id);\n"
        );

        fs::remove_dir_all(project_root).expect("remove project root");
    }

    #[test]
    fn open_project_document_overrides_disk_symbols() {
        let project_root = unique_project_root();
        let src_dir = project_root.join("src");
        fs::create_dir_all(&src_dir).expect("create source dir");
        fs::write(
            project_root.join("composer.json"),
            r#"{"autoload":{"psr-4":{"App\\":"src/"}}}"#,
        )
        .expect("write composer");

        let service_path = src_dir.join("Service.php");
        fs::write(
            &service_path,
            "<?php\nnamespace App;\nclass Service { public static function sync($old) {} }\n",
        )
        .expect("write stale service");

        let caller_path = src_dir.join("Caller.php");
        let caller_uri = Url::from_file_path(&caller_path).expect("caller uri");
        let service_uri = Url::from_file_path(&service_path).expect("service uri");
        let caller_text = "<?php\nnamespace App;\nService::sync($first, $second);\n";
        let open_service_text = "<?php\nnamespace App;\nclass Service { public static function sync($first, $second) {} }\n";
        let open_documents = HashMap::from([(service_uri, open_service_text.to_string())]);

        assert!(named_argument_code_action(&caller_uri, caller_text, position(2, 10)).is_none());

        let action = analyze_code_actions_for_position(
            &caller_uri,
            caller_text,
            position(2, 10),
            &open_documents,
        )
        .actions
        .into_iter()
        .find_map(|action| match action {
            CodeActionOrCommand::CodeAction(action) => Some(action),
            CodeActionOrCommand::Command(_) => None,
        })
        .expect("code action from open service document");
        let edits = action
            .edit
            .expect("workspace edit")
            .changes
            .expect("changes")
            .remove(&caller_uri)
            .expect("edits");

        assert_eq!(
            apply_edits(caller_text, &edits),
            "<?php\nnamespace App;\nService::sync(first: $first, second: $second);\n"
        );

        fs::remove_dir_all(project_root).expect("remove project root");
    }

    #[test]
    fn project_index_cache_reuses_disk_symbols_and_applies_open_overrides() {
        let project_root = unique_project_root();
        let src_dir = project_root.join("src");
        fs::create_dir_all(&src_dir).expect("create source dir");
        fs::write(
            project_root.join("composer.json"),
            r#"{"autoload":{"psr-4":{"App\\":"src/"}}}"#,
        )
        .expect("write composer");

        let service_path = src_dir.join("Service.php");
        fs::write(
            &service_path,
            "<?php\nnamespace App;\nclass Service { public static function sync($first) {} }\n",
        )
        .expect("write service");

        let caller_path = src_dir.join("Caller.php");
        let caller_uri = Url::from_file_path(&caller_path).expect("caller uri");
        let service_uri = Url::from_file_path(&service_path).expect("service uri");
        let caller_text = "<?php\nnamespace App;\nService::sync($first, $second);\n";
        let mut cache = ProjectIndexCache::default();

        let first = analyze_code_actions_for_position_with_cache(
            &caller_uri,
            caller_text,
            position(2, 10),
            &HashMap::new(),
            &mut cache,
        );
        assert!(matches!(
            first.index_cache_status,
            IndexCacheStatus::Miss(_)
        ));
        assert_eq!(first.skip_reason, Some(SkipReason::UnsafeNamedArguments));

        let open_documents = HashMap::from([(
            service_uri,
            "<?php\nnamespace App;\nclass Service { public static function sync($first, $second) {} }\n"
                .to_string(),
        )]);
        let second = analyze_code_actions_for_position_with_cache(
            &caller_uri,
            caller_text,
            position(2, 10),
            &open_documents,
            &mut cache,
        );

        assert!(matches!(
            second.index_cache_status,
            IndexCacheStatus::Hit(_)
        ));
        assert_eq!(second.skip_reason, None);
        assert_eq!(second.actions.len(), 1);

        fs::remove_dir_all(project_root).expect("remove project root");
    }

    #[test]
    fn project_index_cache_invalidates_changed_project_files() {
        let project_root = unique_project_root();
        let src_dir = project_root.join("src");
        fs::create_dir_all(&src_dir).expect("create source dir");
        fs::write(
            project_root.join("composer.json"),
            r#"{"autoload":{"psr-4":{"App\\":"src/"}}}"#,
        )
        .expect("write composer");

        let service_path = src_dir.join("Service.php");
        fs::write(
            &service_path,
            "<?php\nnamespace App;\nclass Service { public static function sync($first) {} }\n",
        )
        .expect("write service");
        let caller_path = src_dir.join("Caller.php");
        let caller_uri = Url::from_file_path(&caller_path).expect("caller uri");
        let service_uri = Url::from_file_path(&service_path).expect("service uri");
        let caller_text = "<?php\nnamespace App;\nService::sync($first, $second);\n";
        let mut cache = ProjectIndexCache::default();

        let first = analyze_code_actions_for_position_with_cache(
            &caller_uri,
            caller_text,
            position(2, 10),
            &HashMap::new(),
            &mut cache,
        );
        assert_eq!(first.skip_reason, Some(SkipReason::UnsafeNamedArguments));

        fs::write(
            &service_path,
            "<?php\nnamespace App;\nclass Service { public static function sync($first, $second) {} }\n",
        )
        .expect("update service");
        let stale = analyze_code_actions_for_position_with_cache(
            &caller_uri,
            caller_text,
            position(2, 10),
            &HashMap::new(),
            &mut cache,
        );
        assert_eq!(stale.skip_reason, Some(SkipReason::UnsafeNamedArguments));

        assert!(cache.invalidate_document(&service_uri));
        let refreshed = analyze_code_actions_for_position_with_cache(
            &caller_uri,
            caller_text,
            position(2, 10),
            &HashMap::new(),
            &mut cache,
        );
        assert_eq!(refreshed.skip_reason, None);

        fs::remove_dir_all(project_root).expect("remove project root");
    }

    #[test]
    fn skips_project_when_composer_php_constraint_allows_php_7() {
        let project_root = unique_project_root();
        let src_dir = project_root.join("src");
        fs::create_dir_all(&src_dir).expect("create source dir");
        fs::write(
            project_root.join("composer.json"),
            r#"{"require":{"php":"^7.4"},"autoload":{"psr-4":{"App\\":"src/"}}}"#,
        )
        .expect("write composer");

        let caller_path = src_dir.join("Caller.php");
        let caller_uri = Url::from_file_path(&caller_path).expect("caller uri");
        let caller_text = "<?php\nnamespace App;\nfunction send_invoice($invoice, $notify) {}\nsend_invoice($invoice, true);\n";

        let analysis = analyze_code_actions_for_position(
            &caller_uri,
            caller_text,
            position(3, 5),
            &HashMap::new(),
        );
        assert!(analysis.actions.is_empty());
        assert_eq!(analysis.skip_reason, Some(SkipReason::PhpVersionBelow8));

        fs::remove_dir_all(project_root).expect("remove project root");
    }

    #[test]
    fn allows_project_when_composer_requires_php_8() {
        let project_root = unique_project_root();
        let src_dir = project_root.join("src");
        fs::create_dir_all(&src_dir).expect("create source dir");
        fs::write(
            project_root.join("composer.json"),
            r#"{"require":{"php":">=8.0 <9.0"},"autoload":{"psr-4":{"App\\":"src/"}}}"#,
        )
        .expect("write composer");

        let caller_path = src_dir.join("Caller.php");
        let caller_uri = Url::from_file_path(&caller_path).expect("caller uri");
        let caller_text = "<?php\nnamespace App;\nfunction send_invoice($invoice, $notify) {}\nsend_invoice($invoice, true);\n";

        let action =
            named_argument_code_action(&caller_uri, caller_text, position(3, 5)).expect("action");
        let edits = action
            .edit
            .expect("workspace edit")
            .changes
            .expect("changes")
            .remove(&caller_uri)
            .expect("edits");

        assert_eq!(
            apply_edits(caller_text, &edits),
            "<?php\nnamespace App;\nfunction send_invoice($invoice, $notify) {}\nsend_invoice(invoice: $invoice, notify: true);\n"
        );

        fs::remove_dir_all(project_root).expect("remove project root");
    }

    #[test]
    fn indexes_trait_and_interface_methods() {
        let mut index = SymbolIndex::default();
        index.index_text(
            "<?php\nnamespace App;\ntrait Dispatchable { public function dispatch($invoice, $notify) {} }\ninterface Sender { public function send($invoice, $notify); }\n",
        );

        let trait_info = index
            .classes
            .get(&normalize_symbol_key("App\\Dispatchable"))
            .expect("trait indexed");
        let interface_info = index
            .classes
            .get(&normalize_symbol_key("App\\Sender"))
            .expect("interface indexed");

        assert_eq!(
            trait_info
                .methods
                .get(&normalize_method_key("dispatch"))
                .expect("trait method")
                .parameters,
            vec!["invoice", "notify"]
        );
        assert_eq!(
            interface_info
                .methods
                .get(&normalize_method_key("send"))
                .expect("interface method")
                .parameters,
            vec!["invoice", "notify"]
        );
    }

    #[test]
    fn skips_calls_when_all_arguments_are_named() {
        let text = "<?php\nfunction send_invoice($invoice, $notify) {}\nsend_invoice(invoice: $invoice, notify: true);\n";

        assert!(named_argument_code_action(&uri(), text, position(2, 5)).is_none());
    }

    #[test]
    fn converts_missing_argument_names_in_partially_named_call() {
        let text = "<?php\nfunction send_invoice($invoice, $notify, $priority) {}\nsend_invoice(invoice: $invoice, notify: true, $priority);\n";

        let edits = action_edits(text, 2, 45);

        assert_eq!(
            apply_edits(text, &edits),
            "<?php\nfunction send_invoice($invoice, $notify, $priority) {}\nsend_invoice(invoice: $invoice, notify: true, priority: $priority);\n"
        );
    }

    #[test]
    fn converts_single_missing_name_after_named_static_arguments() {
        let text = "<?php\nclass customer_supplier { public static function accumulatePoints($shop_id, $promotion_id, $customer_id, $is_update_transaction, $customer_used_point, $pay, $product, $multipay_methods, $order_id, $extra_discount, $grand_total, $exchange_gift = null) {} }\ncustomer_supplier::accumulatePoints(\n    shop_id: $shop_id,\n    promotion_id: $order->promotion_id,\n    customer_id: $customer_id,\n    is_update_transaction: $is_update_transaction,\n    customer_used_point: $item['customer_used_point'] ?? 0,\n    pay: $request->pay,\n    product: $request->product,\n    multipay_methods: $multipay_methods,\n    order_id: $order->id,\n    extra_discount: $request->extra_discount,\n    grand_total: $request->grand_total,\n    $request->exchange_gift,\n);\n";

        let action = named_argument_code_action(&uri(), text, position(14, 5)).expect("action");
        let edits = action
            .edit
            .expect("workspace edit")
            .changes
            .expect("changes")
            .remove(&uri())
            .expect("edits");

        assert_eq!(
            action.title,
            "[Rephactor] Add name identifier 'exchange_gift'"
        );
        assert_eq!(edits.len(), 1);
        assert_eq!(edits[0].new_text, "exchange_gift: ");
        assert_eq!(
            apply_edits(text, &edits),
            "<?php\nclass customer_supplier { public static function accumulatePoints($shop_id, $promotion_id, $customer_id, $is_update_transaction, $customer_used_point, $pay, $product, $multipay_methods, $order_id, $extra_discount, $grand_total, $exchange_gift = null) {} }\ncustomer_supplier::accumulatePoints(\n    shop_id: $shop_id,\n    promotion_id: $order->promotion_id,\n    customer_id: $customer_id,\n    is_update_transaction: $is_update_transaction,\n    customer_used_point: $item['customer_used_point'] ?? 0,\n    pay: $request->pay,\n    product: $request->product,\n    multipay_methods: $multipay_methods,\n    order_id: $order->id,\n    extra_discount: $request->extra_discount,\n    grand_total: $request->grand_total,\n    exchange_gift: $request->exchange_gift,\n);\n"
        );
    }

    #[test]
    fn skips_partially_named_calls_when_existing_names_do_not_match_position() {
        let text = "<?php\nfunction send_invoice($invoice, $notify) {}\nsend_invoice(notify: true, $invoice);\n";

        assert!(named_argument_code_action(&uri(), text, position(2, 25)).is_none());
        assert_eq!(
            skip_reason(&uri(), text, position(2, 25)),
            Some(SkipReason::UnsafeNamedArguments)
        );
    }

    #[test]
    fn skips_calls_with_unpacking() {
        let text = "<?php\nfunction send_invoice($invoice, $notify) {}\nsend_invoice($invoice, ...$flags);\n";

        assert!(named_argument_code_action(&uri(), text, position(2, 5)).is_none());
        assert_eq!(
            skip_reason(&uri(), text, position(2, 5)),
            Some(SkipReason::UnpackingArgument)
        );
    }

    #[test]
    fn skips_dynamic_function_calls() {
        let text = "<?php\nfunction send_invoice($invoice, $notify) {}\n$fn($invoice, true);\n";

        assert!(named_argument_code_action(&uri(), text, position(2, 2)).is_none());
        assert_eq!(
            skip_reason(&uri(), text, position(2, 2)),
            Some(SkipReason::UnsupportedDynamicCall)
        );
    }

    #[test]
    fn reports_unresolved_callable_skip_reason() {
        let text = "<?php\nmissing_function($invoice, true);\n";

        assert_eq!(
            skip_reason(&uri(), text, position(1, 5)),
            Some(SkipReason::UnresolvedCallable(
                "missing_function".to_string()
            ))
        );
    }
}
